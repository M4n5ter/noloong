# 实施计划：Telegram Interaction Client v1

> 状态：已完成实现与本地验证。Telegram 已作为 noloong 的第一个第一方用户交互通道接入：Rust 实现、root CLI 使用 `clap`、支持单二进制 loopback 模式，也支持 interaction server 与 Telegram bridge 分进程部署。

## 目标

在现有 `InteractionControlHandler`、HTTP/WebSocket JSON-RPC transport 和 display events 之上，新增内置 Telegram interaction client。Telegram client 只负责用户交互通道，不决定 agent profile、provider 或 credential；这些仍由 host/binary 层通过 profile config 构造。

v1 聚焦：

- Telegram long polling。
- 文本对话输入。
- streaming delta edit 和 final reply。
- tool lifecycle display。
- Telegram inline approval buttons。
- 默认安全 allowlist。
- 可复用 `InteractionWsClient`。
- 单进程 `noloong telegram` 和分进程 `noloong serve interaction` + `noloong telegram-bridge`。

不在本轮范围：

- Telegram webhook。
- 图像/视频/音频 Telegram 消息输入输出。
- per-chat profile mapping。
- DM topic 自动管理。
- 真实 Telegram Bot live smoke 自动化。

## 架构决策

- 不修改 `noloong-agent-core`；Telegram 属于 application/interaction 层。
- Telegram bridge 必须通过 WebSocket JSON-RPC 调用 interaction control plane；即使 `noloong telegram` 单进程模式也走 loopback WS，避免绕过协议。
- 新增第一方 crate `crates/noloong-agent-telegram`，负责 Telegram 平台适配、消息渲染、long polling、access policy、approval callback 和 display event 投递。
- 在 `noloong-agent` 中新增 feature-gated `InteractionWsClient`，供 Telegram 和未来 Web/TUI/其它第一方 interaction clients 复用。
- root `noloong` binary 负责 profile config loading、provider/runtime 构造、interaction server 启动和 Telegram bridge 组合。
- Telegram bridge 不包含 provider/model/credential 字段；它只持有可选 `profileId`，未配置时使用 `profile/list` 的默认 profile。
- 默认要求 allowlist；未配置 allowed users/chats 且未显式 allow all 时启动失败。
- group/supergroup mention gating 使用配置的 bot username 判断 `@bot` 和 reply-to-bot。
- Bot API 使用 direct `reqwest` adapter；没有保留未使用的 Telegram framework 依赖，便于精确控制 long polling、fallback network、错误处理和 fake API 测试。
- root CLI 使用 `clap` derive，一次性覆盖 `serve interaction`、`telegram-bridge`、`telegram` 三个入口。

## 已完成改动

### 1. Workspace 和 feature 边界

- [x] 新增 workspace crate `crates/noloong-agent-telegram`。
- [x] `noloong-agent` 新增可选 feature `interaction-client`。
- [x] `InteractionWsClient` 依赖 `tokio-tungstenite`、`futures-util`，并保持 feature-gated。
- [x] `noloong-agent-telegram` 通过 `noloong-agent` 的 `interaction-client` feature 使用 interaction client。
- [x] 未引入 `teloxide`；Telegram Bot API 通过 direct `reqwest` adapter 实现。

### 2. 可复用 WebSocket JSON-RPC client

- [x] 新增 `InteractionWsClientConfig`、`InteractionWsClient`、`InteractionWsNotification`、`InteractionClientError`。
- [x] 支持 bearer auth、request id、typed `request_as`、JSON-RPC error 映射和 request timeout。
- [x] notifications 使用 bounded broadcast channel 暴露，不阻塞 response dispatch。
- [x] connection close 会唤醒 pending requests。
- [x] client 不依赖 Telegram 类型。

### 3. Telegram 配置、权限与 session 映射

- [x] 新增 `TelegramBridgeConfig`、`TelegramAccessPolicy`、`TelegramNetworkConfig`、`TelegramSessionMapper`。
- [x] 支持 bot token、interaction URL/token、可选 profile id、UX 权限、batch/edit throttle 和 network config。
- [x] 默认要求 allowed users/chats；显式 allow all 才能关闭 allowlist。
- [x] DM、group、forum topic 映射为稳定 session id：`telegram:{chatId}` 或 `telegram:{chatId}:thread:{threadId}`。
- [x] session metadata 记录 `channel=telegram`、`chatId`、`threadId`、`chatType`。

### 4. Telegram 到 interaction control plane 的 lifecycle

- [x] bridge 初始化请求 `agent.run`、`agent.queue`、`approval.resolve` authority。
- [x] bridge 请求 `displayEvents=true`、`streamText=true`、`editMessage=true`、`markdown=true`。
- [x] 未配置 profile id 时使用 interaction server 返回的默认 profile。
- [x] 首次收到 chat/thread 消息时创建 session 并订阅 display events。
- [x] idle/completed/failed/aborted session 使用 `agent/prompt`。
- [x] running/paused session 使用 `agent/follow_up`。

### 5. 输入聚合与 group gating

- [x] DM 文本无需 mention。
- [x] group/supergroup 默认要求 mention 或 reply-to-bot。
- [x] 支持 `TELEGRAM_BOT_USERNAME` / `--telegram-bot-username`。
- [x] 支持关闭 group mention gating。
- [x] 同一 chat/thread/user 的连续文本默认 600ms 后合并提交。
- [x] 接近 Telegram 4096 UTF-16 split threshold 时延长等待窗口。
- [x] 空白消息忽略，不调用 interaction。

### 6. Telegram 输出、渲染与 delivery

- [x] MarkdownV2 escaping。
- [x] fenced code block 保持可读。
- [x] GFM pipe table 转换为 Telegram 可读行。
- [x] 按 Telegram UTF-16 长度限制拆分 outgoing text。
- [x] send/edit 遇到 parse error 时 fallback plain text。
- [x] edit 遇到 `message is not modified` 时视为成功。

### 7. Display event 和 approval 投递

- [x] assistant delta 创建或编辑 preview message。
- [x] assistant final 立即 flush。
- [x] final 太长或 edit 失败时发送 fresh final message，不丢内容。
- [x] tool lifecycle 可渲染为简短 status。
- [x] approval request 渲染为 inline allow/deny buttons。
- [x] callback data 使用短 key，避免超过 Telegram callback data 限制。
- [x] callback query 解析后调用 interaction `approval/resolve`。

### 8. Telegram network 和 polling

- [x] 支持 proxy mode。
- [x] 支持 fallback IP discovery 和 static fallback IP。
- [x] 拒绝 private、loopback、link-local、unspecified fallback IP。
- [x] long polling 支持 offset advance。
- [x] transient network error 会 backoff retry。
- [x] 409 conflict 到达重试上限后变成 fatal error。

### 9. Root host、profile config 和 CLI

- [x] 新增 root profile config loader。
- [x] 支持 chat completions、responses、anthropic messages、ChatGPT responses provider 配置。
- [x] 支持 memory、SQLite、PostgreSQL、object memory、object fs registry store 配置。
- [x] 新增 `noloong serve interaction`。
- [x] 新增 `noloong telegram-bridge`。
- [x] 新增 `noloong telegram`，在同一进程启动 loopback interaction server 和 Telegram bridge。
- [x] root CLI 已迁移到 `clap` derive。

### 10. 文档

- [x] 更新 `crates/noloong-agent/docs/ARCHITECTURE.md`，加入第一方 Telegram client 架构。
- [x] 更新 `crates/noloong-agent/docs/INTERACTION.md`，加入 Telegram client 使用入口。
- [x] 新增 `crates/noloong-agent-telegram/docs/TELEGRAM.md`。
- [x] 新增 `examples/profile-configs/telegram-openrouter-free.json` 作为无凭据示例配置。

## 主要文件

- `src/main.rs`
- `src/config.rs`
- `src/host.rs`
- `crates/noloong-agent/src/interaction/client.rs`
- `crates/noloong-agent/tests/interaction_ws_client.rs`
- `crates/noloong-agent-telegram/src/access.rs`
- `crates/noloong-agent-telegram/src/approval.rs`
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/config.rs`
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/display.rs`
- `crates/noloong-agent-telegram/src/input.rs`
- `crates/noloong-agent-telegram/src/network.rs`
- `crates/noloong-agent-telegram/src/polling.rs`
- `crates/noloong-agent-telegram/src/render.rs`
- `crates/noloong-agent-telegram/src/session.rs`
- `crates/noloong-agent-telegram/src/telegram_api.rs`

## 已执行验证

- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace`
- [x] `cargo test -p noloong`
- [x] `cargo test -p noloong-agent-telegram`
- [x] `cargo test -p noloong-agent --features interaction-client,interaction-http --test interaction_ws_client`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo clippy -p noloong-agent --features interaction-client,interaction-http --all-targets -- -D warnings`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" crates src`
- [x] `rg -n "teloxide" Cargo.toml crates`
- [x] Telegram Bot API live smoke: `getMe` / `getUpdates` with fallback IP routing.
- [x] `noloong telegram` live startup smoke with real Telegram credentials, real allowlist, temporary profile, and controlled shutdown after 12s.

## 未执行项

- [ ] 未执行主动发送消息或 end-to-end user prompt smoke。原因是 bot 主动发送 Telegram 消息属于外部可见动作；需要单独确认后再执行。可在本机 shell 通过环境变量提供 `TELEGRAM_BOT_TOKEN`、`TELEGRAM_ALLOWED_USERS`、必要时提供 `TELEGRAM_BOT_USERNAME`，再配置 provider/profile config 后手动运行：

```bash
cargo run -p noloong -- telegram
```

## 后续演进方向

- Telegram webhook transport。
- Telegram media input/output。
- per-chat/per-topic profile mapping。
- 更完善的 Telegram message formatting，包括图片、文件、引用、thread-aware status。
- 多用户/多 chat 级别的 quota、rate limit 和 audit policy。
- 将 Telegram bridge 的 fake API 测试扩展为可复用 platform conformance suite。
