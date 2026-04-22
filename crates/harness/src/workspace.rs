use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::{
    WorkspaceArtifactKind, WorkspaceArtifactSummary, WorkspaceScriptRunStatus,
    WorkspaceScriptRunSummary, WorkspaceScriptSummary, WorkspaceScriptVersionSummary,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::RuntimeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceArtifactStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone)]
pub struct NewWorkspaceArtifact {
    pub workspace_artifact_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub artifact_kind: WorkspaceArtifactKind,
    pub title: String,
    pub content_text: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct UpdateWorkspaceArtifact {
    pub workspace_artifact_id: Uuid,
    pub title: String,
    pub content_text: Option<String>,
    pub status: WorkspaceArtifactStatus,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct WorkspaceArtifactRecord {
    pub workspace_artifact_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub artifact_kind: WorkspaceArtifactKind,
    pub title: String,
    pub content_text: Option<String>,
    pub status: WorkspaceArtifactStatus,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewWorkspaceScript {
    pub workspace_script_id: Uuid,
    pub workspace_artifact_id: Uuid,
    pub workspace_script_version_id: Uuid,
    pub trace_id: Option<Uuid>,
    pub execution_id: Option<Uuid>,
    pub title: String,
    pub metadata: Value,
    pub language: String,
    pub entrypoint: Option<String>,
    pub content_text: String,
    pub change_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewWorkspaceScriptVersion {
    pub workspace_script_version_id: Uuid,
    pub workspace_script_id: Uuid,
    pub content_text: String,
    pub change_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceScriptRecord {
    pub workspace_script_id: Uuid,
    pub workspace_artifact_id: Uuid,
    pub language: String,
    pub entrypoint: Option<String>,
    pub latest_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceScriptVersionRecord {
    pub workspace_script_version_id: Uuid,
    pub workspace_script_id: Uuid,
    pub version: u32,
    pub content_text: String,
    pub content_sha256: String,
    pub change_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceScriptBundle {
    pub artifact: WorkspaceArtifactRecord,
    pub script: WorkspaceScriptRecord,
    pub initial_version: WorkspaceScriptVersionRecord,
}

#[derive(Debug, Clone)]
pub struct NewWorkspaceScriptRun {
    pub workspace_script_run_id: Uuid,
    pub workspace_script_id: Uuid,
    pub workspace_script_version_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub status: WorkspaceScriptRunStatus,
    pub risk_tier: contracts::GovernedActionRiskTier,
    pub args: Vec<String>,
    pub output_ref: Option<String>,
    pub failure_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct UpdateWorkspaceScriptRunStatus {
    pub workspace_script_run_id: Uuid,
    pub status: WorkspaceScriptRunStatus,
    pub output_ref: Option<String>,
    pub failure_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceScriptRunRecord {
    pub workspace_script_run_id: Uuid,
    pub workspace_script_id: Uuid,
    pub workspace_script_version_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub governed_action_execution_id: Option<Uuid>,
    pub approval_request_id: Option<Uuid>,
    pub status: WorkspaceScriptRunStatus,
    pub risk_tier: contracts::GovernedActionRiskTier,
    pub args: Vec<String>,
    pub output_ref: Option<String>,
    pub failure_summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create_workspace_artifact(
    config: &RuntimeConfig,
    pool: &PgPool,
    artifact: &NewWorkspaceArtifact,
) -> Result<WorkspaceArtifactRecord> {
    if artifact.artifact_kind == WorkspaceArtifactKind::Script {
        bail!("script artifacts must be created through create_workspace_script");
    }
    validate_artifact_title(&artifact.title)?;
    validate_artifact_content(config, artifact.content_text.as_deref())?;

    sqlx::query(
        r#"
        INSERT INTO workspace_artifacts (
            workspace_artifact_id,
            trace_id,
            execution_id,
            artifact_kind,
            title,
            content_text,
            status,
            metadata_json,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            'active',
            $7,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(artifact.workspace_artifact_id)
    .bind(artifact.trace_id)
    .bind(artifact.execution_id)
    .bind(workspace_artifact_kind_as_str(artifact.artifact_kind))
    .bind(&artifact.title)
    .bind(&artifact.content_text)
    .bind(&artifact.metadata)
    .execute(pool)
    .await
    .context("failed to insert workspace artifact")?;

    get_workspace_artifact(pool, artifact.workspace_artifact_id).await
}

pub async fn update_workspace_artifact(
    config: &RuntimeConfig,
    pool: &PgPool,
    update: &UpdateWorkspaceArtifact,
) -> Result<WorkspaceArtifactRecord> {
    validate_artifact_title(&update.title)?;
    validate_artifact_content(config, update.content_text.as_deref())?;

    let result = sqlx::query(
        r#"
        UPDATE workspace_artifacts
        SET
            title = $2,
            content_text = $3,
            status = $4,
            metadata_json = $5,
            updated_at = NOW()
        WHERE workspace_artifact_id = $1
        "#,
    )
    .bind(update.workspace_artifact_id)
    .bind(&update.title)
    .bind(&update.content_text)
    .bind(workspace_artifact_status_as_str(update.status))
    .bind(&update.metadata)
    .execute(pool)
    .await
    .context("failed to update workspace artifact")?;

    if result.rows_affected() == 0 {
        bail!(
            "workspace artifact '{}' was not found",
            update.workspace_artifact_id
        );
    }

    get_workspace_artifact(pool, update.workspace_artifact_id).await
}

pub async fn get_workspace_artifact(
    pool: &PgPool,
    workspace_artifact_id: Uuid,
) -> Result<WorkspaceArtifactRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            workspace_artifact_id,
            trace_id,
            execution_id,
            artifact_kind,
            title,
            content_text,
            status,
            metadata_json,
            created_at,
            updated_at
        FROM workspace_artifacts
        WHERE workspace_artifact_id = $1
        "#,
    )
    .bind(workspace_artifact_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch workspace artifact")?;

    decode_workspace_artifact_row(row)
}

pub async fn list_workspace_artifacts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<WorkspaceArtifactRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            workspace_artifact_id,
            trace_id,
            execution_id,
            artifact_kind,
            title,
            content_text,
            status,
            metadata_json,
            created_at,
            updated_at
        FROM workspace_artifacts
        ORDER BY updated_at DESC, workspace_artifact_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list workspace artifacts")?;

    rows.into_iter()
        .map(decode_workspace_artifact_row)
        .collect()
}

pub async fn list_workspace_artifact_summaries(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<WorkspaceArtifactSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            artifact.workspace_artifact_id,
            artifact.artifact_kind,
            artifact.title,
            COALESCE(script.latest_version, 1) AS latest_version,
            artifact.updated_at
        FROM workspace_artifacts artifact
        LEFT JOIN workspace_scripts script
            ON script.workspace_artifact_id = artifact.workspace_artifact_id
        ORDER BY artifact.updated_at DESC, artifact.workspace_artifact_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list workspace artifact summaries")?;

    rows.into_iter()
        .map(|row| {
            Ok(WorkspaceArtifactSummary {
                artifact_id: row.get("workspace_artifact_id"),
                artifact_kind: parse_workspace_artifact_kind(row.get("artifact_kind"))?,
                title: row.get("title"),
                latest_version: row.get::<i32, _>("latest_version") as u32,
                updated_at: row.get("updated_at"),
            })
        })
        .collect()
}

pub async fn create_workspace_script(
    config: &RuntimeConfig,
    pool: &PgPool,
    script: &NewWorkspaceScript,
) -> Result<WorkspaceScriptBundle> {
    validate_artifact_title(&script.title)?;
    validate_script_content(config, &script.content_text)?;

    let content_sha256 = sha256_hex(&script.content_text);
    let mut tx = pool
        .begin()
        .await
        .context("failed to start workspace script transaction")?;

    sqlx::query(
        r#"
        INSERT INTO workspace_artifacts (
            workspace_artifact_id,
            trace_id,
            execution_id,
            artifact_kind,
            title,
            content_text,
            status,
            metadata_json,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            'script',
            $4,
            NULL,
            'active',
            $5,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(script.workspace_artifact_id)
    .bind(script.trace_id)
    .bind(script.execution_id)
    .bind(&script.title)
    .bind(&script.metadata)
    .execute(&mut *tx)
    .await
    .context("failed to insert workspace script artifact")?;

    sqlx::query(
        r#"
        INSERT INTO workspace_scripts (
            workspace_script_id,
            workspace_artifact_id,
            language,
            entrypoint,
            latest_version,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            1,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(script.workspace_script_id)
    .bind(script.workspace_artifact_id)
    .bind(&script.language)
    .bind(&script.entrypoint)
    .execute(&mut *tx)
    .await
    .context("failed to insert workspace script")?;

    sqlx::query(
        r#"
        INSERT INTO workspace_script_versions (
            workspace_script_version_id,
            workspace_script_id,
            version,
            content_text,
            content_sha256,
            change_summary,
            created_at
        ) VALUES (
            $1,
            $2,
            1,
            $3,
            $4,
            $5,
            NOW()
        )
        "#,
    )
    .bind(script.workspace_script_version_id)
    .bind(script.workspace_script_id)
    .bind(&script.content_text)
    .bind(&content_sha256)
    .bind(&script.change_summary)
    .execute(&mut *tx)
    .await
    .context("failed to insert initial workspace script version")?;

    tx.commit()
        .await
        .context("failed to commit workspace script creation transaction")?;

    Ok(WorkspaceScriptBundle {
        artifact: get_workspace_artifact(pool, script.workspace_artifact_id).await?,
        script: get_workspace_script(pool, script.workspace_script_id).await?,
        initial_version: get_workspace_script_version(pool, script.workspace_script_version_id)
            .await?,
    })
}

pub async fn append_workspace_script_version(
    config: &RuntimeConfig,
    pool: &PgPool,
    version: &NewWorkspaceScriptVersion,
) -> Result<WorkspaceScriptVersionRecord> {
    validate_script_content(config, &version.content_text)?;
    let content_sha256 = sha256_hex(&version.content_text);
    let mut tx = pool
        .begin()
        .await
        .context("failed to start workspace script version transaction")?;

    let row = sqlx::query(
        r#"
        SELECT workspace_artifact_id, latest_version
        FROM workspace_scripts
        WHERE workspace_script_id = $1
        FOR UPDATE
        "#,
    )
    .bind(version.workspace_script_id)
    .fetch_one(&mut *tx)
    .await
    .context("failed to lock workspace script for version append")?;

    let workspace_artifact_id: Uuid = row.get("workspace_artifact_id");
    let next_version = row.get::<i32, _>("latest_version") + 1;

    sqlx::query(
        r#"
        INSERT INTO workspace_script_versions (
            workspace_script_version_id,
            workspace_script_id,
            version,
            content_text,
            content_sha256,
            change_summary,
            created_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            NOW()
        )
        "#,
    )
    .bind(version.workspace_script_version_id)
    .bind(version.workspace_script_id)
    .bind(next_version)
    .bind(&version.content_text)
    .bind(&content_sha256)
    .bind(&version.change_summary)
    .execute(&mut *tx)
    .await
    .context("failed to insert workspace script version")?;

    sqlx::query(
        r#"
        UPDATE workspace_scripts
        SET
            latest_version = $2,
            updated_at = NOW()
        WHERE workspace_script_id = $1
        "#,
    )
    .bind(version.workspace_script_id)
    .bind(next_version)
    .execute(&mut *tx)
    .await
    .context("failed to update workspace script latest version")?;

    sqlx::query(
        r#"
        UPDATE workspace_artifacts
        SET updated_at = NOW()
        WHERE workspace_artifact_id = $1
        "#,
    )
    .bind(workspace_artifact_id)
    .execute(&mut *tx)
    .await
    .context("failed to update workspace script artifact timestamp")?;

    tx.commit()
        .await
        .context("failed to commit workspace script version transaction")?;

    get_workspace_script_version(pool, version.workspace_script_version_id).await
}

pub async fn get_workspace_script(
    pool: &PgPool,
    workspace_script_id: Uuid,
) -> Result<WorkspaceScriptRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            workspace_script_id,
            workspace_artifact_id,
            language,
            entrypoint,
            latest_version,
            created_at,
            updated_at
        FROM workspace_scripts
        WHERE workspace_script_id = $1
        "#,
    )
    .bind(workspace_script_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch workspace script")?;

    Ok(decode_workspace_script_row(row))
}

pub async fn list_workspace_scripts(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<WorkspaceScriptSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            workspace_script_id,
            workspace_artifact_id,
            language,
            latest_version,
            updated_at
        FROM workspace_scripts
        ORDER BY updated_at DESC, workspace_script_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list workspace scripts")?;

    Ok(rows
        .into_iter()
        .map(|row| WorkspaceScriptSummary {
            script_id: row.get("workspace_script_id"),
            workspace_artifact_id: row.get("workspace_artifact_id"),
            language: row.get("language"),
            latest_version: row.get::<i32, _>("latest_version") as u32,
            updated_at: row.get("updated_at"),
        })
        .collect())
}

pub async fn get_workspace_script_version(
    pool: &PgPool,
    workspace_script_version_id: Uuid,
) -> Result<WorkspaceScriptVersionRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            workspace_script_version_id,
            workspace_script_id,
            version,
            content_text,
            content_sha256,
            change_summary,
            created_at
        FROM workspace_script_versions
        WHERE workspace_script_version_id = $1
        "#,
    )
    .bind(workspace_script_version_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch workspace script version")?;

    Ok(decode_workspace_script_version_row(row))
}

pub async fn get_latest_workspace_script_version(
    pool: &PgPool,
    workspace_script_id: Uuid,
) -> Result<Option<WorkspaceScriptVersionRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            workspace_script_version_id,
            workspace_script_id,
            version,
            content_text,
            content_sha256,
            change_summary,
            created_at
        FROM workspace_script_versions
        WHERE workspace_script_id = $1
        ORDER BY version DESC
        LIMIT 1
        "#,
    )
    .bind(workspace_script_id)
    .fetch_optional(pool)
    .await
    .context("failed to fetch latest workspace script version")?;

    Ok(row.map(decode_workspace_script_version_row))
}

pub async fn list_workspace_script_versions(
    pool: &PgPool,
    workspace_script_id: Uuid,
    limit: i64,
) -> Result<Vec<WorkspaceScriptVersionSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            workspace_script_version_id,
            workspace_script_id,
            version,
            content_sha256,
            created_at
        FROM workspace_script_versions
        WHERE workspace_script_id = $1
        ORDER BY version DESC
        LIMIT $2
        "#,
    )
    .bind(workspace_script_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list workspace script versions")?;

    Ok(rows
        .into_iter()
        .map(|row| WorkspaceScriptVersionSummary {
            script_version_id: row.get("workspace_script_version_id"),
            script_id: row.get("workspace_script_id"),
            version: row.get::<i32, _>("version") as u32,
            content_sha256: row.get("content_sha256"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn record_workspace_script_run(
    pool: &PgPool,
    run: &NewWorkspaceScriptRun,
) -> Result<WorkspaceScriptRunRecord> {
    sqlx::query(
        r#"
        INSERT INTO workspace_script_runs (
            workspace_script_run_id,
            workspace_script_id,
            workspace_script_version_id,
            trace_id,
            execution_id,
            governed_action_execution_id,
            approval_request_id,
            status,
            risk_tier,
            args_json,
            output_ref,
            failure_summary,
            started_at,
            completed_at,
            created_at,
            updated_at
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
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(run.workspace_script_run_id)
    .bind(run.workspace_script_id)
    .bind(run.workspace_script_version_id)
    .bind(run.trace_id)
    .bind(run.execution_id)
    .bind(run.governed_action_execution_id)
    .bind(run.approval_request_id)
    .bind(workspace_script_run_status_as_str(run.status))
    .bind(governed_action_risk_tier_as_str(run.risk_tier))
    .bind(json_string_array(&run.args))
    .bind(&run.output_ref)
    .bind(&run.failure_summary)
    .bind(run.started_at)
    .bind(run.completed_at)
    .execute(pool)
    .await
    .context("failed to insert workspace script run")?;

    get_workspace_script_run(pool, run.workspace_script_run_id).await
}

pub async fn update_workspace_script_run_status(
    pool: &PgPool,
    update: &UpdateWorkspaceScriptRunStatus,
) -> Result<WorkspaceScriptRunRecord> {
    let result = sqlx::query(
        r#"
        UPDATE workspace_script_runs
        SET
            status = $2,
            output_ref = $3,
            failure_summary = $4,
            started_at = $5,
            completed_at = $6,
            updated_at = NOW()
        WHERE workspace_script_run_id = $1
        "#,
    )
    .bind(update.workspace_script_run_id)
    .bind(workspace_script_run_status_as_str(update.status))
    .bind(&update.output_ref)
    .bind(&update.failure_summary)
    .bind(update.started_at)
    .bind(update.completed_at)
    .execute(pool)
    .await
    .context("failed to update workspace script run")?;

    if result.rows_affected() == 0 {
        bail!(
            "workspace script run '{}' was not found",
            update.workspace_script_run_id
        );
    }

    get_workspace_script_run(pool, update.workspace_script_run_id).await
}

pub async fn get_workspace_script_run(
    pool: &PgPool,
    workspace_script_run_id: Uuid,
) -> Result<WorkspaceScriptRunRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            workspace_script_run_id,
            workspace_script_id,
            workspace_script_version_id,
            trace_id,
            execution_id,
            governed_action_execution_id,
            approval_request_id,
            status,
            risk_tier,
            args_json,
            output_ref,
            failure_summary,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM workspace_script_runs
        WHERE workspace_script_run_id = $1
        "#,
    )
    .bind(workspace_script_run_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch workspace script run")?;

    decode_workspace_script_run_row(row)
}

pub async fn list_workspace_script_runs(
    pool: &PgPool,
    workspace_script_id: Uuid,
    limit: i64,
) -> Result<Vec<WorkspaceScriptRunSummary>> {
    let rows = sqlx::query(
        r#"
        SELECT
            workspace_script_run_id,
            workspace_script_id,
            workspace_script_version_id,
            status,
            risk_tier,
            started_at,
            completed_at
        FROM workspace_script_runs
        WHERE workspace_script_id = $1
        ORDER BY created_at DESC, workspace_script_run_id DESC
        LIMIT $2
        "#,
    )
    .bind(workspace_script_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to list workspace script runs")?;

    rows.into_iter()
        .map(|row| {
            Ok(WorkspaceScriptRunSummary {
                script_run_id: row.get("workspace_script_run_id"),
                script_id: row.get("workspace_script_id"),
                script_version_id: row.get("workspace_script_version_id"),
                status: parse_workspace_script_run_status(row.get("status"))?,
                risk_tier: parse_governed_action_risk_tier(row.get("risk_tier"))?,
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
            })
        })
        .collect()
}

fn decode_workspace_artifact_row(row: sqlx::postgres::PgRow) -> Result<WorkspaceArtifactRecord> {
    Ok(WorkspaceArtifactRecord {
        workspace_artifact_id: row.get("workspace_artifact_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        artifact_kind: parse_workspace_artifact_kind(row.get("artifact_kind"))?,
        title: row.get("title"),
        content_text: row.get("content_text"),
        status: parse_workspace_artifact_status(row.get("status"))?,
        metadata: row.get("metadata_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn decode_workspace_script_row(row: sqlx::postgres::PgRow) -> WorkspaceScriptRecord {
    WorkspaceScriptRecord {
        workspace_script_id: row.get("workspace_script_id"),
        workspace_artifact_id: row.get("workspace_artifact_id"),
        language: row.get("language"),
        entrypoint: row.get("entrypoint"),
        latest_version: row.get::<i32, _>("latest_version") as u32,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn decode_workspace_script_version_row(row: sqlx::postgres::PgRow) -> WorkspaceScriptVersionRecord {
    WorkspaceScriptVersionRecord {
        workspace_script_version_id: row.get("workspace_script_version_id"),
        workspace_script_id: row.get("workspace_script_id"),
        version: row.get::<i32, _>("version") as u32,
        content_text: row.get("content_text"),
        content_sha256: row.get("content_sha256"),
        change_summary: row.get("change_summary"),
        created_at: row.get("created_at"),
    }
}

fn decode_workspace_script_run_row(row: sqlx::postgres::PgRow) -> Result<WorkspaceScriptRunRecord> {
    Ok(WorkspaceScriptRunRecord {
        workspace_script_run_id: row.get("workspace_script_run_id"),
        workspace_script_id: row.get("workspace_script_id"),
        workspace_script_version_id: row.get("workspace_script_version_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        governed_action_execution_id: row.get("governed_action_execution_id"),
        approval_request_id: row.get("approval_request_id"),
        status: parse_workspace_script_run_status(row.get("status"))?,
        risk_tier: parse_governed_action_risk_tier(row.get("risk_tier"))?,
        args: decode_string_vec(row.get("args_json"), "args_json")?,
        output_ref: row.get("output_ref"),
        failure_summary: row.get("failure_summary"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn validate_artifact_title(title: &str) -> Result<()> {
    if title.trim().is_empty() {
        bail!("workspace artifact title must not be empty");
    }
    Ok(())
}

fn validate_artifact_content(config: &RuntimeConfig, content_text: Option<&str>) -> Result<()> {
    if let Some(content_text) = content_text {
        let size_bytes = content_text.len() as u64;
        if size_bytes > config.workspace.max_artifact_bytes {
            bail!(
                "workspace artifact content exceeds configured limit of {} bytes",
                config.workspace.max_artifact_bytes
            );
        }
    }
    Ok(())
}

fn validate_script_content(config: &RuntimeConfig, content_text: &str) -> Result<()> {
    let size_bytes = content_text.len() as u64;
    if size_bytes > config.workspace.max_script_bytes {
        bail!(
            "workspace script content exceeds configured limit of {} bytes",
            config.workspace.max_script_bytes
        );
    }
    Ok(())
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn json_string_array(values: &[String]) -> Value {
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

fn workspace_artifact_kind_as_str(kind: WorkspaceArtifactKind) -> &'static str {
    match kind {
        WorkspaceArtifactKind::Note => "note",
        WorkspaceArtifactKind::Runbook => "runbook",
        WorkspaceArtifactKind::Scratchpad => "scratchpad",
        WorkspaceArtifactKind::TaskList => "task_list",
        WorkspaceArtifactKind::Script => "script",
    }
}

fn parse_workspace_artifact_kind(value: &str) -> Result<WorkspaceArtifactKind> {
    match value {
        "note" => Ok(WorkspaceArtifactKind::Note),
        "runbook" => Ok(WorkspaceArtifactKind::Runbook),
        "scratchpad" => Ok(WorkspaceArtifactKind::Scratchpad),
        "task_list" => Ok(WorkspaceArtifactKind::TaskList),
        "script" => Ok(WorkspaceArtifactKind::Script),
        other => bail!("unrecognized workspace artifact kind '{other}'"),
    }
}

fn workspace_artifact_status_as_str(status: WorkspaceArtifactStatus) -> &'static str {
    match status {
        WorkspaceArtifactStatus::Active => "active",
        WorkspaceArtifactStatus::Archived => "archived",
    }
}

fn parse_workspace_artifact_status(value: &str) -> Result<WorkspaceArtifactStatus> {
    match value {
        "active" => Ok(WorkspaceArtifactStatus::Active),
        "archived" => Ok(WorkspaceArtifactStatus::Archived),
        other => bail!("unrecognized workspace artifact status '{other}'"),
    }
}

fn workspace_script_run_status_as_str(status: WorkspaceScriptRunStatus) -> &'static str {
    match status {
        WorkspaceScriptRunStatus::Pending => "pending",
        WorkspaceScriptRunStatus::Running => "running",
        WorkspaceScriptRunStatus::Completed => "completed",
        WorkspaceScriptRunStatus::Failed => "failed",
        WorkspaceScriptRunStatus::TimedOut => "timed_out",
        WorkspaceScriptRunStatus::Blocked => "blocked",
    }
}

fn parse_workspace_script_run_status(value: &str) -> Result<WorkspaceScriptRunStatus> {
    match value {
        "pending" => Ok(WorkspaceScriptRunStatus::Pending),
        "running" => Ok(WorkspaceScriptRunStatus::Running),
        "completed" => Ok(WorkspaceScriptRunStatus::Completed),
        "failed" => Ok(WorkspaceScriptRunStatus::Failed),
        "timed_out" => Ok(WorkspaceScriptRunStatus::TimedOut),
        "blocked" => Ok(WorkspaceScriptRunStatus::Blocked),
        other => bail!("unrecognized workspace script run status '{other}'"),
    }
}

fn governed_action_risk_tier_as_str(risk_tier: contracts::GovernedActionRiskTier) -> &'static str {
    match risk_tier {
        contracts::GovernedActionRiskTier::Tier0 => "tier_0",
        contracts::GovernedActionRiskTier::Tier1 => "tier_1",
        contracts::GovernedActionRiskTier::Tier2 => "tier_2",
        contracts::GovernedActionRiskTier::Tier3 => "tier_3",
    }
}

fn parse_governed_action_risk_tier(value: &str) -> Result<contracts::GovernedActionRiskTier> {
    match value {
        "tier_0" => Ok(contracts::GovernedActionRiskTier::Tier0),
        "tier_1" => Ok(contracts::GovernedActionRiskTier::Tier1),
        "tier_2" => Ok(contracts::GovernedActionRiskTier::Tier2),
        "tier_3" => Ok(contracts::GovernedActionRiskTier::Tier3),
        other => bail!("unrecognized governed action risk tier '{other}'"),
    }
}
