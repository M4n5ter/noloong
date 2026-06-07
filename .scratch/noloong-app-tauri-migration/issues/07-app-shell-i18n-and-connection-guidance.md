# App shell i18n and connection guidance

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

为 Tauri/WebView app 建立统一的 app shell 文案、语言选择和连接状态引导。UI 支持 zh/en，但运行时只显示当前语言。Chat 和 Settings 共享同一套 i18n catalog。缺少配置、interaction unavailable、外部 runtime 初始化失败、连接中断等状态要在 Chat 画布中以低干扰方式表达，并引导用户进入配置入口或检查 runtime。

## Acceptance criteria

- [x] zh/en 两种 UI 语言可选择或自动检测。
- [x] 同一时刻只显示一种语言。
- [x] Chat 画布主要文案走 i18n catalog。
- [x] Settings 主要文案走 i18n catalog。
- [x] 缺少配置时 Chat 中显示清晰引导。
- [x] interaction unavailable 时显示清晰状态。
- [x] external runtime 初始化失败时显示失败原因。
- [x] 连接中断或 WebSocket reconnect 状态有低干扰提示。
- [x] i18n 测试覆盖关键文案 key。

## Blocked by

- [03-interaction-client-and-authoritative-session-snapshots.md](./03-interaction-client-and-authoritative-session-snapshots.md)
- [06-settings-entry-with-codemirror-jsonc-editing.md](./06-settings-entry-with-codemirror-jsonc-editing.md)
