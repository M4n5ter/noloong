# 思考展示与 reasoning 展示事件

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

扩展展示事件，让 runtime 的 thinking stream 可以作为思考展示被 Chat 消费。GUI 优先展示 reasoning summary；只有存在且允许展示的 reasoning 原文才可展开。思考结束后折叠为类似 “Thought for 2 seconds” 的耗时摘要，不让 GUI 订阅 raw event 或解析 provider 私有事件。

## Acceptance criteria

- [ ] 展示事件包含 reasoning/thought 语义，覆盖开始、delta 或 summary、完成和耗时信息。
- [ ] interaction projector 能把现有 thinking stream 投影为展示事件。
- [ ] 有 reasoning summary 时 GUI 始终优先展示 summary。
- [ ] 有可展示 reasoning 原文时，GUI 可以按需展开查看。
- [ ] 思考结束后自动折叠为耗时摘要。
- [ ] GUI 不订阅 raw event 来实现思考展示。
- [ ] 有覆盖 DisplayEvent serde、projector、summary 优先、raw 展开和完成折叠的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/03-composer-send-and-basic-streaming.md
