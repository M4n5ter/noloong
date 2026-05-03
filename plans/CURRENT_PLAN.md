# Implementation Plan: Fine-Grained Phase Hooks

## Overview

为 `noloong-agent-core` 增加比 `PhaseNode` 更细粒度、比 `ToolCallHook` 更通用的 `PhaseHook` 扩展层。v1 覆盖四个稳定 hook 点：`before_model_request`、`after_model_request`、`before_assistant_commit`、`after_assistant_commit`。Rust 内部扩展和 JSON-RPC 外部扩展都应使用同一语义：hook 按注册顺序串行执行，后一个 hook 看到前一个 hook 的修改结果，任意 hook 返回 error 时当前 phase 失败。

## Architecture Decisions

- 新增 `PhaseHook` trait，不复用 `ToolCallHook`，避免把 tool-specific context 扩散到 agent loop 其它阶段。
- `PhaseHook` 是标准 phase 内的拦截点；`PhaseNode` 仍用于替换完整 phase。
- hook result 使用完整替换语义，不引入 patch DSL：
  - `before_model_request` 可以替换 `ModelRequest`。
  - `after_model_request` 可以替换后续 phase 使用的 `Vec<ModelStreamEvent>`。
  - `before_assistant_commit` 可以替换折叠前的 `Vec<ModelStreamEvent>`。
  - `after_assistant_commit` 可以替换最终 append 的 `AgentMessage`。
- raw provider stream events 仍作为 provider 原始输出被记录；hook 修改后的 events/message 作为后续 phase 和最终 state commit 的输入。
- v1 不支持 hook priority、条件匹配、动态 enable/disable 或静默跳过 assistant commit；需要阻止 commit 时返回 error。

## Dependency Graph

1. Native `PhaseHook` API
2. Runtime registration
3. Standard phase integration
4. JSON-RPC extension adapter
5. Tests and docs

## Task List

### Phase 1: Native Hook Foundation

## Task 1: Define Native PhaseHook API

**Description:** 新增 `PhaseHook` trait 及四组 context/result 类型，建立 Rust 内部扩展的稳定公共 API。

**Acceptance criteria:**

- [ ] `PhaseHook` 是 `Send + Sync`，所有 hook methods 默认 no-op。
- [ ] context 类型包含 `run_id`、`turn_id`、`state` 和当前 hook point 必需的数据。
- [ ] result 类型只表达完整替换，不包含 provider-specific 字段或 patch DSL。
- [ ] hook method 返回 crate 现有 `Result<Option<...>>` 风格，error 会自然进入现有 phase failure 路径。

**Verification:**

- [ ] API compiles without requiring downstream providers to implement new methods.
- [ ] Unit test validates a no-op hook does not change request/events/message.

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/providers.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/phase_hooks.rs`

**Estimated scope:** S

## Task 2: Register Phase Hooks in Runtime Builders

**Description:** 在 runtime 和高层 agent builder 中注册 `PhaseHook`，并为标准 phase 提供只读访问。

**Acceptance criteria:**

- [ ] `AgentRuntimeBuilder` exposes `with_phase_hook`.
- [ ] 高层 `AgentBuilder` exposes matching phase hook registration if it already forwards tool hooks.
- [ ] `AgentRuntime` stores hooks as `Vec<Arc<dyn PhaseHook>>` and exposes read-only access.
- [ ] Existing runtime construction tests still pass without registering any hook.

**Verification:**

- [ ] Unit test builds a runtime with two hooks and observes registration order.
- [ ] `cargo test -p noloong-agent-core phase_hook_registration`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/src/agent.rs`
- `crates/noloong-agent-core/tests/phase_hooks.rs`

**Estimated scope:** S

### Checkpoint: Native Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core phase_hook`

### Phase 2: Standard Phase Integration

## Task 3: Add before_model_request Hook Execution

**Description:** 在 `model_request_prepare` 构造基础 `ModelRequest` 后执行 `before_model_request` hooks，并将最终 request 写入 `scratch.model_request`。

**Acceptance criteria:**

- [ ] hooks 按注册顺序串行执行。
- [ ] 每个 hook 接收到上一个 hook 修改后的 request。
- [ ] final `scratch.model_request` 是最后一个修改结果。
- [ ] hook error causes `model.request.prepare` to fail without calling model provider.

**Verification:**

- [ ] Test a hook can add request metadata observed by a mock provider.
- [ ] Test two hooks compose in registration order.
- [ ] Test hook error prevents model provider invocation.

**Dependencies:** Tasks 1-2

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/phase_hooks.rs`

**Estimated scope:** S

## Task 4: Add after_model_request Hook Execution

**Description:** 在 `model_stream` 完成 provider stream 后执行 `after_model_request` hooks，允许转换后续 phase 使用的 `ModelStreamEvent` 列表。

**Acceptance criteria:**

- [ ] hooks receive the final model request and provider-produced events.
- [ ] hooks can replace the event list used by later phases.
- [ ] raw streamed events are not retroactively mutated or double-recorded.
- [ ] hook error causes `model.stream` to fail before assistant commit.

**Verification:**

- [ ] Test `after_model_request` can replace a text delta before assistant commit.
- [ ] Test live stream forwarding still records provider raw events once.
- [ ] Test hook error records phase failure and no assistant message is appended.

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/phase_hooks.rs`

**Estimated scope:** M

## Task 5: Add Assistant Commit Hooks

**Description:** 在 `assistant_commit` 中加入 `before_assistant_commit` 和 `after_assistant_commit`，分别控制 event folding 输入和最终 append 的 assistant message。

**Acceptance criteria:**

- [ ] `before_assistant_commit` runs before folding `ModelStreamEvent` into `AgentMessage`.
- [ ] `after_assistant_commit` runs after message creation and before `AppendMessage` effect.
- [ ] `after_assistant_commit` can replace the final assistant message.
- [ ] hooks cannot silently skip commit; blocking commit requires returning error.

**Verification:**

- [ ] Test `before_assistant_commit` can modify final text through event replacement.
- [ ] Test `after_assistant_commit` can replace final assistant message content.
- [ ] Test error path appends no assistant message.

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/phase_hooks.rs`

**Estimated scope:** S

### Checkpoint: Core Hook Behavior

- [ ] `cargo test -p noloong-agent-core --test phase_hooks`
- [ ] `cargo test -p noloong-agent-core runtime`
- [ ] `cargo fmt --check`

### Phase 3: JSON-RPC Extension Support

## Task 6: Add PhaseHook Capability and Stdio Adapter

**Description:** 让非 Rust 扩展可以通过 JSON-RPC 注册和执行 phase hooks。

**Acceptance criteria:**

- [ ] `ExtensionCapability` supports `PhaseHook { id }`.
- [ ] `StdioPhaseHook` implements native `PhaseHook`.
- [ ] extension discovery registers phase hooks alongside model providers, tools, context providers, and phase nodes.
- [ ] adapter preserves native hook ordering relative to registration order.

**Verification:**

- [ ] Test extension manifest with `PhaseHook` capability registers a hook.
- [ ] Test native and JSON-RPC hooks compose in registration order.

**Dependencies:** Tasks 1-2

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/tests/jsonrpc.rs`

**Estimated scope:** M

## Task 7: Define phase_hook/run Wire Contract

**Description:** 新增 JSON-RPC method `phase_hook/run`，用统一 envelope 表达四类 hook point 的输入输出。

**Acceptance criteria:**

- [ ] request contains `hookId`、`hookPoint`、`runId`、`turnId`、`state` and hook-specific payload.
- [ ] response accepts optional `modelRequest`、`modelEvents`、`assistantMessage`.
- [ ] missing hook-specific response field means no-op.
- [ ] wrong response field type returns extension error.
- [ ] `hookPoint` uses stable snake_case values matching native hook names.

**Verification:**

- [ ] Test external `before_model_request` modifies request.
- [ ] Test external `after_model_request` modifies events.
- [ ] Test external `after_assistant_commit` modifies final message.
- [ ] Test malformed response fails the active phase.

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/tests/jsonrpc.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-extension.*`

**Estimated scope:** M

### Checkpoint: External Hook Path

- [ ] `cargo test -p noloong-agent-core --test jsonrpc phase_hook`
- [ ] `cargo test -p noloong-agent-core --test phase_hooks`

### Phase 4: Documentation and Quality Gate

## Task 8: Update Architecture Documentation

**Description:** 更新架构文档，把 “更细粒度 phase hooks” 从 future work 改为已支持的扩展机制，并明确与 `PhaseNode` / `ToolCallHook` 的边界。

**Acceptance criteria:**

- [ ] docs describe when to use `PhaseHook` versus `PhaseNode`.
- [ ] docs describe the four v1 hook points and mutation semantics.
- [ ] docs explain raw stream events versus hook-transformed commit input.
- [ ] docs mention JSON-RPC `phase_hook/run` for non-Rust extensions.

**Verification:**

- [ ] Manual doc review confirms no obsolete future-work wording remains.

**Dependencies:** Tasks 3-7

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** S

## Task 9: Run Full Quality Gate

**Description:** 对完整实现运行格式、lint 和测试，确保 hooks 没有破坏现有 provider/runtime 行为。

**Acceptance criteria:**

- [ ] Formatting passes.
- [ ] Clippy passes with no warnings.
- [ ] Workspace tests pass.
- [ ] Existing provider tests do not require hook-specific changes.

**Verification:**

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features`
- [ ] `cargo nextest run --workspace`

**Dependencies:** Tasks 1-8

**Files likely touched:**

- No source file should be changed by this task except fixes required by failing checks.

**Estimated scope:** S

### Checkpoint: Complete

- [ ] All task acceptance criteria are satisfied.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features` passes.
- [ ] `cargo nextest run --workspace` passes.
- [ ] `crates/noloong-agent-core/docs/ARCHITECTURE.md` matches implemented behavior.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Raw stream events and hook-transformed final message can differ | Medium | Document the distinction and test that final state follows transformed hook output. |
| Hook API becomes too broad and hard to stabilize | Medium | v1 only supports four explicit hook points and full replacement result types. |
| JSON-RPC envelope becomes ambiguous | Medium | Validate hook-specific response fields per `hookPoint`; missing means no-op, wrong type means error. |
| Hook ordering bugs cause nondeterministic behavior | High | Store hooks in registration order and add composition tests. |
| Error handling leaves partial state | High | Execute hooks before committing effects and assert no assistant message is appended on hook failure. |

## Parallelization Opportunities

- Tasks 1-2 must be sequential.
- Tasks 3-5 should be sequential because they share `phase.rs` and hook execution helpers.
- Tasks 6-7 should start after Task 1 and can be implemented after Task 2 while phase integration tests are being expanded.
- Task 8 can run in parallel after the public API and wire contract are stable.
- Task 9 must be last.

## Open Questions

- None. v1 defaults are fixed in this plan.
