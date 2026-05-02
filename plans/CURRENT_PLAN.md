# Implementation Plan: Built-in Chat Completions Provider

## Overview
为 `noloong-agent-core` 增加一个内置 OpenAI-compatible Chat Completions provider，同时把现有 text-only thinking 模型升级为结构化 thinking。目标不是把 OpenRouter 或 DeepSeek 写死进 core，而是提供一个稳定的 Rust 内置 provider、可配置的 Chat Completions wire adapter、可扩展的 thinking extraction/replay 机制，以及一个必须通过的 OpenRouter DeepSeek official live gate。

## Implementation Status
本计划已落地到当前工作区。最终 gate 已覆盖 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`、`cargo test --workspace`、`cargo test -p noloong-agent-core --examples`、三个 Node fixture `node --check`，以及 manual OpenRouter live gate `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`。

## Architecture Decisions
- Provider 形态：新增内置 `ChatCompletionsProvider`，实现现有 `ModelProvider` trait；外部 JS/TS/Python provider 仍通过 JSON-RPC 扩展接入，二者并行存在。
- 协议目标：实现 OpenAI Chat Completions 兼容协议的 streaming SSE path，默认 POST 到 `{base_url}/chat/completions`，而不是引入新的 runtime phase。
- Thinking 模型：把 `ContentBlock::Thinking { text }` 和 `ModelStreamEvent::ThinkingDelta { text }` 升级为结构化类型，支持 display text、raw JSON/object/list、summary、redacted/encrypted/replay metadata。
- Thinking extraction：内置识别 `reasoning_content`, `reasoning`, `reasoning_text`, `reasoning_details`，同时保留配置式 field rules，避免把 vendor-specific 字段散落在 core loop。
- Replay 策略：同 provider/model 下允许把 replay descriptor 写回 assistant message；跨 provider/model 默认只保留可展示 summary/text，不泄露 raw thinking；descriptor scope 包含 `providerId` 和 `model`。
- OpenRouter live gate：必须使用 `OPENROUTER_API_KEY`、model `deepseek/deepseek-v4-flash`、provider `deepseek` official-only、thinking enabled；这些 provider-specific 参数只在测试/调用方 config 中组装，不进入 core provider 硬编码 preset；该测试保持 ignored/manual，不进入默认 CI。
- HTTP 依赖：执行实现前先用 `cargo search --registry crates-io` 确认主流版本；优先使用 `reqwest`，SSE parser 优先手写轻量 parser，只有复杂度明显上升才引入专门 SSE crate。

## Dependency Graph
1. Structured thinking public API
2. Assistant commit/reducer/test migration
3. Chat Completions config and payload model
4. SSE transport and chunk parser
5. Thinking extraction/replay
6. Tool call streaming accumulation
7. OpenRouter DeepSeek caller-side config and live test
8. Docs, examples, lint/test cleanup

## Task List

### Phase 1: Thinking Foundation

#### Task 1: Introduce Structured Thinking Types
**Description:** Replace text-only thinking with first-class structured thinking types that can represent provider raw payloads, summaries, replay descriptors, and redacted placeholders without forcing every provider into plain text.
**Acceptance criteria:**
- [ ] `ContentBlock::Thinking` stores a `ThinkingBlock`, not only a `String`
- [ ] `ModelStreamEvent::ThinkingDelta` stores a `ThinkingDelta`, not only a `String`
- [ ] Types serialize with stable camelCase fields and deserialize through serde without custom callers needing manual JSON handling
- [ ] Text-only thinking remains easy to construct through helper constructors
**Verification:**
- [ ] `cargo test -p noloong-agent-core thinking_type_serde`
- [ ] `cargo test -p noloong-agent-core --test core`
**Dependencies:** None
**Files likely touched:**
- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/tests/core.rs`
**Estimated scope:** M

#### Task 2: Migrate Assistant Commit and Existing Tests
**Description:** Update assistant commit logic so thinking deltas are accumulated into coherent `ThinkingBlock` content while preserving existing text/tool ordering semantics.
**Acceptance criteria:**
- [ ] Adjacent text thinking deltas commit into one thinking block before text/tool blocks
- [ ] JSON/raw thinking deltas preserve latest raw snapshot and append display text when available
- [ ] Tool call boundaries close any open thinking/text accumulation before committing tool calls
- [ ] Existing OpenRouter fixture and conformance tests are migrated to structured thinking assertions
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test conformance`
- [ ] `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`
**Dependencies:** Task 1
**Files likely touched:**
- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/conformance.rs`
- `crates/noloong-agent-core/tests/openrouter_live.rs`
- `crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs`
**Estimated scope:** M

#### Checkpoint: Structured Thinking
- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `cargo test -p noloong-agent-core --test conformance`

### Phase 2: Chat Completions Provider

#### Task 3: Add Provider Configuration and Builder API
**Description:** Add a Rust-native `ChatCompletionsProvider` with explicit config for base URL, model, auth, headers, request body extensions, timeout, stream idle timeout, and thinking behavior.
**Acceptance criteria:**
- [ ] Provider implements `ModelProvider` and can be registered through existing `AgentRuntimeBuilder::with_model_provider`
- [ ] Config supports API key from environment without exposing secrets in events or metadata
- [ ] Config supports static headers and extra JSON body fields for compatible providers
- [ ] `lib.rs` exports the provider and config types
**Verification:**
- [ ] `cargo test -p noloong-agent-core chat_completions_config`
- [ ] `cargo test -p noloong-agent-core --test core`
**Dependencies:** Task 1
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/Cargo.toml`
- `Cargo.toml`
**Estimated scope:** M

#### Task 4: Build Chat Completions Payload Mapping
**Description:** Convert `ModelRequest` into an OpenAI-compatible Chat Completions request body, including messages, tools, token limits, temperature, and provider-specific extra body fields.
**Acceptance criteria:**
- [ ] `System`, `User`, `Assistant`, and `ToolResult` messages map to valid Chat Completions roles
- [ ] `ToolSpec` maps to `tools: [{ type: "function", function: ... }]`
- [ ] Assistant tool calls replay with `tool_calls` and JSON-stringified arguments
- [ ] Structured thinking replay is only included when the descriptor says it is valid for the same provider family
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_`
**Dependencies:** Tasks 1, 3
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
**Estimated scope:** M

#### Task 5: Implement Streaming SSE Transport
**Description:** Implement the network streaming path for Chat Completions using SSE frames, cancellation, stream idle timeout, structured HTTP errors, and terminal `[DONE]` handling.
**Acceptance criteria:**
- [ ] Provider emits `Started` before first content event
- [ ] Provider parses `data: {json}` frames and ignores comments/empty SSE lines
- [ ] Provider treats `data: [DONE]` as stream completion when no explicit terminal chunk arrives
- [ ] Cancellation returns `AgentCoreError::Aborted`
- [ ] Non-2xx responses return a structured provider error that includes status and body excerpt, without logging API keys
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions sse_`
- [ ] `cargo test -p noloong-agent-core --test chat_completions http_error_`
**Dependencies:** Task 3
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/Cargo.toml`
**Estimated scope:** M

#### Checkpoint: Provider Skeleton
- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_ sse_`
- [ ] `cargo test -p noloong-agent-core --test core`

### Phase 3: Stream Semantics

#### Task 6: Parse Text, Finish Reasons, and Usage
**Description:** Map Chat Completions chunks into core stream events for normal text responses, finish reasons, and optional usage metadata without changing the core event ordering contract.
**Acceptance criteria:**
- [ ] `choices[].delta.content` emits `ModelStreamEvent::TextDelta`
- [ ] `finish_reason` maps `stop`, `length`, `tool_calls`, `function_call`, `content_filter`, and unknown values into `StopReason`
- [ ] `stream_options.include_usage=true` is sent by default when compatible
- [ ] Usage fields are stored in event or message metadata only if the current public types can represent them cleanly; otherwise they remain provider-local until a separate usage API is designed
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions text_stream_`
- [ ] `cargo test -p noloong-agent-core --test chat_completions finish_reason_`
**Dependencies:** Task 5
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
**Estimated scope:** S

#### Task 7: Implement Streaming Tool Call Accumulation
**Description:** Accumulate fragmented Chat Completions `tool_calls` chunks by provider index and emit complete core `ToolCall` events after arguments are parseable or at finish.
**Acceptance criteria:**
- [ ] Multiple interleaved `tool_calls` indexes are accumulated independently
- [ ] Function name/id updates are merged as chunks arrive
- [ ] JSON arguments are parsed from fragmented `function.arguments`
- [ ] Legacy single `function_call` is supported for compatible older providers
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions tool_call_`
- [ ] `cargo test -p noloong-agent-core --test conformance tool_`
**Dependencies:** Task 5
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
**Estimated scope:** M

#### Task 8: Implement Thinking Extraction and Replay
**Description:** Implement provider-compatible reasoning extraction based on the Python reference: text reasoning fields are streamed as display text; object/list reasoning details are merged and preserved as raw replay payloads.
**Acceptance criteria:**
- [ ] Extracts `reasoning_content`, `reasoning`, `reasoning_text`, and `reasoning_details`
- [ ] Handles string, object with `text` or `summary`, dict `reasoning_details`, and list `reasoning_details`
- [ ] Merges cumulative and incremental provider payloads without duplicating display text
- [ ] Produces replay descriptor `{ v, kind, providerId, model, field }` for same-provider/model replay; raw replay payload remains in the thinking raw snapshot
- [ ] Summary-only thinking is represented as `ThinkingKind::Summary`
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions thinking_text_`
- [ ] `cargo test -p noloong-agent-core --test chat_completions thinking_details_`
- [ ] `cargo test -p noloong-agent-core --test chat_completions thinking_replay_`
**Dependencies:** Tasks 1, 4, 5
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
**Estimated scope:** M

#### Checkpoint: Complete Stream Semantics
- [ ] `cargo test -p noloong-agent-core --test chat_completions`
- [ ] `cargo test -p noloong-agent-core --test conformance`
- [ ] `cargo test --workspace`

### Phase 4: OpenRouter DeepSeek Gate

#### Task 9: Add OpenRouter DeepSeek Official Live Config
**Description:** Add the required real model route in the live test by composing generic `ChatCompletionsProviderConfig`, without hardcoding OpenRouter or DeepSeek in core provider code.
**Acceptance criteria:**
- [ ] `crates/noloong-agent-core/src` contains no OpenRouter/DeepSeek-specific constants or constructors
- [ ] Live test config uses `base_url = "https://openrouter.ai/api/v1"`
- [ ] Live test config uses `model = "deepseek/deepseek-v4-flash"`
- [ ] Live test config reads auth from `OPENROUTER_API_KEY`
- [ ] Live test request body enforces `provider.only = ["deepseek"]`, `allow_fallbacks = false`, and `require_parameters = true`
- [ ] Live test request body enables thinking with OpenRouter-compatible fields
**Verification:**
- [ ] `rg -n "openrouter|deepseek|OPENROUTER|deepseek-v4" crates/noloong-agent-core/src -S` returns no matches
- [ ] `cargo test -p noloong-agent-core --test chat_completions config_carries_provider_specific_body_without_core_presets`
**Dependencies:** Tasks 3, 8
**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/tests/openrouter_live.rs`
**Estimated scope:** S

#### Task 10: Replace JSON-RPC-only Live Test With Built-in Provider Live Test
**Description:** Add a live test that exercises the new built-in provider directly against OpenRouter DeepSeek official routing and keeps the older JSON-RPC fixture as extension coverage if still useful.
**Acceptance criteria:**
- [ ] Ignored live test calls `ChatCompletionsProvider` directly
- [ ] Test asserts at least one non-empty structured thinking event
- [ ] Test asserts committed assistant message contains a non-empty thinking block
- [ ] Test asserts final text contains an exact sentinel phrase and the built-in provider live path can stream a real tool call
- [ ] Test fails if OpenRouter routes away from DeepSeek official provider
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test openrouter_live openrouter_deepseek_v4_flash_official_provider_with_builtin_chat_completions -- --ignored --nocapture`
**Dependencies:** Task 9
**Files likely touched:**
- `crates/noloong-agent-core/tests/openrouter_live.rs`
- `crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs`
**Estimated scope:** S

#### Checkpoint: External Model Gate
- [ ] `OPENROUTER_API_KEY` is present in the environment
- [ ] `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`
- [ ] Thinking event, thinking block, sentinel answer, visible text, and tool-call streaming are all observed across the ignored live gate

### Phase 5: Documentation and Hardening

#### Task 11: Update Examples and Public Documentation
**Description:** Document how to use the built-in provider, how thinking is represented, and when to use JSON-RPC external providers instead.
**Acceptance criteria:**
- [ ] README includes a minimal built-in Chat Completions example
- [ ] README documents structured thinking semantics at a high level
- [ ] README documents OpenRouter DeepSeek live verification command and why it is manual
- [ ] Existing examples compile after thinking API migration
**Verification:**
- [ ] `cargo test -p noloong-agent-core --examples`
- [ ] `rg -n "ChatCompletionsProvider|ThinkingBlock|OPENROUTER_API_KEY|deepseek/deepseek-v4-flash" README.md crates/noloong-agent-core/examples`
**Dependencies:** Tasks 1-10
**Files likely touched:**
- `README.md`
- `crates/noloong-agent-core/examples/native_kernel.rs`
- `crates/noloong-agent-core/examples/stateful_agent.rs`
- `crates/noloong-agent-core/examples/stdio_ai_sdk.rs`
**Estimated scope:** M

#### Task 12: Final Quality Gate and CI Compatibility
**Description:** Run the full local quality gate, fix all clippy warnings without broad lint suppression, and make sure default CI remains deterministic and does not require external network/model keys.
**Acceptance criteria:**
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo nextest run --workspace` passes
- [ ] `cargo test --workspace` passes if nextest is unavailable
- [ ] OpenRouter live test remains ignored by default
**Verification:**
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`
**Dependencies:** Tasks 1-11
**Files likely touched:**
- Any files changed by prior tasks, only for fixes required by checks
**Estimated scope:** M

## Risks and Mitigations
| Risk | Impact | Mitigation |
|---|---:|---|
| Thinking API break affects many tests and examples | High | Do the type migration first, keep helper constructors for text-only thinking, and run core/conformance tests before provider work |
| Vendor-specific reasoning fields leak into core abstractions | High | Isolate extraction in `ChatCompletionsProvider` rules and store vendor payloads in structured `raw/replay` fields |
| JSON/object thinking cannot be represented as a text delta | High | Separate display text from raw snapshot; `ThinkingDelta` can carry both |
| SSE parsing becomes subtly incomplete | Medium | Keep parser minimal but test comments, multi-line data, empty lines, `[DONE]`, invalid JSON, and idle timeout |
| Tool call fragments arrive interleaved | Medium | Accumulate by provider index/id and emit only complete core `ToolCall` events |
| OpenRouter live test is flaky or routes to non-official provider | Medium | Require provider-only routing, no fallbacks, and keep test ignored/manual |
| New HTTP dependencies create clippy or feature bloat | Medium | Add only required features, prefer rustls, and gate with workspace clippy |

## Parallelization Opportunities
- Tasks 1 and 2 are sequential and should be completed before other work.
- After Task 3, Task 4 payload tests and Task 5 SSE transport tests can be implemented in parallel if write scopes are coordinated.
- Tasks 6 and 7 can proceed in parallel after Task 5 because text/finish parsing and tool accumulation are independent.
- Task 8 should wait for Tasks 4 and 5 because replay depends on payload conversion and extraction depends on chunk parsing.
- Documentation in Task 11 can start after Task 9, but examples should wait until the API migration is final.

## Final Acceptance Criteria
- [ ] `noloong-agent-core` exposes a built-in Chat Completions provider that can be registered as a normal `ModelProvider`
- [ ] Structured thinking supports text, summary, raw JSON/object/list, redaction, and replay metadata
- [ ] Chat Completions streaming supports text deltas, thinking deltas, tool calls, finish reasons, errors, cancellation, and `[DONE]`
- [ ] OpenRouter DeepSeek V4 Flash official-provider live test passes with thinking enabled using `OPENROUTER_API_KEY`
- [ ] Default CI remains deterministic and does not require live model access
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and workspace tests pass

## Assumptions
- This crate is still pre-1.0, so a controlled breaking change to thinking events/content is acceptable.
- The first built-in provider is OpenAI-compatible Chat Completions; OpenAI Responses and Anthropic Messages remain future providers.
- Usage accounting is not forced into public types in this plan unless it fits cleanly; correctness of streaming content and thinking comes first.
- No API keys are committed, logged, or stored in event metadata.
- Existing JSON-RPC extension APIs remain supported after the structured thinking migration.

## References
- OpenAI Chat Completions API reference: https://platform.openai.com/docs/api-reference/chat/create-chat-completion
- OpenAI function calling with Chat Completions: https://developers.openai.com/cookbook/examples/how_to_call_functions_with_chat_models
- OpenRouter DeepSeek V4 Flash: https://openrouter.ai/deepseek/deepseek-v4-flash
- OpenRouter reasoning tokens: https://openrouter.ai/docs/use-cases/reasoning-tokens
- OpenRouter provider routing: https://openrouter.ai/docs/features/provider-routing
