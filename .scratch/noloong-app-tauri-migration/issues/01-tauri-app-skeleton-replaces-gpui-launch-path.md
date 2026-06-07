# Tauri app skeleton replaces GPUI launch path

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

让 `noloong app` 启动 Tauri/WebView 版本的主交互客户端，而不是 GPUI 窗口。完成后，用户可以通过现有 CLI 入口打开一个最小但真实的 Noloong 桌面 app：它使用 `com.noloong.desktop` 身份、当前 Noloong icon、自绘 title bar、Bun/Vite/React 前端，并且不再依赖旧的 GPUI app fallback 或自制 macOS bundle helper。

这一切片的目标不是实现完整 Chat，而是打通新的 app shell 和启动链路，使后续 Chat、Settings 和 interaction 功能可以落在 Tauri/WebView 之上。

## Acceptance criteria

- [x] `noloong app` 启动 Tauri/WebView 窗口，而不是 GPUI 窗口。
- [x] app product name 为 `Noloong`，bundle identifier 为 `com.noloong.desktop`。
- [x] 窗口使用 Noloong icon，并显示最小自绘 title bar。
- [x] 根目录建立 Bun workspace，`apps/desktop` 建立 Vite + React + TypeScript 前端。
- [x] `crates/noloong-app` 作为 Tauri Rust host 保留，`lib.rs` 仍保持简洁。
- [x] 旧自制 macOS bundle relaunch helper 不再参与启动路径。
- [x] `cargo check -p noloong-app` 和前端 build/check 命令通过。

## Blocked by

None - can start immediately
