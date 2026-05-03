# Implementation Plan: Strict JSON-RPC Extension Conformance Suite

## Overview

为 `noloong-agent-core` 增加更严格的内部 JSON-RPC stdio extension conformance suite。目标是系统性覆盖 extension lifecycle、capability discovery、typed adapter payload、JSON-RPC request/response failure semantics、stream notification 行为，以及 model/tool/context/phase/hook/compaction summarizer 的跨语言扩展契约。

本计划只做 crate 内部 integration tests 和 dedicated JS fixture，不新增公开 conformance CLI。默认严格策略是 core strict：typed response/schema 错误必须 fail，active stream 的 malformed event 必须 fail；未知 notification 和未知 response id 保持忽略或超时，不引入 `deny_unknown_fields` 级别的完全严格兼容破坏。

## Architecture Decisions

- 新增独立 fixture `jsonrpc-conformance-extension.mjs`，避免继续膨胀现有 `stdio-extension.mjs`。
- 新增 `tests/jsonrpc_conformance.rs`，把协议级 conformance 与现有端到端功能测试分开。
- 失败策略保持分层：JSON-RPC protocol failures 映射为 `AgentCoreError::JsonRpc`，typed payload serde failures 保持 `AgentCoreError::Json`。
- 如测试暴露 bridge 当前过松，只做最小内部修复，不新增 public crate API。
- Conformance tests 不依赖真实 provider，不使用 API key，不访问网络。

## Dependency Graph

1. Dedicated fixture harness and Rust test helpers
2. Lifecycle and capability conformance
3. Request/response failure semantics
4. Adapter payload conformance
5. Stream notification conformance
6. Documentation and quality gates

## Task List

### Phase 1: Foundation

## Task 1: Add Dedicated JSON-RPC Conformance Fixture

**Description:** 新增一个专门用于协议 conformance 的 JS stdio fixture，通过 mode 参数模拟 lifecycle、capability、response、stream、payload assertion 和 failure 场景。该 fixture 不承担现有 happy path fixture 的兼容职责，只服务严格测试矩阵。

**Acceptance criteria:**

- [x] Fixture 支持 `initialize`、`capabilities/list`、`model/stream`、`tool/execute`、`context/apply`、`phase/run`、`phase_hook/run`、`compaction/summarize`。
- [x] Fixture 支持 mode-based 行为切换：malformed result、JSON-RPC error、wrong response id、stdout close、request timeout、stream notification variants。
- [x] Fixture 能断言 core 发送的 request params，并在断言失败时返回 JSON-RPC error。
- [x] Rust test helper 能统一创建 `StdioExtensionConfig`、设置短 timeout、构造 runtime。

**Verification:**

- [x] `node --check crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance fixture_smoke`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**Estimated scope:** M

## Task 2: Add Lifecycle Conformance Tests

**Description:** 覆盖 extension startup lifecycle，确保 `initialize`、manifest parsing、capability listing 和 shutdown 行为符合 core 当前协议边界。

**Acceptance criteria:**

- [x] `initialize` request 包含 `protocolVersion: 1`。
- [x] manifest `name` 和 `version` 正确反序列化并可通过 `StdioExtension::manifest()` 读取。
- [x] malformed manifest 会让 `StdioExtension::connect` 失败。
- [x] `shutdown` request 成功时 process 可正常退出。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance lifecycle`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** S

### Checkpoint: Fixture and Lifecycle

- [x] Fixture passes `node --check`.
- [x] Lifecycle conformance tests pass.
- [x] Existing `tests/jsonrpc.rs` still compiles.

### Phase 2: Capability and Request Semantics

## Task 3: Add Capability Discovery Conformance

**Description:** 覆盖 extension capability schema 和 registration policy，确保 core 对 malformed capability 和 duplicate registration 做确定性处理。

**Acceptance criteria:**

- [x] `model_provider`、`tool`、`context_provider`、`phase_node`、`phase_hook`、`compaction_summarizer` capability 都能被注册到 runtime。
- [x] malformed capability list 和 unknown capability type 会让 extension registration fail。
- [x] duplicate model provider id、tool name、context provider id、phase id、phase hook id、compaction summarizer id 均会 fail，错误信息包含 duplicate 或具体冲突标识。
- [x] duplicate failure 不会留下半注册 runtime 可继续使用的状态。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance capabilities`

**Dependencies:** Tasks 1-2

**Files likely touched:**

- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**Estimated scope:** M

## Task 4: Add JSON-RPC Request/Response Failure Tests

**Description:** 明确 JSON-RPC response 层的严格行为，防止 silent success、pending request 泄漏或永久挂起。

**Acceptance criteria:**

- [x] JSON-RPC error response 传播为 `AgentCoreError::JsonRpc`，错误文本保留 extension message。
- [x] wrong response id 不匹配 pending request，并最终走 request timeout。
- [x] missing `result`、invalid typed `result` 对当前 request fail。
- [x] extension stdout close 会 fail 所有 pending requests。
- [x] cancellation 会移除 pending request，后续 late response 不影响 runtime。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance request_response`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

### Checkpoint: Protocol Semantics

- [x] Capability conformance tests pass.
- [x] Request/response failure tests pass.
- [x] `cargo test -p noloong-agent-core --test jsonrpc` still passes.

### Phase 3: Adapter Payloads and Streaming

## Task 5: Add Adapter Payload Conformance

**Description:** 验证 Rust bridge 发给 extension 的 typed params shape，覆盖所有 extension adapter 的必需字段和 serde wire shape。

**Acceptance criteria:**

- [x] `model/stream` params 包含 `providerId`、`streamId`、完整 `ModelRequest`。
- [x] `tool/execute` params 包含 `toolName` 和 `ToolRequest`，tool output updates/media 能 round-trip。
- [x] `context/apply` params 包含 `providerId` 和 `ContextRequest`。
- [x] `phase/run` params 包含 `phaseId`、`runId`、`turnId`、`state`、`scratch`。
- [x] `phase_hook/run` 覆盖 `before_model_request`、`after_model_request`、`before_assistant_commit`、`after_assistant_commit` 四个 hook point 的 payload。
- [x] `compaction/summarize` params 包含 `summarizerId`、`runId`、`turnId`、`previousSummary`、`messagesToSummarize`、`turnPrefixMessages`、`tokenBudget`、`metadata`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance adapter_payloads`

**Dependencies:** Tasks 1-4

**Files likely touched:**

- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

## Task 6: Add Adapter Malformed Result Conformance

**Description:** 覆盖每个 adapter 的 malformed result 行为，确保错误落在正确 phase/request，并能通过 event replay 观察到失败状态。

**Acceptance criteria:**

- [x] malformed `model/stream` result 或 event 会 fail `model.stream` phase。
- [x] malformed `tool/execute` output 会按 core tool policy 转成 auditable error tool result，不直接 fail run。
- [x] malformed `context/apply` effect 会 fail `context.prepare` phase。
- [x] malformed `phase/run` output 会 fail active extension phase。
- [x] malformed `phase_hook/run` output 会 fail对应 hook 所在 phase。
- [x] malformed `compaction/summarize` output 会 fail `context.compact` phase。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance malformed_results`
- [x] `cargo test -p noloong-agent-core --test conformance runtime_failure_records_failed_replay_state`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

## Task 7: Add Stream Notification Conformance

**Description:** 严格覆盖 `stream/event` notification 的 ordering、buffering、terminal、timeout、malformed event 和 stream id 隔离行为。

**Acceptance criteria:**

- [x] `stream/event` 可在 `model/stream` response 前增量到达，并实时写入 runtime event sink。
- [x] response 携带 buffered events 时不会重复提交同一 stream event。
- [x] terminal stream event 可在 response 缺失时 settle 当前 model stream。
- [x] active stream 的 malformed event 会立即 fail 当前 stream，而不是静默等待 timeout。
- [x] unrelated unknown stream notification 不影响 active stream。
- [x] `stream_timeout` 和 `request_timeout` 行为保持独立。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance stream`
- [x] `cargo test -p noloong-agent-core --test conformance jsonrpc`

**Dependencies:** Tasks 1 and 4

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

### Checkpoint: Adapter and Stream Coverage

- [x] Adapter payload tests pass.
- [x] Malformed adapter result tests pass.
- [x] Stream conformance tests pass.
- [x] Existing provider/tool/context/phase/hook JSON-RPC tests still pass.

### Phase 4: Documentation and Quality Gate

## Task 8: Document JSON-RPC Conformance Policy

**Description:** 更新架构文档，说明 JSON-RPC extension 的 conformance policy、strictness boundary、failure mapping 和测试覆盖范围。

**Acceptance criteria:**

- [x] `ARCHITECTURE.md` 说明 core strict policy：typed schema error fail、active stream malformed event fail、unknown notification ignored。
- [x] 文档说明 JSON-RPC protocol failures 与 serde typed failures 的错误归类。
- [x] 文档说明 conformance suite 是内部 integration tests，不是 public CLI。
- [x] `plans/CURRENT_PLAN.md` 保持本计划并反映最终完成状态。

**Verification:**

- [x] Manual doc review confirms policy matches implemented behavior.

**Dependencies:** Tasks 1-7

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

## Task 9: Final Quality Gate

**Description:** 对完整实现运行格式、lint、node syntax check、目标测试和全量测试，确保 conformance suite 不破坏已有 runtime/provider 行为。

**Acceptance criteria:**

- [x] All new JSON-RPC conformance tests pass.
- [x] Existing JSON-RPC, conformance, compaction, phase hook tests pass.
- [x] Workspace clippy has no warnings.
- [x] Node fixtures pass syntax checks.

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [x] `cargo test -p noloong-agent-core --test jsonrpc`
- [x] `cargo test -p noloong-agent-core --test conformance`
- [x] `cargo nextest run --workspace`
- [x] `node --check crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- [x] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`
- [x] `git diff --check`

**Dependencies:** Tasks 1-8

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

### Checkpoint: Complete

- [x] `cargo nextest run --workspace` passes.
- [x] `cargo clippy --workspace --all-targets --all-features` passes.
- [x] New conformance suite is deterministic and does not require network or API keys.
- [x] Documentation matches implemented strictness policy.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Duplicate capability policy exposes existing silent overwrite behavior | Medium | Add tests first, then make registration fail with clear `AgentCoreError::JsonRpc` or `AgentCoreError::Phase` message. |
| Malformed stream notification currently buffers silently | Medium | Route active stream serde failures through the stream channel as `Err`, while preserving unknown stream id ignore behavior. |
| Fixture modes become too broad and brittle | Medium | Keep one behavior per mode, use helper functions for JSON-RPC response and request assertion. |
| Test suite runtime grows due many stdio process launches | Low | Keep timeout short, group related assertions where it does not hide failure source. |
| Over-tightening JSON compatibility breaks external extensions | Medium | Do not add `deny_unknown_fields`; only fail missing/invalid typed fields required by current serde contracts. |

## Parallelization Opportunities

- Task 2 and Task 3 can be implemented after Task 1 by different agents if they do not touch shared runtime validation at the same time.
- Task 5 adapter payload tests can be written in parallel with Task 7 stream tests after fixture helpers exist.
- Documentation in Task 8 can start once strictness decisions from Tasks 3-7 are stable.

## Open Questions

- None. Current defaults are internal conformance suite plus core strict policy.
