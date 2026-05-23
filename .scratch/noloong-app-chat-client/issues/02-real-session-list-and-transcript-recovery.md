# 真实会话列表与 transcript 恢复

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

让 Chat 画布展示来自 interaction runtime 的真实会话列表，并能选择当前会话。选中会话后，从 agent 会话状态恢复稳定 transcript，不使用 GUI 私有 transcript 缓存作为事实来源。切换当前会话不能影响其它正在运行的 agent 会话。

## Acceptance criteria

- [ ] 会话列表来自 interaction 协议的 session 数据，而不是 app-only state。
- [ ] 可以创建、选择和刷新当前会话。
- [ ] 当前会话 transcript 从 `AgentState.messages` 恢复用户消息和 assistant final 消息。
- [ ] 切换当前会话不会暂停或取消其它正在运行的会话。
- [ ] 会话列表能显示 running、paused、failed、completed 等真实状态。
- [ ] 有覆盖无会话、单会话、多会话切换、运行中切换和 transcript 恢复的状态机测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/01-chat-default-entry-and-embedded-interaction.md
