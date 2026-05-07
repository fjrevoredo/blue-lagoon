# Model Providers

---

## 1. Overview

The model provider layer is the harness-owned gateway between Blue Lagoon
worker protocol model-call requests and external OpenAI-compatible chat
completion APIs. Workers construct provider-agnostic `ModelCallRequest` values;
the harness resolves operator configuration, records the call for traceability,
sends the HTTP request, parses the provider response, and returns a
provider-agnostic `ModelCallResponse`.

The committed default remains Z.AI. OpenRouter is supported as an alternate
operator-selected provider for testing many OpenRouter-hosted model IDs through
the same foreground and background gateway path.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/contracts/src/lib.rs` | `ModelProviderKind` (line 1821), `ModelCallRequest` (line 1883), `ModelCallResponse` (line 1900) |
| `crates/harness/src/config.rs` | `ModelGatewayConfig` (line 200), `ForegroundModelRouteConfig` (line 209), `OpenRouterProviderConfig` (line 234), `parse_model_provider_override()` (line 597), `require_foreground_api_base_url()` (line 1010), `foreground_provider_headers()` (line 1055), `foreground_provider_reasoning()` (line 1082) |
| `crates/harness/src/model_gateway.rs` | `ProviderHttpRequest` (line 18), `ReqwestModelProviderTransport` (line 69), `execute_model_call_unchecked()` (line 213), `execute_z_ai_call()` (line 341), `execute_openrouter_call()` (line 369), `openai_compatible_request_body()` (line 401), `parse_openai_compatible_response()` (line 443) |
| `crates/harness/src/model_calls.rs` | `insert_pending_model_call_record()` (line 41), `model_provider_label()` (line 348) |
| `config/default.toml` | committed default model gateway provider and provider-specific defaults |
| `config/local.example.toml` | local OpenRouter override example |
| `.env.example` | direct foreground route and API key override examples |

### Provider Contract

`ModelProviderKind` is serialized into worker protocol payloads, provider hints,
model-call records, and response payloads. Supported wire labels:

| Provider | Serialized label | Route override aliases |
|---|---|---|
| Z.AI | `z_ai` | `z_ai`, `zai`, `z-ai` |
| OpenRouter | `openrouter` | `openrouter`, `open_router`, `open-router` |

`BLUE_LAGOON_FOREGROUND_ROUTE` uses `<provider>/<exact-model>` format. The
parser splits only at the first slash, so OpenRouter model IDs that contain
slashes are preserved. Example:

```text
openrouter/openai/gpt-5.2
```

resolves to provider `openrouter` and model `openai/gpt-5.2`.

### HTTP Request Shape

Both supported providers use the same OpenAI-compatible chat completions body:

| Field | Source |
|---|---|
| `model` | resolved foreground route model |
| `messages[0]` | `system` role containing `ModelInput.system_prompt` |
| `messages[1..]` | `ModelInput.messages`, mapped through `model_role_as_str()` |
| `max_tokens` | `ModelBudget.max_output_tokens` |
| `temperature` | fixed `0.2` |
| `response_format.type` | `json_object` when `ModelOutputMode::JsonObject` is requested |

Request URL is always `{api_base_url}/chat/completions` after trimming trailing
slashes from the configured base URL. Authentication uses bearer auth with the
resolved foreground API key.

OpenRouter support follows the OpenRouter quickstart for direct API usage:
base URL `https://openrouter.ai/api/v1`, bearer authorization, and optional
`HTTP-Referer` and `X-OpenRouter-Title` attribution headers.

For OpenRouter routes, the gateway also injects a provider-specific
`reasoning` object when configured. The committed default is
`{"effort":"none"}`. This follows OpenRouter's documented reasoning controls
and avoids reasoning-capable models consuming the full completion budget on
reasoning tokens and returning `choices[0].message.content = null`.

### Response Parsing

`parse_openai_compatible_response()` reads:

| Response field | Runtime field |
|---|---|
| `choices[0].message.content` | `ModelOutput.text` |
| `choices[0].text` | fallback `ModelOutput.text` when a provider returns the documented non-chat shape |
| `choices[0].finish_reason` | `ModelOutput.finish_reason`, defaulting to `unknown` |
| `usage.prompt_tokens` | `ModelUsage.input_tokens`, defaulting to `0` |
| `usage.completion_tokens` | `ModelUsage.output_tokens`, defaulting to `0` |

For JSON-object calls, the returned message content must parse as JSON or the
gateway returns `InvalidResponse`. Provider error responses are surfaced as
`ProviderRejected` with the provider status and best available error message.
For malformed or rejected provider responses, the raw response body is retained
in `model_call_records.response_payload_json` so `admin trace explain --json`
can show the failing payload.

---

## 3. Configuration & Extension

### Config Keys

| Config key | Default | Valid range | Read by |
|---|---|---|---|
| `model_gateway.foreground.provider` | `z_ai` | `z_ai` or `openrouter` | `config.rs:200`, `config.rs:574` |
| `model_gateway.foreground.model` | `z-ai-foreground` | non-empty exact provider model ID | `config.rs:209`, `config.rs:453` |
| `model_gateway.foreground.api_base_url` | provider-specific | optional non-empty URL override | `config.rs:209`, `config.rs:987` |
| `model_gateway.foreground.api_key_env` | `BLUE_LAGOON_FOREGROUND_API_KEY` | non-empty environment variable name | `config.rs:209`, `config.rs:973` |
| `model_gateway.foreground.timeout_ms` | `60000` | integer greater than zero | `config.rs:209`, `model_gateway.rs:336`, `model_gateway.rs:367` |
| `model_gateway.z_ai.api_surface` | `coding` | `general` or `coding` | `config.rs:222`, `config.rs:1000` |
| `model_gateway.z_ai.api_base_url` | unset | optional non-empty URL | `config.rs:219`, `config.rs:991` |
| `model_gateway.openrouter.api_base_url` | `https://openrouter.ai/api/v1` | optional non-empty URL | `config.rs:234`, `config.rs:1016` |
| `model_gateway.openrouter.http_referer` | unset | optional non-empty string | `config.rs:238`, `config.rs:1042` |
| `model_gateway.openrouter.app_title` | unset | optional non-empty string | `config.rs:240`, `config.rs:1048` |
| `model_gateway.openrouter.reasoning_effort` | `none` | `xhigh`, `high`, `medium`, `low`, `minimal`, `none` | `config.rs:242`, `config.rs:1088` |
| `model_gateway.openrouter.exclude_reasoning` | unset | `true` or `false` | `config.rs:244`, `config.rs:1094` |

### Environment Overrides

| Environment variable | Default | Valid range | Read by |
|---|---|---|---|
| `BLUE_LAGOON_FOREGROUND_ROUTE` | unset | `<provider>/<exact-model>` | `config.rs:333`, `config.rs:586` |
| `BLUE_LAGOON_FOREGROUND_API_KEY` | unset | non-empty secret | `config.rs:973` |
| `BLUE_LAGOON_FOREGROUND_API_BASE_URL` | unset | non-empty URL | `config.rs:988` |

Direct `BLUE_LAGOON_FOREGROUND_API_KEY` takes precedence over the configured
`api_key_env`. Direct `BLUE_LAGOON_FOREGROUND_API_BASE_URL` takes precedence
over provider-specific base URLs.

### OpenRouter Local Example

```toml
[model_gateway.foreground]
provider = "openrouter"
model = "openai/gpt-5.2"
api_key_env = "OPENROUTER_API_KEY"
timeout_ms = 60000

[model_gateway.openrouter]
api_base_url = "https://openrouter.ai/api/v1"
http_referer = "https://example.invalid"
app_title = "Blue Lagoon"
reasoning_effort = "none"
exclude_reasoning = true
```

Equivalent emergency route override:

```text
BLUE_LAGOON_FOREGROUND_ROUTE=openrouter/openai/gpt-5.2
BLUE_LAGOON_FOREGROUND_API_KEY=<secret>
```

### Adding A Provider

1. Add a `ModelProviderKind` variant with an explicit serialized label.
2. Add provider-specific config only for defaults or required headers that do
   not belong in the provider-agnostic foreground route.
3. Extend `parse_model_provider_override()`,
   `require_foreground_api_base_url()`, `foreground_provider_headers()`, and
   `execute_model_call_unchecked()`.
4. Add or reuse a provider-specific request/response adapter in
   `model_gateway.rs`.
5. Add a stable provider label in `model_calls.rs` and management helpers.
6. Add config and gateway tests before running the broader workspace checks.

---

## 4. Further Reading

- `docs/internal/conscious_loop/CONTEXT_ASSEMBLY.md` explains how the model
  input is assembled before the provider gateway sends it.
- `docs/internal/harness/TRACE_EXPLORER.md` explains how model-call records are
  retained and inspected after provider invocation.
- `docs/LOOP_ARCHITECTURE.md` describes the canonical loop boundary that keeps
  provider calls behind the harness.
- [OpenRouter quickstart](https://openrouter.ai/docs/quickstart/llms-full.txt)
  is the reference used for endpoint, auth, and optional attribution header
  behavior.
- [OpenRouter reasoning tokens](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)
  is the reference used for `reasoning.effort` and `reasoning.exclude`.

---

Verified: 2026-05-07.
