use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use contracts::{
    ForegroundTrigger, RetrievalUpdateOperation, RetrievalUpdateProposal, RetrievedContext,
    RetrievedContextItem, RetrievedEpisodeContext, RetrievedMemoryArtifactContext,
    UnconsciousScope,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
    continuity,
};

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
    semantic_document: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetrievalUpdateApplicationSummary {
    pub evaluated_count: usize,
    pub upserted_count: usize,
    pub archived_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RetrievalUpdateApplicationContext<'a> {
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub source_loop_kind: &'a str,
    pub subsystem: &'a str,
    pub worker_pid: Option<i32>,
    pub scope: &'a UnconsciousScope,
}

pub async fn apply_retrieval_updates(
    pool: &PgPool,
    context: RetrievalUpdateApplicationContext<'_>,
    updates: &[RetrievalUpdateProposal],
) -> Result<RetrievalUpdateApplicationSummary> {
    let mut summary = RetrievalUpdateApplicationSummary::default();
    let source_episode_id = context.scope.episode_ids.first().copied();

    for update in updates {
        summary.evaluated_count += 1;
        validate_retrieval_update(update)?;

        match update.operation {
            RetrievalUpdateOperation::Upsert => {
                let archived_count = continuity::archive_retrieval_artifacts_by_source_ref(
                    pool,
                    &update.source_ref,
                    update.internal_conversation_ref.as_deref(),
                )
                .await?;
                continuity::insert_retrieval_artifact(
                    pool,
                    &continuity::NewRetrievalArtifact {
                        retrieval_artifact_id: update.update_id,
                        source_kind: "episode".to_string(),
                        source_episode_id: Some(source_episode_id.context(
                            "retrieval maintenance upsert requires a scoped episode anchor",
                        )?),
                        source_memory_artifact_id: None,
                        internal_conversation_ref: update.internal_conversation_ref.clone(),
                        lexical_document: update.lexical_document.clone(),
                        relevance_timestamp: update.relevance_timestamp,
                        status: "active".to_string(),
                        payload: json!({
                            "projection": "background_retrieval_maintenance",
                            "source_ref": update.source_ref,
                            "rationale": update.rationale,
                        }),
                    },
                )
                .await?;
                summary.upserted_count += 1;
                audit::insert(
                    pool,
                    &NewAuditEvent {
                        loop_kind: context.source_loop_kind.to_string(),
                        subsystem: context.subsystem.to_string(),
                        event_kind: "retrieval_update_applied".to_string(),
                        severity: "info".to_string(),
                        trace_id: context.trace_id,
                        execution_id: Some(context.execution_id),
                        worker_pid: context.worker_pid,
                        payload: json!({
                            "update_id": update.update_id,
                            "operation": "upsert",
                            "source_ref": update.source_ref,
                            "archived_prior_count": archived_count,
                            "source_kind": "episode",
                            "source_episode_id": source_episode_id,
                            "internal_conversation_ref": update.internal_conversation_ref,
                        }),
                    },
                )
                .await?;
            }
            RetrievalUpdateOperation::Archive => {
                let archived_count = continuity::archive_retrieval_artifacts_by_source_ref(
                    pool,
                    &update.source_ref,
                    update.internal_conversation_ref.as_deref(),
                )
                .await?;
                summary.archived_count += archived_count as usize;
                audit::insert(
                    pool,
                    &NewAuditEvent {
                        loop_kind: context.source_loop_kind.to_string(),
                        subsystem: context.subsystem.to_string(),
                        event_kind: "retrieval_update_applied".to_string(),
                        severity: "info".to_string(),
                        trace_id: context.trace_id,
                        execution_id: Some(context.execution_id),
                        worker_pid: context.worker_pid,
                        payload: json!({
                            "update_id": update.update_id,
                            "operation": "archive",
                            "source_ref": update.source_ref,
                            "archived_count": archived_count,
                            "internal_conversation_ref": update.internal_conversation_ref,
                        }),
                    },
                )
                .await?;
            }
        }
    }

    Ok(summary)
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
                lexical_document: lexical_document.clone(),
                relevance_timestamp: row.get("started_at"),
                status: "active".to_string(),
                payload: json!({
                    "projection": "episode_retrieval_baseline",
                    "semantic_document": build_semantic_document(&[&lexical_document]),
                }),
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
            ma.artifact_kind,
            ma.subject_ref,
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

        let artifact_kind: String = row.get("artifact_kind");
        let subject_ref: String = row.get("subject_ref");
        let content_text: String = row.get("content_text");

        continuity::insert_retrieval_artifact(
            pool,
            &continuity::NewRetrievalArtifact {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: "memory_artifact".to_string(),
                source_episode_id: None,
                source_memory_artifact_id: Some(memory_artifact_id),
                internal_conversation_ref: row.get("internal_conversation_ref"),
                lexical_document: content_text.clone(),
                relevance_timestamp: row.get("relevance_timestamp"),
                status: "active".to_string(),
                payload: json!({
                    "projection": "memory_retrieval_baseline",
                    "semantic_document": build_semantic_document(&[
                        artifact_kind.as_str(),
                        subject_ref.as_str(),
                        content_text.as_str(),
                    ]),
                }),
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
            ra.payload_json,
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
        .map(|row| {
            let lexical_document: String = row.get("lexical_document");
            let payload: Value = row.get("payload_json");

            RetrievalCandidate {
                retrieval_artifact_id: row.get("retrieval_artifact_id"),
                source_kind: match row.get::<String, _>("source_kind").as_str() {
                    "episode" => RetrievalSourceKind::Episode,
                    _ => RetrievalSourceKind::MemoryArtifact,
                },
                source_episode_id: row.get("source_episode_id"),
                source_memory_artifact_id: row.get("source_memory_artifact_id"),
                lexical_document: lexical_document.clone(),
                semantic_document: payload
                    .get("semantic_document")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| build_semantic_document(&[&lexical_document])),
                relevance_timestamp: row.get("relevance_timestamp"),
                internal_conversation_ref: row.get("internal_conversation_ref"),
            }
        })
        .collect())
}

fn score_candidates(
    candidates: Vec<RetrievalCandidate>,
    trigger_text: &str,
    internal_conversation_ref: &str,
) -> Vec<ScoredCandidate> {
    let trigger_tokens = tokenize(trigger_text);
    let trigger_semantic_tokens = semantic_tokens(trigger_text);

    let mut scored = candidates
        .into_iter()
        .map(|candidate| {
            let candidate_tokens = tokenize(&candidate.lexical_document);
            let candidate_semantic_tokens = semantic_tokens(&candidate.semantic_document);
            let lexical_match_count = trigger_tokens.intersection(&candidate_tokens).count();
            let semantic_match_count = trigger_semantic_tokens
                .intersection(&candidate_semantic_tokens)
                .count();
            let same_conversation =
                candidate.internal_conversation_ref.as_deref() == Some(internal_conversation_ref);
            let score = lexical_match_count as i64 * 100
                + semantic_match_count as i64 * 40
                + if same_conversation { 50 } else { 0 }
                + recency_bonus(candidate.relevance_timestamp);
            let relevance_reason = if lexical_match_count > 0 {
                format!("lexical_match:{lexical_match_count}")
            } else if semantic_match_count > 0 {
                format!("semantic_match:{semantic_match_count}")
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

fn semantic_tokens(value: &str) -> BTreeSet<String> {
    tokenize(value)
        .into_iter()
        .flat_map(|token| {
            let normalized = normalize_semantic_token(&token);
            let mut variants = vec![normalized.clone()];
            variants.extend(
                semantic_expansions(&normalized)
                    .iter()
                    .map(|value| (*value).to_string()),
            );
            variants
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn build_semantic_document(values: &[&str]) -> String {
    let mut tokens = BTreeSet::new();
    for value in values {
        tokens.extend(semantic_tokens(value));
    }
    tokens.into_iter().collect::<Vec<_>>().join(" ")
}

fn validate_retrieval_update(update: &RetrievalUpdateProposal) -> Result<()> {
    if update.source_ref.trim().is_empty() {
        anyhow::bail!("retrieval update source_ref must not be empty");
    }

    match update.operation {
        RetrievalUpdateOperation::Upsert => {
            if update.lexical_document.trim().is_empty() {
                anyhow::bail!("retrieval upsert lexical_document must not be empty");
            }
            let Some(internal_conversation_ref) = update.internal_conversation_ref.as_deref()
            else {
                anyhow::bail!(
                    "retrieval upsert requires an internal_conversation_ref for conservative persistence"
                );
            };
            if internal_conversation_ref.trim().is_empty() {
                anyhow::bail!("retrieval upsert internal_conversation_ref must not be empty");
            }
        }
        RetrievalUpdateOperation::Archive => {}
    }

    Ok(())
}

fn normalize_semantic_token(token: &str) -> String {
    let token = token.trim().to_ascii_lowercase();
    if token.len() > 4 && token.ends_with("ies") {
        return format!("{}y", &token[..token.len() - 3]);
    }
    if token.len() > 5 && token.ends_with("ing") {
        return token[..token.len() - 3].to_string();
    }
    if token.len() > 4 && token.ends_with("ed") {
        return token[..token.len() - 2].to_string();
    }
    if token.len() > 3 && token.ends_with('s') && !token.ends_with("ss") {
        return token[..token.len() - 1].to_string();
    }
    token
}

fn semantic_expansions(token: &str) -> &'static [&'static str] {
    match token {
        "brief" => &["concise", "succinct", "short", "direct"],
        "concise" => &["brief", "succinct", "short", "direct"],
        "succinct" => &["brief", "concise", "short"],
        "short" => &["brief", "concise", "succinct"],
        "direct" => &["concise", "brief", "straightforward"],
        "straightforward" => &["direct", "concise"],
        "reply" | "response" | "answer" => &["reply", "response", "answer"],
        "preference" | "prefer" | "like" => &["preference", "prefer", "like"],
        "travel" => &["trip", "journey"],
        "trip" => &["travel", "journey"],
        "journey" => &["travel", "trip"],
        _ => &[],
    }
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
                    semantic_document: build_semantic_document(&["prefers concise replies"]),
                    relevance_timestamp: now - Duration::minutes(10),
                    internal_conversation_ref: Some("telegram-primary".to_string()),
                },
                RetrievalCandidate {
                    retrieval_artifact_id: Uuid::now_v7(),
                    source_kind: RetrievalSourceKind::Episode,
                    source_episode_id: Some(Uuid::now_v7()),
                    source_memory_artifact_id: None,
                    lexical_document: "discussed travel plans".to_string(),
                    semantic_document: build_semantic_document(&["discussed travel plans"]),
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
    fn score_candidates_uses_semantic_match_when_lexical_overlap_is_absent() {
        let now = Utc::now();
        let scored = score_candidates(
            vec![RetrievalCandidate {
                retrieval_artifact_id: Uuid::now_v7(),
                source_kind: RetrievalSourceKind::MemoryArtifact,
                source_episode_id: None,
                source_memory_artifact_id: Some(Uuid::now_v7()),
                lexical_document: "prefers concise replies".to_string(),
                semantic_document: build_semantic_document(&["prefers concise replies"]),
                relevance_timestamp: now,
                internal_conversation_ref: Some("telegram-primary".to_string()),
            }],
            "please be brief",
            "telegram-primary",
        );

        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].lexical_match_count, 0);
        assert!(scored[0].relevance_reason.starts_with("semantic_match:"));
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

    #[test]
    fn validate_retrieval_update_accepts_conversation_scoped_upsert() {
        let update = RetrievalUpdateProposal {
            update_id: Uuid::now_v7(),
            operation: RetrievalUpdateOperation::Upsert,
            source_ref: "background_job:test".to_string(),
            lexical_document: "retrieval maintenance summary".to_string(),
            relevance_timestamp: Utc::now(),
            internal_conversation_ref: Some("telegram-primary".to_string()),
            rationale: Some("refresh lexical retrieval projection".to_string()),
        };

        assert!(validate_retrieval_update(&update).is_ok());
    }

    #[test]
    fn validate_retrieval_update_rejects_upsert_without_conversation_scope() {
        let update = RetrievalUpdateProposal {
            update_id: Uuid::now_v7(),
            operation: RetrievalUpdateOperation::Upsert,
            source_ref: "background_job:test".to_string(),
            lexical_document: "retrieval maintenance summary".to_string(),
            relevance_timestamp: Utc::now(),
            internal_conversation_ref: None,
            rationale: None,
        };

        let error = validate_retrieval_update(&update).expect_err("missing scope should fail");
        assert!(
            error
                .to_string()
                .contains("requires an internal_conversation_ref")
        );
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
