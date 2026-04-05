use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use contracts::{
    AttachmentReference, ChannelKind, EpisodeExcerpt, IngressEventKind, NormalizedIngress,
};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
    pub channel_kind: String,
    pub internal_principal_ref: Option<String>,
    pub internal_conversation_ref: Option<String>,
    pub event_kind: String,
    pub external_event_id: String,
    pub external_message_id: Option<String>,
    pub status: String,
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

pub async fn upsert_conversation_binding(
    pool: &PgPool,
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
        ON CONFLICT (channel_kind, external_user_id, external_conversation_id)
        DO UPDATE SET
            internal_principal_ref = EXCLUDED.internal_principal_ref,
            internal_conversation_ref = EXCLUDED.internal_conversation_ref,
            updated_at = NOW()
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
    .fetch_one(pool)
    .await
    .context("failed to upsert conversation binding")?;

    Ok(ConversationBindingRecord {
        conversation_binding_id: row.get("conversation_binding_id"),
        channel_kind: row.get("channel_kind"),
        external_user_id: row.get("external_user_id"),
        external_conversation_id: row.get("external_conversation_id"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
    })
}

pub async fn insert_ingress_event(pool: &PgPool, event: &NewIngressEvent) -> Result<()> {
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
    .execute(pool)
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
            channel_kind,
            internal_principal_ref,
            internal_conversation_ref,
            event_kind,
            external_event_id,
            external_message_id,
            status,
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
        channel_kind: row.get("channel_kind"),
        internal_principal_ref: row.get("internal_principal_ref"),
        internal_conversation_ref: row.get("internal_conversation_ref"),
        event_kind: row.get("event_kind"),
        external_event_id: row.get("external_event_id"),
        external_message_id: row.get("external_message_id"),
        status: row.get("status"),
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

pub async fn list_recent_episode_excerpts(
    pool: &PgPool,
    internal_conversation_ref: &str,
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
        ORDER BY e.started_at DESC
        LIMIT $2
        "#,
    )
    .bind(internal_conversation_ref)
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

fn channel_kind_as_str(channel_kind: ChannelKind) -> &'static str {
    match channel_kind {
        ChannelKind::Telegram => "telegram",
    }
}

fn ingress_event_kind_as_str(event_kind: IngressEventKind) -> &'static str {
    match event_kind {
        IngressEventKind::MessageCreated => "message_created",
        IngressEventKind::CommandIssued => "command_issued",
        IngressEventKind::ApprovalCallback => "approval_callback",
    }
}

fn decode_json_field<T>(value: Value, field_name: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value)
        .with_context(|| format!("failed to decode persisted {field_name} from JSON"))
}
