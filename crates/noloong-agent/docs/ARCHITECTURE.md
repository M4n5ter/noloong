# noloong-agent 架构说明

`noloong-agent` 是基于 `noloong-agent-core` 的应用层 runtime。它负责宿主机环境认知、后台命令工具、自进化 manifest、approval reviewer 和面向模型的 i18n 文案；`noloong-agent-core` 继续作为不可自变异的 providerless kernel，不引入 host、shell、SSH、VMM 或 process manager 概念。

## 分层边界

```text
noloong-agent
  owns AgentSession
  owns AgentManifest
  owns HostProcessManager
  owns built-in approval reviewer
  builds AgentRuntime per application turn

noloong-agent-core
  owns event-sourced kernel
  owns phase graph
  owns provider traits and tool approval events
```

应用层通过 core 已有扩展点接入：

- `ContextProvider`：注入当前宿主机环境说明。内置 provider id 是 `noloong.builtin.host-context`。
- `ToolProvider`：暴露后台命令 lifecycle tools 和 manifest patch proposal tool。
- `ToolCallHook`：统一处理命令执行、stdin 写入、终止命令和 manifest patch 的 approval。内置 approval hook id 是 `noloong.builtin.approval`。

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

输出由应用层 spool/ring buffer 保存。core event log 只记录 tool result 中的摘要、cursor、cap 和 truncation metadata，不把大 stdout/stderr chunk 全量写入 event store。

## Background Completion Steering

后台命令进入终态后，`HostProcessManager` 会发布一次 `HostProcessEvent::JobCompleted`。事件在 child process 结束并且 stdout/stderr reader drain 之后生成，因此 completion preview 能包含最终输出尾部。

`AgentSession::attach_background_completion_steering(...)` 可以把这些 completion events 接到 core `Agent::steer(...)`。这条路径只排队 steering message，不做 auto continuation：它不会主动调用 `prompt`、`continue_run` 或模型 provider。Agent 空闲时，completion message 会等到下一次 run 开始，并在第一轮 model request 前注入；Agent 正在运行时，completion message 会在当前安全 turn 边界进入下一轮。

completion message 使用 `MessageRole::User`，不是 `MessageRole::ToolResult`。原因是后台完成事件是异步外部观察，不对应当前 provider transcript 中一个仍在等待结果的 assistant tool call；伪造 `ToolResult` 会破坏 chat completions、responses、anthropic messages 等 provider 的工具调用配对约束。

默认 completion preview 是 bounded tail output，上限 `16 KiB`。完整 stdout/stderr 历史仍由 `host.exec.read` 按 `jobId` 和 `afterSeq` cursor 拉取。

## Tool Output Overflow

`AgentSession::runtime_builder()` 默认注册 `BuiltInToolOutputOverflowHook`。该 hook 位于应用层，通过 core 的 `ToolCallHook::after_tool_call` 检查完整 `ToolOutput` 的 serialized byte size，hook id 是 `noloong.builtin.tool-output-overflow`。

默认 inline 上限是 `64 KiB`。未超限的 tool output 保持不变；超限时，hook 会把原始 `ToolOutput` 写入 `${TMPDIR}/noloong-agent/tool-output/{runId}-{turnId}-{toolCallId}.json`，并把 inline tool result 改写成短提示，包含文件路径、原始字节数、inline limit、tool name、tool call id，以及按模型可读 output content 生成的 head/tail preview。这样 core event log、模型上下文和后续 provider request 都只携带 bounded output，而完整结果仍可由 Agent 通过 host command tooling 读取。

如果写入临时 JSON 失败，hook 不会静默截断数据；它会返回 `is_error = true` 的 auditable tool output，并说明 overflow persistence failed。应用集成方可以通过 `AgentSessionBuilder::with_max_inline_tool_output_bytes(...)`、`with_tool_output_temp_dir(...)` 或 `with_tool_output_overflow_config(...)` 覆盖默认策略。

## Manifest Evolution

`AgentManifest` 描述 application session 的可变配置：

- locale。
- system prompt profile。
- enabled tools。
- approval policy。
- reserved phase profile。

Agent 不能直接修改 live manifest，只能通过 `agent.manifest.propose_patch` 提交 proposal。proposal 进入 approval path；审批通过后，由 `AgentSession` 在下一 application turn 前应用 patch 并重建 core runtime。

v1 真正支持的 patch 范围：

- replace system prompt。
- set locale。
- enable/disable tool。
- update approval policy。

phase profile patch 只保留为 reserved schema，不执行。

## Approval Reviewer

应用层 approval 通过 `ToolCallHook` 实现，复用 core 的 permission audit 和 pause/resume 事件路径。

当前 policy：

- `AllowAll`：直接允许。
- `RequireApproval`：smart-gated approval。内置 hook 先分类工具调用；已知安全的内置只读操作直接允许，需要人工判断的操作才进入 human approval，明确不可接受的操作可直接 deny。
- `AutoReview`：复用同一套分类。安全调用直接允许；只有分类为 `NeedsApproval` 时才调用 auto-review agent。没有 reviewer 时可按配置回退 human approval 或 deny。

评估顺序：

1. `AllowAll` 直接 bypass 内置检查。
2. session approval cache 命中时直接 allow。cache 只记录当前 `AgentSession` 内由 `noloong.builtin.approval` 产生、带有内置 cache key，且 application 显式记录为 allow 的审批结果。
3. 内置工具类别分类：`host.exec.read`、`host.exec.wait`、`host.exec.list` 直接 allow；`host.exec.write`、`host.exec.terminate`、`agent.manifest.propose_patch` 进入 approval；未知工具名进入 approval。
4. `host.exec.start` 走命令安全分类器。已知只读命令允许；unsupported shell syntax、env assignment、redirection、command substitution、glob-heavy syntax 和未知命令都进入 approval；危险命令同样需要 approval。
5. 对 `NeedsApproval` 结果，`RequireApproval` 产生 core pause/resume approval request；`AutoReview` 调用 reviewer 或按 fallback 策略处理。

`AgentSession::record_tool_approval_resolution` 是 application 层接入 cache 的显式 API。调用方在用 core 的 `ToolApprovalResolution` resume 之前或之后，都可以把对应 `ToolApprovalRequest` 和 allow decision 传给 session；denial、外部 hook、缺少 built-in cache metadata 的 request 不会被记录。

v1 没有完整 sandbox 边界，也没有持久化 execpolicy 文件；因此 unknown host command 默认需要 approval。后续可以在不改变 core approval 语义的前提下，加入持久化规则、host sandbox/VMM policy 或更细粒度的 capability policy。

所有 approval decision 都进入 core 的 `ToolPermissionDecided` audit。进入 human approval 的请求也会保留 classification metadata 和 cache key，便于 application 层审计和记录 session cache。

## i18n

所有给模型看的 application-generated natural-language text 都走 typed catalog，包括 host context、tool description、permission description、approval prompt/reason、tool input/process/manifest error、background completion steering message，以及 oversized tool output rewrite message。v1 默认支持 English 和 Chinese。

locale 解析顺序：

1. 显式配置。
2. 宿主机 `LC_ALL`、`LC_MESSAGES`、`LANG`。
3. English fallback。

catalog key 必须完整；缺失 key 应在测试中失败，而不是运行时静默 fallback。
