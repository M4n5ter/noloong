# Tauri macOS packaging and manual app smoke

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

完成 Tauri macOS packaging 和人工 smoke 验收。Tauri bundler 应生成正式 `.app`，使用 `com.noloong.desktop` bundle id、Noloong product name 和正确 icon。人工验证 Noloong app 能被 macOS、Dock 和 Computer Use 正常识别，并用真实 ChatGPT profile 跑一次主交互客户端 smoke：打开 app、创建或选择会话、发送文本、观察流式输出、等待完成并确认 transcript 收敛。

这一切片标记为人工参与，因为它需要系统集成、视觉和真实 provider 行为确认。

## Acceptance criteria

- [x] Tauri bundler 能生成正式 macOS `.app`。
- [x] `.app` bundle id 为 `com.noloong.desktop`。
- [x] `.app` 显示 Noloong product name 和正确 icon。
- [x] Computer Use 能识别并截图 Noloong app。
- [x] `noloong app` 和 Tauri dev/bundle 启动路径都有清晰文档。
- [x] 使用真实 ChatGPT profile 能创建或继续 agent 会话。
- [x] 真实 smoke 中用户消息立即显示。
- [x] 真实 smoke 中 assistant 回复可见流式输出。
- [x] 真实 smoke 完成后 transcript 可靠收敛。
- [x] smoke 结果记录到本主题审计或评论中。

## Blocked by

- [08-frontend-regression-harness-for-display-and-settings-behavior.md](./08-frontend-regression-harness-for-display-and-settings-behavior.md)
- [09-remove-gpui-dependencies-and-stale-app-implementation.md](./09-remove-gpui-dependencies-and-stale-app-implementation.md)

## Implementation audit

- `package.json` 新增 `app:dev` 和 `app:bundle`，统一从 workspace root 启动 Tauri dev/build；`README.md` 已记录 `bun run app:bundle`、`bun run app:dev` 和 `cargo run -p noloong -- app` 的使用边界。
- `crates/noloong-app` 新增 Tauri binary entrypoint，支持通过 `NOLOONG_APP_LAUNCH_OPTIONS_JSON` 接收 root CLI 注入的 launch options。
- `noloong app` 现在启动 packaged `.app` executable，而不是直接在 root CLI 进程内创建 WebView；这样 macOS、Dock 和 Computer Use 都能识别真实 bundle。
- 发现并修复真实 smoke 暴露的 transcript 收敛竞态：terminal Display event 可能早于最终 session snapshot 可见，前端现在会做有限 settle refresh，并且 `stream.prompt()` 返回后重新读取权威 `session/get`。

## Verification

- `bun run app:bundle` 成功生成 `/Users/m4n5ter/rust/noloong/target/release/bundle/macos/Noloong.app`。
- `Info.plist` 验证：
  - `CFBundleIdentifier = com.noloong.desktop`
  - `CFBundleName = Noloong`
  - `CFBundleDisplayName = Noloong`
  - `CFBundleExecutable = noloong-app`
  - `CFBundleIconFile = noloong-logo.icns`
- Computer Use 验证 `com.noloong.desktop` 可识别，路径为 `target/release/bundle/macos/Noloong.app`，窗口标题为 `Noloong`。
- 真实 ChatGPT profile smoke 使用 `examples/profile-configs/chatgpt-codex-subscription.json`：
  - `noloong tauri live smoke. reply exactly: noloong tauri smoke ok` 返回 `noloong tauri smoke ok`。
  - 发送长 numbered-sentence prompt 后，1 秒内可见 assistant partial，例如输出停在 `2. Updat` 或 `5. P` 的中间态。
  - 完成后通过 UI 和 `session/get` 对照确认 transcript 收敛到完整最终文本；最后一次 16 条 smoke 的权威末尾为 `16. Correct settling prevents confusing re-renders.`。
- 自动检查：
  - `cargo fmt --all --check`
  - `bun --cwd apps/desktop test`
  - `bun --cwd apps/desktop build`
  - `bun run app:bundle`
  - `cargo test -p noloong-app`
  - `cargo test -p noloong cli_app`
  - `cargo clippy -p noloong-app -p noloong --all-targets -- -D warnings`
  - `cargo test -p noloong`

## Notes

- Bundle identifier 已改为 `com.noloong.desktop`，避免把 macOS bundle 目录后缀 `.app` 混入应用身份。
- Vite 仍提示主 chunk 超过 500 kB；这是 bundle size 警告，不影响本轮 packaging 或 smoke 验收。
