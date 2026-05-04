# Implementation Plan: Split Bloated Core Files And Deny Dead Code

## Overview

本轮做行为保持型重构，第一阶段只处理 agent core 最膨胀、最容易继续堆积复杂度的文件：`runtime.rs`、`phase.rs`、`types.rs`、`jsonrpc.rs` 和 `tests/core.rs`。目标是拆出清晰模块边界，同时在 Cargo lint 层面禁止 `dead_code`，并移除当前唯一的 `#[allow(dead_code)]`。

已确认的当前状态：

- `crates/noloong-agent-core/src/runtime.rs` 约 1737 行，是当前最大 core 文件。
- `crates/noloong-agent-core/src/phase.rs` 约 1251 行，混合了标准 phase、hook runner、assistant commit、tool execution 和 approval resume。
- `crates/noloong-agent-core/src/types.rs` 约 1052 行，混合 event/state/message/media/thinking/tool/extension wire types。
- `crates/noloong-agent-core/src/jsonrpc.rs` 约 1023 行，混合 stdio process、adapter implementations、wire DTO 和 hook payload。
- `crates/noloong-agent-core/tests/core.rs` 约 1667 行，适合按行为域拆分。
- 当前唯一 dead-code allow 是 `crates/noloong-agent-core/src/runtime.rs` 中的 `_standard_phase_ids`。

## Architecture Decisions

- 先做 **Core first** 拆分：本轮不拆 `chat_completions.rs`、`responses.rs`、`anthropic_messages.rs`，除非 core module 重排造成必要 import 调整。
- 拆分是行为保持型重构：不改变 public API、JSON-RPC wire contract、event schema、provider payload 或 serde tag/name。
- `mod.rs` 只保留 public facade、共享类型和 re-export；具体实现进入子模块。
- 子模块默认保持 private，只把跨模块必须使用的项提升到 `pub(super)` 或 `pub(crate)`。
- 添加 workspace 级 lint：`dead_code = "deny"`；不允许用新的 `#[allow(dead_code)]` 绕过。
- 拆分后单个 core implementation 文件目标不超过约 900 行；如果超出，继续按职责拆一层。

## Task List

### Phase 1: Lint Foundation

#### Task 1: Add Cargo dead-code deny

**Description:** 在 workspace Cargo 配置里启用 dead-code deny，并让 root package 与 `noloong-agent-core` 继承 workspace lint。删除或替换 `_standard_phase_ids`，确保没有任何 `#[allow(dead_code)]`。

**Acceptance criteria:**

- [ ] Root `Cargo.toml` 包含 workspace lint 配置，`dead_code` 为 deny。
- [ ] `crates/noloong-agent-core/Cargo.toml` 继承 workspace lint。
- [ ] 仓库内没有 `#[allow(dead_code)]` 或 `allow(dead_code)`。

**Verification:**

- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `rg '#\[allow\([^\]]*dead_code|allow\(dead_code\)' Cargo.toml crates/noloong-agent-core`

**Dependencies:** None

**Files likely touched:**

- `Cargo.toml`
- `crates/noloong-agent-core/Cargo.toml`
- `crates/noloong-agent-core/src/runtime.rs`

**Estimated scope:** Small

### Checkpoint: Lint Foundation

- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo fmt --check`

### Phase 2: Runtime Split

#### Task 2: Split runtime approval and run-flow internals

**Description:** 把 `runtime.rs` 转成 `runtime/mod.rs` facade，把 tool approval resume/timeout helpers 和 run loop/event commit internals 拆出，保持 `AgentRuntime` public methods 不变。

**Acceptance criteria:**

- [ ] `AgentRuntime`, `AgentRuntimeBuilder`, `AgentInput`, `RunReport`, `RuntimeQueues`, `AgentEventSink` 仍从 `noloong_agent_core` 原路径导出。
- [ ] Tool approval pause/resume/timeout/abort 行为不变。
- [ ] Runtime event sequencing、event sink、replay state 语义不变。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test agent`
- [ ] `cargo test -p noloong-agent-core --test core tool_approval`
- [ ] `cargo test -p noloong-agent-core --test conformance`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/src/runtime/mod.rs`
- `crates/noloong-agent-core/src/runtime/approval.rs`
- `crates/noloong-agent-core/src/runtime/run_loop.rs`
- `crates/noloong-agent-core/src/runtime/builder.rs`

**Estimated scope:** Medium

#### Task 3: Keep runtime builder registration isolated

**Description:** 将 builder、extension capability validation、default phase construction 和 context compaction resolution 从 runtime facade 中拆出，避免 runtime facade 继续承担注册细节。

**Acceptance criteria:**

- [ ] Builder API 链式调用完全保持原签名。
- [ ] Stdio extension capability duplicate validation 行为不变。
- [ ] Context compaction registration 和 default phase insertion 行为不变。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test extension_conformance`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc_conformance public_runner_strict_fixture_passes`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/src/runtime/builder.rs`
- `crates/noloong-agent-core/src/runtime/mod.rs`
- `crates/noloong-agent-core/src/runtime/compaction.rs`

**Estimated scope:** Medium

### Checkpoint: Runtime Split

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test -p noloong-agent-core --test agent`
- [ ] `cargo test -p noloong-agent-core --test conformance`

### Phase 3: Phase Split

#### Task 4: Split phase facade and standard phases

**Description:** 把 `phase.rs` 转成 `phase/mod.rs` facade，保留 phase constants、`PhaseContext`、`PhaseScratch`、`PhaseOutput`、`PhaseNode`、`StandardPhase` 的公开位置；把 context/model/assistant/turn standard phase implementation 拆到子模块。

**Acceptance criteria:**

- [ ] `lib.rs` 的 phase re-export 不需要对外改变。
- [ ] Standard phase IDs、phase order 和 insertion behavior 不变。
- [ ] Context compaction、model request prepare、model stream、assistant commit 行为不变。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test phase_hooks`
- [ ] `cargo test -p noloong-agent-core --test compaction`
- [ ] `cargo test -p noloong-agent-core --test core phase`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/src/phase/mod.rs`
- `crates/noloong-agent-core/src/phase/standard.rs`
- `crates/noloong-agent-core/src/phase/hooks.rs`
- `crates/noloong-agent-core/src/phase/assistant.rs`

**Estimated scope:** Medium

#### Task 5: Split tool execution and approval continuation

**Description:** 将 `tool.execute`、parallel/sequential prepared execution、approval preflight、approval continuation resume、tool output helpers 拆入独立子模块，保留现有 source-order commit 和 approval pause semantics。

**Acceptance criteria:**

- [ ] Parallel tool execution completion order 和 commit source order 测试仍通过。
- [ ] Tool approval preflight 并发测试仍通过。
- [ ] Approval resume 后 audit、tool output、run status replay 不变。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test core tool`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc stdio_tool_call_hook_can_pause_for_approval_and_resume`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent-core/src/phase/tool.rs`
- `crates/noloong-agent-core/src/phase/mod.rs`
- `crates/noloong-agent-core/src/runtime/approval.rs`

**Estimated scope:** Medium

### Checkpoint: Phase Split

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `cargo test -p noloong-agent-core --test phase_hooks`
- [ ] `cargo test -p noloong-agent-core --test compaction`

### Phase 4: Types And JSON-RPC Split

#### Task 6: Split public type modules without changing re-exports

**Description:** 把 `types.rs` 拆成 `types/mod.rs` facade，以及 event/state、message/media/thinking、tool/approval、model/hook、extension capability 子模块。所有 public type 名称和 serde 行为保持不变。

**Acceptance criteria:**

- [ ] `pub use types::*` 仍导出同一组 public items。
- [ ] Existing serde round-trip tests 不需要改期望值。
- [ ] Event replay 和 extension docs contract 不受影响。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test core permission_events_serde_round_trip`
- [ ] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc_conformance`

**Dependencies:** Tasks 2, 4

**Files likely touched:**

- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/types/mod.rs`
- `crates/noloong-agent-core/src/types/events.rs`
- `crates/noloong-agent-core/src/types/messages.rs`
- `crates/noloong-agent-core/src/types/tools.rs`

**Estimated scope:** Medium

#### Task 7: Split JSON-RPC bridge internals

**Description:** 把 `jsonrpc.rs` 拆成 stdio process/connection、adapter implementations、hook payloads、wire DTOs 四类模块。`StdioExtension` 和 `StdioExtensionConfig` 公开 API 保持不变。

**Acceptance criteria:**

- [ ] JSON-RPC request/response/error handling 行为不变。
- [ ] Model stream notification buffering、timeout 和 cancellation 行为不变。
- [ ] Tool hook approval wire output 仍通过 strict conformance。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test jsonrpc`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [ ] `cargo test -p noloong-agent-core --test extension_language_examples`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/src/jsonrpc/mod.rs`
- `crates/noloong-agent-core/src/jsonrpc/process.rs`
- `crates/noloong-agent-core/src/jsonrpc/adapters.rs`
- `crates/noloong-agent-core/src/jsonrpc/wire.rs`

**Estimated scope:** Medium

### Checkpoint: Types And JSON-RPC

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc_conformance`

### Phase 5: Test Suite Split

#### Task 8: Split core tests by behavior

**Description:** 将 `tests/core.rs` 中的 mixed tests 拆成行为域测试文件，并将共享 fixtures/helpers 放入 `tests/support`。尽量保留原测试函数名，便于历史失败定位。

**Acceptance criteria:**

- [ ] `tests/core.rs` 不再承担所有 core behavior tests。
- [ ] Shared helpers 不复制粘贴到多个测试文件。
- [ ] 测试覆盖不减少，原有关键测试名称保留或迁移后可 grep。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test core`
- [ ] `cargo nextest run -p noloong-agent-core`

**Dependencies:** Tasks 2, 4, 6

**Files likely touched:**

- `crates/noloong-agent-core/tests/core.rs`
- `crates/noloong-agent-core/tests/runtime_core.rs`
- `crates/noloong-agent-core/tests/tool_flow.rs`
- `crates/noloong-agent-core/tests/media_flow.rs`
- `crates/noloong-agent-core/tests/support/mod.rs`

**Estimated scope:** Medium

### Final Checkpoint

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc`
- [ ] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [ ] `cargo test -p noloong-agent-core --test phase_hooks`
- [ ] `cargo test -p noloong-agent-core --test extension_language_examples`
- [ ] `rg '#\[allow\([^\]]*dead_code|allow\(dead_code\)' Cargo.toml crates/noloong-agent-core`
- [ ] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Public re-export drift | High | Keep facade modules and `lib.rs` exports stable; verify with full test suite |
| Serde wire shape drift | High | Do not rename fields/tags; run JSON-RPC and docs contract tests after type split |
| Event replay behavior drift | High | Keep reducer/event tests green after every runtime/phase checkpoint |
| Circular module dependencies | Medium | Put shared structs in facade or small internal modules; prefer `pub(super)` over broad `pub(crate)` |
| Test split loses coverage | Medium | Move tests first with same names; avoid rewriting test logic during split |
| `dead_code = deny` exposes unused helpers in examples/tests | Medium | Delete unused code or make it used; do not add allow attributes |

## Parallelization Opportunities

- Runtime split and phase split can be worked in separate branches only after Task 1, but they both touch imports and should be merged carefully.
- Type split should wait until runtime and phase module boundaries settle, because many modules import public core types.
- Test split can start after the production module split stabilizes; otherwise helper import churn will dominate.

## Open Questions

- None. Scope is locked to Core first for this pass; provider adapter file splitting is a later plan.
