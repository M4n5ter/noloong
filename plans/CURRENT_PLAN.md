# Implementation Plan: Built-in Anthropic Messages Provider

## Overview

为 `noloong-agent-core` 增加一个内置 `AnthropicMessagesProvider`，让它和现有 `ChatCompletionsProvider` 一样只是普通 `ModelProvider`，不获得 runtime 特权。v1 覆盖 Anthropic Messages 的核心能力：文本/system/messages、streaming、tool use、extended thinking、image/document/file 输入映射、same-provider thinking replay、mock conformance，以及 OpenRouter Anthropic-compatible Messages endpoint live gate。官方 Anthropic live test 只作为显式 opt-in diagnostic 保留，不作为当前必需 gate。

## Architecture Decisions

- 新增 `crates/noloong-agent-core/src/anthropic_messages.rs`，provider-specific wire shape 只留在 provider adapter 内，不泄漏进 phase/runtime。
- 新增 public exports：`AnthropicMessagesProvider`、`AnthropicMessagesProviderConfig`、`AnthropicAuthScheme`、`AnthropicThinkingConfig`。
- 官方 Anthropic 默认配置：
  - `base_url = "https://api.anthropic.com"`
  - endpoint = `{base_url}/v1/messages`
  - auth header = `x-api-key`
  - API key env = `ANTHROPIC_API_KEY`
  - `anthropic-version = "2023-06-01"`
  - `max_tokens = 1024`
- OpenRouter Anthropic-compatible endpoint 通过配置支持：
  - `AnthropicAuthScheme::Bearer`
  - `base_url = "https://openrouter.ai/api"`
  - `api_key_env = "OPENROUTER_API_KEY"`
  - optional `anthropic_version`
- `MediaKind::Image` 映射为 Anthropic `image` block；`MediaKind::File` 映射为 `document` 或 opt-in Files API `file_id`。
- Anthropic Messages v1 对 `MediaKind::Audio` 和 `MediaKind::Video` fail-fast，错误信息说明该 provider v1 不支持 audio/video。
- Extended thinking 默认关闭；通过 `enable_thinking(budget_tokens)` 打开。stream 中 `thinking_delta` 进入 `ThinkingDelta`，`signature_delta` 保存在 metadata/raw replay descriptor。
- Files API/provider `file_id` 为 opt-in v1：`allow_files_api_media(true)` 才允许 provider-hosted file refs 映射到 Anthropic file source，并添加所需 beta header。

## Dependency Graph

1. Shared SSE decoder
2. Anthropic provider config and HTTP shell
3. Request payload mapping
4. Streaming parser
5. Thinking replay and assistant history
6. Live gates, docs, conformance matrix
7. Final quality gate

## Phase 1: Foundation

### Task 1: Extract Shared SSE Decoder

**Description:** 把当前 Chat Completions provider 内部的 SSE decoder 提取为 crate-private shared module，供 Chat 和 Anthropic provider 复用。

**Acceptance criteria:**

- [ ] Chat Completions 现有 SSE behavior 不变。
- [ ] shared decoder 支持 multiline `data:`, CRLF, split CRLF across chunks, final unfinished frame flush。
- [ ] decoder API 不暴露到 public crate API。

**Verification:**

- [ ] `cargo test -p noloong-agent-core chat_completions::tests::sse_decoder`
- [ ] `cargo test -p noloong-agent-core --test chat_completions`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/src/sse.rs`

**Estimated scope:** S

### Task 2: Add Anthropic Provider Config and HTTP Shell

**Description:** 新增 `AnthropicMessagesProviderConfig`、`AnthropicMessagesProvider`、headers、auth scheme、endpoint construction、timeouts、HTTP error reporting 和 `ModelProvider` skeleton。

**Acceptance criteria:**

- [ ] Official Anthropic config 默认使用 `x-api-key`、`ANTHROPIC_API_KEY`、`anthropic-version`。
- [ ] OpenRouter-compatible config 可切换 Bearer auth，并可关闭或覆盖 version header。
- [ ] `max_tokens` 默认存在，可通过 builder 覆盖。
- [ ] non-2xx error 包含 status 和 body excerpt。
- [ ] config `Debug` redacts API key。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test anthropic_messages config_`
- [ ] `cargo test -p noloong-agent-core --test anthropic_messages http_error_`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

## Checkpoint: Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test -p noloong-agent-core --test anthropic_messages`
- [ ] `cargo test -p noloong-agent-core --test chat_completions`

## Phase 2: Request Mapping

### Task 3: Map Core Messages to Anthropic Messages Payload

**Description:** 将 `ModelRequest` 转成 Anthropic Messages request body，包括 top-level system、messages array、assistant history、tool specs 和 extra body。

**Acceptance criteria:**

- [ ] `MessageRole::System` 合并为 top-level `system`，不进入 `messages` array。
- [ ] `User` 和 `Assistant` 映射为 Anthropic role；`Custom` role fail-fast。
- [ ] pure text content 保持 Anthropic text block，mixed content 使用 typed content blocks。
- [ ] `ToolSpec` 映射为 Anthropic `tools`。
- [ ] caller-owned `extra_body` 最后 merge，允许兼容 provider 覆盖或追加字段。

**Verification:**

- [ ] `payload_maps_text_system_and_extra_body`
- [ ] `payload_rejects_custom_roles`
- [ ] `payload_maps_tools`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

### Task 4: Map Tool Calls and Tool Results

**Description:** 实现 Anthropic assistant `tool_use` history block 和 user `tool_result` block 映射，使 runtime 工具循环可直接复用。

**Acceptance criteria:**

- [ ] assistant `ContentBlock::ToolCall` 映射为 `tool_use` block。
- [ ] `ToolResult` message 映射为 user role message 中的 `tool_result` block。
- [ ] tool result content 可包含 text/json/media，其中不支持的 media fail-fast。
- [ ] tool arguments 保持 JSON object；非 object 时按当前 core value 原样传入并由 Anthropic 侧决定。

**Verification:**

- [ ] `payload_maps_assistant_tool_use_history`
- [ ] `payload_maps_tool_result_message`
- [ ] `payload_tool_result_rejects_unsupported_media`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** S

### Task 5: Map Image, Document, and File Media

**Description:** 将 provider-neutral `MediaBlock` 映射为 Anthropic Messages 支持的 image/document/file source blocks。

**Acceptance criteria:**

- [ ] `MediaKind::Image + Inline(base64)` 映射为 Anthropic base64 image source，需要 `mime_type`。
- [ ] `MediaKind::Image + Uri` 映射为 Anthropic URL image source。
- [ ] `MediaKind::File + Inline(base64)` 映射为 document base64 source，需要 `mime_type`，`name` 映射为 title/filename metadata。
- [ ] `MediaKind::File + Uri` 映射为 document URL source。
- [ ] `MediaKind::File + Provider` 只有 `allow_files_api_media(true)` 且 provider scope 匹配时映射为 file source。
- [ ] `Audio`、`Video`、custom media kind 和 system media 都返回清晰 provider error。

**Verification:**

- [ ] `payload_maps_inline_and_url_images`
- [ ] `payload_maps_inline_and_url_documents`
- [ ] `payload_maps_provider_file_when_enabled`
- [ ] `payload_rejects_unsupported_audio_video_custom_media`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

## Checkpoint: Request Mapping

- [ ] `cargo test -p noloong-agent-core --test anthropic_messages payload_`
- [ ] `cargo test --workspace`
- [ ] `rg -n "anthropic|claude|ANTHROPIC" crates/noloong-agent-core/src -S` shows provider module/export only, no runtime preset leakage.

## Phase 3: Streaming

### Task 6: Parse Text, Stop Reasons, and Stream Lifecycle

**Description:** 实现 Anthropic SSE event parser 的基础路径，转换 message/content lifecycle 为 core stream events。

**Acceptance criteria:**

- [ ] `message_start` emits `ModelStreamEvent::Started`。
- [ ] `content_block_delta.text_delta` emits `TextDelta`。
- [ ] `message_delta.stop_reason` maps `end_turn`、`max_tokens`、`tool_use`、`stop_sequence`、unknown values into `StopReason`。
- [ ] `message_stop` emits final `Finished` if not already emitted。
- [ ] stream `error` becomes provider failure and fails the run.

**Verification:**

- [ ] `stream_text_and_finish_reason`
- [ ] `stream_error_reports_provider_failure`
- [ ] `stream_ignores_unknown_nonfatal_events`

**Dependencies:** Tasks 1-3

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

### Task 7: Parse Streaming Tool Use

**Description:** 支持 Anthropic `tool_use` content block 和 `input_json_delta` accumulation，最终 emit core `ToolCall`。

**Acceptance criteria:**

- [ ] tool block start captures Anthropic tool id/name by content block index。
- [ ] fragmented `input_json_delta.partial_json` accumulates by block index。
- [ ] block stop emits exactly one `ModelStreamEvent::ToolCall` with parsed JSON arguments。
- [ ] malformed JSON falls back to string or provider error using the same policy as Chat Completions; policy must be documented in test name and error/message。

**Verification:**

- [ ] `stream_accumulates_tool_use_input_json`
- [ ] `stream_handles_interleaved_tool_blocks`
- [ ] `stream_tool_use_malformed_json_policy`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** S

### Task 8: Parse Extended Thinking

**Description:** 支持 Anthropic extended thinking stream，保留 display text、raw snapshot、signature 和 replay descriptor。

**Acceptance criteria:**

- [ ] `thinking_delta` emits `ThinkingDelta` with `ThinkingKind::Raw` and visible text delta。
- [ ] `signature_delta` is preserved in metadata and replay descriptor。
- [ ] thinking raw snapshot is sufficient to replay same-provider/model assistant history。
- [ ] thinking remains disabled unless `enable_thinking(budget_tokens)` is configured。

**Verification:**

- [ ] `stream_thinking_delta_preserves_raw_and_signature`
- [ ] `payload_sends_thinking_config_when_enabled`
- [ ] `payload_omits_thinking_config_by_default`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

## Checkpoint: Streaming

- [ ] `cargo test -p noloong-agent-core --test anthropic_messages stream_`
- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `cargo test -p noloong-agent-core --test conformance`

## Phase 4: Replay and Runtime Integration

### Task 9: Replay Assistant Thinking and Supported History

**Description:** 支持同 provider/model scope 下的 Anthropic thinking replay，并明确处理不可渲染的 assistant media/history。

**Acceptance criteria:**

- [ ] Matching prior `ThinkingBlock` renders as Anthropic thinking block with signature when available。
- [ ] Cross-provider or cross-model thinking replay is ignored。
- [ ] Assistant text + thinking + tool_use order is preserved as much as Anthropic Messages content blocks allow。
- [ ] Unsupported assistant media returns clear provider error instead of silently dropping content。

**Verification:**

- [ ] `payload_replays_thinking_with_matching_scope`
- [ ] `payload_ignores_cross_provider_thinking_replay`
- [ ] `payload_rejects_unrenderable_assistant_media`

**Dependencies:** Tasks 4, 8

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** S

### Task 10: Runtime End-to-End Mock Tests

**Description:** 用 mock Anthropic provider server 跑完整 `AgentRuntime` flow，证明 provider 可作为普通 `ModelProvider` 使用。

**Acceptance criteria:**

- [ ] runtime commits Anthropic streamed text into assistant message。
- [ ] runtime commits Anthropic thinking into `ContentBlock::Thinking`。
- [ ] runtime resolves Anthropic streamed tool call and executes local tool。
- [ ] cancellation and idle timeout behavior matches Chat provider style。

**Verification:**

- [ ] `runtime_commits_anthropic_text_and_thinking`
- [ ] `runtime_executes_anthropic_tool_call`
- [ ] `cancellation_aborts_pending_anthropic_request`
- [ ] `request_timeout_applies_before_anthropic_initial_response`

**Dependencies:** Tasks 6-9

**Files likely touched:**

- `crates/noloong-agent-core/tests/anthropic_messages.rs`
- `crates/noloong-agent-core/src/anthropic_messages.rs`

**Estimated scope:** M

## Checkpoint: Runtime Integration

- [ ] `cargo test -p noloong-agent-core --test anthropic_messages`
- [ ] `cargo test --workspace`
- [ ] `cargo nextest run --workspace`

## Phase 5: Live Gates and Documentation

### Task 11: Add OpenRouter Anthropic-Compatible Live Gate

**Description:** 添加 ignored live test，验证 OpenRouter Anthropic Messages-compatible endpoint 可以通过同一个 provider config 工作。

**Acceptance criteria:**

- [ ] Test reads `OPENROUTER_API_KEY`。
- [ ] Test uses Bearer auth and `base_url = "https://openrouter.ai/api"`。
- [ ] Test defaults to `openrouter/free` and can be overridden by `NOLOONG_OPENROUTER_ANTHROPIC_LIVE_MODEL`。
- [ ] Test asserts exact visible sentinel text。
- [ ] Test asserts tool-call path only when `NOLOONG_OPENROUTER_ANTHROPIC_TOOL_MODEL` names a tool-capable model；否则测试名和 skip 条件明确说明只覆盖 text compatibility。
- [ ] No OpenRouter-specific constants enter runtime/phase/core types。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test anthropic_live openrouter_anthropic_messages -- --ignored --nocapture`
- [ ] `rg -n "openrouter|OPENROUTER" crates/noloong-agent-core/src -S` shows no provider preset leakage outside generic config support。

**Dependencies:** Tasks 1-10

**Files likely touched:**

- `crates/noloong-agent-core/tests/anthropic_live.rs`
- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CONFORMANCE_MATRIX.md`

**Estimated scope:** S

### Task 12: Add Optional Official Anthropic Diagnostic

**Description:** 添加 ignored diagnostic tests，只有调用方显式设置 `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1` 且提供有效 `ANTHROPIC_API_KEY` 时才直连 Anthropic official Messages API。当前用户没有官方 key，因此这不是必需 gate。

**Acceptance criteria:**

- [ ] Test skips by default unless `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1`。
- [ ] Test reads `ANTHROPIC_API_KEY` only after opt-in。
- [ ] Test model defaults to `claude-sonnet-4-5` and can be overridden by `NOLOONG_ANTHROPIC_LIVE_MODEL`。
- [ ] Test enables thinking and asserts a non-empty thinking event or thinking block。
- [ ] Test asserts exact visible sentinel text。
- [ ] Test includes a tool-call path and verifies tool execution。
- [ ] Test includes one image or document payload path supported by official Anthropic API。

**Verification:**

- [ ] `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1 cargo test -p noloong-agent-core --test anthropic_live official_anthropic_messages -- --ignored --nocapture`

**Dependencies:** Tasks 1-10

**Files likely touched:**

- `crates/noloong-agent-core/tests/anthropic_live.rs`
- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CONFORMANCE_MATRIX.md`

**Estimated scope:** S

### Task 13: Update Documentation and Conformance Matrix

**Description:** 更新 README、架构文档和验证矩阵，让 Anthropic provider 的能力和边界可审计。

**Acceptance criteria:**

- [ ] README contains a minimal `AnthropicMessagesProvider` example。
- [ ] `ARCHITECTURE.md` explains Anthropic provider boundaries, thinking replay, media limitations, and OpenRouter-compatible config。
- [ ] `CONFORMANCE_MATRIX.md` lists Anthropic payload, streaming, replay, live gates, and vendor-neutrality checks。
- [ ] `CURRENT_PLAN.md` remains implementation-ready and matches actual decisions。

**Verification:**

- [ ] `rg -n "AnthropicMessagesProvider|AnthropicMessagesProviderConfig|anthropic_live|OpenRouter Anthropic" README.md crates/noloong-agent-core/docs/ARCHITECTURE.md plans/CONFORMANCE_MATRIX.md plans/CURRENT_PLAN.md`
- [ ] `git diff --check README.md crates/noloong-agent-core/docs/ARCHITECTURE.md plans`

**Dependencies:** Tasks 1-12

**Files likely touched:**

- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CONFORMANCE_MATRIX.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

## Final Quality Gate

### Task 14: Run Full Verification

**Description:** 运行完整质量门，确保 Anthropic provider 不破坏现有 Chat/JSON-RPC/runtime 行为。

**Acceptance criteria:**

- [ ] `cargo fmt --check` passes。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes。
- [ ] `cargo test --workspace` passes。
- [ ] `cargo nextest run --workspace` passes。
- [ ] `cargo test -p noloong-agent-core --examples` passes。
- [ ] JS fixtures still pass `node --check`。
- [ ] OpenRouter Anthropic-compatible ignored live gate passes when `OPENROUTER_API_KEY` exists。
- [ ] Official Anthropic diagnostic is skipped by default and only runs when explicitly opted in。

**Verification:**

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test -p noloong-agent-core --examples`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs`
- [ ] `node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs`
- [ ] `cargo test -p noloong-agent-core --test anthropic_live openrouter_anthropic_messages -- --ignored --nocapture`
- [ ] `git diff --check`

**Dependencies:** Tasks 1-13

**Files likely touched:**

- Any files changed by prior tasks, only for fixes required by checks.

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Anthropic streaming event model differs from Chat SSE | High | Build parser from Anthropic event names and block indexes; do not reuse Chat chunk assumptions except shared SSE framing. |
| Thinking signature replay is subtle | High | Preserve raw/signature metadata and add same-provider replay tests before live gate. |
| Files API behavior depends on beta headers | Medium | Keep provider file support opt-in and test headers explicitly. |
| OpenRouter Anthropic-compatible endpoint diverges from official Anthropic | Medium | Support auth/version/base URL through config; keep OpenRouter-specific values in tests/docs only. |
| Media support may be overclaimed | Medium | v1 supports image/document/file only; audio/video fail fast with provider-specific error. |
| Tool JSON stream can be partial/malformed | Medium | Accumulate by content block index and document fallback/error policy in tests. |

## Parallelization Opportunities

- Tasks 3 and 4 can be implemented by one agent while another writes request payload tests, after Task 2 lands.
- Tasks 6 and 7 can proceed in parallel after Task 3 because text lifecycle and tool JSON accumulation are separable.
- Task 8 should wait for Task 6 because thinking uses the same content block lifecycle.
- Tasks 11 and 12 can be implemented in parallel after runtime mock tests pass.
- Task 13 can start once public type names stabilize, but final docs should wait for live gate outcomes.

## Final Acceptance Criteria

- [ ] `noloong-agent-core` exposes a built-in `AnthropicMessagesProvider` as a normal `ModelProvider`。
- [ ] Anthropic Messages payload mapping supports text/system/tools/tool results/thinking/image/document/file boundaries.
- [ ] Anthropic SSE streaming supports text, thinking, tool use, stop reasons, errors, cancellation, and timeouts.
- [ ] Same-provider thinking replay works and cross-provider replay is ignored.
- [ ] OpenRouter Anthropic-compatible endpoint works through generic provider config without OpenRouter preset leakage.
- [ ] Full local quality gate and ignored live gates pass.

## Assumptions

- `OPENROUTER_API_KEY` exists in the environment for OpenRouter Anthropic-compatible live gate.
- Official Anthropic diagnostic requires explicit `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1` and a valid `ANTHROPIC_API_KEY`; it is not part of the current required gate.
- OpenRouter live model is selected via `NOLOONG_OPENROUTER_ANTHROPIC_LIVE_MODEL` if endpoint/model behavior changes.
- No API keys are committed, logged, or stored in event metadata.
- No core runtime API changes are required beyond exporting the new provider types.

## References

- Anthropic Messages API: https://docs.anthropic.com/en/api/messages
- Anthropic Messages examples: https://docs.anthropic.com/en/api/messages-examples
- Anthropic streaming: https://platform.claude.com/docs/en/build-with-claude/streaming
- Anthropic Files API: https://platform.claude.com/docs/en/build-with-claude/files
- OpenRouter Anthropic Messages endpoint: https://openrouter.ai/docs/api/api-reference/anthropic-messages/create-messages
