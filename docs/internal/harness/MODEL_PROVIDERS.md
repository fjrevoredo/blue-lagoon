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
the same harness gateway implementation, with separate foreground and
unconscious route configuration.

---

## 2. Implementation

### Source Files

| File | Relevant symbol |
|---|---|
| `crates/contracts/src/lib.rs` | `ModelProviderKind` (line 1821), `ModelCallRequest` (line 1906), `ModelCallResponse` (line 1924) |
| `crates/harness/src/config.rs` | `ModelGatewayConfig` (line 200), `ForegroundModelRouteConfig` (line 210), `ResolvedModelGatewayConfig` (line 285), `ForegroundReasoningMode` (line 222), `OpenRouterProviderConfig` (line 259), `parse_model_route_override()` (line 724), `parse_reasoning_mode_override()` (line 750), `require_route_api_key()` (line 1169), `require_route_api_base_url()` (line 1187), `route_provider_headers()` (line 1234), `resolve_route_reasoning_mode()` (line 1262), `validate_route_reasoning_mode_support()` (line 1282), `route_provider_reasoning()` (line 1310) |
| `crates/harness/src/model_gateway.rs` | `ProviderHttpRequest` (line 18), `ReqwestModelProviderTransport` (line 69), `execute_foreground_model_call()` (line 195), `execute_background_model_call()` (line 204), `execute_model_call_unchecked()` (line 219), `resolve_foreground_route()` (line 314), `resolve_unconscious_route()` (line 322), `execute_z_ai_call()` (line 360), `execute_openrouter_call()` (line 388), `openai_compatible_request_body()` (line 420), `parse_openai_compatible_response()` (line 462) |
| `crates/harness/src/model_calls.rs` | `insert_pending_model_call_record()` (line 41), `model_provider_label()` (line 348) |
| `config/default.toml` | committed default model gateway provider and provider-specific defaults |
| `config/local.example.toml` | local OpenRouter foreground/unconscious override example |
| `.env.example` | direct foreground and unconscious route/API-key override examples |

### Provider Contract

`ModelProviderKind` is serialized into worker protocol payloads, provider hints,
model-call records, and response payloads. Supported wire labels:

| Provider | Serialized label | Route override aliases |
|---|---|---|
| Z.AI | `z_ai` | `z_ai`, `zai`, `z-ai` |
| OpenRouter | `openrouter` | `openrouter`, `open_router`, `open-router` |

`BLUE_LAGOON_FOREGROUND_ROUTE` and `BLUE_LAGOON_UNCONSCIOUS_ROUTE` use
`<provider>/<exact-model>` format. The parser splits only at the first slash,
so OpenRouter model IDs that contain slashes are preserved. Example:

```text
openrouter/openai/gpt-5.2
```

resolves to provider `openrouter` and model `openai/gpt-5.2`.

Foreground reasoning policy is provider-agnostic at the config and resolved
route level. The active vocabulary is:

| Mode | Meaning |
|---|---|
| `off` | actively disable provider-controlled reasoning where the route supports that control |
| `minimal` / `low` / `medium` / `high` / `xhigh` | request an explicit reasoning level |
| `provider_default` | omit reasoning-strength override and accept provider default behavior |

The harness resolves the active reasoning mode before constructing any provider
request. Workers remain unaware of provider-specific reasoning payload shapes.

### HTTP Request Shape

Both supported providers use the same OpenAI-compatible chat completions body:

| Field | Source |
|---|---|
| `model` | resolved route model (foreground or unconscious) |
| `messages[0]` | `system` role containing `ModelInput.system_prompt` |
| `messages[1..]` | `ModelInput.messages`, mapped through `model_role_as_str()` |
| `max_tokens` | `ModelBudget.max_output_tokens` |
| `temperature` | fixed `0.2` |
| `response_format.type` | `json_object` when `ModelOutputMode::JsonObject` is requested |

Request URL is always `{api_base_url}/chat/completions` after trimming trailing
slashes from the configured base URL. Authentication uses bearer auth with the
resolved route API key.

OpenRouter support follows the OpenRouter quickstart for direct API usage:
base URL `https://openrouter.ai/api/v1`, bearer authorization, and optional
`HTTP-Referer` and `X-OpenRouter-Title` attribution headers.

For OpenRouter routes, the gateway derives a provider-specific `reasoning`
object from the resolved route reasoning mode:

| Resolved mode | OpenRouter payload |
|---|---|
| `off` | `{"effort":"none"}` |
| `minimal` | `{"effort":"minimal"}` |
| `low` | `{"effort":"low"}` |
| `medium` | `{"effort":"medium"}` |
| `high` | `{"effort":"high"}` |
| `xhigh` | `{"effort":"xhigh"}` |
| `provider_default` | omit the `effort` override entirely |

If `model_gateway.openrouter.exclude_reasoning` is set, the gateway also adds
`"exclude": true|false` to the OpenRouter `reasoning` object. That knob remains
provider-specific because it changes OpenRouter response shaping, not the
generic reasoning-policy vocabulary.

Current compatibility rules:

- `model_gateway.foreground.reasoning_mode` and
  `model_gateway.unconscious.reasoning_mode` are the primary route contracts.
- `BLUE_LAGOON_FOREGROUND_REASONING_MODE` and
  `BLUE_LAGOON_UNCONSCIOUS_REASONING_MODE` override file config.
- `model_gateway.openrouter.reasoning_effort` remains accepted as a
  compatibility alias only when the selected route reasoning mode is absent.
- Explicit reasoning levels are currently rejected for `z_ai` routes.
- Foreground conscious structured-output routing rejects OpenRouter `auto`
  (`auto` / `openrouter/auto`) and requires a pinned model id in
  `<provider>/<model>` form.

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

When OpenRouter returns `choices[0].message.content = null`, the gateway now
adds a more specific invalid-response detail when it can prove the provider
consumed the budget on reasoning instead of emitting final assistant content.

---

## 3. Configuration & Extension

### Config Keys

| Config key | Default | Valid range | Read by |
|---|---|---|---|
| `model_gateway.foreground.provider` | `z_ai` | `z_ai` or `openrouter` | `crates/harness/src/config.rs:201`, `crates/harness/src/config.rs:556` |
| `model_gateway.foreground.model` | `z-ai-foreground` | non-empty exact provider model ID | `crates/harness/src/config.rs:201`, `crates/harness/src/config.rs:557` |
| `model_gateway.foreground.reasoning_mode` | `off` | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `provider_default` | `crates/harness/src/config.rs:216`, `crates/harness/src/config.rs:1262` |
| `model_gateway.foreground.api_base_url` | provider-specific | optional non-empty URL override | `crates/harness/src/config.rs:213`, `crates/harness/src/config.rs:1187` |
| `model_gateway.foreground.api_key_env` | `BLUE_LAGOON_FOREGROUND_API_KEY` | non-empty environment variable name | `crates/harness/src/config.rs:217`, `crates/harness/src/config.rs:1169` |
| `model_gateway.foreground.timeout_ms` | `60000` | integer greater than zero | `crates/harness/src/config.rs:218`, `crates/harness/src/model_gateway.rs:372`, `crates/harness/src/model_gateway.rs:404` |
| `model_gateway.unconscious.provider` | `z_ai` | `z_ai` or `openrouter` | `crates/harness/src/config.rs:202`, `crates/harness/src/config.rs:579` |
| `model_gateway.unconscious.model` | `z-ai-unconscious` | non-empty exact provider model ID | `crates/harness/src/config.rs:202`, `crates/harness/src/config.rs:580` |
| `model_gateway.unconscious.reasoning_mode` | `off` | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `provider_default` | `crates/harness/src/config.rs:216`, `crates/harness/src/config.rs:1262` |
| `model_gateway.unconscious.api_base_url` | provider-specific | optional non-empty URL override | `crates/harness/src/config.rs:213`, `crates/harness/src/config.rs:1187` |
| `model_gateway.unconscious.api_key_env` | `BLUE_LAGOON_UNCONSCIOUS_API_KEY` | non-empty environment variable name | `crates/harness/src/config.rs:217`, `crates/harness/src/config.rs:1169` |
| `model_gateway.unconscious.timeout_ms` | `60000` | integer greater than zero | `crates/harness/src/config.rs:218`, `crates/harness/src/model_gateway.rs:372`, `crates/harness/src/model_gateway.rs:404` |
| `model_gateway.z_ai.api_surface` | `coding` | `general` or `coding` | `crates/harness/src/config.rs:245`, `crates/harness/src/config.rs:1099` |
| `model_gateway.z_ai.api_base_url` | unset | optional non-empty URL | `crates/harness/src/config.rs:247`, `crates/harness/src/config.rs:1094` |
| `model_gateway.openrouter.api_base_url` | `https://openrouter.ai/api/v1` | optional non-empty URL | `crates/harness/src/config.rs:260`, `crates/harness/src/config.rs:1106` |
| `model_gateway.openrouter.http_referer` | unset | optional non-empty string | `crates/harness/src/config.rs:262`, `crates/harness/src/config.rs:1135` |
| `model_gateway.openrouter.app_title` | unset | optional non-empty string | `crates/harness/src/config.rs:264`, `crates/harness/src/config.rs:1141` |
| `model_gateway.openrouter.reasoning_effort` | unset | compatibility alias: `xhigh`, `high`, `medium`, `low`, `minimal`, `none` | `crates/harness/src/config.rs:266`, `crates/harness/src/config.rs:1163` |
| `model_gateway.openrouter.exclude_reasoning` | unset | `true` or `false` | `crates/harness/src/config.rs:268`, `crates/harness/src/config.rs:1211` |

### Environment Overrides

| Environment variable | Default | Valid range | Read by |
|---|---|---|---|
| `BLUE_LAGOON_FOREGROUND_ROUTE` | unset | `<provider>/<exact-model>` | `crates/harness/src/config.rs:401`, `crates/harness/src/config.rs:743` |
| `BLUE_LAGOON_FOREGROUND_REASONING_MODE` | unset | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `provider_default` | `crates/harness/src/config.rs:406`, `crates/harness/src/config.rs:768` |
| `BLUE_LAGOON_FOREGROUND_API_KEY` | unset | non-empty secret | `crates/harness/src/config.rs:567`, `crates/harness/src/config.rs:1169` |
| `BLUE_LAGOON_FOREGROUND_API_BASE_URL` | unset | non-empty URL | `crates/harness/src/config.rs:562`, `crates/harness/src/config.rs:1187` |
| `BLUE_LAGOON_UNCONSCIOUS_ROUTE` | unset | `<provider>/<exact-model>` | `crates/harness/src/config.rs:411`, `crates/harness/src/config.rs:747` |
| `BLUE_LAGOON_UNCONSCIOUS_REASONING_MODE` | unset | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `provider_default` | `crates/harness/src/config.rs:417`, `crates/harness/src/config.rs:772` |
| `BLUE_LAGOON_UNCONSCIOUS_API_KEY` | unset | non-empty secret | `crates/harness/src/config.rs:590`, `crates/harness/src/config.rs:1169` |
| `BLUE_LAGOON_UNCONSCIOUS_API_BASE_URL` | unset | non-empty URL | `crates/harness/src/config.rs:585`, `crates/harness/src/config.rs:1187` |

Direct `*_API_KEY` and `*_API_BASE_URL` variables take precedence over the
configured `api_key_env` and provider-specific base URLs for the matching
route. Direct `*_REASONING_MODE` variables take precedence over
`model_gateway.<route>.reasoning_mode`.

### OpenRouter Local Example

```toml
[model_gateway.foreground]
provider = "openrouter"
model = "openai/gpt-5.2"
reasoning_mode = "off"
api_key_env = "OPENROUTER_API_KEY"
timeout_ms = 60000

[model_gateway.unconscious]
provider = "openrouter"
model = "openai/gpt-5.2"
reasoning_mode = "off"
api_key_env = "OPENROUTER_API_KEY"
timeout_ms = 60000

[model_gateway.openrouter]
api_base_url = "https://openrouter.ai/api/v1"
http_referer = "https://example.invalid"
app_title = "Blue Lagoon"
exclude_reasoning = true
```

Equivalent emergency route override:

```text
BLUE_LAGOON_FOREGROUND_ROUTE=openrouter/openai/gpt-5.2
BLUE_LAGOON_FOREGROUND_REASONING_MODE=low
BLUE_LAGOON_FOREGROUND_API_KEY=<secret>
BLUE_LAGOON_UNCONSCIOUS_ROUTE=openrouter/openai/gpt-5.2
BLUE_LAGOON_UNCONSCIOUS_REASONING_MODE=minimal
BLUE_LAGOON_UNCONSCIOUS_API_KEY=<secret>
```

### Structured-Output Preflight

Run:

```text
cargo run -p runtime -- admin model preflight-structured-output
```

This performs one foreground conscious `json_object` model call against the
resolved foreground route and fails closed unless the provider returns
`output.json` with non-empty `assistant_text` and (when present) a bounded
`governed_actions` array.

### Adding A Provider

1. Add a `ModelProviderKind` variant with an explicit serialized label.
2. Add provider-specific config only for defaults or required headers that do
   not belong in the provider-agnostic route blocks.
3. Extend the provider-agnostic reasoning policy resolution path before adding
   provider-specific request encoding.
4. Extend `parse_model_route_override()`, `require_route_api_base_url()`,
   `route_provider_headers()`, `resolve_route_reasoning_mode()`, and
   `execute_model_call_unchecked()`.
5. Add or reuse a provider-specific request/response adapter in
   `model_gateway.rs`.
6. Add a stable provider label in `model_calls.rs` and management helpers.
7. Add config and gateway tests before running the broader workspace checks.

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

Verified: 2026-05-17.
