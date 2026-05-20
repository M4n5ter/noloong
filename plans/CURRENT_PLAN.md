# Noloong GPUI App v1 实施计划

## Summary

实现 `noloong app` 的第一版桌面 GUI：使用 git 版 Zed GPUI 与 `gpui-component` 构建一个简洁、Zed 风格的 profile 配置应用，并保留一个非真实后端的聊天壳作为未来主 interaction client 的视觉与导航基础。v1 不接入真实 session/chat JSON-RPC，重点是 profile 配置编辑体验。界面文案必须走 i18n，运行时只显示一种语言。

## Implementation Status

- [x] 参考图和 logo 已放入 `plans/gui-reference/`，运行时只复制 logo。
- [x] 新增 `crates/noloong-config`，迁移 profile config 类型、JSONC parser、schema generation、默认路径解析和 starter draft。
- [x] Root CLI、schema wrapper、host 构建继续复用同一套配置类型。
- [x] 新增 `crates/noloong-app`，使用 git 版 `gpui`、`gpui_platform` 和 `gpui-component`。
- [x] `lib.rs` 已收缩为模块声明和公开导出；窗口启动在 `runtime.rs`，界面在 `view.rs`，状态模型在 `model.rs`，文案在 `i18n.rs`。
- [x] `noloong app [--profile-config <path>] [--locale <zh|en>]` 已接入根 CLI。
- [x] macOS 下 `noloong app` 会生成并通过 `~/Library/Application Support/Noloong/Noloong.app` 启动，使 Computer Use/Accessibility 能按正常 App 识别。
- [x] App starter draft 使用 `chatgpt_responses`、`gpt-5.4-mini` 和 `compaction.type = auto`，不写入 secret。
- [x] Profile 设置页已覆盖身份、provider、compaction、storage、plugins、manifest patches、metadata 的 v1 可视化/编辑骨架。
- [x] JSONC 预览默认隐藏，通过右侧悬浮 `{}` 入口打开，支持只读预览和复制。
- [x] Chat route 是极简视觉 shell，发送禁用，不显示假 session。
- [x] zh/en app 文案 catalog 已实现，UI 语言不从 agent profile locale 反推。
- [x] README、schema 命令和 workspace 静态回归已收口。
- [x] 可见窗口 smoke：`noloong app --locale zh` 能打开 `Noloong.app` 窗口，并已通过 Computer Use `get_app_state` 读取窗口截图和 accessibility tree。

## Key Changes

- 新增 `noloong-config` crate：
  - 承载 `HostProfileConfig`、profile config JSONC parser、schema 生成、默认路径解析、starter draft。
  - 保存时输出 canonical pretty JSON，不保留旧注释或历史格式。
- 新增 `noloong-app` crate：
  - `runtime.rs`：GPUI app/window 启动。
  - `macos_bundle.rs`：macOS app bundle 生成和 relaunch，保证桌面自动化工具可见。
  - `view.rs`：Profile/Chat/toolbar/JSONC overlay。
  - `model.rs`：typed draft、dirty state、validation、save、selected profile。
  - `i18n.rs`：zh/en 文案 catalog。
- CLI 增加 `noloong app`：
  - profile 配置路径优先级：`--profile-config` > `NOLOONG_PROFILE_CONFIG` > `~/.agents/noloong/profile-config.jsonc`。
  - UI 语言优先级：`--locale` > 系统检测。

## Test Plan

- [x] `cargo check -p noloong-app`
- [x] `cargo check -p noloong`
- [x] `cargo test -p noloong-config`
- [x] `cargo test -p noloong-app`
- [x] `cargo test -p noloong`
- [x] `cargo test -p noloong-agent --test interaction_control`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] Manual：`./target/debug/noloong app --locale zh --profile-config /tmp/noloong-app-visible-profile.jsonc`
- [x] Manual：Computer Use `list_apps` 能看到 `Noloong — .../Noloong.app/ — dev.noloong.Noloong`
- [x] Manual：Computer Use `get_app_state(app="Noloong")` 能读取窗口状态和截图。

## Assumptions

- v1 只做“配置编辑 + 聊天壳”，不实现真实 interaction client。
- UI 文案单语言显示。
- JSONC preview 只读，不做双向 JSON 编辑。
- 没有兼容性负担；旧的显式配置路径要求可以被全局默认路径替换。
- 保存输出 canonical JSON，不保留原文件注释或格式。
