# noloong-agent 架构说明

`noloong-agent` 是基于 `noloong-agent-core` 的产品层 runtime。它负责宿主机环境认知、后台命令工具、自进化 manifest、approval reviewer 和面向模型的 i18n 文案；`noloong-agent-core` 继续作为不可自变异的 providerless kernel，不引入 host、shell、SSH、VMM 或 process manager 概念。

## 分层边界

```text
noloong-agent
  owns AgentSession
  owns AgentManifest
  owns HostProcessManager
  owns product approval reviewer
  builds AgentRuntime per product turn

noloong-agent-core
  owns event-sourced kernel
  owns phase graph
  owns provider traits and tool approval events
```

产品层通过 core 已有扩展点接入：

- `ContextProvider`：注入当前宿主机环境说明。
- `ToolProvider`：暴露后台命令 lifecycle tools 和 manifest patch proposal tool。
- `ToolCallHook`：统一处理命令执行、stdin 写入、终止命令和 manifest patch 的 approval。

## Host-first Execution

v1 默认在宿主机执行。SSH、VMM、`clone`、Lima、QEMU 等不是统一 target abstraction，而是宿主机命令能力：Agent 可以通过 `host.exec.start` 启动这些命令，并在后续 turn 中读取或控制它们。

给模型的 host context 由 `HostEnvironment` 生成，包含：

- OS 和 CPU 架构。
- 当前目录。
- 默认 shell。
- 可用 shell hints。
- path style。
- locale。

这样可以避免模型在 PowerShell、`sh`、`bash`、`zsh` 等环境里混用错误命令。

## Background Command Lifecycle

后台命令不是单个阻塞式 `exec`，而是一组 lifecycle tools：

- `host.exec.start`
- `host.exec.read`
- `host.exec.wait`
- `host.exec.write`
- `host.exec.terminate`
- `host.exec.list`

`host.exec.start` 使用 optimistic foreground window。命令若在 `foregroundWaitMs` 内完成，tool result 直接返回 completed status；超过窗口则返回 running job handle，Agent 可以继续做其它事，并在后续 turn 中调用 `read`、`wait`、`write` 或 `terminate`。

输出由 product 层 spool/ring buffer 保存。core event log 只记录 tool result 中的摘要、cursor、cap 和 truncation metadata，不把大 stdout/stderr chunk 全量写入 event store。

## Background Completion Steering

后台命令进入终态后，`HostProcessManager` 会发布一次 `HostProcessEvent::JobCompleted`。事件在 child process 结束并且 stdout/stderr reader drain 之后生成，因此 completion preview 能包含最终输出尾部。

`AgentSession::attach_background_completion_steering(...)` 可以把这些 completion events 接到 core `Agent::steer(...)`。这条路径只排队 steering message，不做 auto continuation：它不会主动调用 `prompt`、`continue_run` 或模型 provider。Agent 空闲时，completion message 会等到下一次 run 开始，并在第一轮 model request 前注入；Agent 正在运行时，completion message 会在当前安全 turn 边界进入下一轮。

completion message 使用 `MessageRole::User`，不是 `MessageRole::ToolResult`。原因是后台完成事件是异步外部观察，不对应当前 provider transcript 中一个仍在等待结果的 assistant tool call；伪造 `ToolResult` 会破坏 chat completions、responses、anthropic messages 等 provider 的工具调用配对约束。

默认 completion preview 是 bounded tail output，上限 `16 KiB`。完整 stdout/stderr 历史仍由 `host.exec.read` 按 `jobId` 和 `afterSeq` cursor 拉取。

## Tool Output Overflow

`AgentSession::runtime_builder()` 默认注册 `ProductToolOutputOverflowHook`。该 hook 位于 product 层，通过 core 的 `ToolCallHook::after_tool_call` 检查完整 `ToolOutput` 的 serialized byte size。

默认 inline 上限是 `64 KiB`。未超限的 tool output 保持不变；超限时，hook 会把原始 `ToolOutput` 写入 `${TMPDIR}/noloong-agent/tool-output/{runId}-{turnId}-{toolCallId}.json`，并把 inline tool result 改写成短提示，包含文件路径、原始字节数、inline limit、tool name、tool call id，以及按模型可读 output content 生成的 head/tail preview。这样 core event log、模型上下文和后续 provider request 都只携带 bounded output，而完整结果仍可由 Agent 通过 host command tooling 读取。

如果写入临时 JSON 失败，hook 不会静默截断数据；它会返回 `is_error = true` 的 auditable tool output，并说明 overflow persistence failed。产品集成方可以通过 `AgentSessionBuilder::with_max_inline_tool_output_bytes(...)`、`with_tool_output_temp_dir(...)` 或 `with_tool_output_overflow_config(...)` 覆盖默认策略。

## Manifest Evolution

`AgentManifest` 描述 product session 的可变配置：

- locale。
- system prompt profile。
- enabled tools。
- approval policy。
- reserved phase profile。

Agent 不能直接修改 live manifest，只能通过 `agent.manifest.propose_patch` 提交 proposal。proposal 进入 approval path；审批通过后，由 `AgentSession` 在下一 product turn 前应用 patch 并重建 core runtime。

v1 真正支持的 patch 范围：

- replace system prompt。
- set locale。
- enable/disable tool。
- update approval policy。

phase profile patch 只保留为 reserved schema，不执行。

## Approval Reviewer

产品层 approval 通过 `ToolCallHook` 实现，复用 core 的 permission audit 和 pause/resume 事件路径。

当前 policy：

- `AllowAll`：直接允许。
- `RequireApproval`：进入 human approval。
- `AutoReview`：如果配置了 auto-review agent，则由它做 decision；没有 reviewer 时可按配置回退 human approval 或 deny。

所有 approval decision 都进入 core 的 `ToolPermissionDecided` audit。

## i18n

所有给模型看的 product-generated natural-language text 都走 typed catalog，包括 host context、tool description、permission description、approval prompt/reason、tool input/process/manifest error、background completion steering message，以及 oversized tool output rewrite message。v1 默认支持 English 和 Chinese。

locale 解析顺序：

1. 显式配置。
2. 宿主机 `LC_ALL`、`LC_MESSAGES`、`LANG`。
3. English fallback。

catalog key 必须完整；缺失 key 应在测试中失败，而不是运行时静默 fallback。
