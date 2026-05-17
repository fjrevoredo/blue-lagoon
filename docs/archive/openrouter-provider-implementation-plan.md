# OpenRouter Provider Implementation Plan

Plan Status: COMPLETED
Created: 2026-05-07
Owner: Codex

## Goal

Add OpenRouter as a supported foreground/background model gateway provider so operators can test Blue Lagoon against OpenRouter-hosted model IDs while preserving the current Z.AI behavior.

## Scope

- Extend the provider contract, configuration loading, route override parsing, gateway dispatch, request construction, response parsing, and provider labels.
- Keep the existing Z.AI default route unchanged.
- Document model provider configuration in internal documentation.
- Add focused tests for config parsing/resolution and OpenRouter gateway behavior.

## Non-Goals

- No dynamic multi-provider routing, automatic fallback, or model discovery.
- No changes to governed action policy, worker protocol shape beyond adding the provider enum variant, migrations, or schema.
- No live OpenRouter API call in automated tests.

## Assumptions

- OpenRouter chat completions are OpenAI-compatible at `https://openrouter.ai/api/v1/chat/completions`.
- Authorization uses bearer auth with the configured API key.
- `HTTP-Referer` and `X-OpenRouter-Title` are optional attribution headers and can be omitted.
- `BLUE_LAGOON_FOREGROUND_ROUTE=openrouter/<provider-model-id>` must preserve slashes inside OpenRouter model IDs, for example `openrouter/openai/gpt-5.2`.

## Open Questions

None. The user explicitly requested planning and implementation in the same turn.

## Tasks

### 1. Provider Contract And Config

Status: COMPLETED

Objective: The runtime can parse, validate, and resolve both `z_ai` and `openrouter` foreground routes.

Steps:

- Add `OpenRouter` to `ModelProviderKind` with stable serialized name `openrouter`.
- Add `[model_gateway.openrouter]` config support with optional `api_base_url`, `http_referer`, and `app_title`.
- Resolve the default OpenRouter base URL to `https://openrouter.ai/api/v1`.
- Extend `BLUE_LAGOON_FOREGROUND_ROUTE` parsing to accept `openrouter` and `open_router`.
- Preserve existing direct `BLUE_LAGOON_FOREGROUND_API_KEY` and `BLUE_LAGOON_FOREGROUND_API_BASE_URL` overrides.

Validation:

- `cargo test -p harness config::tests --lib -- --nocapture`
- Inspect `.env.example`, `config/default.toml`, and `config/local.example.toml` for consistent examples.

Notes:

- Do not change the committed default provider from `z_ai`.

### 2. Gateway Dispatch And HTTP Shape

Status: COMPLETED

Objective: OpenRouter requests use the documented endpoint, bearer auth, chat-completions body, optional attribution headers, and existing response parsing.

Steps:

- Add provider-specific dispatch for `ModelProviderKind::OpenRouter`.
- Generalize the OpenAI-compatible request body and response parser currently used for Z.AI.
- Extend `ProviderHttpRequest` and `ReqwestModelProviderTransport` to carry optional headers.
- Add tests asserting OpenRouter URL, model ID with slash, request body, attribution headers, and parsed response provider/model.

Validation:

- `cargo test -p harness model_gateway::tests --lib -- --nocapture`

Notes:

- Keep `Developer` role mapping behavior unchanged unless a test proves incompatibility.

### 3. Persistence And Operator Labels

Status: COMPLETED

Objective: OpenRouter calls persist and render stable provider labels without breaking existing trace/admin flows.

Steps:

- Add `openrouter` labels in model-call persistence and management helpers.
- Update affected tests/helpers that construct `ModelGatewayConfig`.

Validation:

- `cargo test -p harness management::tests --lib -- --nocapture`
- `cargo test -p harness --test management_component -- --nocapture`

Notes:

- No migration is expected because provider is persisted as text.

### 4. Internal Documentation

Status: COMPLETED

Objective: Internal docs explain how to configure and extend model providers, including OpenRouter.

Steps:

- Add `docs/internal/harness/MODEL_PROVIDERS.md` following the internal doc template.
- Update `docs/internal/INTERNAL_DOCUMENTATION.md` folder structure.
- Include config keys, env overrides, provider defaults, extension points, and validation commands.
- Include the OpenRouter documentation reference used for implementation.

Validation:

- Re-read the new doc and verify source paths and config defaults are accurate.

Notes:

- Internal docs must not contradict canonical architecture docs.

### 5. Cleanup And Final Verification

Status: COMPLETED

Objective: The implementation is formatted, tested at the lowest effective layers, and self-checked for regressions.

Steps:

- Run formatting and focused test suites.
- Run broader workspace compilation if focused tests pass.
- Inspect Windows Git status/diff and verify unrelated `logs.txt` remains untouched.
- Re-read changed code/docs for stale names, incorrect defaults, and provider-specific assumptions.
- Mark this plan completed only after final verification.

Validation:

- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo test -p contracts --lib -- --nocapture`
- `cargo test -p harness --lib -- --nocapture`
- Focused integration/component tests when practical.

Notes:

- If a long PostgreSQL-backed suite cannot run in this turn, record that explicitly in the final result.

## Approval Gate

Execution is approved by the user's request to "Create a proper plan for all of this and implement it."

## Self-Check

- Plan location follows the repository `docs/` convention.
- Plan status is `COMPLETED` because implementation and verification are done.
- Scope, non-goals, assumptions, and open questions are explicit.
- Every task has concrete steps and validation.
- Cleanup and final verification are included.
- Implementation self-check completed on 2026-05-07.
- `cargo fmt --all --check`, `cargo check --workspace`, focused provider tests,
  `cargo test -p harness --test management_component -- --nocapture`, and
  `cargo test -p harness --test foreground_component -- --nocapture` passed.
- `cargo test --workspace -- --nocapture --test-threads=1` was attempted twice.
  Both attempts reached `foreground_component` and failed only
  `conscious_worker_protocol_failure_includes_phase_exit_and_stderr`; the exact
  failed test passed when rerun in isolation. This appears to be an existing
  nondeterministic worker-protocol assertion, not an OpenRouter regression.
