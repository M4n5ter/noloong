# 会话 metadata：标题、工作目录、profile/model 上下文

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

把会话标题、会话工作目录、profile/model 上下文作为真实会话信息呈现在 Chat 标题栏和会话列表中。默认会话标题由第一条用户消息本地生成，并持久化到 agent 会话 metadata；工作目录是真实运行上下文，切换目录只影响新会话或新 run，不改变已经运行中的 run。

## Acceptance criteria

- [ ] 新会话默认标题从第一条用户消息生成短标题。
- [ ] 用户可以手动重命名会话标题。
- [ ] 会话标题持久化到 agent 会话 metadata，重新打开 app 后可恢复。
- [ ] Chat 标题栏显示当前会话标题、profile/model 和会话工作目录。
- [ ] 新会话默认工作目录来自 app cwd 或 profile 默认 cwd。
- [ ] 用户可以为新会话或后续 run 选择工作目录。
- [ ] 工作目录切换不影响已经运行中的 run。
- [ ] 有覆盖 title generation、metadata persistence、workdir selection 和 profile/model display 的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/02-real-session-list-and-transcript-recovery.md
- .scratch/noloong-app-chat-client/issues/03-composer-send-and-basic-streaming.md
