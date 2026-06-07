# Remove GPUI dependencies and stale app implementation

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

在 Tauri/WebView 主路径可用后，彻底清理旧 GPUI 实现。删除 GPUI workspace dependencies、GPUI macro dev profile 优化、旧 GPUI view/runtime 代码、旧 GPUI assets 和旧自制 app bundle helper。保留与 UI 无关的真实修复，例如 ChatGPT Responses request timeout。清理后，workspace 不应再为 GPUI 编译付出成本。

## Acceptance criteria

- [x] workspace 不再声明 `gpui`、`gpui_platform`、`gpui-component` 依赖。
- [x] dev profile 不再包含 GPUI macro 相关优化项。
- [x] `noloong-app` 不再包含 GPUI view/runtime 代码。
- [x] 旧自制 macOS bundle helper 被删除或完全脱离构建。
- [x] 旧 GPUI assets 被删除，保留 Tauri 所需 icon assets。
- [x] ChatGPT Responses request timeout 修复仍保留并有测试。
- [x] `cargo check --workspace` 不再编译 GPUI 栈。
- [x] `cargo test` 相关回归通过。

## Blocked by

- [01-tauri-app-skeleton-replaces-gpui-launch-path.md](./01-tauri-app-skeleton-replaces-gpui-launch-path.md)
- [06-settings-entry-with-codemirror-jsonc-editing.md](./06-settings-entry-with-codemirror-jsonc-editing.md)
