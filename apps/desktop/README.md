# Noloong Desktop Frontend

这是 `noloong app` 的 Tauri/WebView 前端。开发和测试使用 Bun。

## 回归测试

```bash
bun --cwd apps/desktop test
bun --cwd apps/desktop typecheck
bun --cwd apps/desktop build
```

`src/App.test.tsx` 使用 `src/test/fakeInteractionRuntime.ts` 启动假的 interaction runtime，不依赖真实 ChatGPT、Telegram 或微信。测试覆盖：

- fake runtime bootstrap、initialize、session list/get 和 display stream；
- 发送后用户消息立即出现在 transcript；
- assistant display delta 分批显示，而不是完成后一次性出现；
- run completed 后通过权威 session snapshot 收敛；
- 接近底部时自动贴底；
- 用户主动上滚后不强制贴底。

Settings 的 JSONC invalid 保存保护由 `src/settings/store.test.ts` 覆盖。
