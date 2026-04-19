use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewProposalRecord {
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub source_ingress_id: Option<Uuid>,
    pub source_loop_kind: String,
    pub proposal_kind: String,
    pub canonical_target: String,
    pub status: String,
    pub confidence: f64,
    pub conflict_posture: String,
    pub subject_ref: String,
    pub content_text: String,
    pub rationale: Option<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub supersedes_artifact_kind: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct ProposalRecord {
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub source_ingress_id: Option<Uuid>,
    pub source_loop_kind: String,
    pub proposal_kind: String,
    pub canonical_target: String,
    pub status: String,
    pub confidence: f64,
    pub conflict_posture: String,
    pub subject_ref: String,
    pub content_text: String,
    pub rationale: Option<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub supersedes_artifact_kind: Option<String>,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewMemoryArtifact {
    pub memory_artifact_id: Uuid,
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub source_ingress_id: Option<Uuid>,
    pub artifact_kind: String,
    pub subject_ref: String,
    pub content_text: String,
    pub confidence: f64,
    pub provenance_kind: String,
    pub status: String,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub superseded_by_artifact_id: Option<Uuid>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct MemoryArtifactRecord {
    pub memory_artifact_id: Uuid,
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub source_ingress_id: Option<Uuid>,
    pub artifact_kind: String,
    pub subject_ref: String,
    pub content_text: String,
    pub confidence: f64,
    pub provenance_kind: String,
    pub status: String,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub superseded_by_artifact_id: Option<Uuid>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewSelfModelArtifact {
    pub self_model_artifact_id: Uuid,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub episode_id: Option<Uuid>,
    pub artifact_origin: String,
    pub status: String,
    pub stable_identity: String,
    pub role: String,
    pub communication_style: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
    pub preferences: Vec<String>,
    pub current_goals: Vec<String>,
    pub current_subgoals: Vec<String>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub superseded_by_artifact_id: Option<Uuid>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct SelfModelArtifactRecord {
    pub self_model_artifact_id: Uuid,
    pub proposal_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub episode_id: Option<Uuid>,
    pub artifact_origin: String,
    pub status: String,
    pub stable_identity: String,
    pub role: String,
    pub communication_style: String,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
    pub preferences: Vec<String>,
    pub current_goals: Vec<String>,
    pub current_subgoals: Vec<String>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub superseded_by_artifact_id: Option<Uuid>,
    pub supersedes_artifact_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewRetrievalArtifact {
    pub retrieval_artifact_id: Uuid,
    pub source_kind: String,
    pub source_episode_id: Option<Uuid>,
    pub source_memory_artifact_id: Option<Uuid>,
    pub internal_conversation_ref: Option<String>,
    pub lexical_document: String,
    pub relevance_timestamp: DateTime<Utc>,
    pub status: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct RetrievalArtifactRecord {
    pub retrieval_artifact_id: Uuid,
    pub source_kind: String,
    pub source_episode_id: Option<Uuid>,
    pub source_memory_artifact_id: Option<Uuid>,
    pub internal_conversation_ref: Option<String>,
    pub lexical_document: String,
    pub relevance_timestamp: DateTime<Utc>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct NewMergeDecision {
    pub merge_decision_id: Uuid,
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub decision_kind: String,
    pub decision_reason: String,
    pub accepted_memory_artifact_id: Option<Uuid>,
    pub accepted_self_model_artifact_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct MergeDecisionRecord {
    pub merge_decision_id: Uuid,
    pub proposal_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub episode_id: Option<Uuid>,
    pub decision_kind: String,
    pub decision_reason: String,
    pub accepted_memory_artifact_id: Option<Uuid>,
    pub accepted_self_model_artifact_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub payload: Value,
}

pub async fn insert_proposal<'e, E>(executor: E, proposal: &NewProposalRecord) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO proposals (
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            source_loop_kind,
            proposal_kind,
            canonical_target,
            status,
            confidence,
            conflict_posture,
            subject_ref,
            content_text,
            rationale,
            valid_from,
            valid_to,
            supersedes_artifact_id,
            supersedes_artifact_kind,
            created_at,
            payload_json
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            $13,
            $14,
            $15,
            $16,
            $17,
            $18,
            NOW(),
            $19
        )
        "#,
    )
    .bind(proposal.proposal_id)
    .bind(proposal.trace_id)
    .bind(proposal.execution_id)
    .bind(proposal.episode_id)
    .bind(proposal.source_ingress_id)
    .bind(&proposal.source_loop_kind)
    .bind(&proposal.proposal_kind)
    .bind(&proposal.canonical_target)
    .bind(&proposal.status)
    .bind(proposal.confidence)
    .bind(&proposal.conflict_posture)
    .bind(&proposal.subject_ref)
    .bind(&proposal.content_text)
    .bind(&proposal.rationale)
    .bind(proposal.valid_from)
    .bind(proposal.valid_to)
    .bind(proposal.supersedes_artifact_id)
    .bind(&proposal.supersedes_artifact_kind)
    .bind(&proposal.payload)
    .execute(executor)
    .await
    .context("failed to insert proposal")?;
    Ok(())
}

pub async fn get_proposal(pool: &PgPool, proposal_id: Uuid) -> Result<ProposalRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            source_loop_kind,
            proposal_kind,
            canonical_target,
            status,
            confidence,
            conflict_posture,
            subject_ref,
            content_text,
            rationale,
            valid_from,
            valid_to,
            supersedes_artifact_id,
            supersedes_artifact_kind,
            created_at,
            payload_json
        FROM proposals
        WHERE proposal_id = $1
        "#,
    )
    .bind(proposal_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch proposal")?;

    Ok(decode_proposal_row(row))
}

pub async fn list_proposals_for_execution(
    pool: &PgPool,
    execution_id: Uuid,
) -> Result<Vec<ProposalRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            source_loop_kind,
            proposal_kind,
            canonical_target,
            status,
            confidence,
            conflict_posture,
            subject_ref,
            content_text,
            rationale,
            valid_from,
            valid_to,
            supersedes_artifact_id,
            supersedes_artifact_kind,
            created_at,
            payload_json
        FROM proposals
        WHERE execution_id = $1
        ORDER BY created_at, proposal_id
        "#,
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .context("failed to list proposals for execution")?;

    Ok(rows.into_iter().map(decode_proposal_row).collect())
}

pub async fn insert_memory_artifact<'e, E>(executor: E, artifact: &NewMemoryArtifact) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO memory_artifacts (
            memory_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            artifact_kind,
            subject_ref,
            content_text,
            confidence,
            provenance_kind,
            status,
            valid_from,
            valid_to,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            updated_at,
            payload_json
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            $13,
            $14,
            $15,
            $16,
            $17,
            NOW(),
            NOW(),
            $18
        )
        "#,
    )
    .bind(artifact.memory_artifact_id)
    .bind(artifact.proposal_id)
    .bind(artifact.trace_id)
    .bind(artifact.execution_id)
    .bind(artifact.episode_id)
    .bind(artifact.source_ingress_id)
    .bind(&artifact.artifact_kind)
    .bind(&artifact.subject_ref)
    .bind(&artifact.content_text)
    .bind(artifact.confidence)
    .bind(&artifact.provenance_kind)
    .bind(&artifact.status)
    .bind(artifact.valid_from)
    .bind(artifact.valid_to)
    .bind(artifact.superseded_at)
    .bind(artifact.superseded_by_artifact_id)
    .bind(artifact.supersedes_artifact_id)
    .bind(&artifact.payload)
    .execute(executor)
    .await
    .context("failed to insert memory artifact")?;
    Ok(())
}

pub async fn get_memory_artifact(
    pool: &PgPool,
    memory_artifact_id: Uuid,
) -> Result<MemoryArtifactRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            memory_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            artifact_kind,
            subject_ref,
            content_text,
            confidence,
            provenance_kind,
            status,
            valid_from,
            valid_to,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            updated_at,
            payload_json
        FROM memory_artifacts
        WHERE memory_artifact_id = $1
        "#,
    )
    .bind(memory_artifact_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch memory artifact")?;

    Ok(decode_memory_artifact_row(row))
}

pub async fn list_active_memory_artifacts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<MemoryArtifactRecord>> {
    list_memory_artifacts_by_status(pool, "active", limit).await
}

pub async fn list_active_memory_artifacts_by_subject(
    pool: &PgPool,
    subject_ref: &str,
    limit: i64,
) -> Result<Vec<MemoryArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            memory_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            artifact_kind,
            subject_ref,
            content_text,
            confidence,
            provenance_kind,
            status,
            valid_from,
            valid_to,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            updated_at,
            payload_json
        FROM memory_artifacts
        WHERE status = 'active'
          AND subject_ref = $1
        ORDER BY created_at DESC, memory_artifact_id DESC
        LIMIT $2
        "#,
    )
    .bind(subject_ref)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list active memory artifacts by subject")?;

    Ok(rows.into_iter().map(decode_memory_artifact_row).collect())
}

pub async fn list_superseded_memory_artifacts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<MemoryArtifactRecord>> {
    list_memory_artifacts_by_status(pool, "superseded", limit).await
}

async fn list_memory_artifacts_by_status(
    pool: &PgPool,
    status: &str,
    limit: i64,
) -> Result<Vec<MemoryArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            memory_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            source_ingress_id,
            artifact_kind,
            subject_ref,
            content_text,
            confidence,
            provenance_kind,
            status,
            valid_from,
            valid_to,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            updated_at,
            payload_json
        FROM memory_artifacts
        WHERE status = $1
        ORDER BY created_at DESC, memory_artifact_id DESC
        LIMIT $2
        "#,
    )
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await
    .with_context(|| format!("failed to list {status} memory artifacts"))?;

    Ok(rows.into_iter().map(decode_memory_artifact_row).collect())
}

pub async fn insert_self_model_artifact<'e, E>(
    executor: E,
    artifact: &NewSelfModelArtifact,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO self_model_artifacts (
            self_model_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            artifact_origin,
            status,
            stable_identity,
            role,
            communication_style,
            capabilities_json,
            constraints_json,
            preferences_json,
            current_goals_json,
            current_subgoals_json,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            payload_json
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            $13,
            $14,
            $15,
            $16,
            $17,
            $18,
            NOW(),
            $19
        )
        "#,
    )
    .bind(artifact.self_model_artifact_id)
    .bind(artifact.proposal_id)
    .bind(artifact.trace_id)
    .bind(artifact.execution_id)
    .bind(artifact.episode_id)
    .bind(&artifact.artifact_origin)
    .bind(&artifact.status)
    .bind(&artifact.stable_identity)
    .bind(&artifact.role)
    .bind(&artifact.communication_style)
    .bind(json_array(&artifact.capabilities))
    .bind(json_array(&artifact.constraints))
    .bind(json_array(&artifact.preferences))
    .bind(json_array(&artifact.current_goals))
    .bind(json_array(&artifact.current_subgoals))
    .bind(artifact.superseded_at)
    .bind(artifact.superseded_by_artifact_id)
    .bind(artifact.supersedes_artifact_id)
    .bind(&artifact.payload)
    .execute(executor)
    .await
    .context("failed to insert self-model artifact")?;
    Ok(())
}

pub async fn get_latest_active_self_model_artifact(
    pool: &PgPool,
) -> Result<Option<SelfModelArtifactRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            self_model_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            artifact_origin,
            status,
            stable_identity,
            role,
            communication_style,
            capabilities_json,
            constraints_json,
            preferences_json,
            current_goals_json,
            current_subgoals_json,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            payload_json
        FROM self_model_artifacts
        WHERE status = 'active'
        ORDER BY created_at DESC, self_model_artifact_id DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch latest active self-model artifact")?;

    row.map(decode_self_model_artifact_row).transpose()
}

pub async fn list_active_self_model_artifacts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SelfModelArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            self_model_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            artifact_origin,
            status,
            stable_identity,
            role,
            communication_style,
            capabilities_json,
            constraints_json,
            preferences_json,
            current_goals_json,
            current_subgoals_json,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            payload_json
        FROM self_model_artifacts
        WHERE status = 'active'
        ORDER BY created_at DESC, self_model_artifact_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list active self-model artifacts")?;

    rows.into_iter()
        .map(decode_self_model_artifact_row)
        .collect()
}

pub async fn list_superseded_self_model_artifacts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<SelfModelArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            self_model_artifact_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            artifact_origin,
            status,
            stable_identity,
            role,
            communication_style,
            capabilities_json,
            constraints_json,
            preferences_json,
            current_goals_json,
            current_subgoals_json,
            superseded_at,
            superseded_by_artifact_id,
            supersedes_artifact_id,
            created_at,
            payload_json
        FROM self_model_artifacts
        WHERE status = 'superseded'
        ORDER BY created_at DESC, self_model_artifact_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list superseded self-model artifacts")?;

    rows.into_iter()
        .map(decode_self_model_artifact_row)
        .collect()
}

pub async fn insert_retrieval_artifact<'e, E>(
    executor: E,
    artifact: &NewRetrievalArtifact,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO retrieval_artifacts (
            retrieval_artifact_id,
            source_kind,
            source_episode_id,
            source_memory_artifact_id,
            internal_conversation_ref,
            lexical_document,
            relevance_timestamp,
            status,
            created_at,
            payload_json
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            NOW(),
            $9
        )
        "#,
    )
    .bind(artifact.retrieval_artifact_id)
    .bind(&artifact.source_kind)
    .bind(artifact.source_episode_id)
    .bind(artifact.source_memory_artifact_id)
    .bind(&artifact.internal_conversation_ref)
    .bind(&artifact.lexical_document)
    .bind(artifact.relevance_timestamp)
    .bind(&artifact.status)
    .bind(&artifact.payload)
    .execute(executor)
    .await
    .context("failed to insert retrieval artifact")?;
    Ok(())
}

pub async fn list_active_retrieval_artifacts_for_conversation(
    pool: &PgPool,
    internal_conversation_ref: &str,
    limit: i64,
) -> Result<Vec<RetrievalArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            retrieval_artifact_id,
            source_kind,
            source_episode_id,
            source_memory_artifact_id,
            internal_conversation_ref,
            lexical_document,
            relevance_timestamp,
            status,
            created_at,
            payload_json
        FROM retrieval_artifacts
        WHERE status = 'active'
          AND internal_conversation_ref = $1
        ORDER BY relevance_timestamp DESC, retrieval_artifact_id DESC
        LIMIT $2
        "#,
    )
    .bind(internal_conversation_ref)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list retrieval artifacts for conversation")?;

    Ok(rows
        .into_iter()
        .map(decode_retrieval_artifact_row)
        .collect())
}

pub async fn insert_merge_decision<'e, E>(executor: E, decision: &NewMergeDecision) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO merge_decisions (
            merge_decision_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            decision_kind,
            decision_reason,
            accepted_memory_artifact_id,
            accepted_self_model_artifact_id,
            created_at,
            payload_json
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            NOW(),
            $10
        )
        "#,
    )
    .bind(decision.merge_decision_id)
    .bind(decision.proposal_id)
    .bind(decision.trace_id)
    .bind(decision.execution_id)
    .bind(decision.episode_id)
    .bind(&decision.decision_kind)
    .bind(&decision.decision_reason)
    .bind(decision.accepted_memory_artifact_id)
    .bind(decision.accepted_self_model_artifact_id)
    .bind(&decision.payload)
    .execute(executor)
    .await
    .context("failed to insert merge decision")?;
    Ok(())
}

pub async fn get_merge_decision_by_proposal(
    pool: &PgPool,
    proposal_id: Uuid,
) -> Result<Option<MergeDecisionRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            merge_decision_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            decision_kind,
            decision_reason,
            accepted_memory_artifact_id,
            accepted_self_model_artifact_id,
            created_at,
            payload_json
        FROM merge_decisions
        WHERE proposal_id = $1
        "#,
    )
    .bind(proposal_id)
    .fetch_optional(pool)
    .await
    .context("failed to fetch merge decision by proposal")?;

    row.map(decode_merge_decision_row).transpose()
}

pub async fn list_merge_decisions_for_execution(
    pool: &PgPool,
    execution_id: Uuid,
) -> Result<Vec<MergeDecisionRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            merge_decision_id,
            proposal_id,
            trace_id,
            execution_id,
            episode_id,
            decision_kind,
            decision_reason,
            accepted_memory_artifact_id,
            accepted_self_model_artifact_id,
            created_at,
            payload_json
        FROM merge_decisions
        WHERE execution_id = $1
        ORDER BY created_at, merge_decision_id
        "#,
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .context("failed to list merge decisions for execution")?;

    rows.into_iter().map(decode_merge_decision_row).collect()
}

pub async fn update_merge_decision_targets(
    pool: &PgPool,
    proposal_id: Uuid,
    accepted_memory_artifact_id: Option<Uuid>,
    accepted_self_model_artifact_id: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE merge_decisions
        SET
            accepted_memory_artifact_id = $2,
            accepted_self_model_artifact_id = $3
        WHERE proposal_id = $1
        "#,
    )
    .bind(proposal_id)
    .bind(accepted_memory_artifact_id)
    .bind(accepted_self_model_artifact_id)
    .execute(pool)
    .await
    .context("failed to update merge decision targets")?;
    Ok(())
}

pub async fn update_merge_decision_outcome(
    pool: &PgPool,
    proposal_id: Uuid,
    decision_kind: &str,
    decision_reason: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE merge_decisions
        SET
            decision_kind = $2,
            decision_reason = $3,
            accepted_memory_artifact_id = NULL,
            accepted_self_model_artifact_id = NULL
        WHERE proposal_id = $1
        "#,
    )
    .bind(proposal_id)
    .bind(decision_kind)
    .bind(decision_reason)
    .execute(pool)
    .await
    .context("failed to update merge decision outcome")?;
    Ok(())
}

pub async fn update_merge_decision_targets_in_tx<'e, E>(
    executor: E,
    proposal_id: Uuid,
    accepted_memory_artifact_id: Option<Uuid>,
    accepted_self_model_artifact_id: Option<Uuid>,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE merge_decisions
        SET
            accepted_memory_artifact_id = $2,
            accepted_self_model_artifact_id = $3
        WHERE proposal_id = $1
        "#,
    )
    .bind(proposal_id)
    .bind(accepted_memory_artifact_id)
    .bind(accepted_self_model_artifact_id)
    .execute(executor)
    .await
    .context("failed to update merge decision targets in transaction")?;
    Ok(())
}

pub async fn mark_memory_artifact_superseded<'e, E>(
    executor: E,
    memory_artifact_id: Uuid,
    superseded_by_artifact_id: Uuid,
    superseded_at: DateTime<Utc>,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE memory_artifacts
        SET
            status = 'superseded',
            superseded_at = $2,
            superseded_by_artifact_id = $3,
            updated_at = NOW()
        WHERE memory_artifact_id = $1
        "#,
    )
    .bind(memory_artifact_id)
    .bind(superseded_at)
    .bind(superseded_by_artifact_id)
    .execute(executor)
    .await
    .context("failed to mark memory artifact as superseded")?;
    Ok(())
}

pub async fn mark_self_model_artifact_superseded<'e, E>(
    executor: E,
    self_model_artifact_id: Uuid,
    superseded_by_artifact_id: Uuid,
    superseded_at: DateTime<Utc>,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE self_model_artifacts
        SET
            status = 'superseded',
            superseded_at = $2,
            superseded_by_artifact_id = $3
        WHERE self_model_artifact_id = $1
        "#,
    )
    .bind(self_model_artifact_id)
    .bind(superseded_at)
    .bind(superseded_by_artifact_id)
    .execute(executor)
    .await
    .context("failed to mark self-model artifact as superseded")?;
    Ok(())
}

fn decode_proposal_row(row: sqlx::postgres::PgRow) -> ProposalRecord {
    ProposalRecord {
        proposal_id: row.get("proposal_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        episode_id: row.get("episode_id"),
        source_ingress_id: row.get("source_ingress_id"),
        source_loop_kind: row.get("source_loop_kind"),
        proposal_kind: row.get("proposal_kind"),
        canonical_target: row.get("canonical_target"),
        status: row.get("status"),
        confidence: row.get("confidence"),
        conflict_posture: row.get("conflict_posture"),
        subject_ref: row.get("subject_ref"),
        content_text: row.get("content_text"),
        rationale: row.get("rationale"),
        valid_from: row.get("valid_from"),
        valid_to: row.get("valid_to"),
        supersedes_artifact_id: row.get("supersedes_artifact_id"),
        supersedes_artifact_kind: row.get("supersedes_artifact_kind"),
        created_at: row.get("created_at"),
        payload: row.get("payload_json"),
    }
}

fn decode_memory_artifact_row(row: sqlx::postgres::PgRow) -> MemoryArtifactRecord {
    MemoryArtifactRecord {
        memory_artifact_id: row.get("memory_artifact_id"),
        proposal_id: row.get("proposal_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        episode_id: row.get("episode_id"),
        source_ingress_id: row.get("source_ingress_id"),
        artifact_kind: row.get("artifact_kind"),
        subject_ref: row.get("subject_ref"),
        content_text: row.get("content_text"),
        confidence: row.get("confidence"),
        provenance_kind: row.get("provenance_kind"),
        status: row.get("status"),
        valid_from: row.get("valid_from"),
        valid_to: row.get("valid_to"),
        superseded_at: row.get("superseded_at"),
        superseded_by_artifact_id: row.get("superseded_by_artifact_id"),
        supersedes_artifact_id: row.get("supersedes_artifact_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        payload: row.get("payload_json"),
    }
}

fn decode_self_model_artifact_row(row: sqlx::postgres::PgRow) -> Result<SelfModelArtifactRecord> {
    Ok(SelfModelArtifactRecord {
        self_model_artifact_id: row.get("self_model_artifact_id"),
        proposal_id: row.get("proposal_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        episode_id: row.get("episode_id"),
        artifact_origin: row.get("artifact_origin"),
        status: row.get("status"),
        stable_identity: row.get("stable_identity"),
        role: row.get("role"),
        communication_style: row.get("communication_style"),
        capabilities: decode_string_vec(row.get("capabilities_json"), "capabilities_json")?,
        constraints: decode_string_vec(row.get("constraints_json"), "constraints_json")?,
        preferences: decode_string_vec(row.get("preferences_json"), "preferences_json")?,
        current_goals: decode_string_vec(row.get("current_goals_json"), "current_goals_json")?,
        current_subgoals: decode_string_vec(
            row.get("current_subgoals_json"),
            "current_subgoals_json",
        )?,
        superseded_at: row.get("superseded_at"),
        superseded_by_artifact_id: row.get("superseded_by_artifact_id"),
        supersedes_artifact_id: row.get("supersedes_artifact_id"),
        created_at: row.get("created_at"),
        payload: row.get("payload_json"),
    })
}

fn decode_retrieval_artifact_row(row: sqlx::postgres::PgRow) -> RetrievalArtifactRecord {
    RetrievalArtifactRecord {
        retrieval_artifact_id: row.get("retrieval_artifact_id"),
        source_kind: row.get("source_kind"),
        source_episode_id: row.get("source_episode_id"),
        source_memory_artifact_id: row.get("source_memory_artifact_id"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        lexical_document: row.get("lexical_document"),
        relevance_timestamp: row.get("relevance_timestamp"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        payload: row.get("payload_json"),
    }
}

fn decode_merge_decision_row(row: sqlx::postgres::PgRow) -> Result<MergeDecisionRecord> {
    Ok(MergeDecisionRecord {
        merge_decision_id: row.get("merge_decision_id"),
        proposal_id: row.get("proposal_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        episode_id: row.get("episode_id"),
        decision_kind: row.get("decision_kind"),
        decision_reason: row.get("decision_reason"),
        accepted_memory_artifact_id: row.get("accepted_memory_artifact_id"),
        accepted_self_model_artifact_id: row.get("accepted_self_model_artifact_id"),
        created_at: row.get("created_at"),
        payload: row.get("payload_json"),
    })
}

fn json_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    )
}

fn decode_string_vec(value: Value, field_name: &str) -> Result<Vec<String>> {
    serde_json::from_value(value)
        .with_context(|| format!("failed to decode persisted {field_name} field"))
}
