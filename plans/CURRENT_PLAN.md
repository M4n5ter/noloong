# Implementation Plan: Tool Permission Model

## Overview

为 `noloong-agent-core` 增加一等的 tool permission / approval 机制。工具通过 `ToolSpec` 声明 capability requirements，`ToolCallHook` 在执行前返回 allow/deny decision，runtime 把 permission request 和 decision 写入 event log，denied tool call 转成可审计的 error tool result，而不是直接让 run fail。

v1 不做交互式人工等待队列，只实现同步 policy approval。这个选择能先把核心抽象、审计事件、native hook 和 JSON-RPC hook 打通，后续再把 interactive/human approval queue 建在同一套 decision/event 模型上。

## Architecture Decisions

- Permission 是 tool lifecycle 的一等审计边界，不再只是 `before_tool_call` 返回 `block: bool` 的隐藏副作用。
- `Deny` 不等于 phase failure；它生成 `ToolOutput { is_error: true }` 并作为 tool result message 进入上下文，保留模型后续自我修正能力。
- `AgentCoreError::Aborted` 仍是控制流中止信号，不会被 permission policy 转换为 tool result。
- Permission metadata 默认不泄漏到 Chat Completions / Anthropic Messages / Responses hosted tool schema；JSON-RPC typed model request 保留完整 `ToolSpec`。
- JSON-RPC 外部扩展通过新的 `tool_call_hook` capability 实现同一套 permission policy，不要求扩展语言是 Rust。
- 没有兼容性负担，因此可以把现有 `BeforeToolCallResult { block, reason }` 重构为 decision-based API。

## Dependency Graph

1. Core permission types and hook API
2. Runtime tool execution audit events
3. Replay/error semantics and native tests
4. JSON-RPC tool hook adapter and conformance coverage
5. Provider payload boundary tests
6. Architecture documentation

## Task List

### Phase 1: Core API Foundation

## Task 1: Add Permission Types and Hook API

**Description:** 在核心类型中加入 permission requirement、approval decision 和 hook attribution 基础 API。此任务只建立类型边界，不改变 tool execution 行为。

**Acceptance criteria:**

- [x] `ToolSpec` 新增 `permissions: Vec<ToolPermissionRequirement>`，serde 默认空，现有 tool specs 不需要显式填写。
- [x] `ToolPermissionRequirement` 包含 `capability: String`、`description: Option<String>`、`metadata: Value`。
- [x] 新增 `ToolPermissionOutcome::{Allow, Deny}` 和 `ToolPermissionDecision { outcome, reason, approver, metadata }`。
- [x] `BeforeToolCallResult` 改为 `decision: ToolPermissionDecision`，hook 返回 `None` 表示 no opinion。
- [x] `ToolCallHook` 新增默认 `id() -> Option<&str>`，用于 audit attribution。
- [x] `BeforeToolCallContext` 增加 `tool_spec`，permission requirements 通过 `tool_spec.permissions` 单一来源读取。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test core tool_hooks`
- [x] `cargo fmt --check`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/providers.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/core.rs`

**Estimated scope:** M

## Task 2: Add Permission Audit Events

**Description:** 扩展 event model，让 permission request 和 decision 可 replay、可观察、可由 event sink 实时消费。此任务只定义事件和 reducer no-op 行为，暂不接入执行路径。

**Acceptance criteria:**

- [x] `AgentEventKind` 新增 `ToolPermissionRequested { tool_call, permissions }`。
- [x] `AgentEventKind` 新增 `ToolPermissionDecided { tool_call_id, tool_name, hook_id, decision }`。
- [x] `reduce_events` 对这两个事件保持 state no-op，不影响 `AgentState`。
- [x] serde round-trip 覆盖新事件和 decision 类型。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test core permission_event`
- [x] `cargo test -p noloong-agent-core --test core event_log_replays_to_report_state`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/reducer.rs`
- `crates/noloong-agent-core/tests/core.rs`

**Estimated scope:** S

### Checkpoint: API Foundation

- [x] Public types compile.
- [x] Existing native tool hook tests are migrated to decision API.
- [x] `cargo test -p noloong-agent-core --test core tool_hooks`

### Phase 2: Runtime Behavior

## Task 3: Wire Permission Decisions into Tool Execution

**Description:** 在 `tool.execute` phase 中接入 permission request/decision。每个 tool call 执行前记录 request，依序运行 before hooks；deny 时跳过 tool provider，生成 error tool output；allow/no opinion 继续走现有工具执行路径。

**Acceptance criteria:**

- [x] 每个 tool call 在 before hooks 前记录 `ToolPermissionRequested`。
- [x] 每个返回 decision 的 hook 都记录 `ToolPermissionDecided`，`hook_id` 来自 `ToolCallHook::id()`。
- [x] 任一 hook 返回 `Deny` 时不调用 `ToolProvider::execute_tool`。
- [x] Denied output 的 `is_error=true`，content 包含 deny reason，details 包含 decision metadata。
- [x] `Allow` 或 no opinion 保持现有 before/execute/after hook 顺序。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test core tool_permission`
- [x] `cargo test -p noloong-agent-core --test core tool_hooks_can_block_and_rewrite_results`

**Dependencies:** Tasks 1-2

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/tests/core.rs`

**Estimated scope:** M

## Task 4: Preserve Parallel, Sequential, and Error Semantics

**Description:** 扩展工具执行测试，证明 permission model 不破坏现有 parallel/sequential source-order commit、tool provider error、after hook rewrite 和 abort 行为。

**Acceptance criteria:**

- [x] Parallel 模式下 denied tool 和 executed tool 仍按 completion event 顺序记录，最终 tool result message 按 source order commit。
- [x] Sequential 模式下 deny 会阻止当前 tool provider 调用，但后续 source-order tool 仍按既有 sequential 语义运行。
- [x] 普通 tool provider error 仍转成 auditable error tool result。
- [x] `AgentCoreError::Aborted` 仍中止 run，不产生 denied result。
- [x] `reduce_events(report.events) == report.state` 对 allow/deny 场景成立。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test core`
- [x] `cargo test -p noloong-agent-core --test conformance`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/tests/core.rs`
- `crates/noloong-agent-core/tests/conformance.rs`

**Estimated scope:** M

### Checkpoint: Native Permission Model

- [x] Native permission model works without JSON-RPC.
- [x] `cargo test -p noloong-agent-core --test core`
- [x] `cargo test -p noloong-agent-core --test conformance`
- [x] `cargo fmt --check`

### Phase 3: JSON-RPC Extension Support

## Task 5: Add JSON-RPC Tool Call Hook Capability

**Description:** 让外部语言扩展能注册 tool call hook，并通过 JSON-RPC 实现 before approval 和 after rewrite。该 adapter 必须复用 native `ToolCallHook` 语义。

**Acceptance criteria:**

- [x] `ExtensionCapability` 新增 `ToolCallHook { id }`，wire type 为 `tool_call_hook`。
- [x] `AgentRuntimeBuilder::with_stdio_extension` 能把 `tool_call_hook` 注册为 runtime tool hook，并做 duplicate id 校验。
- [x] 新增 `StdioToolCallHook`，`id()` 返回 capability id。
- [x] 新增 JSON-RPC method `tool_hook/run`。
- [x] before payload 包含 `hookId`、`hookPoint`、`runId`、`turnId`、`state`、`toolCall`、`toolSpec`、`permissions`。
- [x] after payload 包含 `hookId`、`hookPoint`、`runId`、`turnId`、`state`、`toolCall`、`output`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc tool_hook`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance capabilities`

**Dependencies:** Tasks 1-4

**Files likely touched:**

- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/tests/jsonrpc.rs`

**Estimated scope:** M

## Task 6: Extend JSON-RPC Conformance Coverage

**Description:** 扩展 dedicated JSON-RPC conformance fixture，覆盖 tool hook payload shape、allow/deny decision、malformed hook response 和 audit event 行为。

**Acceptance criteria:**

- [x] Fixture `capabilities/list` 支持 `tool_call_hook`。
- [x] Fixture 能断言 before payload 中的 tool spec permissions 和 tool call arguments。
- [x] JSON-RPC deny decision 不 fail run，而是产生 error tool result。
- [x] Malformed `tool_hook/run` response 会让当前 `tool.execute` phase fail。
- [x] Conformance tests 覆盖 duplicate `tool_call_hook` id。

**Verification:**

- [x] `node --check crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance tool_hook`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance capabilities_duplicate_ids_fail_registration`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

**Estimated scope:** M

### Checkpoint: JSON-RPC Extension Support

- [x] Native and JSON-RPC tool hooks expose the same decision semantics.
- [x] `cargo test -p noloong-agent-core --test jsonrpc`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`

### Phase 4: Provider Boundaries and Documentation

## Task 7: Lock Provider Payload Boundaries

**Description:** 确认 permission metadata 在 provider 边界上的行为：core 和 JSON-RPC typed model request 可见完整 `ToolSpec`，但 built-in HTTP providers 不把 permission metadata 混入 hosted tool schema。

**Acceptance criteria:**

- [x] Chat Completions payload tests 证明只发送 tool name/description/input schema。
- [x] Anthropic Messages payload tests 证明只发送 tool name/description/input schema。
- [x] Responses payload tests 证明只发送 tool name/description/input schema。
- [x] JSON-RPC model request test 证明 typed `ToolSpec` 保留 `permissions`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test chat_completions`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`
- [x] `cargo test -p noloong-agent-core --test responses`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance adapter_payloads`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/responses.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**Estimated scope:** M

## Task 8: Document Tool Permission Model

**Description:** 更新架构文档，把 permission model 描述为 tool lifecycle 的审计边界，并说明 native hook、JSON-RPC hook、provider payload boundary 和后续 interactive approval queue 的关系。

**Acceptance criteria:**

- [x] `ARCHITECTURE.md` 说明 capability declaration、approval decision、audit events、deny behavior。
- [x] `Process Extension Bridge` 文档包含 `tool_call_hook` capability 和 `tool_hook/run` method。
- [x] `tool.execute` phase 文档说明 permission request/decision 记录顺序。
- [x] “后续演进方向” 把 tool permission model 替换为 interactive/human approval queue。

**Verification:**

- [x] `git diff --check`
- [x] 文档中的 public type names 与代码一致。

**Dependencies:** Tasks 1-6

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** S

## Task 9: Final Quality Gate

**Description:** 跑完整质量门，确保 API 重构、runtime 行为、JSON-RPC bridge 和 provider payload tests 在 workspace 层面一致。

**Acceptance criteria:**

- [x] Formatting passes.
- [x] Clippy passes without warnings.
- [x] Targeted native, JSON-RPC, provider tests pass.
- [x] Workspace nextest passes.

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo test -p noloong-agent-core --test core`
- [x] `cargo test -p noloong-agent-core --test conformance`
- [x] `cargo test -p noloong-agent-core --test jsonrpc`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [x] `cargo test -p noloong-agent-core --test chat_completions`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`
- [x] `cargo test -p noloong-agent-core --test responses`
- [x] `cargo nextest run --workspace`

**Dependencies:** Tasks 1-8

**Files likely touched:**

- No new feature files expected; this task fixes issues found by verification only.

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Permission denial accidentally becomes run failure | High | Encode deny as `ToolOutput { is_error: true }` and test state replay / run completed behavior. |
| Permission metadata leaks into built-in provider tool schemas | Medium | Add explicit payload tests for Chat Completions, Anthropic Messages, and Responses. |
| Parallel execution audit order becomes confusing | Medium | Record request/decision per tool call and preserve existing completion/source-order tests. |
| JSON-RPC hook contract diverges from native hook | Medium | Implement `StdioToolCallHook` directly as `ToolCallHook` and cover the same allow/deny/rewrite cases. |
| Decision API grows too broad too early | Medium | Keep v1 synchronous and policy-only; defer pending/human approval queue to future work. |

## Parallelization Opportunities

- Tasks 1-4 must be sequential because they change shared public API and runtime behavior.
- Task 7 provider payload tests can run in parallel with Task 6 after Task 1 lands.
- Task 8 documentation can start after Task 5 defines final JSON-RPC names, then be finalized after Task 6.
- Task 9 is sequential and should run only after all implementation tasks complete.

## Open Questions

- None for v1. Default decision is synchronous policy approval only; interactive human approval queue is a follow-up architecture item.
