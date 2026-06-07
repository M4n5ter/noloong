# Composer prompt flow with live display stream

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

实现 Chat 输入区和 prompt 提交流程。用户可以在输入区可靠聚焦、输入多行文本并发送；发送后用户消息立即出现在 transcript。前端订阅 display stream，按真实 delta 显示 assistant 流式回复，并在 run completed 后通过会话快照收敛到最终 transcript。Transcript 需要支持自动贴底，并在用户主动上滚时暂停强制贴底。

这一切片要用 fake interaction server 证明流式输出是逐步显示，而不是完成后一次性出现。

## Acceptance criteria

- [x] 输入区整块可聚焦，输入内容清晰可见。
- [x] 支持多行输入。
- [x] 空输入不能发送，有内容时发送按钮可用。
- [x] 发送后用户消息立即显示在 transcript。
- [x] 前端订阅 display stream，并分批显示 assistant delta。
- [x] 新内容到达时在接近底部状态下自动跟随到底部。
- [x] 用户主动上滚后不强制贴底。
- [x] run completed 后通过会话快照收敛到最终 transcript，不重复 assistant 消息。
- [x] fake interaction server 或前端测试覆盖分批 delta 和最终收敛。

## Blocked by

- [03-interaction-client-and-authoritative-session-snapshots.md](./03-interaction-client-and-authoritative-session-snapshots.md)
