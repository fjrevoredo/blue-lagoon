use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::ResolvedTelegramConfig;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<TelegramMessage>,
    #[serde(default)]
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub date: i64,
    pub chat: TelegramChat,
    #[serde(default)]
    pub from: Option<TelegramUser>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub photo: Vec<TelegramPhotoSize>,
    #[serde(default)]
    pub document: Option<TelegramDocument>,
    #[serde(default)]
    pub reply_to_message: Option<Box<TelegramMessage>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: TelegramChatKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramUser {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramPhotoSize {
    pub file_id: String,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TelegramDocument {
    pub file_id: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramOutboundMessage {
    pub chat_id: i64,
    pub text: String,
    pub reply_to_message_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramDeliveryReceipt {
    pub chat_id: i64,
    pub message_id: i64,
}

pub trait TelegramUpdateSource {
    fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>>;
}

pub trait TelegramDelivery {
    fn send_message(
        &mut self,
        message: &TelegramOutboundMessage,
    ) -> Result<TelegramDeliveryReceipt>;
}

pub struct TelegramAdapter<S, D> {
    source: S,
    delivery: D,
    config: ResolvedTelegramConfig,
}

impl<S, D> TelegramAdapter<S, D>
where
    S: TelegramUpdateSource,
    D: TelegramDelivery,
{
    pub fn new(config: ResolvedTelegramConfig, source: S, delivery: D) -> Self {
        Self {
            source,
            delivery,
            config,
        }
    }

    pub fn poll_once(&mut self) -> Result<Vec<TelegramUpdate>> {
        self.source.fetch_updates(self.config.poll_limit)
    }

    pub fn send_text(
        &mut self,
        chat_id: i64,
        text: impl Into<String>,
        reply_to_message_id: Option<i64>,
    ) -> Result<TelegramDeliveryReceipt> {
        self.delivery.send_message(&TelegramOutboundMessage {
            chat_id,
            text: text.into(),
            reply_to_message_id,
        })
    }

    pub fn into_parts(self) -> (S, D, ResolvedTelegramConfig) {
        (self.source, self.delivery, self.config)
    }
}

#[derive(Debug, Clone)]
pub struct FixtureTelegramSource {
    updates: Vec<TelegramUpdate>,
}

impl FixtureTelegramSource {
    pub fn from_updates(updates: Vec<TelegramUpdate>) -> Self {
        Self { updates }
    }

    pub fn from_fixture(path: &Path) -> Result<Self> {
        Ok(Self {
            updates: load_fixture_updates(path)?,
        })
    }
}

impl TelegramUpdateSource for FixtureTelegramSource {
    fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>> {
        let take = usize::from(limit);
        let count = self.updates.len().min(take);
        Ok(self.updates.drain(0..count).collect())
    }
}

#[derive(Debug, Default, Clone)]
pub struct FakeTelegramDelivery {
    sent_messages: Vec<TelegramOutboundMessage>,
    next_message_id: i64,
}

impl FakeTelegramDelivery {
    pub fn sent_messages(&self) -> &[TelegramOutboundMessage] {
        &self.sent_messages
    }
}

impl TelegramDelivery for FakeTelegramDelivery {
    fn send_message(
        &mut self,
        message: &TelegramOutboundMessage,
    ) -> Result<TelegramDeliveryReceipt> {
        self.next_message_id += 1;
        self.sent_messages.push(message.clone());
        Ok(TelegramDeliveryReceipt {
            chat_id: message.chat_id,
            message_id: self.next_message_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum TelegramFixture {
    Single(TelegramUpdate),
    Batch(TelegramBatchResponse),
    List(Vec<TelegramUpdate>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TelegramBatchResponse {
    ok: bool,
    result: Vec<TelegramUpdate>,
}

pub fn load_fixture_updates(path: &Path) -> Result<Vec<TelegramUpdate>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read Telegram fixture {}", path.display()))?;
    let fixture: TelegramFixture =
        serde_json::from_str(&raw).context("failed to parse Telegram fixture JSON")?;

    let updates = match fixture {
        TelegramFixture::Single(update) => vec![update],
        TelegramFixture::List(updates) => updates,
        TelegramFixture::Batch(batch) => {
            if !batch.ok {
                bail!("Telegram fixture batch marked itself as not ok");
            }
            batch.result
        }
    };

    if updates.is_empty() {
        bail!("Telegram fixture did not contain any updates");
    }

    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("telegram")
            .join(name)
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
    fn fixture_loader_reads_private_text_message() {
        let updates = load_fixture_updates(&fixture_path("private_text_message.json"))
            .expect("fixture should load");
        assert_eq!(updates.len(), 1);
        let update = &updates[0];
        assert_eq!(update.update_id, 1001);
        let message = update.message.as_ref().expect("message should exist");
        assert_eq!(message.chat.kind, TelegramChatKind::Private);
        assert_eq!(message.text.as_deref(), Some("hello from telegram"));
    }

    #[test]
    fn fixture_source_respects_one_shot_poll_limit() {
        let mut source = FixtureTelegramSource::from_fixture(&fixture_path("private_batch.json"))
            .expect("fixture source should load");
        let first = source.fetch_updates(1).expect("first poll should succeed");
        let second = source
            .fetch_updates(10)
            .expect("second poll should succeed");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(
            source
                .fetch_updates(10)
                .expect("third poll should succeed")
                .is_empty()
        );
    }

    #[test]
    fn fake_delivery_captures_outbound_messages() {
        let source = FixtureTelegramSource::from_updates(vec![]);
        let delivery = FakeTelegramDelivery::default();
        let mut adapter = TelegramAdapter::new(sample_config(), source, delivery);

        let receipt = adapter
            .send_text(42, "reply", Some(7))
            .expect("delivery should succeed");
        assert_eq!(receipt.chat_id, 42);
        assert_eq!(receipt.message_id, 1);

        let (_source, delivery, _config) = adapter.into_parts();
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].text, "reply");
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(7));
    }
}
