# Bootstrap and generated TypeScript contracts

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

为 Tauri/WebView app 建立 bootstrap 和类型契约。Rust 侧提供 Tauri command 返回 app 启动所需状态，包括 profile config 路径、locale、interaction endpoint、初始 interaction 状态和版本信息。Interaction protocol DTO 与 profile config wire 类型通过 Rust 类型自动生成 TypeScript 文件，前端使用生成类型而不是手写接口。

这一切片要把“前端知道什么、Rust 提供什么、协议类型在哪里生成”定成可测试边界。

## Acceptance criteria

- [x] 前端启动后能读取 bootstrap state 并显示基础连接/配置状态。
- [x] Interaction DTO 从正式协议类型生成 TypeScript。
- [x] Profile config wire 类型生成 TypeScript。
- [x] 生成的 TypeScript 文件提交到前端源码可用位置。
- [x] CI 或测试能检查生成文件是否最新。
- [x] 前端不手写重复的 interaction/config DTO shape。
- [x] Rust 侧 bootstrap command 有单测或等价验证。

## Blocked by

- [01-tauri-app-skeleton-replaces-gpui-launch-path.md](./01-tauri-app-skeleton-replaces-gpui-launch-path.md)
