# Implementation Plan: High-Performance SSE Client with Conservative Reconnect

## Overview

为 `noloong-agent-core` 增强 built-in HTTP provider 共享的 SSE 客户端能力：把当前 `Vec<u8>` + `Vec<String>` 的 per-provider 解码路径替换为低分配的 streaming decoder，并抽出内部共享的 SSE stream runner，统一处理 request timeout、stream idle timeout、cancellation、provider terminal event 和保守自动重连。

重连策略采用 conservative semantics：只有在还没有向 provider state 交付任何 SSE `data:` frame 前，才允许自动重新发起请求；一旦已经交付过任何模型数据，断连、chunk error 或 idle timeout 都必须返回可审计的 provider error，避免重复 token、重复 tool call 或生成分叉。

## Architecture Decisions

- `bytes::Buf` 本身不是主要性能收益点；核心优化是 `bytes::BytesMut` rolling buffer、`memchr` delimiter scanning、callback-based frame delivery，以及复用 scratch buffer 避免每帧创建 `String`。
- SSE parser 仍只向上层交付 `data:` payload；comment、unknown field、empty frame 被忽略，multiline `data:` 按 SSE 语义用 `\n` 拼接。
- SSE stream runner 先作为 crate 内部共享 helper，不暴露为 public generic client；public surface 只增加 provider config 可调的 `SseReconnectConfig`。
- 自动重连只覆盖 pre-data 阶段，包括 request send error、retryable HTTP status、EOF before data、idle timeout before data；post-data 阶段不做 replay/resume。
- 内置 provider 需要显式区分 terminal EOF 和 broken EOF：只有看到 provider terminal event 后 EOF 才是正常完成，否则 retry 或返回 error。
- 不实现 `Last-Event-ID` / mid-stream resume。当前 Chat Completions、Anthropic Messages、Responses built-in provider 没有统一可靠的事件恢复协议，盲目 re-POST 比 fail-fast 更危险。

## Dependency Graph

1. Direct dependencies: `bytes`, `memchr`
2. High-performance SSE decoder
3. Shared internal SSE stream runner
4. Provider config and integration
5. Reconnect and broken stream tests
6. Architecture documentation and quality gate

## Task List

### Phase 1: Decoder Foundation

## Task 1: Add Direct Parser Dependencies

**Description:** 为 `noloong-agent-core` 添加直接依赖 `bytes` 和 `memchr`，避免依赖 transitive deps 的偶然可见性，并为后续 decoder rewrite 建立明确依赖边界。

**Acceptance criteria:**

- [x] `crates/noloong-agent-core/Cargo.toml` 直接依赖 `bytes.workspace` 和 `memchr.workspace`，或在 workspace root 中声明后由 crate 引用。
- [x] 依赖版本通过 `cargo search --registry crates-io bytes` 和 `cargo search --registry crates-io memchr` 核对主流版本后选择。
- [x] `Cargo.lock` 不出现不必要的重复版本。

**Verification:**

- [x] `cargo check -p noloong-agent-core`
- [x] `cargo fmt --check`

**Dependencies:** None

**Files likely touched:**

- `Cargo.toml`
- `crates/noloong-agent-core/Cargo.toml`
- `Cargo.lock`

**Estimated scope:** S

## Task 2: Rewrite SSE Decoder for Low Allocation

**Description:** 重写 `src/sse.rs`，用 `BytesMut` 保存 rolling buffer，用 `memchr` 搜索 frame delimiter，并改为 callback-based API。单行 `data:` frame 尽量直接以 `&str` 交付；只有 multiline `data:` 需要使用复用 scratch buffer 拼接。

**Acceptance criteria:**

- [x] `SseDecoder::push` 不再返回 `Vec<String>`。
- [x] 单行 `data:` payload 不需要为每帧创建 owned `String`。
- [x] Multiline `data:` payload 使用 decoder 内部 scratch buffer 复用内存。
- [x] 正确支持 LF、CRLF、跨 chunk CRLF、comment、unknown field、empty frame、UTF-8 payload。
- [x] Invalid UTF-8 返回 `AgentCoreError::Provider`，不使用 lossy conversion 静默吞错。
- [x] `finish` 只 flush 已完整或可安全补齐的 pending frame，不把纯 whitespace/comment 误交付为 data。

**Verification:**

- [x] `cargo test -p noloong-agent-core sse_decoder`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/sse.rs`
- `crates/noloong-agent-core/src/chat_completions.rs`

**Estimated scope:** M

### Checkpoint: Decoder

- [x] Decoder unit tests cover previous behavior and stricter UTF-8 behavior.
- [x] No provider behavior is changed yet except adapting to callback API where needed.
- [x] `cargo test -p noloong-agent-core sse_decoder`

### Phase 2: Shared SSE Runner

## Task 3: Add Internal SSE Stream Runner

**Description:** 新增 crate 内部共享 helper，统一三类 built-in provider 的 request send、status handling、chunk read、idle timeout、cancellation、decoder feeding 和 frame handler 调用逻辑。每次 retry 必须通过 request factory 重新构造 `reqwest::RequestBuilder`。

**Acceptance criteria:**

- [x] 新增内部 stream runner，provider 传入 provider label、request factory、timeouts、cancellation token、frame handler、terminal predicate。
- [x] Request timeout 和 stream idle timeout 语义与现有 provider 保持一致。
- [x] Cancellation 返回 `AgentCoreError::Aborted`，不触发 retry。
- [x] HTTP non-success 读取最多 2048 chars body 后生成 provider error。
- [x] 408、429、5xx 在 pre-data 阶段可重试；4xx validation/auth error 默认不可重试。
- [x] EOF after terminal event 正常完成；EOF before terminal 按 pre-data retry / post-data error 处理。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions stream_timeout`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages stream_timeout`
- [x] `cargo test -p noloong-agent-core --test responses stream_timeout`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/src/sse.rs`
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/src/responses.rs`

**Estimated scope:** M

## Task 4: Add Reconnect Configuration

**Description:** 增加 public `SseReconnectConfig`，让 built-in provider config 可以配置或关闭保守重连。默认开启少量 pre-data retry，但保持总等待时间有上界。

**Acceptance criteria:**

- [x] `SseReconnectConfig { max_reconnects, initial_backoff, max_backoff }` 使用 `Duration` 字段。
- [x] `Default` 为 `max_reconnects = 2`、`initial_backoff = 200ms`、`max_backoff = 2s`。
- [x] `SseReconnectConfig::disabled()` 等价于 `max_reconnects = 0`。
- [x] `ChatCompletionsProviderConfig::stream_reconnect(config)` 可配置。
- [x] `AnthropicMessagesProviderConfig::stream_reconnect(config)` 可配置。
- [x] `ResponsesApiProviderConfig::stream_reconnect(config)` 可配置。
- [x] Backoff 使用确定性 exponential backoff，不引入 random/jitter 依赖。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions reconnect_config`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages reconnect_config`
- [x] `cargo test -p noloong-agent-core --test responses reconnect_config`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/sse.rs`
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/lib.rs`

**Estimated scope:** M

### Checkpoint: Shared Runner

- [x] 三个 provider 已经不再维护各自重复的 chunk/idle/decoder loop。
- [x] Disabled reconnect 能恢复 fail-fast 行为。
- [x] Existing timeout/cancellation tests pass.

### Phase 3: Provider Integration and Stream Semantics

## Task 5: Wire Chat Completions to Shared Runner

**Description:** 将 Chat Completions provider 接入共享 SSE runner，并把 `[DONE]` 作为 terminal event。保留 synthetic `ModelStreamEvent::Started` 的当前行为。

**Acceptance criteria:**

- [x] Chat provider 使用 shared SSE runner，删除本地 duplicated read loop。
- [x] `[DONE]` 设置 terminal/done，并停止继续解析后续 provider chunks。
- [x] EOF before `[DONE]` 且未交付 data frame 时可重试。
- [x] EOF after any data frame but before `[DONE]` 返回 provider error。
- [x] Existing thinking、tool call、text delta、usage 行为保持不变。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions`

**Dependencies:** Tasks 3-4

**Files likely touched:**

- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**Estimated scope:** M

## Task 6: Wire Anthropic Messages to Shared Runner

**Description:** 将 Anthropic Messages provider 接入共享 SSE runner，并把 `message_stop`、`[DONE]`、provider `error` event 作为 terminal event。

**Acceptance criteria:**

- [x] Anthropic provider 使用 shared SSE runner，删除本地 duplicated read loop。
- [x] `message_stop`、`[DONE]`、`error` 都会结束 stream。
- [x] EOF before terminal 且未交付 data frame时可重试。
- [x] EOF after any data frame but before terminal 返回 provider error。
- [x] Existing thinking、tool use、content block 行为保持不变。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test anthropic_messages`

**Dependencies:** Tasks 3-4

**Files likely touched:**

- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**Estimated scope:** M

## Task 7: Wire Responses API to Shared Runner

**Description:** 将 Responses API provider 接入共享 SSE runner，并把 `response.completed`、`response.done`、`response.failed`、`response.incomplete`、`[DONE]` 作为 terminal event。

**Acceptance criteria:**

- [x] Responses provider 使用 shared SSE runner，删除本地 duplicated read loop。
- [x] Completed/done/failed/incomplete/[DONE] 都会结束 stream。
- [x] EOF before terminal 且未交付 data frame 时可重试。
- [x] EOF after any data frame but before terminal 返回 provider error。
- [x] Existing reasoning、tool call、output item、usage 行为保持不变。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test responses`

**Dependencies:** Tasks 3-4

**Files likely touched:**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Checkpoint: Provider Integration

- [x] Built-in providers share the same SSE stream runner.
- [x] Provider-specific terminal semantics are covered by tests.
- [x] `cargo test -p noloong-agent-core --test chat_completions`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`
- [x] `cargo test -p noloong-agent-core --test responses`

### Phase 4: Reconnect Test Matrix

## Task 8: Add Deterministic Reconnect Fixtures

**Description:** 扩展 test support，让 mock server 能模拟 pre-data close、post-data close、retryable HTTP status、request count 和断开的 chunked stream。现有 `Content-Length` + close mock 对 broken stream 覆盖不足，需要补一套更明确的 fixture。

**Acceptance criteria:**

- [x] Test support 可以按顺序返回多个 response，并记录 request count。
- [x] 支持 close before body / close after partial SSE frame / close after complete data frame before terminal。
- [x] 支持 retryable status 后第二次成功。
- [x] 支持 disabled reconnect 场景下验证只请求一次。
- [x] 新 fixture 不破坏现有 provider tests。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions reconnect`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages reconnect`
- [x] `cargo test -p noloong-agent-core --test responses reconnect`

**Dependencies:** Tasks 3-7

**Files likely touched:**

- `crates/noloong-agent-core/tests/support/mod.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

## Task 9: Cover Conservative Reconnect Semantics

**Description:** 为三个 provider 增加同构的 reconnect behavior tests，确保 pre-data retry 生效、post-data 不 retry、retry exhaustion error 可诊断。

**Acceptance criteria:**

- [x] First response closes before any data frame, second response succeeds，request count 为 2。
- [x] First response 只包含 comment/empty frame 后断开，仍视为 no model data，可以 retry。
- [x] First response 已交付 complete `data:` frame 后断开，不 retry，并返回 provider error。
- [x] Retry attempts exhausted 后 error 包含 provider label 和 attempt count。
- [x] `SseReconnectConfig::disabled()` 场景中第一次 pre-data EOF 直接 error。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions reconnect`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages reconnect`
- [x] `cargo test -p noloong-agent-core --test responses reconnect`

**Dependencies:** Task 8

**Files likely touched:**

- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

### Checkpoint: Reconnect Semantics

- [x] Pre-data automatic reconnect is proven across all built-in providers.
- [x] Post-data disconnect never replays model requests.
- [x] Retry disabled and retry exhaustion behavior are deterministic.

### Phase 5: Documentation and Quality Gate

## Task 10: Document SSE Client Semantics

**Description:** 更新架构文档，说明内置 SSE stream runner 的职责、性能策略、保守重连语义、terminal EOF 判定，以及为什么不实现 mid-stream resume。

**Acceptance criteria:**

- [x] `ARCHITECTURE.md` 增加 built-in provider SSE client 小节。
- [x] 文档明确 `BytesMut`/`memchr` 优化点，而不是把 `Buf` trait 描述成主要收益。
- [x] 文档明确 pre-data retry 和 post-data no-retry 的边界。
- [x] “后续演进方向” 包含可选的 resumable SSE / provider-specific resume extension，而不是把它混入 v1。

**Verification:**

- [x] Manual review: `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- [x] `git diff --check`

**Dependencies:** Tasks 2-9

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** S

## Task 11: Run Full Quality Gate

**Description:** 完成格式、lint、workspace tests 和 nextest，确保 shared runner 改动没有破坏 agent core 其它阶段。

**Acceptance criteria:**

- [x] Formatting clean.
- [x] Clippy clean without adding broad `allow` suppressions.
- [x] Workspace tests pass.
- [x] Diff does not contain whitespace errors.

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo nextest run --workspace`
- [x] `git diff --check`

**Dependencies:** Tasks 1-10

**Files likely touched:**

- No source files expected beyond fixes discovered by verification.

**Estimated scope:** S

### Checkpoint: Complete

- [x] `plans/CURRENT_PLAN.md` reflects the implementation plan.
- [x] SSE decoder is lower allocation and stricter about invalid UTF-8.
- [x] Built-in providers use a shared stream runner.
- [x] Conservative automatic reconnect is tested across Chat Completions, Anthropic Messages, and Responses API.
- [x] Architecture documentation is updated.
- [x] Full quality gate passes.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Retrying after partial model output duplicates tokens or tool calls | High | Track whether any `data:` frame has been delivered; never retry after delivery. |
| EOF before terminal currently looks like successful completion | High | Shared runner requires provider terminal predicate before treating EOF as success. |
| Low-allocation parser introduces SSE edge-case regressions | Medium | Preserve focused decoder tests for multiline, comments, CRLF, cross-chunk boundaries, UTF-8. |
| Shared runner over-generalizes provider-specific terminal behavior | Medium | Provider owns frame handler and terminal predicate; runner owns only transport mechanics. |
| Retry tests become flaky due to timing/backoff | Medium | Use deterministic backoff and small test-specific durations; avoid sleeping longer than needed. |
| Public config type grows too generic too early | Low | Keep `SseReconnectConfig` narrow: retries and bounded backoff only. |

## Not Doing in This Iteration

- Mid-stream resume with `Last-Event-ID`.
- Replaying a request after any model data frame has been delivered.
- Public generic SSE client API for arbitrary users.
- Jitter/randomized backoff dependency.
- Provider-specific persisted stream cursor protocol.

## Parallelization Opportunities

- Task 2 decoder rewrite and Task 8 test fixture design can be prepared independently after dependencies are added, but final integration should be sequential.
- Tasks 5, 6, and 7 are parallelizable after Task 3 and Task 4 define the shared runner contract.
- Task 10 documentation can proceed in parallel with Task 9 once final semantics are stable.

## Open Questions

- None blocking. Current plan assumes conservative reconnect is preferred over lossy mid-stream resume for v1.
