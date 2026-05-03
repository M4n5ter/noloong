# Implementation Plan: Multimodal LLM Provider I/O

## Overview

为 `noloong-agent-core` 增加 provider-neutral 的多模态输入输出能力，让 image、audio、video、file 可以作为 message content、model stream output、tool output、JSON-RPC extension payload 在 core 中稳定流转。第一轮采用 reference-first 设计：事件日志优先保存 URI 或 provider reference，小体积 inline base64 作为可选 source；不在 v1 引入 `MediaStore` 或隐式下载/上传 blob。

当前重点是让 core data model 和内置 Chat Completions provider 具备正确边界，而不是把某个厂商的 wire shape 直接泄漏进公共 API。OpenAI Chat Completions 支持 image/audio/file content parts 和 audio output；Anthropic Messages 使用 typed image/document/file source blocks。core 应抽象成统一 media block，再由 provider adapter 做各自映射。

## Architecture Decisions

- 公共 API 使用一个 `ContentBlock::Media { media: MediaBlock }`，不拆成多个 provider-specific block，也不新增 `Image`、`Audio`、`Video` 三套平行结构。
- `MediaSource` 默认 reference-first：`Uri`、`Provider`、`Inline` 三种 source；`Inline` 只保存调用方已经提供的编码数据，不在 core 内做文件读取、下载或 base64 编码。
- v1 不引入 `MediaStore` trait。后续如果需要大 blob 生命周期、缓存、去重或加密存储，再单独增加 store abstraction。
- video 在 core 中是一等 `MediaKind::Video`；内置 Chat Completions provider 会把 URI/inline base64 映射到 `video_url`，provider file/ref 仍需要通过配置显式允许后才透传。
- Chat Completions provider 只实现标准兼容映射和显式配置开关，不硬编码 OpenRouter、DeepSeek 或其它 provider preset。
- 多模态输出通过 `ModelStreamEvent::MediaDelta` 表达，并由 `assistant_commit` 折叠为 `ContentBlock::Media`，保持 thinking、text、media、tool call 的相对顺序。
- JSON-RPC bridge 不新增方法；外部 JS/TS/Python 扩展通过现有 serde JSON contract 直接收发新的 `media` content block 和 `media_delta` stream event。

## Public API Shape

- `ContentBlock` 新增 variant：`Media { media: MediaBlock }`。
- 新增 `MediaKind`：`Image`、`Audio`、`Video`、`File`、`Custom(String)`；serde 使用 snake_case string，未知值反序列化为 `Custom`。
- 新增 `MediaEncoding`：`Base64`、`Custom(String)`；v1 内置 Chat provider 只消费 `Base64` inline source。
- 新增 `MediaSource`，serde 使用 tagged snake_case object：
  - `Uri { uri: String }`
  - `Inline { data: String, encoding: MediaEncoding }`
  - `Provider { provider_id: String, id: String }`
- 新增 `MediaBlock` 字段：
  - `kind: MediaKind`
  - `source: MediaSource`
  - `data: Option<EncodedMediaData>`
  - `mime_type: Option<String>`
  - `name: Option<String>`
  - `replay_descriptor: Option<Value>`
  - `metadata: Map<String, Value>`
- 新增 `MediaDelta` 字段：
  - `kind: MediaKind`
  - `data_delta: Option<String>`
  - `source: Option<MediaSource>`
  - `mime_type: Option<String>`
  - `name: Option<String>`
  - `replay_descriptor: Option<Value>`
  - `metadata: Map<String, Value>`
  - `done: bool`
- `ModelStreamEvent` 新增 `MediaDelta { delta: MediaDelta }`。
- Helper constructors:
  - `MediaBlock::uri(kind, uri)`
  - `MediaBlock::inline_base64(kind, data)`
  - `MediaBlock::provider(kind, provider_id, id)`
  - `MediaDelta::from_inline_base64_delta(kind, data_delta)`

## Dependency Graph

1. Core media serde types
2. Assistant commit media folding
3. Chat Completions input content part mapping
4. Chat Completions output media parsing
5. JSON-RPC and tool coverage
6. Docs and examples
7. Full quality gate and optional live gate

## Task List

### Phase 1: Core Media Foundation

#### Task 1: Add Provider-neutral Media Types

**Description:** Add the core media data model to `types.rs` so every message-bearing surface can represent image, audio, video, and file content without provider-specific JSON.

**Acceptance criteria:**
- [ ] `ContentBlock::Media { media: MediaBlock }` exists and round-trips through serde.
- [ ] `MediaKind`, `MediaEncoding`, `MediaSource`, `MediaBlock`, and `MediaDelta` are public and exported through `lib.rs`.
- [ ] Unknown media kind and encoding values deserialize into `Custom(String)` instead of failing.
- [ ] Helper constructors cover URI, inline base64, and provider reference sources.

**Verification:**
- [ ] `cargo test -p noloong-agent-core media_type_serde`
- [ ] `cargo test -p noloong-agent-core --test core`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/core.rs`

**Estimated scope:** M

#### Task 2: Fold Media Stream Events Into Assistant Messages

**Description:** Extend assistant commit logic so streamed media deltas become committed `ContentBlock::Media` blocks while preserving the existing ordering contract for thinking, text, and tool calls.

**Acceptance criteria:**
- [ ] `ModelStreamEvent::MediaDelta` is accepted by `assistant_commit`.
- [ ] Media deltas flush any open text/thinking before starting a media block.
- [ ] Repeated inline base64 `data_delta` chunks append into one media block until `done` or until another content type starts.
- [ ] Source-only media deltas commit a media block without requiring inline data.
- [ ] Tool call boundaries flush any open media block before committing the tool call.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test core assistant_commit_media_ordering`
- [ ] `cargo test -p noloong-agent-core --test conformance`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/core.rs`
- `crates/noloong-agent-core/tests/conformance.rs`

**Estimated scope:** M

#### Checkpoint: Core Media Model

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `cargo test -p noloong-agent-core --test conformance`

### Phase 2: Chat Completions Input Mapping

#### Task 3: Render Text and Media Content Parts

**Description:** Update the built-in Chat Completions payload builder so user/custom messages can be rendered as OpenAI-compatible content parts when they contain media, while pure text messages keep the existing compact string content.

**Acceptance criteria:**
- [ ] Pure text user/custom messages still serialize as string `content`.
- [ ] Mixed text/media user/custom messages serialize as content parts array.
- [ ] `MediaKind::Image` with `Uri` maps to `image_url.url`.
- [ ] `MediaKind::Image` with inline base64 maps to a data URL in `image_url.url`.
- [ ] `ChatCompletionsProviderConfig` exposes `image_detail`, defaulting to `auto`.
- [ ] System messages reject media with a clear provider error instead of silently dropping it.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_text_only_remains_string`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_image_uri_content_part`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_image_inline_content_part`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_system_media_rejected`

**Dependencies:** Tasks 1, 2

**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**Estimated scope:** M

#### Task 4: Map Audio, File, and Video Inputs Safely

**Description:** Add deterministic Chat Completions mappings for audio and file media, and define fail-fast behavior for unsupported source/kind combinations.

**Acceptance criteria:**
- [ ] Inline base64 audio with `audio/wav` maps to `input_audio` format `wav`.
- [ ] Inline base64 audio with `audio/mpeg` or `audio/mp3` maps to `input_audio` format `mp3`.
- [ ] Audio URI and unsupported audio MIME types return clear provider errors; no implicit downloads.
- [ ] Provider file references map to `file.file_id`.
- [ ] Inline file source maps to `file.file_data` and uses `MediaBlock.name` as `filename` when present.
- [ ] File URI returns a clear provider error because Chat Completions file URL input is not portable.
- [ ] Video URI/inline media maps to `video_url`.
- [ ] Optional config `allow_provider_video_file_media` allows provider-referenced `Video` to map to `file_id` for compatible providers; provider-referenced `File` maps to `file_id` directly.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_audio_inline_wav`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_audio_uri_rejected`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_file_provider_reference`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_file_uri_rejected`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_video_uri_content_part`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_video_inline_content_part`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_provider_video_default_rejected`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**Estimated scope:** M

#### Checkpoint: Chat Input Mapping

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_`
- [ ] `cargo test -p noloong-agent-core --test core`

### Phase 3: Chat Completions Output Mapping

#### Task 5: Parse Chat Audio Output Into Media Events

**Description:** Extend Chat Completions stream parsing so provider audio output can be represented as `MediaDelta` and committed as assistant media content.

**Acceptance criteria:**
- [ ] `ChatCompletionsProviderConfig` supports output `modalities`, audio `format`, and audio `voice` through typed helpers while keeping `extra_body` available.
- [ ] Request payload includes `modalities` and `audio` only when configured.
- [ ] Stream chunks with audio base64 data emit `ModelStreamEvent::MediaDelta`.
- [ ] Audio transcript is stored in media metadata, not forced into `TextDelta`.
- [ ] Audio output id, expiry, provider id, model, format, and streamed payload are preserved in `MediaBlock.source`, `MediaBlock.data`, `replay_descriptor`, or metadata as appropriate.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_audio_output_config`
- [ ] `cargo test -p noloong-agent-core --test chat_completions stream_audio_delta_to_media_event`
- [ ] `cargo test -p noloong-agent-core --test chat_completions stream_audio_metadata_preserved`

**Dependencies:** Tasks 1, 2, 4

**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**Estimated scope:** M

#### Task 6: Replay Assistant Media Where Chat Supports It

**Description:** Add same-provider replay for assistant media outputs so a later Chat Completions turn can reference prior provider-hosted audio or file output when the provider contract supports it.

**Acceptance criteria:**
- [ ] Assistant `MediaBlock` with matching Chat replay descriptor can render top-level assistant `audio` when it represents previous Chat audio output.
- [ ] Cross-provider or cross-model media replay is ignored or rejected consistently with thinking replay rules.
- [ ] Unsupported assistant media history returns a clear provider error instead of silently dropping media content.
- [ ] Text and tool call replay behavior from the existing provider remains unchanged.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_assistant_audio_replay`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_assistant_media_cross_provider_ignored`
- [ ] `cargo test -p noloong-agent-core --test chat_completions payload_assistant_unsupported_media_rejected`

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**Estimated scope:** S

#### Checkpoint: Chat Output Mapping

- [ ] `cargo test -p noloong-agent-core --test chat_completions`
- [ ] `cargo test -p noloong-agent-core --test conformance`
- [ ] `cargo test --workspace`

### Phase 4: Extension and Tool Coverage

#### Task 7: Cover JSON-RPC Extension Media Contract

**Description:** Update JSON-RPC tests and fixtures so non-Rust model providers and tools can send and receive media content through the existing bridge without new bridge methods.

**Acceptance criteria:**
- [ ] A JS fixture can return a `media_delta` model stream event.
- [ ] The runtime commits that external media stream into assistant message content.
- [ ] A JS tool fixture can return `ContentBlock::Media` in `ToolOutput`.
- [ ] JSON-RPC request payloads for model/tool calls include media blocks without custom bridge translation.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test jsonrpc jsonrpc_model_stream_media_delta`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc jsonrpc_tool_output_media`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

**Dependencies:** Tasks 1, 2

**Files likely touched:**
- `crates/noloong-agent-core/tests/jsonrpc.rs`
- `crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

**Estimated scope:** S

#### Task 8: Add Tool and Hook Regression Coverage

**Description:** Verify that existing tool execution, tool updates, and after-tool hooks preserve media blocks because they already share the `Vec<ContentBlock>` surface.

**Acceptance criteria:**
- [ ] Tool output can contain image/audio/file media blocks.
- [ ] Tool update can contain media blocks.
- [ ] `AfterToolCallResult.content` can replace tool output with media content.
- [ ] Tool error outputs remain text-only unless a tool explicitly returns media.

**Verification:**
- [ ] `cargo test -p noloong-agent-core --test core tool_output_media_preserved`
- [ ] `cargo test -p noloong-agent-core --test core tool_update_media_preserved`
- [ ] `cargo test -p noloong-agent-core --test core after_tool_hook_can_rewrite_to_media`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-core/tests/core.rs`
- `crates/noloong-agent-core/tests/conformance.rs`

**Estimated scope:** S

#### Checkpoint: Extension and Tool Surfaces

- [ ] `cargo test -p noloong-agent-core --test jsonrpc`
- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

### Phase 5: Documentation and Verification

#### Task 9: Update Architecture and README Documentation

**Description:** Document the media model, provider mapping boundaries, and v1 limitations so future provider work can target the core abstractions instead of inventing per-provider payloads.

**Acceptance criteria:**
- [ ] Architecture doc explains `ContentBlock::Media`, `MediaSource`, and `MediaDelta`.
- [ ] Architecture doc states that v1 is reference-first and has no `MediaStore`.
- [ ] Architecture doc explains `video_url` mapping and provider-hosted video opt-in behavior for the built-in Chat provider.
- [ ] README shows one text+image user message example.
- [ ] README shows one tool output media example.
- [ ] README links to OpenAI Chat Completions and Anthropic Messages docs as provider mapping references.

**Verification:**
- [ ] `rg -n "ContentBlock::Media|MediaSource|MediaDelta|image_url|input_audio|file_id" README.md crates/noloong-agent-core/docs/ARCHITECTURE.md`
- [ ] `git diff --check README.md crates/noloong-agent-core/docs/ARCHITECTURE.md plans/CURRENT_PLAN.md`

**Dependencies:** Tasks 1-8

**Files likely touched:**
- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** M

#### Task 10: Run Final Quality Gates

**Description:** Run the full local verification suite and keep real-provider tests explicitly ignored unless they are intentionally invoked.

**Acceptance criteria:**
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo nextest run --workspace` passes.
- [ ] `cargo test --workspace` passes if nextest is unavailable.
- [ ] Existing OpenRouter DeepSeek live test still passes when manually run with `OPENROUTER_API_KEY`.
- [ ] No OpenRouter, DeepSeek, or provider-specific media constants are added under `crates/noloong-agent-core/src`.

**Verification:**
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`
- [ ] `rg -n "openrouter|deepseek|OPENROUTER|deepseek-v4" crates/noloong-agent-core/src -S`

**Dependencies:** Tasks 1-9

**Files likely touched:**
- None expected beyond fixes required by earlier tasks.

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Event log bloat from inline media | High | Make reference-first the default, keep inline optional, and document that large blobs should use URI/provider refs until `MediaStore` exists. |
| Provider wire shapes diverge | Medium | Keep core media model generic and isolate all OpenAI-compatible details in `chat_completions.rs`. |
| Silent media loss in unsupported roles | High | Fail fast with provider errors for unsupported system/assistant media instead of dropping content. |
| Video support becomes fake support | Medium | Map Chat-compatible URI/inline video to `video_url`, but keep provider-hosted video refs opt-in. |
| Audio transcript duplicates visible text | Medium | Store provider transcript in media metadata; only provider `content` becomes `TextDelta`. |
| JSON-RPC extensions fall out of sync | Medium | Use serde contract directly and add JS fixture tests for media stream and tool output. |

## Parallelization Opportunities

- Tasks 3 and 4 should be sequential because they share Chat input rendering.
- Tasks 7 and 8 can run in parallel after Tasks 1 and 2.
- Task 9 can start after public API names stabilize, but final doc examples should wait until Tasks 3-8 are implemented.
- Task 10 must run last.

## References

- OpenAI Chat Completions API: https://platform.openai.com/docs/api-reference/chat/create-chat-completion
- OpenAI Images and vision guide: https://platform.openai.com/docs/guides/images-vision?api-mode=chat
- OpenAI Audio guide: https://platform.openai.com/docs/guides/audio
- Anthropic Messages examples: https://docs.anthropic.com/en/api/messages-examples
- Anthropic Files API: https://docs.anthropic.com/en/docs/build-with-claude/files

## Open Questions

- None. Defaults chosen: reference-first media representation, core plus Chat Completions I/O in v1, and `video_url` for Chat-compatible URI/inline video while provider-hosted video refs remain opt-in.
