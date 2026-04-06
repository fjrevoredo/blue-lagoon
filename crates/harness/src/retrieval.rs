use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use contracts::{
    ForegroundTrigger, RetrievedContext, RetrievedContextItem, RetrievedEpisodeContext,
    RetrievedMemoryArtifactContext,
};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{config::RuntimeConfig, continuity};

const RETRIEVAL_REFRESH_SCAN_MULTIPLIER: i64 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetrievalSourceKind {
    Episode,
    MemoryArtifact,
}

#[derive(Debug, Clone)]
struct RetrievalCandidate {
    retrieval_artifact_id: Uuid,
    source_kind: RetrievalSourceKind,
    source_episode_id: Option<Uuid>,
    source_memory_artifact_id: Option<Uuid>,
    lexical_document: String,
    relevance_timestamp: DateTime<Utc>,
    internal_conversation_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScoredCandidate {
    retrieval_artifact_id: Uuid,
    source_kind: RetrievalSourceKind,
    source_episode_id: Option<Uuid>,
    source_memory_artifact_id: Option<Uuid>,
    score: i64,
    lexical_match_count: usize,
    relevance_reason: String,
}

pub async fn assemble_retrieved_context(
    pool: &PgPool,
    config: &RuntimeConfig,
    trigger: &ForegroundTrigger,
) -> Result<RetrievedContext> {
    let retrieval_config = &config.continuity.retrieval;
    let scan_limit =
        i64::from(retrieval_config.max_context_items) * RETRIEVAL_REFRESH_SCAN_MULTIPLIER;

    ensure_episode_retrieval_artifacts(
        pool,
        &trigger.ingress.internal_conversation_ref,
        trigger.received_at,
        scan_limit.max(1),
    )
    .await?;
    ensure_memory_retrieval_artifacts(
        pool,
        &trigger.ingress.internal_conversation_ref,
        scan_limit.max(1),
    )
    .await?;

    let candidates = fetch_retrieval_candidates(
        pool,
        &trigger.ingress.internal_conversation_ref,
        trigger.received_at,
    )
    .await?;
    let scored = score_candidates(
        candidates,
        trigger.ingress.text_body.as_deref().unwrap_or_default(),
        &trigger.ingress.internal_conversation_ref,
    );
    let selected = select_candidates(
        scored,
        retrieval_config.max_recent_episode_candidates as usize,
        retrieval_config.max_memory_artifact_candidates as usize,
        retrieval_config.max_context_items as usize,
    );

    let mut items = Vec::new();
    for candidate in selected {
        match candidate.source_kind {
            RetrievalSourceKind::Episode => {
                if let Some(episode_id) = candidate.source_episode_id {
                    items.push(RetrievedContextItem::Episode(
                        load_episode_context(pool, episode_id, candidate.relevance_reason.clone())
                            .await?,
                    ));
                }
            }
            RetrievalSourceKind::MemoryArtifact => {
                if let Some(memory_artifact_id) = candidate.source_memory_artifact_id {
                    items.push(RetrievedContextItem::MemoryArtifact(
                        load_memory_context(
                            pool,
                            memory_artifact_id,
                            candidate.relevance_reason.clone(),
                        )
                        .await?,
                    ));
                }
            }
        }
    }

    Ok(RetrievedContext { items })
}

async fn ensure_episode_retrieval_artifacts(
    pool: &PgPool,
    internal_conversation_ref: &str,
    before: DateTime<Utc>,
    limit: i64,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT
            e.episode_id,
            e.internal_conversation_ref,
            e.started_at,
            COALESCE(e.summary, '') AS summary,
            COALESCE(
                (
                    SELECT text_body
                    FROM episode_messages
                    WHERE episode_id = e.episode_id AND message_role = 'user'
                    ORDER BY message_order DESC
                    LIMIT 1
                ),
                ''
            ) AS user_message,
            COALESCE(
                (
                    SELECT text_body
                    FROM episode_messages
                    WHERE episode_id = e.episode_id AND message_role = 'assistant'
                    ORDER BY message_order DESC
                    LIMIT 1
                ),
                ''
            ) AS assistant_message
        FROM episodes e
        WHERE e.internal_conversation_ref = $1
          AND e.started_at < $2
        ORDER BY e.started_at DESC
        LIMIT $3
        "#,
    )
    .bind(internal_conversation_ref)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to scan episodes for retrieval refresh")?;

    for row in rows {
        let episode_id: Uuid = row.get("episode_id");
        if retrieval_artifact_exists_for_episode(pool, episode_id).await? {
            continue;
        }

        let summary: String = row.get("summary");
        let user_message: String = row.get("user_message");
        let assistant_message: String = row.get("assistant_message");
        let lexical_document = [summary, user_message, assistant_message]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if lexical_document.trim().is_empty() {
            continue;
        }

        continuity::insert_retrieval_artifact(
            pool,
            &continuity::NewRetrievalArtifact {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: "episode".to_string(),
                source_episode_id: Some(episode_id),
                source_memory_artifact_id: None,
                internal_conversation_ref: Some(row.get("internal_conversation_ref")),
                lexical_document,
                relevance_timestamp: row.get("started_at"),
                status: "active".to_string(),
                payload: json!({ "projection": "episode_retrieval_baseline" }),
            },
        )
        .await?;
    }

    Ok(())
}

async fn ensure_memory_retrieval_artifacts(
    pool: &PgPool,
    internal_conversation_ref: &str,
    limit: i64,
) -> Result<()> {
    let rows = sqlx::query(
        r#"
        SELECT
            ma.memory_artifact_id,
            ma.content_text,
            COALESCE(ma.valid_from, ma.created_at) AS relevance_timestamp,
            ie.internal_conversation_ref
        FROM memory_artifacts ma
        LEFT JOIN ingress_events ie ON ie.ingress_id = ma.source_ingress_id
        WHERE ma.status = 'active'
          AND ie.internal_conversation_ref = $1
        ORDER BY ma.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(internal_conversation_ref)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to scan memory artifacts for retrieval refresh")?;

    for row in rows {
        let memory_artifact_id: Uuid = row.get("memory_artifact_id");
        if retrieval_artifact_exists_for_memory(pool, memory_artifact_id).await? {
            continue;
        }

        continuity::insert_retrieval_artifact(
            pool,
            &continuity::NewRetrievalArtifact {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: "memory_artifact".to_string(),
                source_episode_id: None,
                source_memory_artifact_id: Some(memory_artifact_id),
                internal_conversation_ref: row.get("internal_conversation_ref"),
                lexical_document: row.get("content_text"),
                relevance_timestamp: row.get("relevance_timestamp"),
                status: "active".to_string(),
                payload: json!({ "projection": "memory_retrieval_baseline" }),
            },
        )
        .await?;
    }

    Ok(())
}

async fn retrieval_artifact_exists_for_episode(pool: &PgPool, episode_id: Uuid) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM retrieval_artifacts
        WHERE source_episode_id = $1
        "#,
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await
    .context("failed to check retrieval artifact existence for episode")?;
    Ok(exists > 0)
}

async fn retrieval_artifact_exists_for_memory(
    pool: &PgPool,
    memory_artifact_id: Uuid,
) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM retrieval_artifacts
        WHERE source_memory_artifact_id = $1
        "#,
    )
    .bind(memory_artifact_id)
    .fetch_one(pool)
    .await
    .context("failed to check retrieval artifact existence for memory artifact")?;
    Ok(exists > 0)
}

async fn fetch_retrieval_candidates(
    pool: &PgPool,
    internal_conversation_ref: &str,
    before: DateTime<Utc>,
) -> Result<Vec<RetrievalCandidate>> {
    let rows = sqlx::query(
        r#"
        SELECT
            ra.retrieval_artifact_id,
            ra.source_kind,
            ra.source_episode_id,
            ra.source_memory_artifact_id,
            ra.lexical_document,
            ra.relevance_timestamp,
            ra.internal_conversation_ref
        FROM retrieval_artifacts ra
        LEFT JOIN memory_artifacts ma ON ma.memory_artifact_id = ra.source_memory_artifact_id
        WHERE ra.status = 'active'
          AND ra.internal_conversation_ref = $1
          AND ra.relevance_timestamp < $2
          AND (
                (ra.source_kind = 'episode' AND ra.source_episode_id IS NOT NULL)
                OR (ra.source_kind = 'memory_artifact' AND ma.status = 'active')
          )
        ORDER BY ra.relevance_timestamp DESC, ra.retrieval_artifact_id DESC
        "#,
    )
    .bind(internal_conversation_ref)
    .bind(before)
    .fetch_all(pool)
    .await
    .context("failed to fetch retrieval candidates")?;

    Ok(rows
        .into_iter()
        .map(|row| RetrievalCandidate {
            retrieval_artifact_id: row.get("retrieval_artifact_id"),
            source_kind: match row.get::<String, _>("source_kind").as_str() {
                "episode" => RetrievalSourceKind::Episode,
                _ => RetrievalSourceKind::MemoryArtifact,
            },
            source_episode_id: row.get("source_episode_id"),
            source_memory_artifact_id: row.get("source_memory_artifact_id"),
            lexical_document: row.get("lexical_document"),
            relevance_timestamp: row.get("relevance_timestamp"),
            internal_conversation_ref: row.get("internal_conversation_ref"),
        })
        .collect())
}

fn score_candidates(
    candidates: Vec<RetrievalCandidate>,
    trigger_text: &str,
    internal_conversation_ref: &str,
) -> Vec<ScoredCandidate> {
    let trigger_tokens = tokenize(trigger_text);

    let mut scored = candidates
        .into_iter()
        .map(|candidate| {
            let candidate_tokens = tokenize(&candidate.lexical_document);
            let lexical_match_count = trigger_tokens.intersection(&candidate_tokens).count();
            let same_conversation =
                candidate.internal_conversation_ref.as_deref() == Some(internal_conversation_ref);
            let score = lexical_match_count as i64 * 100
                + if same_conversation { 50 } else { 0 }
                + recency_bonus(candidate.relevance_timestamp);
            let relevance_reason = if lexical_match_count > 0 {
                format!("lexical_match:{lexical_match_count}")
            } else if same_conversation {
                "same_conversation_recent".to_string()
            } else {
                "recency_only".to_string()
            };

            ScoredCandidate {
                retrieval_artifact_id: candidate.retrieval_artifact_id,
                source_kind: candidate.source_kind,
                source_episode_id: candidate.source_episode_id,
                source_memory_artifact_id: candidate.source_memory_artifact_id,
                score,
                lexical_match_count,
                relevance_reason,
            }
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.lexical_match_count.cmp(&left.lexical_match_count))
            .then_with(|| left.retrieval_artifact_id.cmp(&right.retrieval_artifact_id))
    });
    scored
}

fn select_candidates(
    scored: Vec<ScoredCandidate>,
    max_episode_candidates: usize,
    max_memory_candidates: usize,
    max_total_items: usize,
) -> Vec<ScoredCandidate> {
    let mut selected = Vec::new();
    let mut episode_count = 0;
    let mut memory_count = 0;

    for candidate in scored {
        if selected.len() >= max_total_items {
            break;
        }
        match candidate.source_kind {
            RetrievalSourceKind::Episode if episode_count >= max_episode_candidates => continue,
            RetrievalSourceKind::MemoryArtifact if memory_count >= max_memory_candidates => {
                continue;
            }
            RetrievalSourceKind::Episode => episode_count += 1,
            RetrievalSourceKind::MemoryArtifact => memory_count += 1,
        }
        selected.push(candidate);
    }

    selected
}

async fn load_episode_context(
    pool: &PgPool,
    episode_id: Uuid,
    relevance_reason: String,
) -> Result<RetrievedEpisodeContext> {
    let row = sqlx::query(
        r#"
        SELECT episode_id, internal_conversation_ref, started_at, COALESCE(summary, '') AS summary, COALESCE(outcome, status) AS outcome
        FROM episodes
        WHERE episode_id = $1
        "#,
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await
    .context("failed to load retrieved episode context")?;

    Ok(RetrievedEpisodeContext {
        episode_id: row.get("episode_id"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        started_at: row.get("started_at"),
        summary: row.get("summary"),
        outcome: row.get("outcome"),
        relevance_reason,
    })
}

async fn load_memory_context(
    pool: &PgPool,
    memory_artifact_id: Uuid,
    relevance_reason: String,
) -> Result<RetrievedMemoryArtifactContext> {
    let row = sqlx::query(
        r#"
        SELECT memory_artifact_id, artifact_kind, subject_ref, content_text, status
        FROM memory_artifacts
        WHERE memory_artifact_id = $1
        "#,
    )
    .bind(memory_artifact_id)
    .fetch_one(pool)
    .await
    .context("failed to load retrieved memory context")?;

    Ok(RetrievedMemoryArtifactContext {
        memory_artifact_id: row.get("memory_artifact_id"),
        artifact_kind: row.get("artifact_kind"),
        subject_ref: row.get("subject_ref"),
        content_text: row.get("content_text"),
        validity_status: row.get("status"),
        relevance_reason,
    })
}

fn tokenize(value: &str) -> BTreeSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.trim().is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn recency_bonus(relevance_timestamp: DateTime<Utc>) -> i64 {
    let age_minutes = (Utc::now() - relevance_timestamp).num_minutes().max(0);
    20 - age_minutes.min(20)
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    #[test]
    fn score_candidates_prefers_lexical_match_then_recency() {
        let now = Utc::now();
        let scored = score_candidates(
            vec![
                RetrievalCandidate {
                    retrieval_artifact_id: Uuid::now_v7(),
                    source_kind: RetrievalSourceKind::MemoryArtifact,
                    source_episode_id: None,
                    source_memory_artifact_id: Some(Uuid::now_v7()),
                    lexical_document: "prefers concise replies".to_string(),
                    relevance_timestamp: now - Duration::minutes(10),
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                },
                RetrievalCandidate {
                    retrieval_artifact_id: Uuid::now_v7(),
                    source_kind: RetrievalSourceKind::Episode,
                    source_episode_id: Some(Uuid::now_v7()),
                    source_memory_artifact_id: None,
                    lexical_document: "discussed travel plans".to_string(),
                    relevance_timestamp: now,
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                },
            ],
            "please be concise",
            "telegram-primary",
        );

        assert_eq!(scored[0].source_kind, RetrievalSourceKind::MemoryArtifact);
        assert!(scored[0].lexical_match_count > 0);
    }

    #[test]
    fn select_candidates_respects_per_type_and_total_bounds() {
        let scored = vec![
            sample_scored_candidate(RetrievalSourceKind::Episode, 300),
            sample_scored_candidate(RetrievalSourceKind::Episode, 250),
            sample_scored_candidate(RetrievalSourceKind::MemoryArtifact, 240),
            sample_scored_candidate(RetrievalSourceKind::MemoryArtifact, 230),
        ];

        let selected = select_candidates(scored, 1, 1, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].source_kind, RetrievalSourceKind::Episode);
        assert_eq!(selected[1].source_kind, RetrievalSourceKind::MemoryArtifact);
    }

    fn sample_scored_candidate(source_kind: RetrievalSourceKind, score: i64) -> ScoredCandidate {
        ScoredCandidate {
            retrieval_artifact_id: Uuid::now_v7(),
            source_kind,
            source_episode_id: Some(Uuid::now_v7()),
            source_memory_artifact_id: Some(Uuid::now_v7()),
            score,
            lexical_match_count: 1,
            relevance_reason: "test".to_string(),
        }
    }
}
