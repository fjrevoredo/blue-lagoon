use std::{fs, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::GovernedActionRiskTier;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{ApprovalPromptMode, ResolvedTelegramConfig};

const DEFAULT_TELEGRAM_HTTP_TIMEOUT_MS: u64 = 10_000;
const TELEGRAM_MAX_CALLBACK_DATA_BYTES: usize = 64;
const TELEGRAM_PARSE_MODE_HTML: &str = "HTML";

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
    pub reply_markup: Option<TelegramReplyMarkup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramDeliveryReceipt {
    pub chat_id: i64,
    pub message_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramChatAction {
    Typing,
}

impl TelegramChatAction {
    fn as_telegram_api_value(self) -> &'static str {
        match self {
            TelegramChatAction::Typing => "typing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramApprovalPrompt {
    pub token: String,
    pub title: String,
    pub consequence_summary: String,
    pub action_fingerprint: String,
    pub risk_tier: GovernedActionRiskTier,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum TelegramReplyMarkup {
    InlineKeyboard(TelegramInlineKeyboardMarkup),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TelegramInlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<TelegramInlineKeyboardButton>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TelegramInlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[allow(async_fn_in_trait)]
pub trait TelegramUpdateSource {
    async fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>>;
}

#[allow(async_fn_in_trait)]
pub trait TelegramDelivery {
    async fn send_message(
        &mut self,
        message: &TelegramOutboundMessage,
    ) -> Result<TelegramDeliveryReceipt>;

    async fn send_chat_action(&mut self, chat_id: i64, action: TelegramChatAction) -> Result<()>;
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

    pub async fn poll_once(&mut self) -> Result<Vec<TelegramUpdate>> {
        self.source.fetch_updates(self.config.poll_limit).await
    }

    pub async fn send_text(
        &mut self,
        chat_id: i64,
        text: impl Into<String>,
        reply_to_message_id: Option<i64>,
    ) -> Result<TelegramDeliveryReceipt> {
        self.delivery
            .send_message(&TelegramOutboundMessage {
                chat_id,
                text: text.into(),
                reply_to_message_id,
                reply_markup: None,
            })
            .await
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
    async fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>> {
        let take = usize::from(limit);
        let count = self.updates.len().min(take);
        Ok(self.updates.drain(0..count).collect())
    }
}

#[derive(Debug, Default, Clone)]
pub struct FakeTelegramDelivery {
    sent_messages: Vec<TelegramOutboundMessage>,
    sent_chat_actions: Vec<(i64, TelegramChatAction)>,
    next_message_id: i64,
}

impl FakeTelegramDelivery {
    pub fn sent_messages(&self) -> &[TelegramOutboundMessage] {
        &self.sent_messages
    }

    pub fn sent_chat_actions(&self) -> &[(i64, TelegramChatAction)] {
        &self.sent_chat_actions
    }
}

impl TelegramDelivery for FakeTelegramDelivery {
    async fn send_message(
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

    async fn send_chat_action(&mut self, chat_id: i64, action: TelegramChatAction) -> Result<()> {
        self.sent_chat_actions.push((chat_id, action));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ReqwestTelegramSource {
    client: reqwest::Client,
    config: ResolvedTelegramConfig,
    next_offset: Option<i64>,
}

impl ReqwestTelegramSource {
    pub fn new(config: ResolvedTelegramConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(DEFAULT_TELEGRAM_HTTP_TIMEOUT_MS))
                .build()
                .expect("Telegram reqwest client should build"),
            config,
            next_offset: None,
        }
    }
}

impl TelegramUpdateSource for ReqwestTelegramSource {
    async fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>> {
        let mut request_body = serde_json::Map::new();
        request_body.insert("limit".to_string(), json!(limit));
        request_body.insert("timeout".to_string(), json!(0));
        request_body.insert(
            "allowed_updates".to_string(),
            json!(["message", "callback_query"]),
        );
        if let Some(offset) = self.next_offset {
            request_body.insert("offset".to_string(), json!(offset));
        }
        let response = self
            .client
            .post(telegram_api_url(&self.config, "getUpdates"))
            .json(&serde_json::Value::Object(request_body))
            .send()
            .await
            .context("failed to call Telegram getUpdates")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Telegram getUpdates returned HTTP {status}");
        }

        let body: TelegramApiResponse<Vec<TelegramUpdate>> = response
            .json()
            .await
            .context("failed to decode Telegram getUpdates response")?;
        if !body.ok {
            bail!("Telegram getUpdates response marked itself as not ok");
        }
        if let Some(max_update_id) = body.result.iter().map(|update| update.update_id).max() {
            self.next_offset = Some(max_update_id + 1);
        }
        Ok(body.result)
    }
}

#[derive(Debug, Clone)]
pub struct ReqwestTelegramDelivery {
    client: reqwest::Client,
    config: ResolvedTelegramConfig,
}

impl ReqwestTelegramDelivery {
    pub fn new(config: ResolvedTelegramConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(DEFAULT_TELEGRAM_HTTP_TIMEOUT_MS))
                .build()
                .expect("Telegram reqwest client should build"),
            config,
        }
    }
}

impl TelegramDelivery for ReqwestTelegramDelivery {
    async fn send_message(
        &mut self,
        message: &TelegramOutboundMessage,
    ) -> Result<TelegramDeliveryReceipt> {
        let rendered_message = render_telegram_html_message(&message.text);
        let request_body = {
            let mut body = serde_json::Map::new();
            body.insert("chat_id".to_string(), json!(message.chat_id));
            body.insert("text".to_string(), json!(rendered_message));
            body.insert("parse_mode".to_string(), json!(TELEGRAM_PARSE_MODE_HTML));
            if let Some(reply_to_message_id) = message.reply_to_message_id {
                body.insert(
                    "reply_to_message_id".to_string(),
                    json!(reply_to_message_id),
                );
            }
            if let Some(reply_markup) = &message.reply_markup {
                body.insert(
                    "reply_markup".to_string(),
                    serde_json::to_value(reply_markup)
                        .context("failed to encode Telegram reply markup")?,
                );
            }
            serde_json::Value::Object(body)
        };
        let response = self
            .client
            .post(telegram_api_url(&self.config, "sendMessage"))
            .json(&request_body)
            .send()
            .await
            .context("failed to call Telegram sendMessage")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("<failed to read error body: {error}>"));
            bail!("Telegram sendMessage returned HTTP {status}: {body}");
        }

        let body: TelegramApiResponse<TelegramSendMessageResult> = response
            .json()
            .await
            .context("failed to decode Telegram sendMessage response")?;
        if !body.ok {
            bail!("Telegram sendMessage response marked itself as not ok");
        }

        Ok(TelegramDeliveryReceipt {
            chat_id: body.result.chat.id,
            message_id: body.result.message_id,
        })
    }

    async fn send_chat_action(&mut self, chat_id: i64, action: TelegramChatAction) -> Result<()> {
        let request_body = json!({
            "chat_id": chat_id,
            "action": action.as_telegram_api_value(),
        });
        let response = self
            .client
            .post(telegram_api_url(&self.config, "sendChatAction"))
            .json(&request_body)
            .send()
            .await
            .context("failed to call Telegram sendChatAction")?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("<failed to read error body: {error}>"));
            bail!("Telegram sendChatAction returned HTTP {status}: {body}");
        }

        let body: TelegramApiResponse<bool> = response
            .json()
            .await
            .context("failed to decode Telegram sendChatAction response")?;
        if !body.ok {
            bail!("Telegram sendChatAction response marked itself as not ok");
        }
        if !body.result {
            bail!("Telegram sendChatAction response result was false");
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum TelegramFixture {
    Single(Box<TelegramUpdate>),
    Batch(TelegramBatchResponse),
    List(Vec<TelegramUpdate>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TelegramBatchResponse {
    ok: bool,
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TelegramApiResponse<T> {
    ok: bool,
    result: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct TelegramSendMessageResult {
    message_id: i64,
    chat: TelegramChat,
}

pub async fn fetch_updates_once(config: ResolvedTelegramConfig) -> Result<Vec<TelegramUpdate>> {
    let limit = config.poll_limit;
    ReqwestTelegramSource::new(config)
        .fetch_updates(limit)
        .await
}

pub fn build_approval_prompt_message(
    prompt_mode: ApprovalPromptMode,
    chat_id: i64,
    reply_to_message_id: Option<i64>,
    prompt: &TelegramApprovalPrompt,
) -> Result<TelegramOutboundMessage> {
    let approve_callback = format!("approve:{}", prompt.token);
    let reject_callback = format!("reject:{}", prompt.token);
    let callbacks_fit = approve_callback.len() <= TELEGRAM_MAX_CALLBACK_DATA_BYTES
        && reject_callback.len() <= TELEGRAM_MAX_CALLBACK_DATA_BYTES;

    let reply_markup = if callbacks_fit {
        Some(TelegramReplyMarkup::InlineKeyboard(
            TelegramInlineKeyboardMarkup {
                inline_keyboard: vec![vec![
                    TelegramInlineKeyboardButton {
                        text: "Approve".to_string(),
                        callback_data: approve_callback,
                    },
                    TelegramInlineKeyboardButton {
                        text: "Reject".to_string(),
                        callback_data: reject_callback,
                    },
                ]],
            },
        ))
    } else {
        match prompt_mode {
            ApprovalPromptMode::InlineKeyboard => {
                bail!("approval token is too long for Telegram inline callback data");
            }
            ApprovalPromptMode::InlineKeyboardWithFallback => None,
        }
    };

    let mut lines = vec![
        "Approval required".to_string(),
        format!("Action: {}", prompt.title),
        format!(
            "Risk: {}",
            governed_action_risk_tier_label(prompt.risk_tier)
        ),
        format!("Fingerprint: {}", prompt.action_fingerprint),
        format!("Expires: {}", prompt.expires_at.to_rfc3339()),
        format!("Impact: {}", prompt.consequence_summary),
    ];
    if prompt_mode == ApprovalPromptMode::InlineKeyboardWithFallback {
        lines.push(format!(
            "Fallback: send `/approve {}` or `/reject {}` if needed.",
            prompt.token, prompt.token
        ));
    }

    Ok(TelegramOutboundMessage {
        chat_id,
        text: lines.join("\n"),
        reply_to_message_id,
        reply_markup,
    })
}

fn telegram_api_url(config: &ResolvedTelegramConfig, method: &str) -> String {
    format!(
        "{}/bot{}/{}",
        config.api_base_url.trim_end_matches('/'),
        config.bot_token,
        method
    )
}

fn governed_action_risk_tier_label(risk_tier: GovernedActionRiskTier) -> &'static str {
    match risk_tier {
        GovernedActionRiskTier::Tier0 => "tier_0",
        GovernedActionRiskTier::Tier1 => "tier_1",
        GovernedActionRiskTier::Tier2 => "tier_2",
        GovernedActionRiskTier::Tier3 => "tier_3",
    }
}

fn render_telegram_html_message(text: &str) -> String {
    render_inline_markdownish_html(text)
}

fn render_inline_markdownish_html(text: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let remainder = &text[cursor..];
        if let Some(inner_start) = remainder.strip_prefix("**") {
            if let Some(end_offset) = inner_start.find("**") {
                let inner = &inner_start[..end_offset];
                output.push_str("<b>");
                output.push_str(&escape_telegram_html(inner));
                output.push_str("</b>");
                cursor += 2 + end_offset + 2;
                continue;
            }
        }
        if let Some(inner_start) = remainder.strip_prefix('`') {
            if let Some(end_offset) = inner_start.find('`') {
                let inner = &inner_start[..end_offset];
                output.push_str("<code>");
                output.push_str(&escape_telegram_html(inner));
                output.push_str("</code>");
                cursor += 1 + end_offset + 1;
                continue;
            }
        }

        let ch = remainder
            .chars()
            .next()
            .expect("cursor is inside a non-empty string");
        push_escaped_telegram_html_char(&mut output, ch);
        cursor += ch.len_utf8();
    }
    output
}

fn escape_telegram_html(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        push_escaped_telegram_html_char(&mut escaped, ch);
    }
    escaped
}

fn push_escaped_telegram_html_char(output: &mut String, ch: char) {
    match ch {
        '&' => output.push_str("&amp;"),
        '<' => output.push_str("&lt;"),
        '>' => output.push_str("&gt;"),
        '"' => output.push_str("&quot;"),
        _ => output.push(ch),
    }
}

pub fn load_fixture_updates(path: &Path) -> Result<Vec<TelegramUpdate>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read Telegram fixture {}", path.display()))?;
    let fixture: TelegramFixture =
        serde_json::from_str(&raw).context("failed to parse Telegram fixture JSON")?;

    let updates = match fixture {
        TelegramFixture::Single(update) => vec![*update],
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
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        path::PathBuf,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use super::*;

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

    #[tokio::test]
    async fn fixture_source_respects_one_shot_poll_limit() {
        let mut source = FixtureTelegramSource::from_fixture(&fixture_path("private_batch.json"))
            .expect("fixture source should load");
        let first = source
            .fetch_updates(1)
            .await
            .expect("first poll should succeed");
        let second = source
            .fetch_updates(10)
            .await
            .expect("second poll should succeed");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(
            source
                .fetch_updates(10)
                .await
                .expect("third poll should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn fake_delivery_captures_outbound_messages() {
        let source = FixtureTelegramSource::from_updates(vec![]);
        let delivery = FakeTelegramDelivery::default();
        let mut adapter = TelegramAdapter::new(sample_config(), source, delivery);

        let receipt = adapter
            .send_text(42, "reply", Some(7))
            .await
            .expect("delivery should succeed");
        assert_eq!(receipt.chat_id, 42);
        assert_eq!(receipt.message_id, 1);

        let (_source, delivery, _config) = adapter.into_parts();
        assert_eq!(delivery.sent_messages().len(), 1);
        assert_eq!(delivery.sent_messages()[0].text, "reply");
        assert_eq!(delivery.sent_messages()[0].reply_to_message_id, Some(7));
        assert_eq!(delivery.sent_messages()[0].reply_markup, None);
    }

    #[tokio::test]
    async fn fake_delivery_captures_chat_actions() {
        let mut delivery = FakeTelegramDelivery::default();

        delivery
            .send_chat_action(42, TelegramChatAction::Typing)
            .await
            .expect("chat action should succeed");

        assert_eq!(
            delivery.sent_chat_actions(),
            &[(42, TelegramChatAction::Typing)]
        );
    }

    #[test]
    fn approval_prompt_message_renders_inline_buttons_and_fallback_text() {
        let prompt = TelegramApprovalPrompt {
            token: "approval-token-42".to_string(),
            title: "Run scoped subprocess".to_string(),
            consequence_summary: "Writes a bounded file inside the workspace.".to_string(),
            action_fingerprint: "sha256:abc123".to_string(),
            risk_tier: GovernedActionRiskTier::Tier2,
            expires_at: Utc::now(),
        };

        let message = build_approval_prompt_message(
            ApprovalPromptMode::InlineKeyboardWithFallback,
            42,
            Some(7),
            &prompt,
        )
        .expect("approval prompt should build");

        assert!(message.text.contains("Approval required"));
        assert!(message.text.contains("Action: Run scoped subprocess"));
        assert!(message.text.contains("Fingerprint: sha256:abc123"));
        assert!(message.text.contains("/approve approval-token-42"));
        let Some(TelegramReplyMarkup::InlineKeyboard(markup)) = message.reply_markup else {
            panic!("approval prompt should include inline keyboard markup");
        };
        assert_eq!(markup.inline_keyboard.len(), 1);
        assert_eq!(
            markup.inline_keyboard[0][0].callback_data,
            "approve:approval-token-42"
        );
        assert_eq!(
            markup.inline_keyboard[0][1].callback_data,
            "reject:approval-token-42"
        );
    }

    #[test]
    fn approval_prompt_message_falls_back_when_token_exceeds_callback_limit() {
        let long_token = "x".repeat(128);
        let prompt = TelegramApprovalPrompt {
            token: long_token.clone(),
            title: "Run scoped subprocess".to_string(),
            consequence_summary: "Writes a bounded file inside the workspace.".to_string(),
            action_fingerprint: "sha256:abc123".to_string(),
            risk_tier: GovernedActionRiskTier::Tier2,
            expires_at: Utc::now(),
        };

        let message = build_approval_prompt_message(
            ApprovalPromptMode::InlineKeyboardWithFallback,
            42,
            None,
            &prompt,
        )
        .expect("fallback prompt should build");
        assert_eq!(message.reply_markup, None);
        assert!(message.text.contains("/approve"));
        assert!(message.text.contains(&long_token));
    }

    #[test]
    fn telegram_html_renderer_handles_common_model_markdown() {
        let rendered = render_telegram_html_message(
            "**rate.sx** — A cryptocurrency site.\n- **Market Cap:** ~$2.5T\nUse `BTC & ETH` <now>.",
        );

        assert_eq!(
            rendered,
            "<b>rate.sx</b> — A cryptocurrency site.\n- <b>Market Cap:</b> ~$2.5T\nUse <code>BTC &amp; ETH</code> &lt;now&gt;."
        );
    }

    #[test]
    fn telegram_html_renderer_leaves_unmatched_markdown_escaped() {
        let rendered = render_telegram_html_message("Unmatched **bold and <raw> HTML");

        assert_eq!(rendered, "Unmatched **bold and &lt;raw&gt; HTML");
    }

    #[tokio::test]
    async fn reqwest_source_fetches_updates_from_telegram_api() {
        let (api_base_url, receiver, handle) = spawn_single_use_http_server(serde_json::json!({
            "ok": true,
            "result": [{
                "update_id": 42,
                "message": {
                    "message_id": 42,
                    "date": 1_710_000_000,
                    "chat": {
                        "id": 42,
                        "type": "private"
                    },
                    "from": {
                        "id": 42,
                        "is_bot": false,
                        "first_name": "Blue"
                    },
                    "text": "hello from api"
                }
            }]
        }));
        let mut source = ReqwestTelegramSource::new(ResolvedTelegramConfig {
            api_base_url,
            bot_token: "secret".to_string(),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        });

        let updates = source.fetch_updates(10).await.expect("poll should succeed");

        let request = receiver.recv().expect("request should be captured");
        handle.join().expect("server thread should join");

        assert!(request.contains("POST /botsecret/getUpdates HTTP/1.1"));
        assert!(request.contains("\"limit\":10"));
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].update_id, 42);
    }

    #[tokio::test]
    async fn reqwest_delivery_sends_messages_to_telegram_api() {
        let (api_base_url, receiver, handle) = spawn_single_use_http_server(serde_json::json!({
            "ok": true,
            "result": {
                "message_id": 99,
                "chat": {
                    "id": 42,
                    "type": "private"
                }
            }
        }));
        let mut delivery = ReqwestTelegramDelivery::new(ResolvedTelegramConfig {
            api_base_url,
            bot_token: "secret".to_string(),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        });

        let receipt = delivery
            .send_message(&TelegramOutboundMessage {
                chat_id: 42,
                text: "**reply** <ok>".to_string(),
                reply_to_message_id: Some(7),
                reply_markup: Some(TelegramReplyMarkup::InlineKeyboard(
                    TelegramInlineKeyboardMarkup {
                        inline_keyboard: vec![vec![TelegramInlineKeyboardButton {
                            text: "Approve".to_string(),
                            callback_data: "approve:42".to_string(),
                        }]],
                    },
                )),
            })
            .await
            .expect("send should succeed");

        let request = receiver.recv().expect("request should be captured");
        handle.join().expect("server thread should join");

        assert!(request.contains("POST /botsecret/sendMessage HTTP/1.1"));
        assert!(request.contains("\"chat_id\":42"));
        assert!(request.contains("\"reply_to_message_id\":7"));
        assert!(request.contains("\"parse_mode\":\"HTML\""));
        assert!(request.contains("\"text\":\"<b>reply</b> &lt;ok&gt;\""));
        assert!(request.contains("\"reply_markup\""));
        assert!(request.contains("\"inline_keyboard\""));
        assert!(request.contains("\"callback_data\":\"approve:42\""));
        assert_eq!(receipt.chat_id, 42);
        assert_eq!(receipt.message_id, 99);
    }

    #[tokio::test]
    async fn reqwest_delivery_sends_chat_action_to_telegram_api() {
        let (api_base_url, receiver, handle) = spawn_single_use_http_server(serde_json::json!({
            "ok": true,
            "result": true
        }));
        let mut delivery = ReqwestTelegramDelivery::new(ResolvedTelegramConfig {
            api_base_url,
            bot_token: "secret".to_string(),
            allowed_user_id: 42,
            allowed_chat_id: 42,
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            poll_limit: 10,
        });

        delivery
            .send_chat_action(42, TelegramChatAction::Typing)
            .await
            .expect("chat action should send");

        let request = receiver.recv().expect("request should be captured");
        handle.join().expect("server thread should join");

        assert!(request.contains("POST /botsecret/sendChatAction HTTP/1.1"));
        assert!(request.contains("\"chat_id\":42"));
        assert!(request.contains("\"action\":\"typing\""));
    }

    fn spawn_single_use_http_server(
        response_body: serde_json::Value,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should expose address");
        let (sender, receiver) = mpsc::channel();
        let response_text = response_body.to_string();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("stream should accept timeout");
            let reader_stream = stream.try_clone().expect("stream should clone");
            let mut reader = BufReader::new(reader_stream);

            let mut request_head = String::new();
            loop {
                let mut line = String::new();
                let bytes = reader
                    .read_line(&mut line)
                    .expect("request line should read");
                if bytes == 0 {
                    break;
                }
                request_head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }

            let content_length = request_head
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        if name.eq_ignore_ascii_case("content-length") {
                            value.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or(0);

            let mut body = vec![0_u8; content_length];
            reader
                .read_exact(&mut body)
                .expect("request body should read");
            let request = format!("{request_head}{}", String::from_utf8_lossy(&body));
            sender.send(request).expect("request should send to test");

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_text.len(),
                response_text
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should write");
            stream.flush().expect("response should flush");
        });

        (format!("http://{address}"), receiver, handle)
    }
}
