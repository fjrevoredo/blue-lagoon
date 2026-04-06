use std::{fs, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::ResolvedTelegramConfig;

const DEFAULT_TELEGRAM_HTTP_TIMEOUT_MS: u64 = 10_000;

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
    next_message_id: i64,
}

impl FakeTelegramDelivery {
    pub fn sent_messages(&self) -> &[TelegramOutboundMessage] {
        &self.sent_messages
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
}

#[derive(Debug, Clone)]
pub struct ReqwestTelegramSource {
    client: reqwest::Client,
    config: ResolvedTelegramConfig,
}

impl ReqwestTelegramSource {
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

impl TelegramUpdateSource for ReqwestTelegramSource {
    async fn fetch_updates(&mut self, limit: u16) -> Result<Vec<TelegramUpdate>> {
        let response = self
            .client
            .post(telegram_api_url(&self.config, "getUpdates"))
            .json(&json!({
                "limit": limit,
                "timeout": 0,
                "allowed_updates": ["message", "callback_query"],
            }))
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
        let response = self
            .client
            .post(telegram_api_url(&self.config, "sendMessage"))
            .json(&json!({
                "chat_id": message.chat_id,
                "text": message.text,
                "reply_to_message_id": message.reply_to_message_id,
            }))
            .send()
            .await
            .context("failed to call Telegram sendMessage")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Telegram sendMessage returned HTTP {status}");
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

fn telegram_api_url(config: &ResolvedTelegramConfig, method: &str) -> String {
    format!(
        "{}/bot{}/{}",
        config.api_base_url.trim_end_matches('/'),
        config.bot_token,
        method
    )
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
                text: "reply".to_string(),
                reply_to_message_id: Some(7),
            })
            .await
            .expect("send should succeed");

        let request = receiver.recv().expect("request should be captured");
        handle.join().expect("server thread should join");

        assert!(request.contains("POST /botsecret/sendMessage HTTP/1.1"));
        assert!(request.contains("\"chat_id\":42"));
        assert!(request.contains("\"reply_to_message_id\":7"));
        assert!(request.contains("\"text\":\"reply\""));
        assert_eq!(receipt.chat_id, 42);
        assert_eq!(receipt.message_id, 99);
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
