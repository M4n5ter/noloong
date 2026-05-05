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

所有给模型看的 host context、tool description 和 approval prompt 都走 typed catalog。v1 默认支持 English 和 Chinese。

locale 解析顺序：

1. 显式配置。
2. 宿主机 `LC_ALL`、`LC_MESSAGES`、`LANG`。
3. English fallback。

catalog key 必须完整；缺失 key 应在测试中失败，而不是运行时静默 fallback。
