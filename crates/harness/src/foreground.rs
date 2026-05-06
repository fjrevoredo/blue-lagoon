use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use contracts::{
    AttachmentReference, ChannelKind, EpisodeExcerpt, ForegroundExecutionMode, ForegroundTrigger,
    ForegroundTriggerKind, IngressEventKind, NormalizedIngress, OrderedIngressReference,
};
use serde_json::{Value, json};
use sqlx::{Executor, PgPool, Postgres, Row};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    background,
    causal_links::{self, NewCausalLink},
    config::{ResolvedTelegramConfig, RuntimeConfig, TelegramForegroundBindingConfig},
    execution::{self, NewExecutionRecord},
    policy::{self, PolicyDecision},
    trace::TraceContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConversationBinding {
    pub conversation_binding_id: Uuid,
    pub channel_kind: ChannelKind,
    pub external_user_id: String,
    pub external_conversation_id: String,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBindingRecord {
    pub conversation_binding_id: Uuid,
    pub channel_kind: String,
    pub external_user_id: String,
    pub external_conversation_id: String,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationBindingAction {
    Created,
    Updated,
    Rebound,
    Merged,
}

impl ConversationBindingAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Rebound => "rebound",
            Self::Merged => "merged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBindingWriteResult {
    pub record: ConversationBindingRecord,
    pub action: ConversationBindingAction,
}

#[derive(Debug, Clone)]
pub struct NewIngressEvent {
    pub ingress: NormalizedIngress,
    pub conversation_binding_id: Option<Uuid>,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub status: String,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressEventRecord {
    pub ingress_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub channel_kind: String,
    pub internal_principal_ref: Option<String>,
    pub internal_conversation_ref: Option<String>,
    pub event_kind: String,
    pub external_event_id: String,
    pub external_message_id: Option<String>,
    pub status: String,
    pub foreground_status: String,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub rejection_reason: Option<String>,
    pub text_body: Option<String>,
    pub reply_to_external_message_id: Option<String>,
    pub attachment_count: i32,
    pub attachments: Vec<AttachmentReference>,
    pub command_name: Option<String>,
    pub command_args: Vec<String>,
    pub approval_token: Option<String>,
    pub approval_callback_data: Option<String>,
    pub raw_payload_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEpisode {
    pub episode_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub ingress_id: Option<Uuid>,
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub trigger_kind: String,
    pub trigger_source: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeRecord {
    pub episode_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub internal_conversation_ref: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEpisodeMessage {
    pub episode_message_id: Uuid,
    pub episode_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Uuid,
    pub message_order: i32,
    pub message_role: String,
    pub channel_kind: ChannelKind,
    pub text_body: Option<String>,
    pub external_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeMessageRecord {
    pub episode_message_id: Uuid,
    pub episode_id: Uuid,
    pub message_order: i32,
    pub message_role: String,
    pub text_body: Option<String>,
    pub external_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundTriggerIntakeOutcome {
    Accepted(Box<ForegroundTrigger>),
    Duplicate(DuplicateForegroundTrigger),
    Rejected(RejectedForegroundTrigger),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedForegroundIngressOutcome {
    Accepted(StagedForegroundIngress),
    Duplicate(DuplicateForegroundTrigger),
    Rejected(RejectedForegroundTrigger),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedForegroundIngress {
    pub ingress_id: Uuid,
    pub trace_id: Uuid,
    pub internal_conversation_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateForegroundTrigger {
    pub ingress_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedForegroundTrigger {
    pub ingress_id: Uuid,
    pub trace_id: Uuid,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct NewExecutionIngressLink {
    pub execution_ingress_link_id: Uuid,
    pub execution_id: Uuid,
    pub ingress_id: Uuid,
    pub link_role: String,
    pub sequence_index: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionIngressLinkRecord {
    pub execution_ingress_link_id: Uuid,
    pub execution_id: Uuid,
    pub ingress_id: Uuid,
    pub link_role: String,
    pub sequence_index: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundExecutionDecisionReason {
    SingleIngress,
    StaleProcessingResume,
    PendingSpanThreshold,
    StalePendingBatch,
    ForcedRecovery,
}

impl ForegroundExecutionDecisionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleIngress => "single_ingress",
            Self::StaleProcessingResume => "stale_processing_resume",
            Self::PendingSpanThreshold => "pending_span_threshold",
            Self::StalePendingBatch => "stale_pending_batch",
            Self::ForcedRecovery => "forced_recovery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PendingForegroundExecutionOptions {
    pub force_recovery: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingForegroundExecutionPlan {
    pub mode: ForegroundExecutionMode,
    pub primary_ingress: IngressEventRecord,
    pub interrupted_execution_id: Option<Uuid>,
    pub ordered_ingress: Vec<OrderedIngressReference>,
    pub decision_reason: ForegroundExecutionDecisionReason,
}

const WAKE_SIGNAL_EXTERNAL_EVENT_PREFIX: &str = "wake-signal:";
const WAKE_SIGNAL_RAW_PAYLOAD_PREFIX: &str = "wake_signal:";

pub async fn upsert_conversation_binding(
    pool: &PgPool,
    binding: &NewConversationBinding,
) -> Result<ConversationBindingRecord> {
    Ok(reconcile_conversation_binding(pool, binding).await?.record)
}

pub async fn reconcile_conversation_binding(
    pool: &PgPool,
    binding: &NewConversationBinding,
) -> Result<ConversationBindingWriteResult> {
    let mut tx = pool
        .begin()
        .await
        .context("failed to start conversation binding transaction")?;

    let result = reconcile_conversation_binding_in_tx(&mut tx, binding).await?;

    tx.commit()
        .await
        .context("failed to commit conversation binding transaction")?;

    Ok(result)
}

async fn reconcile_conversation_binding_in_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    binding: &NewConversationBinding,
) -> Result<ConversationBindingWriteResult> {
    let external = find_locked_conversation_binding_by_external_tuple(tx, binding).await?;
    let internal =
        find_locked_conversation_binding_by_internal_ref(tx, &binding.internal_conversation_ref)
            .await?;

    let result = match (external, internal) {
        (None, None) => {
            // The first accepted ingress for a conversation creates the canonical binding row.
            ConversationBindingWriteResult {
                record: insert_conversation_binding(tx, binding).await?,
                action: ConversationBindingAction::Created,
            }
        }
        (Some(external), Some(internal))
            if external.conversation_binding_id == internal.conversation_binding_id =>
        {
            ConversationBindingWriteResult {
                record: update_conversation_binding(tx, internal.conversation_binding_id, binding)
                    .await?,
                action: ConversationBindingAction::Updated,
            }
        }
        (None, Some(internal)) => ConversationBindingWriteResult {
            record: update_conversation_binding(tx, internal.conversation_binding_id, binding)
                .await?,
            action: ConversationBindingAction::Rebound,
        },
        (Some(external), None) => ConversationBindingWriteResult {
            record: update_conversation_binding(tx, external.conversation_binding_id, binding)
                .await?,
            action: ConversationBindingAction::Updated,
        },
        (Some(external), Some(internal)) => {
            // Preserve the canonical internal conversation identity, rewrite any historical
            // ingress rows that still reference the superseded external binding row, then
            // remove the duplicate binding row.
            reassign_ingress_event_bindings(
                tx,
                external.conversation_binding_id,
                internal.conversation_binding_id,
            )
            .await?;
            delete_conversation_binding(tx, external.conversation_binding_id).await?;
            ConversationBindingWriteResult {
                record: update_conversation_binding(tx, internal.conversation_binding_id, binding)
                    .await?,
                action: ConversationBindingAction::Merged,
            }
        }
    };
    Ok(result)
}

async fn insert_conversation_binding(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    binding: &NewConversationBinding,
) -> Result<ConversationBindingRecord> {
    let row = sqlx::query(
        r#"
        INSERT INTO conversation_bindings (
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            NOW(),
            NOW()
        )
        RETURNING
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref
        "#,
    )
    .bind(binding.conversation_binding_id)
    .bind(channel_kind_as_str(binding.channel_kind))
    .bind(&binding.external_user_id)
    .bind(&binding.external_conversation_id)
    .bind(&binding.internal_principal_ref)
    .bind(&binding.internal_conversation_ref)
    .fetch_one(&mut **tx)
    .await
    .context("failed to insert conversation binding")?;

    Ok(decode_conversation_binding_row(row))
}

async fn update_conversation_binding(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    conversation_binding_id: Uuid,
    binding: &NewConversationBinding,
) -> Result<ConversationBindingRecord> {
    let row = sqlx::query(
        r#"
        UPDATE conversation_bindings
        SET
            channel_kind = $2,
            external_user_id = $3,
            external_conversation_id = $4,
            internal_principal_ref = $5,
            internal_conversation_ref = $6,
            updated_at = NOW()
        WHERE conversation_binding_id = $1
        RETURNING
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref
        "#,
    )
    .bind(conversation_binding_id)
    .bind(channel_kind_as_str(binding.channel_kind))
    .bind(&binding.external_user_id)
    .bind(&binding.external_conversation_id)
    .bind(&binding.internal_principal_ref)
    .bind(&binding.internal_conversation_ref)
    .fetch_one(&mut **tx)
    .await
    .context("failed to update conversation binding")?;

    Ok(decode_conversation_binding_row(row))
}

async fn delete_conversation_binding(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    conversation_binding_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM conversation_bindings
        WHERE conversation_binding_id = $1
        "#,
    )
    .bind(conversation_binding_id)
    .execute(&mut **tx)
    .await
    .context("failed to delete superseded conversation binding")?;
    Ok(())
}

async fn reassign_ingress_event_bindings(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    from_binding_id: Uuid,
    to_binding_id: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE ingress_events
        SET conversation_binding_id = $2
        WHERE conversation_binding_id = $1
        "#,
    )
    .bind(from_binding_id)
    .bind(to_binding_id)
    .execute(&mut **tx)
    .await
    .context("failed to reassign ingress events to canonical conversation binding")?;
    Ok(())
}

fn decode_conversation_binding_row(row: sqlx::postgres::PgRow) -> ConversationBindingRecord {
    ConversationBindingRecord {
        conversation_binding_id: row.get("conversation_binding_id"),
        channel_kind: row.get("channel_kind"),
        external_user_id: row.get("external_user_id"),
        external_conversation_id: row.get("external_conversation_id"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
    }
}

async fn find_locked_conversation_binding_by_external_tuple(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    binding: &NewConversationBinding,
) -> Result<Option<ConversationBindingRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref
        FROM conversation_bindings
        WHERE channel_kind = $1
          AND external_user_id = $2
          AND external_conversation_id = $3
        FOR UPDATE
        "#,
    )
    .bind(channel_kind_as_str(binding.channel_kind))
    .bind(&binding.external_user_id)
    .bind(&binding.external_conversation_id)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to look up conversation binding by external tuple")?;

    Ok(row.map(decode_conversation_binding_row))
}

async fn find_locked_conversation_binding_by_internal_ref(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    internal_conversation_ref: &str,
) -> Result<Option<ConversationBindingRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            conversation_binding_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            internal_principal_ref,
            internal_conversation_ref
        FROM conversation_bindings
        WHERE internal_conversation_ref = $1
        FOR UPDATE
        "#,
    )
    .bind(internal_conversation_ref)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to look up conversation binding by internal conversation ref")?;

    Ok(row.map(decode_conversation_binding_row))
}

pub async fn insert_ingress_event<'e, E>(executor: E, event: &NewIngressEvent) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO ingress_events (
            ingress_id,
            conversation_binding_id,
            trace_id,
            execution_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            external_event_id,
            external_message_id,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            occurred_at,
            received_at,
            status,
            rejection_reason,
            text_body,
            reply_to_external_message_id,
            attachment_count,
            attachments_json,
            command_name,
            command_args_json,
            approval_token,
            approval_callback_data,
            raw_payload_ref
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
            NOW(),
            $14,
            $15,
            $16,
            $17,
            $18,
            $19,
            $20,
            $21,
            $22,
            $23,
            $24
        )
        "#,
    )
    .bind(event.ingress.ingress_id)
    .bind(event.conversation_binding_id)
    .bind(event.trace_id)
    .bind(event.execution_id)
    .bind(channel_kind_as_str(event.ingress.channel_kind))
    .bind(&event.ingress.external_user_id)
    .bind(&event.ingress.external_conversation_id)
    .bind(&event.ingress.external_event_id)
    .bind(&event.ingress.external_message_id)
    .bind(&event.ingress.internal_principal_ref)
    .bind(&event.ingress.internal_conversation_ref)
    .bind(ingress_event_kind_as_str(event.ingress.event_kind))
    .bind(event.ingress.occurred_at)
    .bind(&event.status)
    .bind(&event.rejection_reason)
    .bind(&event.ingress.text_body)
    .bind(
        event
            .ingress
            .reply_to
            .as_ref()
            .map(|reply| reply.external_message_id.clone()),
    )
    .bind(event.ingress.attachments.len() as i32)
    .bind(
        serde_json::to_value(&event.ingress.attachments)
            .context("failed to serialize ingress attachment metadata")?,
    )
    .bind(
        event
            .ingress
            .command_hint
            .as_ref()
            .map(|hint| hint.command.clone()),
    )
    .bind(
        serde_json::to_value(
            event
                .ingress
                .command_hint
                .as_ref()
                .map(|hint| hint.args.clone())
                .unwrap_or_default(),
        )
        .context("failed to serialize ingress command args")?,
    )
    .bind(
        event
            .ingress
            .approval_payload
            .as_ref()
            .map(|payload| payload.token.clone()),
    )
    .bind(
        event
            .ingress
            .approval_payload
            .as_ref()
            .and_then(|payload| payload.callback_data.clone()),
    )
    .bind(&event.ingress.raw_payload_ref)
    .execute(executor)
    .await
    .context("failed to insert ingress event")?;
    Ok(())
}

pub async fn get_ingress_event(pool: &PgPool, ingress_id: Uuid) -> Result<IngressEventRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            ingress_id,
            trace_id,
            execution_id,
            occurred_at,
            received_at,
            channel_kind,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            external_event_id,
            external_message_id,
            status,
            foreground_status,
            last_processed_at,
            rejection_reason,
            text_body,
            reply_to_external_message_id,
            attachment_count,
            attachments_json,
            command_name,
            command_args_json,
            approval_token,
            approval_callback_data,
            raw_payload_ref
        FROM ingress_events
        WHERE ingress_id = $1
        "#,
    )
    .bind(ingress_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch ingress event")?;

    Ok(IngressEventRecord {
        ingress_id: row.get("ingress_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        occurred_at: row.get("occurred_at"),
        received_at: row.get("received_at"),
        channel_kind: row.get("channel_kind"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        event_kind: row.get("event_kind"),
        external_event_id: row.get("external_event_id"),
        external_message_id: row.get("external_message_id"),
        status: row.get("status"),
        foreground_status: row.get("foreground_status"),
        last_processed_at: row.get("last_processed_at"),
        rejection_reason: row.get("rejection_reason"),
        text_body: row.get("text_body"),
        reply_to_external_message_id: row.get("reply_to_external_message_id"),
        attachment_count: row.get("attachment_count"),
        attachments: decode_json_field(
            row.get::<Value, _>("attachments_json"),
            "ingress attachment metadata",
        )?,
        command_name: row.get("command_name"),
        command_args: decode_json_field(row.get::<Value, _>("command_args_json"), "command args")?,
        approval_token: row.get("approval_token"),
        approval_callback_data: row.get("approval_callback_data"),
        raw_payload_ref: row.get("raw_payload_ref"),
    })
}

pub async fn load_normalized_ingress(pool: &PgPool, ingress_id: Uuid) -> Result<NormalizedIngress> {
    let row = sqlx::query(
        r#"
        SELECT
            ingress_id,
            channel_kind,
            external_user_id,
            external_conversation_id,
            external_event_id,
            external_message_id,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            occurred_at,
            text_body,
            reply_to_external_message_id,
            attachments_json,
            command_name,
            command_args_json,
            approval_token,
            approval_callback_data,
            raw_payload_ref
        FROM ingress_events
        WHERE ingress_id = $1
        "#,
    )
    .bind(ingress_id)
    .fetch_one(pool)
    .await
    .context("failed to load normalized ingress from persistence")?;

    let command_hint = if let Some(command) = row.get::<Option<String>, _>("command_name") {
        let args: Vec<String> = decode_json_field(
            row.get::<Value, _>("command_args_json"),
            "normalized ingress command args",
        )?;
        Some(contracts::CommandHint { command, args })
    } else {
        None
    };

    Ok(NormalizedIngress {
        ingress_id: row.get("ingress_id"),
        channel_kind: parse_channel_kind(row.get::<String, _>("channel_kind").as_str())?,
        external_user_id: row.get("external_user_id"),
        external_conversation_id: row.get("external_conversation_id"),
        external_event_id: row.get("external_event_id"),
        external_message_id: row.get("external_message_id"),
        internal_principal_ref: row
            .get::<Option<String>, _>("internal_principal_ref")
            .context("persisted ingress is missing internal_principal_ref")?,
        internal_conversation_ref: row
            .get::<Option<String>, _>("internal_conversation_ref")
            .context("persisted ingress is missing internal_conversation_ref")?,
        event_kind: parse_ingress_event_kind(row.get::<String, _>("event_kind").as_str())?,
        occurred_at: row.get("occurred_at"),
        text_body: row.get("text_body"),
        reply_to: row
            .get::<Option<String>, _>("reply_to_external_message_id")
            .map(|external_message_id| contracts::ReplyReference {
                external_message_id,
            }),
        attachments: decode_json_field(
            row.get::<Value, _>("attachments_json"),
            "normalized ingress attachments",
        )?,
        command_hint,
        approval_payload: row.get::<Option<String>, _>("approval_token").map(|token| {
            contracts::ApprovalPayload {
                token,
                callback_data: row.get("approval_callback_data"),
            }
        }),
        raw_payload_ref: row.get("raw_payload_ref"),
    })
}

pub fn build_foreground_trigger(
    config: &RuntimeConfig,
    trace_id: Uuid,
    execution_id: Uuid,
    ingress: NormalizedIngress,
) -> Result<ForegroundTrigger> {
    let trigger_kind = infer_foreground_trigger_kind(&ingress);
    build_foreground_trigger_with_kind(config, trace_id, execution_id, trigger_kind, ingress)
}

pub fn build_foreground_trigger_with_kind(
    config: &RuntimeConfig,
    trace_id: Uuid,
    execution_id: Uuid,
    trigger_kind: ForegroundTriggerKind,
    ingress: NormalizedIngress,
) -> Result<ForegroundTrigger> {
    let budget = policy::default_foreground_budget(config);
    policy::validate_foreground_budget(&budget)?;
    let deduplication_key = foreground_deduplication_key(&ingress);

    Ok(ForegroundTrigger {
        trigger_id: Uuid::now_v7(),
        trace_id,
        execution_id,
        trigger_kind,
        ingress,
        received_at: Utc::now(),
        deduplication_key,
        budget,
    })
}

pub fn infer_foreground_trigger_kind(ingress: &NormalizedIngress) -> ForegroundTriggerKind {
    if is_approval_resolution_ingress(ingress) {
        ForegroundTriggerKind::ApprovalResolutionEvent
    } else if ingress
        .external_event_id
        .starts_with(WAKE_SIGNAL_EXTERNAL_EVENT_PREFIX)
        || ingress
            .raw_payload_ref
            .as_deref()
            .is_some_and(|value| value.starts_with(WAKE_SIGNAL_RAW_PAYLOAD_PREFIX))
    {
        ForegroundTriggerKind::ApprovedWakeSignal
    } else {
        ForegroundTriggerKind::UserIngress
    }
}

fn is_approval_resolution_ingress(ingress: &NormalizedIngress) -> bool {
    if ingress
        .approval_payload
        .as_ref()
        .is_some_and(|payload| !payload.token.trim().is_empty())
    {
        return true;
    }

    ingress.command_hint.as_ref().is_some_and(|hint| {
        matches!(
            hint.command.trim(),
            "approve" | "approved" | "reject" | "rejected"
        )
    })
}

pub async fn insert_execution_ingress_link<'e, E>(
    executor: E,
    link: &NewExecutionIngressLink,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO execution_ingress_links (
            execution_ingress_link_id,
            execution_id,
            ingress_id,
            link_role,
            sequence_index,
            created_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            NOW()
        )
        "#,
    )
    .bind(link.execution_ingress_link_id)
    .bind(link.execution_id)
    .bind(link.ingress_id)
    .bind(&link.link_role)
    .bind(link.sequence_index)
    .execute(executor)
    .await
    .context("failed to insert execution ingress link")?;
    Ok(())
}

pub async fn list_execution_ingress_links(
    pool: &PgPool,
    execution_id: Uuid,
) -> Result<Vec<ExecutionIngressLinkRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            execution_ingress_link_id,
            execution_id,
            ingress_id,
            link_role,
            sequence_index,
            created_at
        FROM execution_ingress_links
        WHERE execution_id = $1
        ORDER BY sequence_index ASC
        "#,
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
    .context("failed to list execution ingress links")?;

    Ok(rows
        .into_iter()
        .map(|row| ExecutionIngressLinkRecord {
            execution_ingress_link_id: row.get("execution_ingress_link_id"),
            execution_id: row.get("execution_id"),
            ingress_id: row.get("ingress_id"),
            link_role: row.get("link_role"),
            sequence_index: row.get("sequence_index"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn plan_pending_foreground_execution(
    pool: &PgPool,
    config: &RuntimeConfig,
    trace_id: Uuid,
    execution_id: Uuid,
    internal_conversation_ref: &str,
    options: PendingForegroundExecutionOptions,
) -> Result<Option<PendingForegroundExecutionPlan>> {
    let max_recovery_batch_size = config.continuity.backlog_recovery.max_recovery_batch_size as i64;
    let now = Utc::now();
    let stale_cutoff = now
        - chrono::Duration::seconds(
            config
                .continuity
                .backlog_recovery
                .stale_pending_ingress_age_seconds_threshold as i64,
        );

    let mut tx = pool
        .begin()
        .await
        .context("failed to start pending foreground execution planning transaction")?;

    let pending = list_recoverable_ingress_events_for_conversation_locked(
        &mut tx,
        internal_conversation_ref,
        stale_cutoff,
        max_recovery_batch_size,
    )
    .await?;
    if pending.is_empty() {
        tx.commit()
            .await
            .context("failed to commit empty pending foreground planning transaction")?;
        return Ok(None);
    }

    let decision = evaluate_pending_foreground_execution(
        &config.continuity.backlog_recovery,
        &pending,
        now,
        options.force_recovery,
    );
    let selected = match decision.mode {
        ForegroundExecutionMode::SingleIngress => vec![
            pending
                .first()
                .expect("single-ingress planning requires one pending ingress")
                .clone(),
        ],
        ForegroundExecutionMode::BacklogRecovery => pending.clone(),
    };
    let primary_ingress = match decision.mode {
        ForegroundExecutionMode::SingleIngress => selected
            .first()
            .expect("single-ingress planning requires one selected ingress")
            .clone(),
        ForegroundExecutionMode::BacklogRecovery => selected
            .last()
            .expect("backlog planning requires at least one selected ingress")
            .clone(),
    };
    let ordered_ingress = selected
        .iter()
        .map(|ingress| OrderedIngressReference {
            ingress_id: ingress.ingress_id,
            external_message_id: ingress.external_message_id.clone(),
            occurred_at: ingress.occurred_at,
            text_body: ingress.text_body.clone(),
        })
        .collect::<Vec<_>>();
    let interrupted_execution_id = selected
        .iter()
        .find(|ingress| ingress.foreground_status == "processing")
        .and_then(|ingress| ingress.execution_id);

    for (index, ingress) in selected.iter().enumerate() {
        mark_ingress_event_processing(&mut *tx, ingress.ingress_id, execution_id).await?;
        insert_execution_ingress_link(
            &mut *tx,
            &NewExecutionIngressLink {
                execution_ingress_link_id: Uuid::now_v7(),
                execution_id,
                ingress_id: ingress.ingress_id,
                link_role: if ingress.ingress_id == primary_ingress.ingress_id {
                    "primary".to_string()
                } else {
                    "batch_member".to_string()
                },
                sequence_index: index as i32,
            },
        )
        .await?;
        causal_links::insert(
            &mut *tx,
            &NewCausalLink {
                trace_id,
                source_kind: "ingress_event".to_string(),
                source_id: ingress.ingress_id,
                target_kind: "execution_record".to_string(),
                target_id: execution_id,
                edge_kind: "triggered_execution".to_string(),
                payload: json!({
                    "link_role": if ingress.ingress_id == primary_ingress.ingress_id {
                        "primary"
                    } else {
                        "batch_member"
                    },
                    "sequence_index": index,
                    "planning_mode": match decision.mode {
                        ForegroundExecutionMode::SingleIngress => "single_ingress",
                        ForegroundExecutionMode::BacklogRecovery => "backlog_recovery",
                    },
                }),
            },
        )
        .await?;
    }

    audit::insert(
        &mut *tx,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_recovery".to_string(),
            event_kind: "foreground_recovery_mode_decided".to_string(),
            severity: "info".to_string(),
            trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "internal_conversation_ref": internal_conversation_ref,
                "mode": match decision.mode {
                    ForegroundExecutionMode::SingleIngress => "single_ingress",
                    ForegroundExecutionMode::BacklogRecovery => "backlog_recovery",
                },
                "decision_reason": decision.reason.as_str(),
                "selected_ingress_ids": selected
                    .iter()
                    .map(|ingress| ingress.ingress_id)
                    .collect::<Vec<_>>(),
                "primary_ingress_id": primary_ingress.ingress_id,
                "force_recovery": options.force_recovery,
                "selected_count": selected.len(),
            }),
        },
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit pending foreground execution planning transaction")?;

    Ok(Some(PendingForegroundExecutionPlan {
        mode: decision.mode,
        primary_ingress,
        interrupted_execution_id,
        ordered_ingress,
        decision_reason: decision.reason,
    }))
}

pub async fn list_recoverable_foreground_conversations(
    pool: &PgPool,
    config: &RuntimeConfig,
) -> Result<Vec<String>> {
    let stale_cutoff = Utc::now()
        - chrono::Duration::seconds(
            config
                .continuity
                .backlog_recovery
                .stale_pending_ingress_age_seconds_threshold as i64,
        );

    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT internal_conversation_ref
        FROM ingress_events
        WHERE internal_conversation_ref IS NOT NULL
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $1
              )
          )
        "#,
    )
    .bind(stale_cutoff)
    .fetch_all(pool)
    .await
    .context("failed to list recoverable foreground conversations")?;

    Ok(rows)
}

pub async fn mark_ingress_event_processed(
    pool: &PgPool,
    ingress_id: Uuid,
    execution_id: Uuid,
) -> Result<()> {
    set_ingress_event_foreground_state(pool, ingress_id, Some(execution_id), "processed").await
}

pub async fn mark_ingress_events_processed(
    pool: &PgPool,
    ingress_ids: &[Uuid],
    execution_id: Uuid,
) -> Result<()> {
    for ingress_id in ingress_ids {
        mark_ingress_event_processed(pool, *ingress_id, execution_id).await?;
    }
    Ok(())
}

pub async fn insert_episode(pool: &PgPool, episode: &NewEpisode) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO episodes (
            episode_id,
            trace_id,
            execution_id,
            ingress_id,
            internal_principal_ref,
            internal_conversation_ref,
            trigger_kind,
            trigger_source,
            status,
            started_at,
            completed_at,
            outcome,
            summary
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
            NULL,
            NULL,
            NULL
        )
        "#,
    )
    .bind(episode.episode_id)
    .bind(episode.trace_id)
    .bind(episode.execution_id)
    .bind(episode.ingress_id)
    .bind(&episode.internal_principal_ref)
    .bind(&episode.internal_conversation_ref)
    .bind(&episode.trigger_kind)
    .bind(&episode.trigger_source)
    .bind(&episode.status)
    .bind(episode.started_at)
    .execute(pool)
    .await
    .context("failed to insert episode")?;
    Ok(())
}

pub async fn mark_episode_completed(
    pool: &PgPool,
    episode_id: Uuid,
    outcome: &str,
    summary: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE episodes
        SET
            status = 'completed',
            completed_at = NOW(),
            outcome = $2,
            summary = $3
        WHERE episode_id = $1
        "#,
    )
    .bind(episode_id)
    .bind(outcome)
    .bind(summary)
    .execute(pool)
    .await
    .context("failed to mark episode completed")?;
    Ok(())
}

pub async fn mark_episode_failed(
    pool: &PgPool,
    episode_id: Uuid,
    outcome: &str,
    summary: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE episodes
        SET
            status = 'failed',
            completed_at = NOW(),
            outcome = $2,
            summary = $3
        WHERE episode_id = $1
        "#,
    )
    .bind(episode_id)
    .bind(outcome)
    .bind(summary)
    .execute(pool)
    .await
    .context("failed to mark episode failed")?;
    Ok(())
}

pub async fn get_episode(pool: &PgPool, episode_id: Uuid) -> Result<EpisodeRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            episode_id,
            trace_id,
            execution_id,
            internal_conversation_ref,
            status,
            started_at,
            completed_at,
            outcome,
            summary
        FROM episodes
        WHERE episode_id = $1
        "#,
    )
    .bind(episode_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch episode")?;

    Ok(EpisodeRecord {
        episode_id: row.get("episode_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        status: row.get("status"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        outcome: row.get("outcome"),
        summary: row.get("summary"),
    })
}

pub async fn insert_episode_message(pool: &PgPool, message: &NewEpisodeMessage) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO episode_messages (
            episode_message_id,
            episode_id,
            trace_id,
            execution_id,
            message_order,
            message_role,
            channel_kind,
            text_body,
            external_message_id,
            created_at
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
            NOW()
        )
        "#,
    )
    .bind(message.episode_message_id)
    .bind(message.episode_id)
    .bind(message.trace_id)
    .bind(message.execution_id)
    .bind(message.message_order)
    .bind(&message.message_role)
    .bind(channel_kind_as_str(message.channel_kind))
    .bind(&message.text_body)
    .bind(&message.external_message_id)
    .execute(pool)
    .await
    .context("failed to insert episode message")?;
    Ok(())
}

pub async fn list_episode_messages(
    pool: &PgPool,
    episode_id: Uuid,
) -> Result<Vec<EpisodeMessageRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            episode_message_id,
            episode_id,
            message_order,
            message_role,
            text_body,
            external_message_id
        FROM episode_messages
        WHERE episode_id = $1
        ORDER BY message_order
        "#,
    )
    .bind(episode_id)
    .fetch_all(pool)
    .await
    .context("failed to list episode messages")?;

    Ok(rows
        .into_iter()
        .map(|row| EpisodeMessageRecord {
            episode_message_id: row.get("episode_message_id"),
            episode_id: row.get("episode_id"),
            message_order: row.get("message_order"),
            message_role: row.get("message_role"),
            text_body: row.get("text_body"),
            external_message_id: row.get("external_message_id"),
        })
        .collect())
}

pub async fn update_episode_message_external_message_id(
    pool: &PgPool,
    episode_message_id: Uuid,
    external_message_id: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE episode_messages
        SET external_message_id = $2
        WHERE episode_message_id = $1
        "#,
    )
    .bind(episode_message_id)
    .bind(external_message_id)
    .execute(pool)
    .await
    .context("failed to update episode message external message id")?;
    Ok(())
}

pub async fn list_recent_episode_excerpts(
    pool: &PgPool,
    internal_conversation_ref: &str,
    limit: i64,
) -> Result<Vec<EpisodeExcerpt>> {
    list_recent_episode_excerpts_before(pool, internal_conversation_ref, Utc::now(), limit).await
}

pub async fn list_recent_episode_excerpts_before(
    pool: &PgPool,
    internal_conversation_ref: &str,
    before: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<EpisodeExcerpt>> {
    let rows = sqlx::query(
        r#"
        SELECT
            e.episode_id,
            e.trace_id,
            e.started_at,
            COALESCE(
                (
                    SELECT text_body
                    FROM episode_messages
                    WHERE episode_id = e.episode_id AND message_role = 'user'
                    ORDER BY message_order DESC
                    LIMIT 1
                ),
                NULL
            ) AS user_message,
            COALESCE(
                (
                    SELECT text_body
                    FROM episode_messages
                    WHERE episode_id = e.episode_id AND message_role = 'assistant'
                    ORDER BY message_order DESC
                    LIMIT 1
                ),
                NULL
            ) AS assistant_message,
            COALESCE(e.outcome, e.status) AS outcome
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
    .context("failed to list recent episode excerpts")?;

    Ok(rows
        .into_iter()
        .map(|row| EpisodeExcerpt {
            episode_id: row.get("episode_id"),
            trace_id: row.get("trace_id"),
            started_at: row.get("started_at"),
            user_message: row.get("user_message"),
            assistant_message: row.get("assistant_message"),
            outcome: row.get("outcome"),
        })
        .collect())
}

pub async fn intake_telegram_foreground_trigger(
    pool: &PgPool,
    config: &RuntimeConfig,
    telegram_config: &ResolvedTelegramConfig,
    ingress: NormalizedIngress,
) -> Result<ForegroundTriggerIntakeOutcome> {
    let deduplication_key = foreground_deduplication_key(&ingress);

    if let Some(existing) =
        find_ingress_event_by_channel_event(pool, ingress.channel_kind, &ingress.external_event_id)
            .await?
    {
        return duplicate_foreground_trigger_outcome(pool, &ingress, &deduplication_key, existing)
            .await;
    }

    match policy::evaluate_telegram_foreground_trigger(telegram_config, &ingress) {
        PolicyDecision::Allowed => {}
        PolicyDecision::Denied { reason } => {
            let trace = TraceContext::root();
            let conversation_binding =
                maybe_upsert_matching_conversation_binding(pool, telegram_config, &ingress).await?;
            insert_ingress_event(
                pool,
                &NewIngressEvent {
                    ingress: ingress.clone(),
                    conversation_binding_id: conversation_binding
                        .as_ref()
                        .map(|binding| binding.record.conversation_binding_id),
                    trace_id: trace.trace_id,
                    execution_id: None,
                    status: "rejected".to_string(),
                    rejection_reason: Some(reason.clone()),
                },
            )
            .await?;
            set_ingress_event_foreground_state(pool, ingress.ingress_id, None, "rejected").await?;

            audit::insert(
                pool,
                &NewAuditEvent {
                    loop_kind: "conscious".to_string(),
                    subsystem: "foreground_trigger".to_string(),
                    event_kind: "foreground_trigger_rejected".to_string(),
                    severity: "warn".to_string(),
                    trace_id: trace.trace_id,
                    execution_id: None,
                    worker_pid: None,
                    payload: json!({
                        "ingress_id": ingress.ingress_id,
                        "channel_kind": channel_kind_as_str(ingress.channel_kind),
                        "external_event_id": ingress.external_event_id,
                        "event_kind": ingress_event_kind_as_str(ingress.event_kind),
                        "deduplication_key": deduplication_key,
                        "reason": reason,
                        "conversation_binding_action": conversation_binding
                            .as_ref()
                            .map(|binding| binding.action.as_str()),
                    }),
                },
            )
            .await?;

            return Ok(ForegroundTriggerIntakeOutcome::Rejected(
                RejectedForegroundTrigger {
                    ingress_id: ingress.ingress_id,
                    trace_id: trace.trace_id,
                    reason,
                },
            ));
        }
    }

    let trace = TraceContext::root();
    let execution_id = Uuid::now_v7();
    let trigger = build_foreground_trigger(config, trace.trace_id, execution_id, ingress.clone())?;

    let request_payload = serde_json::to_value(&trigger)
        .context("failed to serialize foreground trigger request payload")?;
    let binding = matching_conversation_binding(telegram_config, &ingress)
        .context("accepted Telegram ingress must match the configured conversation binding")?;
    let mut tx = pool
        .begin()
        .await
        .context("failed to start accepted foreground trigger transaction")?;

    // Accepted trigger creation is all-or-nothing. Execution start, binding reconciliation,
    // ingress persistence, and acceptance audit commit together so failures cannot strand a
    // foreground execution in `started` before orchestration begins.
    execution::insert(
        &mut *tx,
        &NewExecutionRecord {
            execution_id,
            trace_id: trace.trace_id,
            trigger_kind: "telegram_user_ingress".to_string(),
            synthetic_trigger: None,
            status: "started".to_string(),
            request_payload,
        },
    )
    .await?;

    let conversation_binding = reconcile_conversation_binding_in_tx(&mut tx, &binding).await?;
    if let Err(error) = insert_ingress_event(
        &mut *tx,
        &NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: Some(conversation_binding.record.conversation_binding_id),
            trace_id: trace.trace_id,
            execution_id: Some(execution_id),
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await
    {
        if is_unique_violation(&error) {
            drop(tx);
            if let Some(existing) = find_ingress_event_by_channel_event(
                pool,
                ingress.channel_kind,
                &ingress.external_event_id,
            )
            .await?
            {
                return duplicate_foreground_trigger_outcome(
                    pool,
                    &ingress,
                    &deduplication_key,
                    existing,
                )
                .await;
            }
        }
        return Err(error);
    }
    mark_ingress_event_processing(&mut *tx, ingress.ingress_id, execution_id).await?;

    audit::insert(
        &mut *tx,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_trigger".to_string(),
            event_kind: "foreground_trigger_accepted".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: Some(execution_id),
            worker_pid: None,
            payload: json!({
                "ingress_id": ingress.ingress_id,
                "channel_kind": channel_kind_as_str(ingress.channel_kind),
                "external_event_id": ingress.external_event_id,
                "event_kind": ingress_event_kind_as_str(ingress.event_kind),
                "deduplication_key": deduplication_key,
                "budget": {
                    "iteration_budget": trigger.budget.iteration_budget,
                    "wall_clock_budget_ms": trigger.budget.wall_clock_budget_ms,
                    "token_budget": trigger.budget.token_budget,
                },
                "conversation_binding_action": conversation_binding.action.as_str(),
            }),
        },
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit accepted foreground trigger transaction")?;

    Ok(ForegroundTriggerIntakeOutcome::Accepted(Box::new(trigger)))
}

pub async fn stage_telegram_foreground_ingress(
    pool: &PgPool,
    telegram_config: &ResolvedTelegramConfig,
    ingress: NormalizedIngress,
) -> Result<StagedForegroundIngressOutcome> {
    let deduplication_key = foreground_deduplication_key(&ingress);

    if let Some(existing) =
        find_ingress_event_by_channel_event(pool, ingress.channel_kind, &ingress.external_event_id)
            .await?
    {
        return duplicate_foreground_trigger_outcome(pool, &ingress, &deduplication_key, existing)
            .await
            .map(|outcome| match outcome {
                ForegroundTriggerIntakeOutcome::Duplicate(duplicate) => {
                    StagedForegroundIngressOutcome::Duplicate(duplicate)
                }
                ForegroundTriggerIntakeOutcome::Accepted(_) => unreachable!(),
                ForegroundTriggerIntakeOutcome::Rejected(_) => unreachable!(),
            });
    }

    match policy::evaluate_telegram_foreground_trigger(telegram_config, &ingress) {
        PolicyDecision::Allowed => {}
        PolicyDecision::Denied { reason } => {
            let trace = TraceContext::root();
            let conversation_binding =
                maybe_upsert_matching_conversation_binding(pool, telegram_config, &ingress).await?;
            insert_ingress_event(
                pool,
                &NewIngressEvent {
                    ingress: ingress.clone(),
                    conversation_binding_id: conversation_binding
                        .as_ref()
                        .map(|binding| binding.record.conversation_binding_id),
                    trace_id: trace.trace_id,
                    execution_id: None,
                    status: "rejected".to_string(),
                    rejection_reason: Some(reason.clone()),
                },
            )
            .await?;
            set_ingress_event_foreground_state(pool, ingress.ingress_id, None, "rejected").await?;

            audit::insert(
                pool,
                &NewAuditEvent {
                    loop_kind: "conscious".to_string(),
                    subsystem: "foreground_trigger".to_string(),
                    event_kind: "foreground_trigger_rejected".to_string(),
                    severity: "warn".to_string(),
                    trace_id: trace.trace_id,
                    execution_id: None,
                    worker_pid: None,
                    payload: json!({
                        "ingress_id": ingress.ingress_id,
                        "channel_kind": channel_kind_as_str(ingress.channel_kind),
                        "external_event_id": ingress.external_event_id,
                        "event_kind": ingress_event_kind_as_str(ingress.event_kind),
                        "deduplication_key": deduplication_key,
                        "reason": reason,
                        "conversation_binding_action": conversation_binding
                            .as_ref()
                            .map(|binding| binding.action.as_str()),
                    }),
                },
            )
            .await?;

            return Ok(StagedForegroundIngressOutcome::Rejected(
                RejectedForegroundTrigger {
                    ingress_id: ingress.ingress_id,
                    trace_id: trace.trace_id,
                    reason,
                },
            ));
        }
    }

    let trace = TraceContext::root();
    let binding = matching_conversation_binding(telegram_config, &ingress)
        .context("accepted Telegram ingress must match the configured conversation binding")?;
    let mut tx = pool
        .begin()
        .await
        .context("failed to start staged foreground ingress transaction")?;

    let conversation_binding = reconcile_conversation_binding_in_tx(&mut tx, &binding).await?;
    insert_ingress_event(
        &mut *tx,
        &NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: Some(conversation_binding.record.conversation_binding_id),
            trace_id: trace.trace_id,
            execution_id: None,
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await?;

    audit::insert(
        &mut *tx,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_trigger".to_string(),
            event_kind: "foreground_trigger_staged".to_string(),
            severity: "info".to_string(),
            trace_id: trace.trace_id,
            execution_id: None,
            worker_pid: None,
            payload: json!({
                "ingress_id": ingress.ingress_id,
                "channel_kind": channel_kind_as_str(ingress.channel_kind),
                "external_event_id": ingress.external_event_id,
                "event_kind": ingress_event_kind_as_str(ingress.event_kind),
                "deduplication_key": deduplication_key,
                "conversation_binding_action": conversation_binding.action.as_str(),
            }),
        },
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit staged foreground ingress transaction")?;

    Ok(StagedForegroundIngressOutcome::Accepted(
        StagedForegroundIngress {
            ingress_id: ingress.ingress_id,
            trace_id: trace.trace_id,
            internal_conversation_ref: ingress.internal_conversation_ref,
        },
    ))
}

pub async fn stage_approved_wake_signal_foreground_ingress(
    pool: &PgPool,
    binding_config: &TelegramForegroundBindingConfig,
    wake_signal: &background::WakeSignalRecord,
) -> Result<StagedForegroundIngressOutcome> {
    let ingress = build_approved_wake_signal_ingress(binding_config, wake_signal);
    let deduplication_key = foreground_deduplication_key(&ingress);

    if let Some(existing) =
        find_ingress_event_by_channel_event(pool, ingress.channel_kind, &ingress.external_event_id)
            .await?
    {
        return duplicate_foreground_trigger_outcome(pool, &ingress, &deduplication_key, existing)
            .await
            .map(|outcome| match outcome {
                ForegroundTriggerIntakeOutcome::Duplicate(duplicate) => {
                    StagedForegroundIngressOutcome::Duplicate(duplicate)
                }
                ForegroundTriggerIntakeOutcome::Accepted(_) => unreachable!(),
                ForegroundTriggerIntakeOutcome::Rejected(_) => unreachable!(),
            });
    }

    let binding = matching_conversation_binding_for_binding_config(binding_config, &ingress)
        .context("approved wake-signal ingress must match the configured conversation binding")?;
    let mut tx = pool
        .begin()
        .await
        .context("failed to start wake-signal foreground staging transaction")?;

    let conversation_binding = reconcile_conversation_binding_in_tx(&mut tx, &binding).await?;
    insert_ingress_event(
        &mut *tx,
        &NewIngressEvent {
            ingress: ingress.clone(),
            conversation_binding_id: Some(conversation_binding.record.conversation_binding_id),
            trace_id: wake_signal.trace_id,
            execution_id: None,
            status: "accepted".to_string(),
            rejection_reason: None,
        },
    )
    .await?;

    causal_links::insert(
        &mut *tx,
        &NewCausalLink {
            trace_id: wake_signal.trace_id,
            source_kind: "wake_signal".to_string(),
            source_id: wake_signal.wake_signal_id,
            target_kind: "ingress_event".to_string(),
            target_id: ingress.ingress_id,
            edge_kind: "staged_foreground_trigger".to_string(),
            payload: json!({
                "reason_code": wake_signal.signal.reason_code,
                "background_job_id": wake_signal.background_job_id,
                "background_job_run_id": wake_signal.background_job_run_id,
            }),
        },
    )
    .await?;

    audit::insert(
        &mut *tx,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_trigger".to_string(),
            event_kind: "foreground_trigger_staged_from_wake_signal".to_string(),
            severity: "info".to_string(),
            trace_id: wake_signal.trace_id,
            execution_id: wake_signal.execution_id,
            worker_pid: None,
            payload: json!({
                "wake_signal_id": wake_signal.wake_signal_id,
                "background_job_id": wake_signal.background_job_id,
                "background_job_run_id": wake_signal.background_job_run_id,
                "ingress_id": ingress.ingress_id,
                "external_event_id": ingress.external_event_id,
                "deduplication_key": deduplication_key,
                "reason_code": wake_signal.signal.reason_code,
                "conversation_binding_action": conversation_binding.action.as_str(),
            }),
        },
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit wake-signal foreground staging transaction")?;

    Ok(StagedForegroundIngressOutcome::Accepted(
        StagedForegroundIngress {
            ingress_id: ingress.ingress_id,
            trace_id: wake_signal.trace_id,
            internal_conversation_ref: ingress.internal_conversation_ref,
        },
    ))
}

async fn duplicate_foreground_trigger_outcome(
    pool: &PgPool,
    ingress: &NormalizedIngress,
    deduplication_key: &str,
    existing: IngressEventRecord,
) -> Result<ForegroundTriggerIntakeOutcome> {
    audit::insert(
        pool,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "foreground_trigger".to_string(),
            event_kind: "foreground_trigger_duplicate".to_string(),
            severity: "info".to_string(),
            trace_id: existing.trace_id,
            execution_id: existing.execution_id,
            worker_pid: None,
            payload: json!({
                "ingress_id": existing.ingress_id,
                "channel_kind": channel_kind_as_str(ingress.channel_kind),
                "external_event_id": ingress.external_event_id,
                "deduplication_key": deduplication_key,
                "existing_status": existing.status,
            }),
        },
    )
    .await?;

    Ok(ForegroundTriggerIntakeOutcome::Duplicate(
        DuplicateForegroundTrigger {
            ingress_id: existing.ingress_id,
            trace_id: existing.trace_id,
            execution_id: existing.execution_id,
            status: existing.status,
        },
    ))
}

fn channel_kind_as_str(channel_kind: ChannelKind) -> &'static str {
    match channel_kind {
        ChannelKind::Telegram => "telegram",
    }
}

fn parse_channel_kind(value: &str) -> Result<ChannelKind> {
    match value {
        "telegram" => Ok(ChannelKind::Telegram),
        other => anyhow::bail!("unsupported persisted channel_kind '{other}'"),
    }
}

fn ingress_event_kind_as_str(event_kind: IngressEventKind) -> &'static str {
    match event_kind {
        IngressEventKind::MessageCreated => "message_created",
        IngressEventKind::CommandIssued => "command_issued",
        IngressEventKind::ApprovalCallback => "approval_callback",
    }
}

fn parse_ingress_event_kind(value: &str) -> Result<IngressEventKind> {
    match value {
        "message_created" => Ok(IngressEventKind::MessageCreated),
        "command_issued" => Ok(IngressEventKind::CommandIssued),
        "approval_callback" => Ok(IngressEventKind::ApprovalCallback),
        other => anyhow::bail!("unsupported persisted ingress event_kind '{other}'"),
    }
}

fn decode_json_field<T>(value: Value, field_name: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value)
        .with_context(|| format!("failed to decode persisted {field_name} from JSON"))
}

fn foreground_deduplication_key(ingress: &NormalizedIngress) -> String {
    format!(
        "{}:{}",
        channel_kind_as_str(ingress.channel_kind),
        ingress.external_event_id
    )
}

fn build_approved_wake_signal_ingress(
    binding_config: &TelegramForegroundBindingConfig,
    wake_signal: &background::WakeSignalRecord,
) -> NormalizedIngress {
    NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: binding_config.allowed_user_id.to_string(),
        external_conversation_id: binding_config.allowed_chat_id.to_string(),
        external_event_id: format!(
            "{WAKE_SIGNAL_EXTERNAL_EVENT_PREFIX}{}",
            wake_signal.wake_signal_id
        ),
        external_message_id: None,
        internal_principal_ref: binding_config.internal_principal_ref.clone(),
        internal_conversation_ref: binding_config.internal_conversation_ref.clone(),
        event_kind: IngressEventKind::MessageCreated,
        occurred_at: wake_signal.requested_at,
        text_body: Some(format!(
            "A policy-approved maintenance wake signal requires conscious follow-up.\nReason code: {}\nSummary: {}\nUse the maintenance insight to produce one concise user-facing reply if a proactive update is warranted.",
            wake_signal.signal.reason_code, wake_signal.signal.summary
        )),
        reply_to: None,
        attachments: Vec::new(),
        command_hint: None,
        approval_payload: None,
        raw_payload_ref: Some(format!(
            "{WAKE_SIGNAL_RAW_PAYLOAD_PREFIX}{}:background_job:{}:background_run:{}",
            wake_signal.wake_signal_id,
            wake_signal.background_job_id,
            wake_signal
                .background_job_run_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        )),
    }
}

fn is_unique_violation(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .any(|sqlx_error| {
            matches!(
                sqlx_error,
                sqlx::Error::Database(database_error)
                    if database_error.code().as_deref() == Some("23505")
            )
        })
}

async fn find_ingress_event_by_channel_event(
    pool: &PgPool,
    channel_kind: ChannelKind,
    external_event_id: &str,
) -> Result<Option<IngressEventRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            ingress_id,
            trace_id,
            execution_id,
            occurred_at,
            received_at,
            channel_kind,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            external_event_id,
            external_message_id,
            status,
            foreground_status,
            last_processed_at,
            rejection_reason,
            text_body,
            reply_to_external_message_id,
            attachment_count,
            attachments_json,
            command_name,
            command_args_json,
            approval_token,
            approval_callback_data,
            raw_payload_ref
        FROM ingress_events
        WHERE channel_kind = $1 AND external_event_id = $2
        "#,
    )
    .bind(channel_kind_as_str(channel_kind))
    .bind(external_event_id)
    .fetch_optional(pool)
    .await
    .context("failed to look up ingress event by external event id")?;

    row.map(decode_ingress_event_row).transpose()
}

async fn maybe_upsert_matching_conversation_binding(
    pool: &PgPool,
    config: &ResolvedTelegramConfig,
    ingress: &NormalizedIngress,
) -> Result<Option<ConversationBindingWriteResult>> {
    let Some(binding) = matching_conversation_binding(config, ingress) else {
        return Ok(None);
    };

    Ok(Some(reconcile_conversation_binding(pool, &binding).await?))
}

fn matching_conversation_binding(
    config: &ResolvedTelegramConfig,
    ingress: &NormalizedIngress,
) -> Option<NewConversationBinding> {
    matching_conversation_binding_from_parts(
        config.allowed_user_id,
        config.allowed_chat_id,
        &config.internal_principal_ref,
        &config.internal_conversation_ref,
        ingress,
    )
}

fn matching_conversation_binding_for_binding_config(
    config: &TelegramForegroundBindingConfig,
    ingress: &NormalizedIngress,
) -> Option<NewConversationBinding> {
    matching_conversation_binding_from_parts(
        config.allowed_user_id,
        config.allowed_chat_id,
        &config.internal_principal_ref,
        &config.internal_conversation_ref,
        ingress,
    )
}

fn matching_conversation_binding_from_parts(
    allowed_user_id: i64,
    allowed_chat_id: i64,
    internal_principal_ref: &str,
    internal_conversation_ref: &str,
    ingress: &NormalizedIngress,
) -> Option<NewConversationBinding> {
    if ingress.channel_kind != ChannelKind::Telegram {
        return None;
    }
    if ingress.external_user_id != allowed_user_id.to_string() {
        return None;
    }
    if ingress.external_conversation_id != allowed_chat_id.to_string() {
        return None;
    }
    if ingress.internal_principal_ref != internal_principal_ref {
        return None;
    }
    if ingress.internal_conversation_ref != internal_conversation_ref {
        return None;
    }

    Some(NewConversationBinding {
        conversation_binding_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: ingress.external_user_id.clone(),
        external_conversation_id: ingress.external_conversation_id.clone(),
        internal_principal_ref: ingress.internal_principal_ref.clone(),
        internal_conversation_ref: ingress.internal_conversation_ref.clone(),
    })
}

fn decode_ingress_event_row(row: sqlx::postgres::PgRow) -> Result<IngressEventRecord> {
    Ok(IngressEventRecord {
        ingress_id: row.get("ingress_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        occurred_at: row.get("occurred_at"),
        received_at: row.get("received_at"),
        channel_kind: row.get("channel_kind"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        event_kind: row.get("event_kind"),
        external_event_id: row.get("external_event_id"),
        external_message_id: row.get("external_message_id"),
        status: row.get("status"),
        foreground_status: row.get("foreground_status"),
        last_processed_at: row.get("last_processed_at"),
        rejection_reason: row.get("rejection_reason"),
        text_body: row.get("text_body"),
        reply_to_external_message_id: row.get("reply_to_external_message_id"),
        attachment_count: row.get("attachment_count"),
        attachments: decode_json_field(
            row.get::<Value, _>("attachments_json"),
            "ingress attachment metadata",
        )?,
        command_name: row.get("command_name"),
        command_args: decode_json_field(row.get::<Value, _>("command_args_json"), "command args")?,
        approval_token: row.get("approval_token"),
        approval_callback_data: row.get("approval_callback_data"),
        raw_payload_ref: row.get("raw_payload_ref"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingForegroundExecutionDecision {
    mode: ForegroundExecutionMode,
    reason: ForegroundExecutionDecisionReason,
}

fn evaluate_pending_foreground_execution(
    config: &crate::config::BacklogRecoveryConfig,
    pending: &[IngressEventRecord],
    now: DateTime<Utc>,
    force_recovery: bool,
) -> PendingForegroundExecutionDecision {
    if pending.len() < 2 {
        return PendingForegroundExecutionDecision {
            mode: ForegroundExecutionMode::SingleIngress,
            reason: ForegroundExecutionDecisionReason::SingleIngress,
        };
    }

    let oldest = pending
        .first()
        .expect("pending foreground execution evaluation requires a first ingress");
    let newest = pending
        .last()
        .expect("pending foreground execution evaluation requires a last ingress");
    let pending_span_seconds = newest
        .occurred_at
        .signed_duration_since(oldest.occurred_at)
        .num_seconds()
        .max(0) as u64;
    let oldest_touch = pending
        .iter()
        .map(|ingress| ingress.last_processed_at.unwrap_or(ingress.received_at))
        .min()
        .expect("pending foreground execution evaluation requires an oldest touch");
    let stale_pending_age_seconds =
        now.signed_duration_since(oldest_touch).num_seconds().max(0) as u64;
    let resumed_processing_exists = pending
        .iter()
        .any(|ingress| ingress.foreground_status == "processing");

    if force_recovery {
        return PendingForegroundExecutionDecision {
            mode: ForegroundExecutionMode::BacklogRecovery,
            reason: ForegroundExecutionDecisionReason::ForcedRecovery,
        };
    }

    if resumed_processing_exists {
        return PendingForegroundExecutionDecision {
            mode: ForegroundExecutionMode::BacklogRecovery,
            reason: ForegroundExecutionDecisionReason::StaleProcessingResume,
        };
    }

    if pending.len() >= config.pending_message_count_threshold as usize
        && pending_span_seconds >= config.pending_message_span_seconds_threshold
    {
        return PendingForegroundExecutionDecision {
            mode: ForegroundExecutionMode::BacklogRecovery,
            reason: ForegroundExecutionDecisionReason::PendingSpanThreshold,
        };
    }

    if pending.len() >= config.pending_message_count_threshold as usize
        && stale_pending_age_seconds >= config.stale_pending_ingress_age_seconds_threshold
    {
        return PendingForegroundExecutionDecision {
            mode: ForegroundExecutionMode::BacklogRecovery,
            reason: ForegroundExecutionDecisionReason::StalePendingBatch,
        };
    }

    PendingForegroundExecutionDecision {
        mode: ForegroundExecutionMode::SingleIngress,
        reason: ForegroundExecutionDecisionReason::SingleIngress,
    }
}

async fn list_recoverable_ingress_events_for_conversation_locked(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    internal_conversation_ref: &str,
    stale_cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<IngressEventRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT
            ingress_id,
            trace_id,
            execution_id,
            occurred_at,
            received_at,
            channel_kind,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            external_event_id,
            external_message_id,
            status,
            foreground_status,
            last_processed_at,
            rejection_reason,
            text_body,
            reply_to_external_message_id,
            attachment_count,
            attachments_json,
            command_name,
            command_args_json,
            approval_token,
            approval_callback_data,
            raw_payload_ref
        FROM ingress_events
        WHERE internal_conversation_ref = $1
          AND status = 'accepted'
          AND (
              foreground_status = 'pending'
              OR (
                  foreground_status = 'processing'
                  AND COALESCE(last_processed_at, received_at) <= $2
              )
          )
        ORDER BY occurred_at ASC, received_at ASC
        LIMIT $3
        FOR UPDATE
        "#,
    )
    .bind(internal_conversation_ref)
    .bind(stale_cutoff)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .context("failed to list recoverable ingress events for conversation")?;

    rows.into_iter().map(decode_ingress_event_row).collect()
}

async fn mark_ingress_event_processing<'e, E>(
    executor: E,
    ingress_id: Uuid,
    execution_id: Uuid,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    set_ingress_event_foreground_state(executor, ingress_id, Some(execution_id), "processing").await
}

async fn set_ingress_event_foreground_state<'e, E>(
    executor: E,
    ingress_id: Uuid,
    execution_id: Option<Uuid>,
    foreground_status: &str,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE ingress_events
        SET
            execution_id = COALESCE($2, execution_id),
            foreground_status = $3,
            last_processed_at = NOW()
        WHERE ingress_id = $1
        "#,
    )
    .bind(ingress_id)
    .bind(execution_id)
    .bind(foreground_status)
    .execute(executor)
    .await
    .context("failed to update ingress foreground state")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BacklogRecoveryConfig;
    use contracts::{
        ApprovalPayload, AttachmentReference, ChannelKind, CommandHint, IngressEventKind,
    };

    fn sample_normalized_ingress() -> NormalizedIngress {
        NormalizedIngress {
            ingress_id: Uuid::now_v7(),
            channel_kind: ChannelKind::Telegram,
            external_user_id: "42".to_string(),
            external_conversation_id: "24".to_string(),
            external_event_id: "telegram:update:42".to_string(),
            external_message_id: Some("42".to_string()),
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            event_kind: IngressEventKind::MessageCreated,
            occurred_at: Utc::now(),
            text_body: Some("hello".to_string()),
            reply_to: None,
            attachments: Vec::<AttachmentReference>::new(),
            command_hint: None,
            approval_payload: None,
            raw_payload_ref: None,
        }
    }

    fn sample_ingress_event(
        minutes_ago: i64,
        last_processed_minutes_ago: Option<i64>,
    ) -> IngressEventRecord {
        let now = Utc::now();
        IngressEventRecord {
            ingress_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            occurred_at: now - chrono::Duration::minutes(minutes_ago),
            received_at: now - chrono::Duration::minutes(minutes_ago),
            channel_kind: "telegram".to_string(),
            internal_principal_ref: Some("primary-user".to_string()),
            internal_conversation_ref: Some("telegram-primary".to_string()),
            event_kind: "message_created".to_string(),
            external_event_id: format!("event-{minutes_ago}"),
            external_message_id: Some(format!("{minutes_ago}")),
            status: "accepted".to_string(),
            foreground_status: "pending".to_string(),
            last_processed_at: last_processed_minutes_ago
                .map(|value| now - chrono::Duration::minutes(value)),
            rejection_reason: None,
            text_body: Some(format!("message {minutes_ago}")),
            reply_to_external_message_id: None,
            attachment_count: 0,
            attachments: Vec::new(),
            command_name: None,
            command_args: Vec::new(),
            approval_token: None,
            approval_callback_data: None,
            raw_payload_ref: None,
        }
    }

    fn sample_backlog_recovery_config() -> BacklogRecoveryConfig {
        BacklogRecoveryConfig {
            pending_message_count_threshold: 3,
            pending_message_span_seconds_threshold: 120,
            stale_pending_ingress_age_seconds_threshold: 300,
            max_recovery_batch_size: 8,
        }
    }

    #[test]
    fn pending_execution_stays_single_below_threshold() {
        let now = Utc::now();
        let pending = vec![sample_ingress_event(1, None), sample_ingress_event(0, None)];

        let decision = evaluate_pending_foreground_execution(
            &sample_backlog_recovery_config(),
            &pending,
            now,
            false,
        );

        assert_eq!(decision.mode, ForegroundExecutionMode::SingleIngress);
        assert_eq!(
            decision.reason,
            ForegroundExecutionDecisionReason::SingleIngress
        );
    }

    #[test]
    fn pending_execution_switches_to_backlog_on_span_threshold() {
        let now = Utc::now();
        let pending = vec![
            sample_ingress_event(5, None),
            sample_ingress_event(3, None),
            sample_ingress_event(0, None),
        ];

        let decision = evaluate_pending_foreground_execution(
            &sample_backlog_recovery_config(),
            &pending,
            now,
            false,
        );

        assert_eq!(decision.mode, ForegroundExecutionMode::BacklogRecovery);
        assert_eq!(
            decision.reason,
            ForegroundExecutionDecisionReason::PendingSpanThreshold
        );
    }

    #[test]
    fn pending_execution_switches_to_backlog_when_stale_processing_is_resumed() {
        let now = Utc::now();
        let mut resumed = sample_ingress_event(4, Some(10));
        resumed.foreground_status = "processing".to_string();
        let pending = vec![resumed, sample_ingress_event(0, None)];

        let decision = evaluate_pending_foreground_execution(
            &sample_backlog_recovery_config(),
            &pending,
            now,
            false,
        );

        assert_eq!(decision.mode, ForegroundExecutionMode::BacklogRecovery);
        assert_eq!(
            decision.reason,
            ForegroundExecutionDecisionReason::StaleProcessingResume
        );
    }

    #[test]
    fn pending_execution_switches_to_backlog_on_stale_batch() {
        let now = Utc::now();
        let pending = vec![
            sample_ingress_event(10, Some(10)),
            sample_ingress_event(9, None),
            sample_ingress_event(9, None),
        ];

        let decision = evaluate_pending_foreground_execution(
            &sample_backlog_recovery_config(),
            &pending,
            now,
            false,
        );

        assert_eq!(decision.mode, ForegroundExecutionMode::BacklogRecovery);
        assert_eq!(
            decision.reason,
            ForegroundExecutionDecisionReason::StalePendingBatch
        );
    }

    #[test]
    fn pending_execution_switches_to_backlog_when_forced() {
        let now = Utc::now();
        let pending = vec![sample_ingress_event(1, None), sample_ingress_event(0, None)];

        let decision = evaluate_pending_foreground_execution(
            &sample_backlog_recovery_config(),
            &pending,
            now,
            true,
        );

        assert_eq!(decision.mode, ForegroundExecutionMode::BacklogRecovery);
        assert_eq!(
            decision.reason,
            ForegroundExecutionDecisionReason::ForcedRecovery
        );
    }

    #[test]
    fn infer_foreground_trigger_kind_detects_approved_wake_signal() {
        let mut ingress = sample_normalized_ingress();
        ingress.external_event_id = "wake-signal:123".to_string();

        assert_eq!(
            infer_foreground_trigger_kind(&ingress),
            ForegroundTriggerKind::ApprovedWakeSignal
        );
    }

    #[test]
    fn infer_foreground_trigger_kind_detects_approval_resolution_event() {
        let mut ingress = sample_normalized_ingress();
        ingress.event_kind = IngressEventKind::ApprovalCallback;
        ingress.approval_payload = Some(ApprovalPayload {
            token: "approval-token".to_string(),
            callback_data: Some("approve:approval-token".to_string()),
        });

        assert_eq!(
            infer_foreground_trigger_kind(&ingress),
            ForegroundTriggerKind::ApprovalResolutionEvent
        );
    }

    #[test]
    fn infer_foreground_trigger_kind_detects_approval_resolution_command() {
        let mut ingress = sample_normalized_ingress();
        ingress.event_kind = IngressEventKind::CommandIssued;
        ingress.command_hint = Some(CommandHint {
            command: "approve".to_string(),
            args: vec!["approval-token".to_string()],
        });

        assert_eq!(
            infer_foreground_trigger_kind(&ingress),
            ForegroundTriggerKind::ApprovalResolutionEvent
        );
    }
}
