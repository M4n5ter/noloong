# Implementation Plan: Background Completion Injection

## Overview

让 `noloong-agent` 的后台命令 job 在完成后主动排队注入结果，但不自动启动 continuation。完成事件进入现有 steering 语义：如果 Agent 正在运行，则在当前安全 turn 边界注入；如果 Agent 空闲，则在下一次 `prompt` / `continue_run` 开始时、第一轮模型请求前注入。

同时增加 product-level 通用工具输出溢出处理：任意工具返回结果过长时，通过 `after_tool_call` hook 将完整 `ToolOutput` 写入临时 JSON 文件，并把 inline tool result 改写为短提示，告诉 Agent 完整结果路径和读取方式。

## Architecture Decisions

- 不自动 continuation：后台任务完成只调用 `agent.steer(...)` 排队，不主动调用 `prompt`、`continue_run` 或模型 provider。
- completion message 使用 `MessageRole::User`，不是 `ToolResult`。原因是后台 completion 是异步外部观察，不再对应当前 provider transcript 中合法成对的 `ToolCall`。
- idle queued steering 必须在下一次 run 的第一轮模型请求前注入，并且排在新的用户 prompt 之前，确保真实用户输入仍是最新消息。
- completion preview 默认只注入 bounded tail output，默认上限 `16 KiB`；完整历史仍通过 `host.exec.read` 按 cursor 读取。
- 通用工具输出外置放在 `noloong-agent`，不放进 `noloong-agent-core`，避免 core 默认绑定宿主机文件系统策略。
- tool output inline 默认上限 `64 KiB`；超限时完整输出写入 `${TMPDIR}/noloong-agent/tool-output/{runId}-{turnId}-{toolCallId}.json`。

## Task List

### Phase 1: Core Steering Semantics

#### Task 1: Drain queued steering before the first turn

**Description:** 调整 `noloong-agent-core` run loop，使 run 开始前已存在的 steering messages 在第一轮 `model_request` 前进入 state。该逻辑只改变 queued steering 的注入时机，不改变 follow-up 语义，也不触发自动 run。

**Acceptance criteria:**

- [ ] `agent.steer(message)` 在 `agent.prompt(...)` 前调用时，模型第一轮请求能看到该 steering message。
- [ ] 预先排队的 steering message 顺序在新的 user prompt 之前。
- [ ] active run 中途排队的 steering 仍在当前 turn 完成后的安全边界注入。

**Verification:**

- [ ] Add core test: idle steering is injected before first model request.
- [ ] Existing tests still pass: `steering_is_injected_after_tool_batch` and `steering_waits_until_tool_batch_completes`.
- [ ] `cargo test -p noloong-agent-core agent`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/runtime/run_loop.rs`
- `crates/noloong-agent-core/tests/agent.rs`

**Estimated scope:** Medium

### Checkpoint: Core Queue Behavior

- [ ] `cargo test -p noloong-agent-core agent`
- [ ] `cargo test -p noloong-agent-core conformance`
- [ ] `cargo clippy -p noloong-agent-core --all-targets --all-features -- -D warnings`

### Phase 2: Process Completion Events

#### Task 2: Add terminal process event subscription

**Description:** 为 `HostProcessManager` 增加轻量 subscription API，在 job 进入终态后发布 `HostProcessEvent::JobCompleted`。事件必须只发布一次，并在 stdout/stderr reader drain 后生成，保证 completion preview 能包含最终输出。

**Acceptance criteria:**

- [ ] `Exited`、`Terminated`、`Failed` 都会发布一次 `JobCompleted`。
- [ ] 同一 job 不会重复发布 completion event。
- [ ] completion snapshot 包含 job id、command、shell、cwd、status、started/ended time、cursor、dropped cursor 和 bounded tail chunks。

**Verification:**

- [ ] `cargo test -p noloong-agent host_process_completion_event_exited`
- [ ] `cargo test -p noloong-agent host_process_completion_event_terminated`
- [ ] `cargo test -p noloong-agent host_process_completion_event_is_single_delivery`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/process/manager.rs`
- `crates/noloong-agent/tests/host_process_manager.rs`

**Estimated scope:** Medium

#### Task 3: Render completion events as steering messages

**Description:** 在 product layer 中把 `HostProcessEvent::JobCompleted` 渲染为 model-readable `AgentMessage`，并提供 `AgentSession::attach_background_completion_steering(...)` 将 process manager completion events 连接到 core `Agent::steer(...)`。

**Acceptance criteria:**

- [ ] 空闲时 job 完成只排队 steering，不自动启动模型。
- [ ] 下一次 `prompt` 或 `continue_run` 的第一轮模型请求能看到 completion message。
- [ ] active run 中 job 完成时，completion message 在安全 turn 边界注入。
- [ ] message id 使用 `host-exec-completed-{jobId}`，metadata 包含 `noloong.kind = "host.exec.completed"` 和 `jobId`。
- [ ] message content 是 bounded text，不使用 `ToolResult` role，不伪造 `tool_call_id`。

**Verification:**

- [ ] Product integration test: idle completion is visible in the next prompt's first model request.
- [ ] Product integration test: idle completion does not auto-run the model.
- [ ] Product integration test: active-run completion uses steering boundary behavior.
- [ ] Product integration test: completion preview respects the `16 KiB` default limit.

**Dependencies:** Task 1, Task 2

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/process/manager.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Medium

### Checkpoint: Background Completion Flow

- [ ] `cargo test -p noloong-agent host_process_completion`
- [ ] `cargo test -p noloong-agent agent_session`
- [ ] Manual smoke: start `sleep 0.1; printf done`, wait until idle, then send next prompt and confirm model receives completion context.

### Phase 3: Generic Tool Output Overflow

#### Task 4: Add product tool output overflow hook

**Description:** 新增 `ProductToolOutputOverflowHook`，实现 `ToolCallHook::after_tool_call`。hook 检查完整 `ToolOutput` 的 serialized byte size；超过默认 `64 KiB` 时，将原始 output 写入临时 JSON 文件，并把 inline output 改写为短提示和 metadata。

**Acceptance criteria:**

- [ ] 未超限 output 不被修改。
- [ ] 超限 output 写入 `${TMPDIR}/noloong-agent/tool-output/{runId}-{turnId}-{toolCallId}.json`。
- [ ] 改写后的 inline output 包含 path、original byte size、inline byte limit、tool name、tool call id。
- [ ] 写文件失败时不静默丢数据；返回 `is_error = true` 的 auditable output，说明 overflow persistence failed。

**Verification:**

- [ ] Unit test: small output passes through unchanged.
- [ ] Unit test: large output is persisted and inline output is bounded.
- [ ] Unit test: persisted JSON can deserialize back to original `ToolOutput`.

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/tools/output_overflow.rs`
- `crates/noloong-agent/src/tools/mod.rs`
- `crates/noloong-agent/tests/tool_output_overflow.rs`

**Estimated scope:** Medium

#### Task 5: Register overflow hook in product runtime

**Description:** 让 `AgentSession::runtime_builder()` 默认注册 `ProductToolOutputOverflowHook`，并通过 `AgentSessionBuilder` 暴露可配置 limit 和 temp root。默认配置应能直接工作，测试可注入临时目录。

**Acceptance criteria:**

- [ ] 默认 product runtime 自动应用 overflow hook。
- [ ] `AgentSessionBuilder` 支持覆盖 `max_inline_tool_output_bytes` 和 `tool_output_temp_dir`。
- [ ] rewritten output 明确告诉 Agent：完整工具结果因太长已外置，需要用路径读取完整 JSON。

**Verification:**

- [ ] Integration test: runtime tool result over limit becomes short path prompt.
- [ ] Integration test: custom temp dir is respected.
- [ ] `cargo test -p noloong-agent tool_output_overflow`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/tools/output_overflow.rs`
- `crates/noloong-agent/tests/tool_output_overflow.rs`

**Estimated scope:** Small

### Checkpoint: Tool Output Safety

- [ ] `cargo test -p noloong-agent tool_output_overflow`
- [ ] Manual smoke: dummy tool returns large JSON, Agent sees path prompt, temp file contains full output.
- [ ] Confirm core event log stores only bounded rewritten output for oversized tool results.

### Phase 4: Documentation and Full Verification

#### Task 6: Update architecture documentation

**Description:** 更新 product architecture docs，明确后台 completion injection 语义、非自动 continuation 策略、completion preview limit、tool output overflow policy 和 `MessageRole::User` 的原因。

**Acceptance criteria:**

- [ ] Docs state that background completion queues steering and never starts a run by itself.
- [ ] Docs state queued completion is injected before the next run's first model request.
- [ ] Docs state default limits: completion preview `16 KiB`, tool output inline `64 KiB`.
- [ ] Docs explain why completion messages are user-role observations instead of tool results.

**Verification:**

- [ ] Review `crates/noloong-agent/docs/ARCHITECTURE.md`.
- [ ] `rg -n "auto continuation|MessageRole::ToolResult" crates/noloong-agent/docs/ARCHITECTURE.md` confirms the intended semantics are documented.

**Dependencies:** Tasks 1-5

**Files likely touched:**

- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** Small

### Final Checkpoint

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent`
- [ ] `cargo test -p noloong-agent --examples`
- [ ] `cargo test -p noloong-agent-core agent`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo nextest run --workspace --all-features -j 1`
- [ ] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Completion event fires before stdout/stderr drain | Medium | Emit `JobCompleted` only after child exit and output readers reach EOF or terminal error. |
| Queued steering changes first-turn ordering | Medium | Add core tests that lock ordering: queued completion before new user prompt, active steering unchanged. |
| Large tool output still enters event log | High | Register overflow hook before product tools are used; integration test against event/state size and persisted file. |
| Temp file path leaks sensitive data location | Medium | Store under a predictable product temp root and include only required path metadata in model-visible output. |
| `ToolResult` temptation breaks provider contracts | High | Use `MessageRole::User` for async observations and document the rationale. |

## Open Questions

None. Defaults are: no auto continuation, next-run queued steering injection, product-level overflow hook, `16 KiB` completion preview, `64 KiB` inline tool output limit.
