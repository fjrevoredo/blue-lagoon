use anyhow::Result;
use chrono::{TimeZone, Utc};
use contracts::{
    ApprovalPayload, AttachmentReference, ChannelKind, CommandHint, IngressEventKind,
    NormalizedIngress, ReplyReference,
};
use uuid::Uuid;

use crate::{
    config::ResolvedTelegramConfig,
    telegram::{
        TelegramChatKind, TelegramDocument, TelegramMessage, TelegramPhotoSize, TelegramUpdate,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramNormalizationOutcome {
    Accepted(Box<NormalizedIngress>),
    Rejected(TelegramRejectedIngress),
    Ignored(TelegramIgnoredUpdate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramRejectedIngress {
    pub external_event_id: String,
    pub reason: TelegramRejectionReason,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramRejectionReason {
    UnsupportedConversationMode,
    MissingActor,
    UnsupportedActor,
    UnauthorizedActor,
    UnauthorizedConversation,
    InvalidOccurredAt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramIgnoredUpdate {
    pub external_event_id: String,
    pub reason: TelegramIgnoreReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramIgnoreReason {
    UnsupportedUpdateShape,
}

pub fn normalize_telegram_update(
    config: &ResolvedTelegramConfig,
    update: &TelegramUpdate,
    raw_payload_ref: Option<String>,
) -> Result<TelegramNormalizationOutcome> {
    if let Some(message) = &update.message {
        return Ok(normalize_message_update(
            config,
            update,
            message,
            raw_payload_ref,
        ));
    }

    if let Some(callback) = &update.callback_query {
        let Some(message) = &callback.message else {
            return Ok(TelegramNormalizationOutcome::Ignored(
                TelegramIgnoredUpdate {
                    external_event_id: update.update_id.to_string(),
                    reason: TelegramIgnoreReason::UnsupportedUpdateShape,
                },
            ));
        };

        if message.chat.kind != TelegramChatKind::Private {
            return Ok(reject(
                update.update_id,
                TelegramRejectionReason::UnsupportedConversationMode,
                "Telegram callback came from a non-private chat",
            ));
        }

        if callback.from.is_bot {
            return Ok(reject(
                update.update_id,
                TelegramRejectionReason::UnsupportedActor,
                "Telegram callback came from a bot account",
            ));
        }

        if callback.from.id != config.allowed_user_id {
            return Ok(reject(
                update.update_id,
                TelegramRejectionReason::UnauthorizedActor,
                "Telegram callback actor does not match configured single-user identity",
            ));
        }

        if message.chat.id != config.allowed_chat_id {
            return Ok(reject(
                update.update_id,
                TelegramRejectionReason::UnauthorizedConversation,
                "Telegram callback chat does not match configured conversation binding",
            ));
        }

        return Ok(TelegramNormalizationOutcome::Accepted(Box::new(
            NormalizedIngress {
                ingress_id: Uuid::now_v7(),
                channel_kind: ChannelKind::Telegram,
                external_user_id: callback.from.id.to_string(),
                external_conversation_id: message.chat.id.to_string(),
                external_event_id: update.update_id.to_string(),
                external_message_id: Some(message.message_id.to_string()),
                internal_principal_ref: config.internal_principal_ref.clone(),
                internal_conversation_ref: config.internal_conversation_ref.clone(),
                event_kind: IngressEventKind::ApprovalCallback,
                occurred_at: match timestamp_to_utc(message.date) {
                    Some(timestamp) => timestamp,
                    None => {
                        return Ok(reject(
                            update.update_id,
                            TelegramRejectionReason::InvalidOccurredAt,
                            "Telegram callback used an invalid occurred-at timestamp",
                        ));
                    }
                },
                text_body: None,
                reply_to: None,
                attachments: Vec::new(),
                command_hint: None,
                approval_payload: Some(ApprovalPayload {
                    token: callback.id.clone(),
                    callback_data: callback.data.clone(),
                }),
                raw_payload_ref,
            },
        )));
    }

    Ok(TelegramNormalizationOutcome::Ignored(
        TelegramIgnoredUpdate {
            external_event_id: update.update_id.to_string(),
            reason: TelegramIgnoreReason::UnsupportedUpdateShape,
        },
    ))
}

fn normalize_message_update(
    config: &ResolvedTelegramConfig,
    update: &TelegramUpdate,
    message: &TelegramMessage,
    raw_payload_ref: Option<String>,
) -> TelegramNormalizationOutcome {
    if message.chat.kind != TelegramChatKind::Private {
        return reject(
            update.update_id,
            TelegramRejectionReason::UnsupportedConversationMode,
            "Telegram message came from a non-private chat",
        );
    }

    let Some(actor) = &message.from else {
        return reject(
            update.update_id,
            TelegramRejectionReason::MissingActor,
            "Telegram message is missing a sender",
        );
    };

    if actor.is_bot {
        return reject(
            update.update_id,
            TelegramRejectionReason::UnsupportedActor,
            "Telegram message came from a bot account",
        );
    }

    if actor.id != config.allowed_user_id {
        return reject(
            update.update_id,
            TelegramRejectionReason::UnauthorizedActor,
            "Telegram message actor does not match configured single-user identity",
        );
    }

    if message.chat.id != config.allowed_chat_id {
        return reject(
            update.update_id,
            TelegramRejectionReason::UnauthorizedConversation,
            "Telegram message chat does not match configured conversation binding",
        );
    }

    let Some(occurred_at) = timestamp_to_utc(message.date) else {
        return reject(
            update.update_id,
            TelegramRejectionReason::InvalidOccurredAt,
            "Telegram message used an invalid occurred-at timestamp",
        );
    };

    TelegramNormalizationOutcome::Accepted(Box::new(NormalizedIngress {
        ingress_id: Uuid::now_v7(),
        channel_kind: ChannelKind::Telegram,
        external_user_id: actor.id.to_string(),
        external_conversation_id: message.chat.id.to_string(),
        external_event_id: update.update_id.to_string(),
        external_message_id: Some(message.message_id.to_string()),
        internal_principal_ref: config.internal_principal_ref.clone(),
        internal_conversation_ref: config.internal_conversation_ref.clone(),
        event_kind: message_event_kind(message),
        occurred_at,
        text_body: message.text.clone(),
        reply_to: message
            .reply_to_message
            .as_ref()
            .map(|reply| ReplyReference {
                external_message_id: reply.message_id.to_string(),
            }),
        attachments: attachment_references(message),
        command_hint: command_hint(message),
        approval_payload: None,
        raw_payload_ref,
    }))
}

fn message_event_kind(message: &TelegramMessage) -> IngressEventKind {
    match message.text.as_deref() {
        Some(text) if text.trim_start().starts_with('/') => IngressEventKind::CommandIssued,
        _ => IngressEventKind::MessageCreated,
    }
}

fn command_hint(message: &TelegramMessage) -> Option<CommandHint> {
    let text = message.text.as_deref()?.trim();
    if !text.starts_with('/') {
        return None;
    }

    let mut parts = text.split_whitespace();
    let command = parts.next()?.trim_start_matches('/').to_string();
    let args = parts.map(ToString::to_string).collect();
    Some(CommandHint { command, args })
}

fn attachment_references(message: &TelegramMessage) -> Vec<AttachmentReference> {
    let mut attachments = Vec::new();

    if let Some(document) = &message.document {
        attachments.push(document_attachment(document));
    }

    if let Some(photo) = largest_photo(&message.photo) {
        attachments.push(AttachmentReference {
            attachment_id: photo.file_id.clone(),
            media_type: Some("image/jpeg".to_string()),
            file_name: None,
            size_bytes: photo.file_size,
        });
    }

    attachments
}

fn document_attachment(document: &TelegramDocument) -> AttachmentReference {
    AttachmentReference {
        attachment_id: document.file_id.clone(),
        media_type: document.mime_type.clone(),
        file_name: document.file_name.clone(),
        size_bytes: document.file_size,
    }
}

fn largest_photo(photos: &[TelegramPhotoSize]) -> Option<&TelegramPhotoSize> {
    photos.iter().max_by_key(|photo| {
        photo
            .file_size
            .unwrap_or((photo.width as u64) * (photo.height as u64))
    })
}

fn timestamp_to_utc(timestamp: i64) -> Option<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(timestamp, 0).single()
}

fn reject(
    update_id: i64,
    reason: TelegramRejectionReason,
    detail: &str,
) -> TelegramNormalizationOutcome {
    TelegramNormalizationOutcome::Rejected(TelegramRejectedIngress {
        external_event_id: update_id.to_string(),
        reason,
        detail: detail.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::telegram::load_fixture_updates;

    fn fixture(name: &str) -> TelegramUpdate {
        load_fixture_updates(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("telegram")
                .join(name),
        )
        .expect("fixture should load")
        .into_iter()
        .next()
        .expect("fixture should contain one update")
    }

    fn sample_config() -> ResolvedTelegramConfig {
        ResolvedTelegramConfig {
            api_base_url: "https://api.telegram.org".to_string(),
            bot_token: "secret".to_string(),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        }
    }

    #[test]
    fn accepts_private_text_message_and_maps_identifiers() {
        let outcome = normalize_telegram_update(
            &sample_config(),
            &fixture("private_text_message.json"),
            Some("fixtures/private_text_message.json".to_string()),
        )
        .expect("normalization should succeed");

        let TelegramNormalizationOutcome::Accepted(ingress) = outcome else {
            panic!("private message should be accepted");
        };
        assert_eq!(ingress.channel_kind, ChannelKind::Telegram);
        assert_eq!(ingress.external_user_id, "42");
        assert_eq!(ingress.external_conversation_id, "42");
        assert_eq!(ingress.internal_principal_ref, "primary-user");
        assert_eq!(ingress.internal_conversation_ref, "telegram-primary");
        assert_eq!(ingress.event_kind, IngressEventKind::MessageCreated);
        assert_eq!(ingress.text_body.as_deref(), Some("hello from telegram"));
        assert_eq!(
            ingress.reply_to,
            Some(ReplyReference {
                external_message_id: "41".to_string(),
            })
        );
        assert_eq!(
            ingress.raw_payload_ref.as_deref(),
            Some("fixtures/private_text_message.json")
        );
    }

    #[test]
    fn rejects_group_messages_fail_closed() {
        let outcome = normalize_telegram_update(
            &sample_config(),
            &fixture("rejected_group_message.json"),
            None,
        )
        .expect("normalization should succeed");

        let TelegramNormalizationOutcome::Rejected(rejection) = outcome else {
            panic!("group message should be rejected");
        };
        assert_eq!(
            rejection.reason,
            TelegramRejectionReason::UnsupportedConversationMode
        );
    }

    #[test]
    fn parses_command_hints_from_private_messages() {
        let mut update = fixture("private_text_message.json");
        update.message.as_mut().expect("message should exist").text =
            Some("/start now".to_string());

        let outcome = normalize_telegram_update(&sample_config(), &update, None)
            .expect("normalization should succeed");

        let TelegramNormalizationOutcome::Accepted(ingress) = outcome else {
            panic!("command message should be accepted");
        };
        assert_eq!(ingress.event_kind, IngressEventKind::CommandIssued);
        assert_eq!(
            ingress.command_hint,
            Some(CommandHint {
                command: "start".to_string(),
                args: vec!["now".to_string()],
            })
        );
    }

    #[test]
    fn rejects_invalid_timestamps_fail_closed() {
        let mut update = fixture("private_text_message.json");
        update.message.as_mut().expect("message should exist").date = i64::MAX;

        let outcome = normalize_telegram_update(&sample_config(), &update, None)
            .expect("normalization should succeed");

        let TelegramNormalizationOutcome::Rejected(rejection) = outcome else {
            panic!("invalid timestamp should be rejected");
        };
        assert_eq!(rejection.reason, TelegramRejectionReason::InvalidOccurredAt);
    }
}
