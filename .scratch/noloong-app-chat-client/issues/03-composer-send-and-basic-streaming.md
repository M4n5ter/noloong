# 输入区发送与基础流式回复

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

实现 Chat 输入区的真实发送路径：多行输入、Enter 发送、Shift+Enter 换行；没有当前会话时首条消息自动创建 agent 会话并提交 prompt。订阅展示事件，把 assistant text delta 渲染为流式回复，并在 final message 到达时稳定替换对应 streaming bubble，避免重复 transcript。

## Acceptance criteria

- [ ] 输入区支持多行编辑、Enter 发送、Shift+Enter 换行。
- [ ] 空输入时发送按钮禁用。
- [ ] 没有当前会话时，首次发送会创建 agent 会话并提交用户消息。
- [ ] GUI 通过 display subscription 接收 `AssistantMessageDelta` 和 `AssistantMessageFinal`。
- [ ] assistant delta 能逐步更新当前 streaming bubble。
- [ ] final assistant message 到达后替换或稳定对应 streaming bubble，不生成重复消息。
- [ ] 流式尾迹使用真实 delta segment，并有 120-180ms opacity ramp。
- [ ] 有覆盖 composer keyboard、first send create session、delta merge、final replacement 和 streaming segment aging 的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/02-real-session-list-and-transcript-recovery.md
