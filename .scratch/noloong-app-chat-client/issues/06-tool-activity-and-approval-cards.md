# 工具活动与审批卡片

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

把工具执行和审批请求作为 transcript 中的运行活动呈现。工具 activity row 默认低干扰、可折叠，能展示 started、updated、completed、error 和长输出。审批请求以内联卡片显示上下文，并提供同意和拒绝操作。审批语义必须复用 interaction 协议，不能和停止运行混淆。

## Acceptance criteria

- [ ] `ToolStarted`、`ToolUpdated`、`ToolCompleted` 会聚合为同一工具 activity row。
- [ ] 工具 activity 默认折叠，并可展开查看输出和错误。
- [ ] 长工具输出不会撑爆 Chat 画布，使用折叠、虚拟滚动或文件链接。
- [ ] `ApprovalRequested` 呈现为 transcript 内联审批卡片。
- [ ] 审批卡片可以同意或拒绝，并调用 interaction 协议的 approval resolve。
- [ ] 审批态显示 run paused，不阻塞用户理解当前状态。
- [ ] 停止运行和拒绝审批在 UI 和状态机中保持独立。
- [ ] 有覆盖工具活动聚合、长输出折叠、审批同意/拒绝和 paused 状态的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/05-run-lifecycle-stop-and-error-states.md
