use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::{
    AttachmentReference, IngressAttachmentProcessingStatus, IngressAttachmentSummary,
    RetrievedContextItem, RetrievedMemoryArtifactContext,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Executor, PgPool, Postgres, Row};
use uuid::Uuid;

pub const DEFAULT_ATTACHMENT_CONTEXT_ITEM_LIMIT: usize = 3;
pub const DEFAULT_ATTACHMENT_CONTEXT_EXCERPT_CHAR_LIMIT: usize = 1_200;
pub const MAX_EXTRACTED_ATTACHMENT_CHARS: usize = 12_000;
pub const MAX_EXTRACTED_ATTACHMENT_SUMMARY_CHARS: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressAttachmentRecord {
    pub ingress_attachment_id: Uuid,
    pub ingress_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub channel_kind: String,
    pub attachment_id: String,
    pub media_type: Option<String>,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub raw_payload_ref: Option<String>,
    pub processing_status: IngressAttachmentProcessingStatus,
    pub latest_processing_attempt_id: Option<Uuid>,
    pub latest_extracted_artifact_id: Option<Uuid>,
    pub last_failure_reason: Option<String>,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressAttachmentExtractedArtifactRecord {
    pub ingress_attachment_extracted_artifact_id: Uuid,
    pub ingress_attachment_id: Uuid,
    pub ingress_attachment_processing_attempt_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub extractor_kind: String,
    pub content_format: String,
    pub content_text: String,
    pub summary_text: String,
    pub content_sha256: String,
    pub content_chars: i32,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentProcessStatus {
    Processed,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIngressAttachmentRequest {
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub ingress_id: Uuid,
    pub attachment_id: String,
    pub requested_by: String,
    pub request_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIngressAttachmentResult {
    pub status: AttachmentProcessStatus,
    pub attachment: IngressAttachmentRecord,
    pub extracted_artifact: Option<IngressAttachmentExtractedArtifactRecord>,
    pub detail: String,
    pub content_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentContextProjection {
    pub items: Vec<RetrievedContextItem>,
    pub selected_attachment_ids: Vec<String>,
    pub truncated_excerpt_count: u32,
}

pub async fn register_ingress_attachments(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    ingress_id: Uuid,
    trace_id: Uuid,
    execution_id: Option<Uuid>,
    internal_principal_ref: &str,
    internal_conversation_ref: &str,
    channel_kind: &str,
    raw_payload_ref: Option<&str>,
    attachments: &[AttachmentReference],
) -> Result<()> {
    if attachments.is_empty() {
        return Ok(());
    }

    for attachment in attachments {
        sqlx::query(
            r#"
            INSERT INTO ingress_attachments (
                ingress_attachment_id,
                ingress_id,
                trace_id,
                execution_id,
                internal_principal_ref,
                internal_conversation_ref,
                channel_kind,
                attachment_id,
                media_type,
                file_name,
                size_bytes,
                raw_payload_ref,
                processing_status,
                latest_processing_attempt_id,
                latest_extracted_artifact_id,
                last_failure_reason,
                last_processed_at,
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
                NULL,
                NULL,
                NULL,
                NULL,
                NOW(),
                NOW()
            )
            ON CONFLICT (ingress_id, attachment_id) DO NOTHING
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(ingress_id)
        .bind(trace_id)
        .bind(execution_id)
        .bind(internal_principal_ref)
        .bind(internal_conversation_ref)
        .bind(channel_kind)
        .bind(&attachment.attachment_id)
        .bind(&attachment.media_type)
        .bind(&attachment.file_name)
        .bind(attachment.size_bytes.map(|value| value as i64))
        .bind(raw_payload_ref)
        .bind(attachment_status_as_str(
            IngressAttachmentProcessingStatus::Pending,
        ))
        .execute(&mut **tx)
        .await
        .context("failed to register ingress attachment")?;
    }

    Ok(())
}

pub async fn list_ingress_attachment_summaries(
    pool: &PgPool,
    ingress_id: Uuid,
) -> Result<Vec<IngressAttachmentSummary>> {
    let records = list_ingress_attachments(pool, ingress_id).await?;
    Ok(records
        .into_iter()
        .map(|record| IngressAttachmentSummary {
            ingress_attachment_id: record.ingress_attachment_id,
            ingress_id: record.ingress_id,
            attachment_id: record.attachment_id,
            media_type: record.media_type,
            file_name: record.file_name,
            size_bytes: record.size_bytes,
            processing_status: record.processing_status,
            last_processed_at: record.last_processed_at,
            last_failure_reason: record.last_failure_reason,
        })
        .collect())
}

pub async fn list_ingress_attachments(
    pool: &PgPool,
    ingress_id: Uuid,
) -> Result<Vec<IngressAttachmentRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            ingress_attachment_id,
            ingress_id,
            trace_id,
            execution_id,
            internal_principal_ref,
            internal_conversation_ref,
            channel_kind,
            attachment_id,
            media_type,
            file_name,
            size_bytes,
            raw_payload_ref,
            processing_status,
            latest_processing_attempt_id,
            latest_extracted_artifact_id,
            last_failure_reason,
            last_processed_at,
            created_at,
            updated_at
        FROM ingress_attachments
        WHERE ingress_id = $1
        ORDER BY created_at ASC, ingress_attachment_id ASC
        "#,
    )
    .bind(ingress_id)
    .fetch_all(pool)
    .await
    .context("failed to list ingress attachments")?;

    rows.into_iter()
        .map(decode_ingress_attachment_row)
        .collect()
}

pub async fn process_ingress_attachment(
    pool: &PgPool,
    request: &ProcessIngressAttachmentRequest,
) -> Result<ProcessIngressAttachmentResult> {
    if request.attachment_id.trim().is_empty() {
        bail!("attachment_id must not be empty");
    }
    if request.requested_by.trim().is_empty() {
        bail!("requested_by must not be empty");
    }
    if request.request_kind.trim().is_empty() {
        bail!("request_kind must not be empty");
    }

    let mut tx = pool
        .begin()
        .await
        .context("failed to start ingress attachment processing transaction")?;
    let attachment = get_ingress_attachment_for_update(
        &mut tx,
        request.ingress_id,
        request.attachment_id.trim(),
    )
    .await?
    .with_context(|| {
        format!(
            "ingress attachment '{}' for ingress '{}' was not found",
            request.attachment_id, request.ingress_id
        )
    })?;

    let attempt_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO ingress_attachment_processing_attempts (
            ingress_attachment_processing_attempt_id,
            ingress_attachment_id,
            trace_id,
            execution_id,
            requested_by,
            request_kind,
            status,
            extractor_kind,
            detail,
            bytes_processed,
            extracted_chars,
            started_at,
            completed_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            'started',
            NULL,
            NULL,
            NULL,
            NULL,
            NOW(),
            NULL
        )
        "#,
    )
    .bind(attempt_id)
    .bind(attachment.ingress_attachment_id)
    .bind(request.trace_id)
    .bind(request.execution_id)
    .bind(request.requested_by.trim())
    .bind(request.request_kind.trim())
    .execute(&mut *tx)
    .await
    .context("failed to insert ingress attachment processing attempt")?;

    if !supports_text_extraction(
        attachment.media_type.as_deref(),
        attachment.file_name.as_deref(),
    ) {
        let detail = format!(
            "attachment '{}' is not supported for text extraction in the current pipeline",
            attachment.attachment_id
        );
        let attachment = finalize_attachment_processing_without_artifact(
            &mut tx,
            &attachment,
            attempt_id,
            IngressAttachmentProcessingStatus::Unsupported,
            "unsupported",
            "text_document_extractor",
            &detail,
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit unsupported attachment processing result")?;
        return Ok(ProcessIngressAttachmentResult {
            status: AttachmentProcessStatus::Unsupported,
            attachment,
            extracted_artifact: None,
            detail,
            content_truncated: false,
        });
    }

    let Some(raw_content) = load_attachment_fixture_content(&attachment)? else {
        let detail = format!(
            "attachment '{}' source content is unavailable; no fixture content could be resolved",
            attachment.attachment_id
        );
        let attachment = finalize_attachment_processing_without_artifact(
            &mut tx,
            &attachment,
            attempt_id,
            IngressAttachmentProcessingStatus::Failed,
            "failed",
            "text_document_extractor",
            &detail,
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit failed attachment processing result")?;
        return Ok(ProcessIngressAttachmentResult {
            status: AttachmentProcessStatus::Failed,
            attachment,
            extracted_artifact: None,
            detail,
            content_truncated: false,
        });
    };

    let normalized = normalize_extracted_text(&raw_content);
    if normalized.is_empty() {
        let detail = format!(
            "attachment '{}' resolved to empty content after normalization",
            attachment.attachment_id
        );
        let attachment = finalize_attachment_processing_without_artifact(
            &mut tx,
            &attachment,
            attempt_id,
            IngressAttachmentProcessingStatus::Failed,
            "failed",
            "text_document_extractor",
            &detail,
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit empty-content attachment processing result")?;
        return Ok(ProcessIngressAttachmentResult {
            status: AttachmentProcessStatus::Failed,
            attachment,
            extracted_artifact: None,
            detail,
            content_truncated: false,
        });
    }

    let (bounded_content, content_truncated) =
        truncate_chars(&normalized, MAX_EXTRACTED_ATTACHMENT_CHARS);
    let (summary_text, _) =
        truncate_chars(&bounded_content, MAX_EXTRACTED_ATTACHMENT_SUMMARY_CHARS);
    let content_sha256 = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(bounded_content.as_bytes()))
    );
    let extracted_artifact_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO ingress_attachment_extracted_artifacts (
            ingress_attachment_extracted_artifact_id,
            ingress_attachment_id,
            ingress_attachment_processing_attempt_id,
            trace_id,
            execution_id,
            extractor_kind,
            content_format,
            content_text,
            summary_text,
            content_sha256,
            content_chars,
            metadata_json,
            created_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            'text_document_extractor',
            'text/plain',
            $6,
            $7,
            $8,
            $9,
            $10,
            NOW()
        )
        "#,
    )
    .bind(extracted_artifact_id)
    .bind(attachment.ingress_attachment_id)
    .bind(attempt_id)
    .bind(request.trace_id)
    .bind(request.execution_id)
    .bind(&bounded_content)
    .bind(&summary_text)
    .bind(&content_sha256)
    .bind(bounded_content.chars().count() as i32)
    .bind(json!({
        "source": "fixture_payload_ref",
        "raw_payload_ref": attachment.raw_payload_ref,
        "content_truncated": content_truncated,
        "attachment_id": attachment.attachment_id,
    }))
    .execute(&mut *tx)
    .await
    .context("failed to insert extracted attachment artifact")?;

    sqlx::query(
        r#"
        UPDATE ingress_attachment_processing_attempts
        SET
            status = 'succeeded',
            extractor_kind = 'text_document_extractor',
            detail = $2,
            bytes_processed = $3,
            extracted_chars = $4,
            completed_at = NOW()
        WHERE ingress_attachment_processing_attempt_id = $1
        "#,
    )
    .bind(attempt_id)
    .bind("text extraction completed")
    .bind(raw_content.len() as i64)
    .bind(bounded_content.chars().count() as i32)
    .execute(&mut *tx)
    .await
    .context("failed to finalize successful attachment processing attempt")?;

    sqlx::query(
        r#"
        UPDATE ingress_attachments
        SET
            processing_status = $2,
            latest_processing_attempt_id = $3,
            latest_extracted_artifact_id = $4,
            last_failure_reason = NULL,
            last_processed_at = NOW(),
            updated_at = NOW()
        WHERE ingress_attachment_id = $1
        "#,
    )
    .bind(attachment.ingress_attachment_id)
    .bind(attachment_status_as_str(
        IngressAttachmentProcessingStatus::Processed,
    ))
    .bind(attempt_id)
    .bind(extracted_artifact_id)
    .execute(&mut *tx)
    .await
    .context("failed to update successful attachment processing state")?;

    let attachment = get_ingress_attachment_by_id(&mut *tx, attachment.ingress_attachment_id)
        .await?
        .context("processed attachment row disappeared")?;
    let extracted_artifact =
        get_ingress_attachment_extracted_artifact_by_id(&mut *tx, extracted_artifact_id)
            .await?
            .context("extracted attachment artifact row disappeared")?;

    tx.commit()
        .await
        .context("failed to commit processed attachment transaction")?;

    Ok(ProcessIngressAttachmentResult {
        status: AttachmentProcessStatus::Processed,
        attachment,
        extracted_artifact: Some(extracted_artifact),
        detail: "text extraction completed".to_string(),
        content_truncated,
    })
}

pub async fn project_ingress_attachment_context(
    pool: &PgPool,
    ingress_id: Uuid,
    max_items: usize,
    excerpt_char_limit: usize,
) -> Result<AttachmentContextProjection> {
    if max_items == 0 {
        bail!("max_items must be greater than zero");
    }
    if excerpt_char_limit == 0 {
        bail!("excerpt_char_limit must be greater than zero");
    }

    let rows = sqlx::query(
        r#"
        SELECT
            a.attachment_id,
            a.file_name,
            a.media_type,
            e.ingress_attachment_extracted_artifact_id,
            e.summary_text,
            e.created_at
        FROM ingress_attachments a
        JOIN ingress_attachment_extracted_artifacts e
            ON e.ingress_attachment_extracted_artifact_id = a.latest_extracted_artifact_id
        WHERE a.ingress_id = $1
          AND a.processing_status = 'processed'
        ORDER BY a.updated_at DESC, a.ingress_attachment_id DESC
        LIMIT $2
        "#,
    )
    .bind(ingress_id)
    .bind(max_items as i64)
    .fetch_all(pool)
    .await
    .context("failed to load processed ingress attachment context")?;

    let mut items = Vec::with_capacity(rows.len());
    let mut selected_attachment_ids = Vec::with_capacity(rows.len());
    let mut truncated_excerpt_count = 0u32;
    for row in rows {
        let attachment_id = row.get::<String, _>("attachment_id");
        let file_name = row.get::<Option<String>, _>("file_name");
        let media_type = row.get::<Option<String>, _>("media_type");
        let summary_text = row.get::<String, _>("summary_text");
        let artifact_id = row.get::<Uuid, _>("ingress_attachment_extracted_artifact_id");
        let (summary_text, truncated) = truncate_chars(&summary_text, excerpt_char_limit);
        if truncated {
            truncated_excerpt_count += 1;
        }
        selected_attachment_ids.push(attachment_id.clone());
        let subject_ref = file_name.clone().unwrap_or_else(|| attachment_id.clone());
        let media_label = media_type.unwrap_or_else(|| "unknown".to_string());
        items.push(RetrievedContextItem::MemoryArtifact(
            RetrievedMemoryArtifactContext {
                memory_artifact_id: artifact_id,
                artifact_kind: "attachment_excerpt".to_string(),
                subject_ref,
                content_text: format!("{summary_text}\n[source media: {media_label}]"),
                validity_status: "current".to_string(),
                relevance_reason: "processed attachment from triggering ingress".to_string(),
            },
        ));
    }

    Ok(AttachmentContextProjection {
        items,
        selected_attachment_ids,
        truncated_excerpt_count,
    })
}

fn supports_text_extraction(media_type: Option<&str>, file_name: Option<&str>) -> bool {
    if let Some(media_type) = media_type {
        let media_type = media_type.trim().to_ascii_lowercase();
        if media_type.starts_with("text/")
            || matches!(
                media_type.as_str(),
                "application/json"
                    | "application/x-yaml"
                    | "application/yaml"
                    | "application/xml"
                    | "application/javascript"
            )
        {
            return true;
        }
    }

    if let Some(file_name) = file_name {
        let file_name = file_name.trim().to_ascii_lowercase();
        return file_name.ends_with(".txt")
            || file_name.ends_with(".md")
            || file_name.ends_with(".json")
            || file_name.ends_with(".yaml")
            || file_name.ends_with(".yml")
            || file_name.ends_with(".xml");
    }

    false
}

fn load_attachment_fixture_content(attachment: &IngressAttachmentRecord) -> Result<Option<String>> {
    let Some(raw_payload_ref) = attachment.raw_payload_ref.as_deref() else {
        return Ok(None);
    };
    let payload_path = resolve_workspace_path(raw_payload_ref);
    if !payload_path.exists() {
        return Ok(None);
    }
    let payload = std::fs::read_to_string(&payload_path).with_context(|| {
        format!(
            "failed to read raw payload fixture for attachment processing: {}",
            payload_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&payload).with_context(|| {
        format!(
            "failed to parse raw payload fixture JSON for attachment processing: {}",
            payload_path.display()
        )
    })?;

    let Some(entries) = value.get("blue_lagoon_fixture_attachments") else {
        return Ok(None);
    };
    let Some(entry) = entries.get(&attachment.attachment_id) else {
        return Ok(None);
    };

    if let Some(content_text) = entry.as_str() {
        return Ok(Some(content_text.to_string()));
    }

    let Some(entry_object) = entry.as_object() else {
        return Ok(None);
    };
    if let Some(content_text) = entry_object.get("content_text").and_then(Value::as_str) {
        return Ok(Some(content_text.to_string()));
    }
    if let Some(content_path) = entry_object.get("content_path").and_then(Value::as_str) {
        let path = resolve_content_path(content_path, &payload_path);
        if !path.exists() {
            return Ok(None);
        }
        return Ok(Some(std::fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read fixture attachment content file: {}",
                path.display()
            )
        })?));
    }
    Ok(None)
}

fn resolve_content_path(content_path: &str, payload_path: &Path) -> PathBuf {
    let path = Path::new(content_path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    payload_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn resolve_workspace_path(value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::migration::workspace_root().join(path)
    }
}

fn normalize_extracted_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return (value.to_string(), false);
    }
    (value.chars().take(max_chars).collect(), true)
}

async fn finalize_attachment_processing_without_artifact(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    attachment: &IngressAttachmentRecord,
    attempt_id: Uuid,
    status: IngressAttachmentProcessingStatus,
    attempt_status: &str,
    extractor_kind: &str,
    detail: &str,
) -> Result<IngressAttachmentRecord> {
    sqlx::query(
        r#"
        UPDATE ingress_attachment_processing_attempts
        SET
            status = $2,
            extractor_kind = $3,
            detail = $4,
            completed_at = NOW()
        WHERE ingress_attachment_processing_attempt_id = $1
        "#,
    )
    .bind(attempt_id)
    .bind(attempt_status)
    .bind(extractor_kind)
    .bind(detail)
    .execute(&mut **tx)
    .await
    .context("failed to finalize attachment processing attempt")?;

    sqlx::query(
        r#"
        UPDATE ingress_attachments
        SET
            processing_status = $2,
            latest_processing_attempt_id = $3,
            last_failure_reason = $4,
            last_processed_at = NOW(),
            updated_at = NOW()
        WHERE ingress_attachment_id = $1
        "#,
    )
    .bind(attachment.ingress_attachment_id)
    .bind(attachment_status_as_str(status))
    .bind(attempt_id)
    .bind(detail)
    .execute(&mut **tx)
    .await
    .context("failed to update attachment status")?;

    get_ingress_attachment_by_id(&mut **tx, attachment.ingress_attachment_id)
        .await?
        .context("attachment row disappeared after processing")
}

async fn get_ingress_attachment_for_update(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    ingress_id: Uuid,
    attachment_id: &str,
) -> Result<Option<IngressAttachmentRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            ingress_attachment_id,
            ingress_id,
            trace_id,
            execution_id,
            internal_principal_ref,
            internal_conversation_ref,
            channel_kind,
            attachment_id,
            media_type,
            file_name,
            size_bytes,
            raw_payload_ref,
            processing_status,
            latest_processing_attempt_id,
            latest_extracted_artifact_id,
            last_failure_reason,
            last_processed_at,
            created_at,
            updated_at
        FROM ingress_attachments
        WHERE ingress_id = $1
          AND attachment_id = $2
        FOR UPDATE
        "#,
    )
    .bind(ingress_id)
    .bind(attachment_id)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to fetch ingress attachment for update")?;
    row.map(decode_ingress_attachment_row).transpose()
}

async fn get_ingress_attachment_by_id<'e, E>(
    executor: E,
    ingress_attachment_id: Uuid,
) -> Result<Option<IngressAttachmentRecord>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(
        r#"
        SELECT
            ingress_attachment_id,
            ingress_id,
            trace_id,
            execution_id,
            internal_principal_ref,
            internal_conversation_ref,
            channel_kind,
            attachment_id,
            media_type,
            file_name,
            size_bytes,
            raw_payload_ref,
            processing_status,
            latest_processing_attempt_id,
            latest_extracted_artifact_id,
            last_failure_reason,
            last_processed_at,
            created_at,
            updated_at
        FROM ingress_attachments
        WHERE ingress_attachment_id = $1
        "#,
    )
    .bind(ingress_attachment_id)
    .fetch_optional(executor)
    .await
    .context("failed to fetch ingress attachment by id")?;
    row.map(decode_ingress_attachment_row).transpose()
}

async fn get_ingress_attachment_extracted_artifact_by_id<'e, E>(
    executor: E,
    ingress_attachment_extracted_artifact_id: Uuid,
) -> Result<Option<IngressAttachmentExtractedArtifactRecord>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(
        r#"
        SELECT
            ingress_attachment_extracted_artifact_id,
            ingress_attachment_id,
            ingress_attachment_processing_attempt_id,
            trace_id,
            execution_id,
            extractor_kind,
            content_format,
            content_text,
            summary_text,
            content_sha256,
            content_chars,
            metadata_json,
            created_at
        FROM ingress_attachment_extracted_artifacts
        WHERE ingress_attachment_extracted_artifact_id = $1
        "#,
    )
    .bind(ingress_attachment_extracted_artifact_id)
    .fetch_optional(executor)
    .await
    .context("failed to fetch ingress attachment extracted artifact by id")?;
    row.map(decode_ingress_attachment_extracted_artifact_row)
        .transpose()
}

fn decode_ingress_attachment_row(row: sqlx::postgres::PgRow) -> Result<IngressAttachmentRecord> {
    Ok(IngressAttachmentRecord {
        ingress_attachment_id: row.get("ingress_attachment_id"),
        ingress_id: row.get("ingress_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        channel_kind: row.get("channel_kind"),
        attachment_id: row.get("attachment_id"),
        media_type: row.get("media_type"),
        file_name: row.get("file_name"),
        size_bytes: row
            .get::<Option<i64>, _>("size_bytes")
            .map(|value| value as u64),
        raw_payload_ref: row.get("raw_payload_ref"),
        processing_status: parse_attachment_status(row.get("processing_status"))?,
        latest_processing_attempt_id: row.get("latest_processing_attempt_id"),
        latest_extracted_artifact_id: row.get("latest_extracted_artifact_id"),
        last_failure_reason: row.get("last_failure_reason"),
        last_processed_at: row.get("last_processed_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn decode_ingress_attachment_extracted_artifact_row(
    row: sqlx::postgres::PgRow,
) -> Result<IngressAttachmentExtractedArtifactRecord> {
    Ok(IngressAttachmentExtractedArtifactRecord {
        ingress_attachment_extracted_artifact_id: row
            .get("ingress_attachment_extracted_artifact_id"),
        ingress_attachment_id: row.get("ingress_attachment_id"),
        ingress_attachment_processing_attempt_id: row
            .get("ingress_attachment_processing_attempt_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        extractor_kind: row.get("extractor_kind"),
        content_format: row.get("content_format"),
        content_text: row.get("content_text"),
        summary_text: row.get("summary_text"),
        content_sha256: row.get("content_sha256"),
        content_chars: row.get("content_chars"),
        metadata: row.get("metadata_json"),
        created_at: row.get("created_at"),
    })
}

fn parse_attachment_status(value: &str) -> Result<IngressAttachmentProcessingStatus> {
    match value {
        "pending" => Ok(IngressAttachmentProcessingStatus::Pending),
        "processed" => Ok(IngressAttachmentProcessingStatus::Processed),
        "unsupported" => Ok(IngressAttachmentProcessingStatus::Unsupported),
        "failed" => Ok(IngressAttachmentProcessingStatus::Failed),
        other => bail!("unsupported ingress attachment processing status '{other}'"),
    }
}

fn attachment_status_as_str(value: IngressAttachmentProcessingStatus) -> &'static str {
    match value {
        IngressAttachmentProcessingStatus::Pending => "pending",
        IngressAttachmentProcessingStatus::Processed => "processed",
        IngressAttachmentProcessingStatus::Unsupported => "unsupported",
        IngressAttachmentProcessingStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_text_extraction_accepts_textual_types_and_extensions() {
        assert!(supports_text_extraction(
            Some("text/plain"),
            Some("notes.txt")
        ));
        assert!(supports_text_extraction(
            Some("application/json"),
            Some("payload.bin")
        ));
        assert!(supports_text_extraction(None, Some("notes.md")));
        assert!(!supports_text_extraction(
            Some("image/jpeg"),
            Some("photo.jpg")
        ));
    }

    #[test]
    fn normalize_extracted_text_cleans_line_endings_and_whitespace() {
        let normalized = normalize_extracted_text(" one  \r\ntwo\rthree \n\n");
        assert_eq!(normalized, "one\ntwo\nthree");
    }

    #[test]
    fn truncate_chars_flags_truncation() {
        let (value, truncated) = truncate_chars("abcdef", 4);
        assert_eq!(value, "abcd");
        assert!(truncated);
    }
}
