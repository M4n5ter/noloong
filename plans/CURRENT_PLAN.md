# Implementation Plan: Event-Sourced Human Tool Approval

## Overview

已完成 core 级 event-sourced human tool approval。hook 仍负责判断是否需要人工审批以及审批渠道策略，core 负责 pending approval、pause、resume、timeout、abort 和审计状态机。v1 的 crash recovery 范围限定在 `tool.execute`：一旦同批 tool calls 中任意 call 需要审批，整个 `tool.execute` phase 暂停，后续使用同一个 `runId` 从 event store 恢复。

## Architecture Decisions

- [x] 审批 UI、Slack、Webhook、企业审批系统等集成属于使用者或 extension hook；core 不内置具体人工交互产品。
- [x] Core 新增可审计 approval 状态机：pending、resolved、expired，并通过事件日志恢复。
- [x] `before_tool_call` hook 结果扩展为 no-op、`ToolPermissionDecision`、`ToolApprovalRequestSpec`。
- [x] v1 crash recovery 只支持 `tool.execute` approval continuation，不做任意 phase checkpointing。
- [x] 发现任意 pending approval 时暂停整个 tool batch，不先执行同 batch 里不需要审批的 provider。
- [x] Resume 使用同一个 `runId` 追加事件，而不是创建新的 run。
- [x] Timeout 使用 absolute `expiresAtMs`，重启后 resume 时基于事件日志和当前 clock 判断。

## Completed Tasks

### Phase 1: Approval State Foundation

- [x] Added `ToolApprovalId`, `ToolApprovalRequestSpec`, `ToolApprovalRequest`, `ToolApprovalResolution`, `ToolApprovalContinuation`, `ToolApprovalPreflight`, pause/resume reasons.
- [x] Added `AgentEventKind::{ToolApprovalRequested, ToolApprovalResolved, ToolApprovalExpired, RunPaused, RunResumed}`.
- [x] Added `RunStatus::Paused` and `AgentState.pending_tool_approvals`.
- [x] Updated reducer replay so pending approvals survive restart and resolved/expired/completed/aborted/failed states clear correctly.

### Phase 2: Hook Contract and Pause

- [x] Extended native `BeforeToolCallResult` with `decision` or `approval`.
- [x] Extended stdio JSON-RPC `tool_hook/run` to parse `{ "approval": ... }`.
- [x] `decision` and `approval` together fail fast as malformed hook output.
- [x] Refactored `tool.execute` to preflight all tool calls before provider execution.
- [x] Pending approval records `ToolApprovalRequested` and `RunPaused` without calling any provider in the batch.
- [x] Existing allow/deny/no-op behavior remains compatible and audited.

### Phase 3: Resume, Timeout, and Agent API

- [x] Added `AgentRuntime::resume_tool_approvals` and queue-aware variant.
- [x] Resume loads the original event log, bumps event sequence after crash/restart, writes approval resolution events, writes `RunResumed`, and continues from stored continuation.
- [x] Allow decisions execute provider and continue later phases.
- [x] Deny decisions and expired approvals generate auditable error tool results without provider execution.
- [x] Added `Agent::pending_tool_approvals`, `Agent::resume_tool_approval`, `Agent::resume_tool_approvals`, and timeout resume helper.
- [x] Added paused-run abort support through `AgentRuntime::abort_paused_run` and `Agent::abort`.

### Phase 4: Bridge, Docs, and Conformance

- [x] Updated `docs/EXTENSIONS.md` with approval input/output shapes and state fields.
- [x] Updated `docs/ARCHITECTURE.md` with event-sourced approval semantics and moved approval UI queues to application/adapter concerns.
- [x] Updated JSON-RPC fixtures and strict conformance runner with a `tool_approval` case.
- [x] Updated TypeScript and Python conformance examples to exercise approval.
- [x] Updated docs contract tests to require approval shapes.

## Verification

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo nextest run --workspace`
      Result: 230 passed, 12 skipped.
- [x] `cargo test -p noloong-agent-core --test core`
- [x] `cargo test -p noloong-agent-core --test agent`
- [x] `cargo test -p noloong-agent-core --test jsonrpc`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance public_runner_strict_fixture_passes`
- [x] `cargo test -p noloong-agent-core --test extension_language_examples`
- [x] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [x] `npm run check` in `examples/extensions/typescript-conformance`
- [x] `npm run conformance` in `examples/extensions/typescript-conformance`
- [x] `python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- python3 examples/extensions/python-conformance/full_conformance_extension.py`
- [x] `git diff --check`

## Key Acceptance Criteria

- [x] Run can pause on pending human approval.
- [x] Pending approval is visible from replayed `AgentState`.
- [x] A different runtime instance can resume the paused run from the same event store.
- [x] Allow, deny, timeout, and abort all produce auditable events.
- [x] Existing synchronous tool permission hooks remain intact.
- [x] JSON-RPC extensions can request approval from TS/Python or any other language implementing the wire contract.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| 崩溃恢复扩大成任意 phase checkpoint | High | v1 只支持 `tool.execute` approval continuation |
| 并行工具部分执行后 pause 导致恢复不一致 | High | 发现任意 pending approval 时暂停整个 batch，不先执行 provider |
| Approval state 和 permission audit 双轨漂移 | High | Approval resolution 最终 replay 成现有 `ToolPermissionDecided` 和 tool output 语义 |
| Timeout 依赖内存 timer 无法重启恢复 | Medium | 事件中保存 deadline，resume 时基于 clock 判断 |
| Agent paused/idle 语义混乱 | Medium | Paused 不是 active run；resume/abort 都追加 auditable events |
| JSON-RPC 扩展作者误以为 core 提供 UI | Low | 文档明确 hook/extension 负责交互渠道，core 只负责状态机 |

## Open Questions

- None. Implementation complete for v1 scope.
