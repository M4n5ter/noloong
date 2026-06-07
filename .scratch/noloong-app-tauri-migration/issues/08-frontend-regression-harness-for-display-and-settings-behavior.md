# Frontend regression harness for display and settings behavior

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

建立前端回归测试体系，用 fake interaction server 验证 Chat 和 Settings 的核心外部行为。测试重点是用户可见结果和协议契约：发送消息后用户消息立即出现、display delta 分批显示、tail follow 正确、run completed 后收敛、JSONC invalid 阻止保存。真实 ChatGPT、Telegram 和微信不进入自动化回归，只保留手动 smoke。

## Acceptance criteria

- [x] 前端测试能启动 fake interaction server。
- [x] 测试覆盖发送后用户消息立即显示。
- [x] 测试覆盖分批 delta 逐步显示，而不是一次性出现。
- [x] 测试覆盖接近底部时自动贴底。
- [x] 测试覆盖用户主动上滚后不强制贴底。
- [x] 测试覆盖 run completed 后会话快照收敛。
- [x] 测试覆盖 JSONC invalid 阻止保存。
- [x] 测试不依赖真实 ChatGPT、Telegram 或微信。
- [x] README 或开发文档说明如何运行前端回归。

## Blocked by

- [04-composer-prompt-flow-with-live-display-stream.md](./04-composer-prompt-flow-with-live-display-stream.md)
- [06-settings-entry-with-codemirror-jsonc-editing.md](./06-settings-entry-with-codemirror-jsonc-editing.md)
