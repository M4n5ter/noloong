# Reasoning, run status, stop, tool and approval activity

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

让 Chat 画布展示运行活动，而不是只显示普通文本。前端需要消费展示事件中的 reasoning、run status、tool activity 和 approval request：思考展示优先显示 reasoning summary，存在 raw reasoning 时允许展开；思考结束后折叠为耗时摘要。Run 状态要覆盖 running、paused、failed、aborted、completed。停止按钮调用 interaction abort，只中止当前 run，不拒绝审批或删除会话。审批请求以 inline card 呈现，并能同意或拒绝。

## Acceptance criteria

- [x] reasoning summary 在思考展示中优先于 raw reasoning。
- [x] raw reasoning 存在且允许展示时可以展开查看。
- [x] 思考结束后折叠为耗时摘要。
- [x] running、paused、failed、aborted、completed 状态在 Chat 画布中可见。
- [x] 停止按钮调用当前 run 的 abort 语义。
- [x] 停止运行不拒绝审批、不删除会话。
- [x] Tool started/updated/completed 能以低干扰运行活动呈现。
- [x] Approval requested 能以内联卡片呈现，并支持同意/拒绝。
- [x] reducer/store 测试覆盖 reasoning、run status、stop、tool 和 approval。

## Blocked by

- [04-composer-prompt-flow-with-live-display-stream.md](./04-composer-prompt-flow-with-live-display-stream.md)
