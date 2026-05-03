# Implementation Plan: Built-in Responses API Provider

## Overview

为 `noloong-agent-core` 增加内置 `ResponsesApiProvider`，实现 OpenAI Responses / OpenResponses wire format。这个 provider 仍然只是普通 `ModelProvider`，不能获得 runtime 或 phase graph 特权。v1 采用 stateless full-history 模式：每次从 `AgentState.messages` 构造完整 `input`，默认 `store = false`，与当前 event-sourced kernel 和 OpenRouter Responses Beta 的 stateless 语义保持一致。

## Architecture Decisions

- 新增 `crates/noloong-agent-core/src/responses.rs`，所有 Responses-specific wire shape 只留在 provider adapter 内。
- 新增 public exports：`ResponsesApiProvider`、`ResponsesApiProviderConfig`、`ResponsesReasoningConfig`、`ResponsesReasoningEffort`、`ResponsesReasoningSummary`。
- 官方 OpenAI 默认配置：
  - `base_url = "https://api.openai.com/v1"`
  - endpoint = `{base_url}/responses`
  - auth header = `Authorization: Bearer ...`
  - API key env = `OPENAI_API_KEY`
  - `stream = true`
  - `store = false`
  - `max_output_tokens: Option<u64>`
- OpenRouter Responses-compatible endpoint 只通过 generic config 支持：
  - `base_url = "https://openrouter.ai/api/v1"`
  - `api_key_env = "OPENROUTER_API_KEY"`
  - optional headers such as `X-Title`
- Core provider source 不允许出现 OpenRouter/free、DeepSeek 或任何 vendor/model preset。
- `extra_body` 作为 provider-specific escape hatch，最后 merge，允许调用方覆盖或追加 Responses 字段。
- `native_tools: Vec<Value>` 用于 pass-through hosted tools；core runtime tools 仍映射为 Responses function tools 并由 `AgentRuntime` 执行。
- v1 不自动维护 `previous_response_id`。如果未来需要 stateful Responses，应由独立 context/phase 扩展管理，而不是让 provider 隐式持有 conversation state。
- v1 对 Responses audio/video 输入输出保持 explicit gap，除非上游 wire format 和可测 provider 行为足够稳定。

## Dependency Graph

1. Provider config and HTTP shell
2. Request payload mapping
3. Media and thinking replay mapping
4. Streaming parser
5. Runtime and live gates
6. Docs and conformance evidence
7. Final quality gate

## Phase 1: Foundation

### Task 1: Add Responses Provider Config and HTTP Shell

**Description:** 新增 `ResponsesApiProviderConfig`、`ResponsesApiProvider`、headers、endpoint construction、timeouts、HTTP error reporting 和 `ModelProvider` skeleton。

**Acceptance criteria:**

- [ ] Official OpenAI config 默认使用 Bearer auth、`OPENAI_API_KEY`、`https://api.openai.com/v1/responses`。
- [ ] Compatible providers can be configured only through `base_url`、`api_key_env`、`header`、`extra_body`。
- [ ] `max_output_tokens` exists and no `max_tokens` API is introduced.
- [ ] `store` defaults to `false` and is configurable.
- [ ] non-2xx error includes status and body excerpt.
- [ ] config `Debug` redacts API key.

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test responses config_`
- [ ] `cargo test -p noloong-agent-core --test responses http_error_`
- [ ] `cargo test -p noloong-agent-core --test responses request_timeout_`
- [ ] `cargo test -p noloong-agent-core --test responses cancellation_`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Task 2: Define Responses Reasoning Configuration

**Description:** 增加 Responses reasoning request config，覆盖 effort、summary、include encrypted reasoning，同时保持 provider-neutral `ThinkingBlock` 不变。

**Acceptance criteria:**

- [ ] `ResponsesReasoningEffort` supports `Minimal`、`Low`、`Medium`、`High`、`XHigh`、`Custom(String)`。
- [ ] `ResponsesReasoningSummary` supports `Auto`、`Concise`、`Detailed`、`None`、`Custom(String)`。
- [ ] config can emit `reasoning` request object only when explicitly configured.
- [ ] config can add `include: ["reasoning.encrypted_content"]` without caller manually editing `extra_body`.
- [ ] `extra_body` can still override request fields when intentionally supplied.

**Verification:**

- [ ] `payload_omits_reasoning_config_by_default`
- [ ] `payload_maps_reasoning_effort_summary_and_encrypted_include`
- [ ] `payload_extra_body_can_override_reasoning_fields`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** S

## Checkpoint: Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p noloong-agent-core --test responses config_`

## Phase 2: Request Mapping

### Task 3: Map Core Messages to Responses Input Items

**Description:** 将 `ModelRequest.messages` 转成 Responses `input` items，并将 `System` messages 合并为 top-level `instructions`。

**Acceptance criteria:**

- [ ] `MessageRole::System` content renders to top-level `instructions` and does not enter `input`.
- [ ] `MessageRole::User` maps to `{"type":"message","role":"user","content":[...]}`.
- [ ] `MessageRole::Assistant` text/json maps to completed assistant message with `output_text` content.
- [ ] `MessageRole::Custom` fails fast with a clear provider error.
- [ ] empty content is represented by an empty content array, not invalid JSON.

**Verification:**

- [ ] `payload_maps_system_to_instructions`
- [ ] `payload_maps_user_and_assistant_history`
- [ ] `payload_rejects_custom_roles`
- [ ] `payload_handles_empty_content`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Task 4: Map Runtime Tools and Tool Results

**Description:** 支持 core runtime tool loop：`ToolSpec` 映射为 Responses function tool，assistant `ToolCall` history 映射为 `function_call` item，tool result 映射为 `function_call_output` item。

**Acceptance criteria:**

- [ ] `ToolSpec` maps to `{"type":"function","name","description","parameters"}`.
- [ ] function tool strictness is configurable and defaults to Responses API behavior, without changing `ToolSpec`.
- [ ] assistant `ContentBlock::ToolCall` maps to `function_call` with `call_id` and stringified `arguments`.
- [ ] `MessageRole::ToolResult` maps to `function_call_output` correlated by `tool_call_id`.
- [ ] tool result text/json content renders as string output; unsupported media fails fast in v1.
- [ ] `native_tools` are appended alongside runtime function tools.

**Verification:**

- [ ] `payload_maps_function_tools`
- [ ] `payload_maps_assistant_function_call_history`
- [ ] `payload_maps_function_call_output`
- [ ] `payload_merges_native_tools_with_runtime_tools`
- [ ] `payload_tool_result_rejects_unsupported_media`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Task 5: Map Image and File Media

**Description:** 将 provider-neutral `MediaBlock` 映射到 Responses `input_image` / `input_file` content parts，并明确 v1 不支持的 media。

**Acceptance criteria:**

- [ ] `MediaKind::Image + Uri` maps to `input_image.image_url`.
- [ ] `MediaKind::Image + Inline(base64)` maps to data URL using `mime_type`.
- [ ] `MediaKind::Image + Provider` maps to `input_image.file_id` when `provider_id` matches current provider id.
- [ ] `MediaKind::File + Uri` maps to `input_file.file_url`.
- [ ] `MediaKind::File + Provider` maps to `input_file.file_id` when `provider_id` matches current provider id.
- [ ] `MediaKind::File + Inline(base64)` is supported only when the provider accepts data URL file input; otherwise it fails fast with a targeted error.
- [ ] `Audio`、`Video`、custom media kind return clear provider errors in v1.

**Verification:**

- [ ] `payload_maps_image_url_data_url_and_file_id`
- [ ] `payload_maps_file_url_and_file_id`
- [ ] `payload_ignores_cross_provider_media_replay`
- [ ] `payload_rejects_unsupported_audio_video_custom_media`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

## Checkpoint: Request Mapping

- [ ] `cargo test -p noloong-agent-core --test responses payload_`
- [ ] `cargo test --workspace`

## Phase 3: Streaming

### Task 6: Parse Text and Stream Lifecycle

**Description:** 复用 crate-private `SseDecoder`，解析 Responses SSE lifecycle 和 text deltas。

**Acceptance criteria:**

- [ ] `response.created` emits `ModelStreamEvent::Started` using response id when available.
- [ ] OpenAI `response.output_text.delta` emits `TextDelta`.
- [ ] OpenRouter-compatible `response.content_part.delta` text emits `TextDelta`.
- [ ] `[DONE]` is accepted as terminal marker.
- [ ] `response.completed` or `response.done` emits `Finished { stop_reason: Stop }`.
- [ ] `response.incomplete` with max token details maps to `StopReason::Length`.
- [ ] `response.failed` and `response.error` map to `ModelStreamEvent::Failed`.

**Verification:**

- [ ] `stream_text_delta_and_completed`
- [ ] `stream_openrouter_content_part_delta`
- [ ] `stream_incomplete_maps_to_length`
- [ ] `stream_failed_reports_provider_failure`
- [ ] `stream_accepts_done_marker`

**Dependencies:** Tasks 1-3

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Task 7: Parse Streaming Function Calls

**Description:** 支持 Responses function call stream，将 `response.output_item.added`、`response.function_call_arguments.delta`、`response.function_call_arguments.done` 聚合为 core `ToolCall`。

**Acceptance criteria:**

- [ ] function call item start captures `id`、`call_id`、`name` by `output_index` and `item_id`.
- [ ] arguments delta accumulates by item.
- [ ] done event emits exactly one `ModelStreamEvent::ToolCall`.
- [ ] interleaved text and function call items preserve event ordering at the core stream level.
- [ ] malformed JSON arguments use existing shared `parse_tool_arguments` fallback policy.

**Verification:**

- [ ] `stream_accumulates_function_call_arguments`
- [ ] `stream_handles_interleaved_text_and_function_calls`
- [ ] `stream_function_call_malformed_json_policy_falls_back_to_string`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** S

### Task 8: Parse Reasoning Summary, Raw, and Encrypted Items

**Description:** 支持 Responses reasoning stream 和 output items，映射到 provider-neutral `ThinkingDelta` / `ThinkingBlock`。

**Acceptance criteria:**

- [ ] reasoning summary text deltas map to `ThinkingKind::Summary`.
- [ ] raw reasoning text deltas map to `ThinkingKind::Raw`.
- [ ] encrypted reasoning content maps to `ThinkingKind::Encrypted` with raw snapshot and replay descriptor.
- [ ] final reasoning item can be replayed only when provider id and model match.
- [ ] unknown reasoning fields are preserved in `metadata` or `raw_snapshot` rather than discarded when practical.

**Verification:**

- [ ] `stream_reasoning_summary_delta`
- [ ] `stream_reasoning_text_delta`
- [ ] `stream_encrypted_reasoning_replay_descriptor`
- [ ] `payload_replays_responses_reasoning_with_matching_scope`
- [ ] `payload_ignores_cross_provider_reasoning_replay`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

## Checkpoint: Streaming

- [ ] `cargo test -p noloong-agent-core --test responses stream_`
- [ ] `cargo test -p noloong-agent-core --test responses payload_replays_`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`

## Phase 4: Runtime, Live Gates, and Documentation

### Task 9: Add Runtime Integration Tests

**Description:** 验证 Responses provider 通过现有 `AgentRuntime` 提交 text/thinking/tool calls，不需要 runtime special case。

**Acceptance criteria:**

- [ ] Runtime commits streamed visible text as `ContentBlock::Text`.
- [ ] Runtime commits streamed reasoning as `ContentBlock::Thinking`.
- [ ] Runtime resolves and executes Responses function calls through existing tool phases.
- [ ] Assistant content ordering preserves thinking, text, media, tool call boundaries.

**Verification:**

- [ ] `runtime_commits_responses_text_and_thinking`
- [ ] `runtime_executes_responses_tool_call`
- [ ] `cargo test -p noloong-agent-core --test responses runtime_`

**Dependencies:** Tasks 6-8

**Files likely touched:**

- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** S

### Task 10: Add OpenRouter Responses Live Gate

**Description:** 新增 ignored live tests，只要求 `OPENROUTER_API_KEY`。默认 text smoke 使用 `openrouter/free`，tool/reasoning tests 使用 env-overridable model when free routing cannot guarantee capability。

**Acceptance criteria:**

- [ ] live tests live outside provider source and assemble OpenRouter config generically.
- [ ] text compatibility test defaults to `openrouter/free`.
- [ ] tool live test is skipped unless `NOLOONG_OPENROUTER_RESPONSES_TOOL_MODEL` is set or a known free route proves tool support.
- [ ] reasoning live test is skipped unless `NOLOONG_OPENROUTER_RESPONSES_REASONING_MODEL` is set or free route returns reasoning.
- [ ] no official `OPENAI_API_KEY` is required for required live gate.

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test responses_live -- --ignored --nocapture`
- [ ] `rg -n "openrouter|deepseek|OPENROUTER|deepseek-v4" crates/noloong-agent-core/src -S` returns no matches.

**Dependencies:** Task 9

**Files likely touched:**

- `crates/noloong-agent-core/tests/responses_live.rs`
- `crates/noloong-agent-core/tests/support/mod.rs`

**Estimated scope:** M

### Task 11: Update Docs and Conformance Matrix

**Description:** 更新 README、architecture docs 和 conformance matrix，把 Responses provider 记录为第三个 built-in wire adapter。

**Acceptance criteria:**

- [ ] README includes official OpenAI Responses config example.
- [ ] README includes OpenRouter-compatible config example without core preset.
- [ ] Architecture docs state Responses is provider adapter only, not runtime state owner.
- [ ] Conformance matrix lists payload, streaming/runtime, vendor neutrality, and OpenRouter live evidence.
- [ ] `plans/CURRENT_PLAN.md` remains the source implementation plan until the feature is complete.

**Verification:**

- [ ] docs review
- [ ] `cargo test -p noloong-agent-core --examples`

**Dependencies:** Tasks 1-10

**Files likely touched:**

- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CONFORMANCE_MATRIX.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

## Final Quality Gate

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test -p noloong-agent-core --examples`
- [ ] `cargo test -p noloong-agent-core --test responses`
- [ ] `cargo test -p noloong-agent-core --test responses_live -- --ignored --nocapture`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs`
- [ ] `node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs`
- [ ] `rg -n "openrouter|deepseek|OPENROUTER|deepseek-v4" crates/noloong-agent-core/src -S` returns no matches.
- [ ] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Responses stream event names differ between OpenAI and OpenRouter-compatible providers | High | Cover both OpenAI spec events and OpenRouter documented `content_part.delta` with mock stream tests before live tests. |
| Reasoning item wire shape evolves | Medium | Preserve unknown reasoning payloads in `raw_snapshot` or metadata, and keep request config extensible through `extra_body`. |
| `previous_response_id` conflicts with event-sourced full-history replay | Medium | Keep v1 stateless by default and avoid provider-owned hidden state. |
| Free OpenRouter routing may not support tools or reasoning consistently | Medium | Make text compatibility required; gate tool/reasoning live tests behind explicit model env vars when needed. |
| Media support across Responses-compatible providers is uneven | Medium | Implement image/file core mappings with fail-fast unsupported cases; keep audio/video explicit gap for v1. |

## Open Questions

- None. Defaults are chosen for v1: stateless full-history, `store = false`, OpenRouter live required gate, no official OpenAI key requirement.

## References

- OpenAI Responses API: `https://api.openai.com/v1/responses`
- OpenAI migration guide: `https://developers.openai.com/api/docs/guides/migrate-to-responses`
- OpenAI function-call streaming: `https://developers.openai.com/api/docs/guides/function-calling#streaming`
- OpenRouter Responses basic usage: `https://openrouter.ai/docs/api/reference/responses/basic-usage`
