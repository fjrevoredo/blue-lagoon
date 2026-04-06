use std::{
    collections::VecDeque,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use contracts::{
    LoopKind, ModelCallPurpose, ModelCallRequest, ModelCallResponse, ModelMessageRole, ModelOutput,
    ModelOutputMode, ModelProviderHint, ModelProviderKind, ModelUsage, ToolPolicy,
};
use serde_json::{Value, json};
use thiserror::Error;

use crate::config::{ResolvedForegroundModelRouteConfig, ResolvedModelGatewayConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHttpRequest {
    pub url: String,
    pub api_key: String,
    pub timeout_ms: u64,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHttpResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Error)]
pub enum ModelGatewayError {
    #[error("model gateway validation failed: {0}")]
    Validation(String),
    #[error("model gateway transport failed: {0}")]
    Transport(String),
    #[error("provider returned status {status}: {message}")]
    ProviderRejected { status: u16, message: String },
    #[error("provider response shape was invalid: {0}")]
    InvalidResponse(String),
}

pub trait ModelProviderTransport {
    fn send_json(
        &self,
        request: ProviderHttpRequest,
    ) -> impl Future<Output = std::result::Result<ProviderHttpResponse, ModelGatewayError>> + Send;
}

#[derive(Debug, Default, Clone)]
pub struct ReqwestModelProviderTransport {
    client: reqwest::Client,
}

impl ReqwestModelProviderTransport {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl ModelProviderTransport for ReqwestModelProviderTransport {
    async fn send_json(
        &self,
        request: ProviderHttpRequest,
    ) -> std::result::Result<ProviderHttpResponse, ModelGatewayError> {
        let response = self
            .client
            .post(&request.url)
            .timeout(Duration::from_millis(request.timeout_ms))
            .bearer_auth(&request.api_key)
            .json(&request.body)
            .send()
            .await
            .map_err(|error| {
                ModelGatewayError::Transport(format_reqwest_transport_error(
                    "send request",
                    &request.url,
                    request.timeout_ms,
                    &error,
                ))
            })?;

        let status = response.status().as_u16();
        let body = response.json::<Value>().await.map_err(|error| {
            ModelGatewayError::Transport(format_reqwest_transport_error(
                "decode response body",
                &request.url,
                request.timeout_ms,
                &error,
            ))
        })?;
        Ok(ProviderHttpResponse { status, body })
    }
}

fn format_reqwest_transport_error(
    action: &str,
    url: &str,
    timeout_ms: u64,
    error: &reqwest::Error,
) -> String {
    let mut message = format!("{action} for {url} failed (timeout_ms={timeout_ms}): {error}");
    let mut source = std::error::Error::source(error);
    while let Some(next) = source {
        message.push_str(": ");
        message.push_str(&next.to_string());
        source = next.source();
    }
    message
}

#[derive(Debug, Clone)]
pub struct FakeModelProviderTransport {
    state: Arc<Mutex<FakeTransportState>>,
}

#[derive(Debug, Default)]
struct FakeTransportState {
    queued_responses: VecDeque<std::result::Result<ProviderHttpResponse, ModelGatewayError>>,
    seen_requests: Vec<ProviderHttpRequest>,
}

impl Default for FakeModelProviderTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeModelProviderTransport {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeTransportState::default())),
        }
    }

    pub fn push_response(
        &self,
        response: std::result::Result<ProviderHttpResponse, ModelGatewayError>,
    ) {
        self.state
            .lock()
            .expect("fake transport mutex should not be poisoned")
            .queued_responses
            .push_back(response);
    }

    pub fn seen_requests(&self) -> Vec<ProviderHttpRequest> {
        self.state
            .lock()
            .expect("fake transport mutex should not be poisoned")
            .seen_requests
            .clone()
    }
}

impl ModelProviderTransport for FakeModelProviderTransport {
    async fn send_json(
        &self,
        request: ProviderHttpRequest,
    ) -> std::result::Result<ProviderHttpResponse, ModelGatewayError> {
        let mut state = self
            .state
            .lock()
            .expect("fake transport mutex should not be poisoned");
        state.seen_requests.push(request);
        state.queued_responses.pop_front().unwrap_or_else(|| {
            Err(ModelGatewayError::Transport(
                "no fake provider response queued".to_string(),
            ))
        })
    }
}

pub async fn execute_foreground_model_call<T: ModelProviderTransport>(
    gateway: &ResolvedModelGatewayConfig,
    request: &ModelCallRequest,
    transport: &T,
) -> std::result::Result<ModelCallResponse, ModelGatewayError> {
    validate_foreground_request(request)?;
    let route = resolve_foreground_route(gateway, request)?;

    match route.provider {
        ModelProviderKind::ZAi => execute_z_ai_call(request, route, transport).await,
    }
}

fn validate_foreground_request(
    request: &ModelCallRequest,
) -> std::result::Result<(), ModelGatewayError> {
    if request.loop_kind != LoopKind::Conscious {
        return Err(ModelGatewayError::Validation(
            "foreground model gateway supports only conscious-loop requests".to_string(),
        ));
    }
    if request.purpose != ModelCallPurpose::ForegroundResponse {
        return Err(ModelGatewayError::Validation(
            "foreground model gateway supports only foreground-response requests".to_string(),
        ));
    }
    if request.budget.max_input_tokens == 0 {
        return Err(ModelGatewayError::Validation(
            "model-call max_input_tokens must be greater than zero".to_string(),
        ));
    }
    if request.budget.max_output_tokens == 0 {
        return Err(ModelGatewayError::Validation(
            "model-call max_output_tokens must be greater than zero".to_string(),
        ));
    }
    if request.budget.timeout_ms == 0 {
        return Err(ModelGatewayError::Validation(
            "model-call timeout_ms must be greater than zero".to_string(),
        ));
    }
    if request.input.system_prompt.trim().is_empty() {
        return Err(ModelGatewayError::Validation(
            "model-call system_prompt must not be empty".to_string(),
        ));
    }
    if request.input.messages.is_empty() {
        return Err(ModelGatewayError::Validation(
            "model-call messages must not be empty".to_string(),
        ));
    }
    if request.output_mode == ModelOutputMode::JsonObject
        && request.schema_name.is_none()
        && request.schema_json.is_none()
    {
        return Err(ModelGatewayError::Validation(
            "json_object model calls must provide schema_name or schema_json".to_string(),
        ));
    }
    if request.tool_policy != ToolPolicy::NoTools && request.tool_policy != ToolPolicy::ProposalOnly
    {
        return Err(ModelGatewayError::Validation(
            "unsupported tool policy".to_string(),
        ));
    }
    Ok(())
}

fn resolve_foreground_route<'a>(
    gateway: &'a ResolvedModelGatewayConfig,
    request: &ModelCallRequest,
) -> std::result::Result<&'a ResolvedForegroundModelRouteConfig, ModelGatewayError> {
    validate_provider_hint(&request.provider_hint, &gateway.foreground)?;
    Ok(&gateway.foreground)
}

fn validate_provider_hint(
    hint: &Option<ModelProviderHint>,
    route: &ResolvedForegroundModelRouteConfig,
) -> std::result::Result<(), ModelGatewayError> {
    let Some(hint) = hint else {
        return Ok(());
    };

    if let Some(provider) = hint.preferred_provider
        && provider != route.provider
    {
        return Err(ModelGatewayError::Validation(format!(
            "provider hint {:?} did not match configured foreground provider {:?}",
            provider, route.provider
        )));
    }

    if let Some(model) = &hint.preferred_model
        && model != &route.model
    {
        return Err(ModelGatewayError::Validation(format!(
            "provider hint model '{model}' did not match configured foreground model '{}'",
            route.model
        )));
    }

    Ok(())
}

async fn execute_z_ai_call<T: ModelProviderTransport>(
    request: &ModelCallRequest,
    route: &ResolvedForegroundModelRouteConfig,
    transport: &T,
) -> std::result::Result<ModelCallResponse, ModelGatewayError> {
    let http_request = ProviderHttpRequest {
        url: format!(
            "{}/chat/completions",
            route.api_base_url.trim_end_matches('/')
        ),
        api_key: route.api_key.clone(),
        timeout_ms: request.budget.timeout_ms.min(route.timeout_ms),
        body: z_ai_request_body(request, route),
    };

    let response = transport.send_json(http_request).await?;
    if !(200..300).contains(&response.status) {
        return Err(ModelGatewayError::ProviderRejected {
            status: response.status,
            message: provider_error_message(&response.body),
        });
    }

    parse_z_ai_response(request, route, response.body)
}

fn z_ai_request_body(
    request: &ModelCallRequest,
    route: &ResolvedForegroundModelRouteConfig,
) -> Value {
    let mut messages = Vec::with_capacity(request.input.messages.len() + 1);
    messages.push(json!({
        "role": "system",
        "content": request.input.system_prompt,
    }));
    for message in &request.input.messages {
        messages.push(json!({
            "role": model_role_as_str(message.role),
            "content": message.content,
        }));
    }

    let mut body = json!({
        "model": route.model,
        "messages": messages,
        "max_tokens": request.budget.max_output_tokens,
        "temperature": 0.2,
    });
    if request.output_mode == ModelOutputMode::JsonObject {
        body["response_format"] = json!({
            "type": "json_object"
        });
    }
    body
}

fn parse_z_ai_response(
    request: &ModelCallRequest,
    route: &ResolvedForegroundModelRouteConfig,
    body: Value,
) -> std::result::Result<ModelCallResponse, ModelGatewayError> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| {
            ModelGatewayError::InvalidResponse(
                "missing choices[0] in provider response".to_string(),
            )
        })?;

    let text = extract_message_text(choice).ok_or_else(|| {
        ModelGatewayError::InvalidResponse(
            "missing choices[0].message.content in provider response".to_string(),
        )
    })?;

    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let usage = body.get("usage");
    let input_tokens = usage
        .and_then(|usage| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let json_output = match request.output_mode {
        ModelOutputMode::PlainText => None,
        ModelOutputMode::JsonObject => {
            Some(serde_json::from_str::<Value>(&text).map_err(|error| {
                ModelGatewayError::InvalidResponse(format!(
                    "provider returned non-JSON content for json_object mode: {error}"
                ))
            })?)
        }
    };

    Ok(ModelCallResponse {
        request_id: request.request_id,
        trace_id: request.trace_id,
        execution_id: request.execution_id,
        provider: route.provider,
        model: route.model.clone(),
        received_at: chrono::Utc::now(),
        output: ModelOutput {
            text,
            json: json_output,
            finish_reason,
        },
        usage: ModelUsage {
            input_tokens,
            output_tokens,
        },
    })
}

fn extract_message_text(choice: &Value) -> Option<String> {
    let content = choice.get("message")?.get("content")?;
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let combined = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            if combined.is_empty() {
                None
            } else {
                Some(combined)
            }
        }
        _ => None,
    }
}

fn provider_error_message(body: &Value) -> String {
    body.get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .unwrap_or("provider request failed")
        .to_string()
}

fn model_role_as_str(role: ModelMessageRole) -> &'static str {
    match role {
        ModelMessageRole::System => "system",
        ModelMessageRole::Developer => "system",
        ModelMessageRole::User => "user",
        ModelMessageRole::Assistant => "assistant",
    }
}

#[cfg(test)]
mod tests {
    use contracts::{ModelBudget, ModelInput, ModelInputMessage, ModelOutputMode};

    use super::*;

    #[tokio::test]
    async fn executes_foreground_call_through_zai_route() {
        let gateway = sample_gateway();
        let request = sample_request();
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 200,
            body: json!({
                "choices": [{
                    "message": { "content": "hello from provider" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 5
                }
            }),
        }));

        let response = execute_foreground_model_call(&gateway, &request, &transport)
            .await
            .expect("gateway call should succeed");

        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.provider, ModelProviderKind::ZAi);
        assert_eq!(response.model, "z-ai-foreground");
        assert_eq!(response.output.text, "hello from provider");
        assert_eq!(response.usage.input_tokens, 12);

        let seen = transport.seen_requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].url, "https://api.z.ai/api/paas/v4/chat/completions");
        assert_eq!(seen[0].timeout_ms, 20_000);
        assert_eq!(
            seen[0].body.get("model").and_then(Value::as_str),
            Some("z-ai-foreground")
        );
    }

    #[tokio::test]
    async fn rejects_provider_hint_mismatch() {
        let gateway = sample_gateway();
        let mut request = sample_request();
        request.provider_hint = Some(ModelProviderHint {
            preferred_provider: Some(ModelProviderKind::ZAi),
            preferred_model: Some("different-model".to_string()),
        });

        let error =
            execute_foreground_model_call(&gateway, &request, &FakeModelProviderTransport::new())
                .await
                .expect_err("mismatched provider hint should fail");
        assert!(error.to_string().contains("different-model"));
    }

    #[tokio::test]
    async fn rejects_provider_error_responses() {
        let gateway = sample_gateway();
        let request = sample_request();
        let transport = FakeModelProviderTransport::new();
        transport.push_response(Ok(ProviderHttpResponse {
            status: 429,
            body: json!({
                "error": { "message": "rate limit" }
            }),
        }));

        let error = execute_foreground_model_call(&gateway, &request, &transport)
            .await
            .expect_err("provider error should surface");
        assert!(error.to_string().contains("429"));
        assert!(error.to_string().contains("rate limit"));
    }

    #[tokio::test]
    async fn rejects_invalid_request_shape_before_transport() {
        let gateway = sample_gateway();
        let mut request = sample_request();
        request.input.messages.clear();

        let error =
            execute_foreground_model_call(&gateway, &request, &FakeModelProviderTransport::new())
                .await
                .expect_err("invalid request should fail");
        assert!(error.to_string().contains("messages"));
    }

    fn sample_gateway() -> ResolvedModelGatewayConfig {
        ResolvedModelGatewayConfig {
            foreground: ResolvedForegroundModelRouteConfig {
                provider: ModelProviderKind::ZAi,
                model: "z-ai-foreground".to_string(),
                api_base_url: "https://api.z.ai/api/paas/v4".to_string(),
                api_key: "secret".to_string(),
                timeout_ms: 20_000,
            },
        }
    }

    fn sample_request() -> ModelCallRequest {
        ModelCallRequest {
            request_id: uuid::Uuid::now_v7(),
            trace_id: uuid::Uuid::now_v7(),
            execution_id: uuid::Uuid::now_v7(),
            loop_kind: LoopKind::Conscious,
            purpose: ModelCallPurpose::ForegroundResponse,
            task_class: "telegram_foreground_reply".to_string(),
            budget: ModelBudget {
                max_input_tokens: 2_000,
                max_output_tokens: 600,
                timeout_ms: 30_000,
            },
            input: ModelInput {
                system_prompt: "You are Blue Lagoon.".to_string(),
                messages: vec![
                    ModelInputMessage {
                        role: ModelMessageRole::Developer,
                        content: "Stay concise.".to_string(),
                    },
                    ModelInputMessage {
                        role: ModelMessageRole::User,
                        content: "hello".to_string(),
                    },
                ],
            },
            output_mode: ModelOutputMode::PlainText,
            schema_name: None,
            schema_json: None,
            tool_policy: ToolPolicy::NoTools,
            provider_hint: None,
        }
    }
}
