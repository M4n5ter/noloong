# 微信 iLink 客户端

## 目标

微信 iLink 客户端是 noloong 的第二个交互式客户端。第一版以个人私聊为主，采用 final-only 交付：用户在微信里发送文本、图片或文件，noloong 通过 interaction server 创建或恢复会话，最终回答再发回微信。

微信 iLink 的能力弱于 Telegram，因此本客户端不模拟消息编辑、inline button 或 streaming preview。控制面统一使用带 `/` 或 `／` 前缀的文本命令，普通中文消息会作为 agent 输入。

## 登录

```bash
cargo run -p noloong -- weixin login
cargo run -p noloong -- weixin login --qr-png /tmp/noloong-weixin-login-qr.png
```

登录会拉取 iLink QR、在终端渲染二维码，并默认写出 PNG 到 `/tmp/noloong-weixin-login-qr.png`。确认后账号凭据保存到 `~/.agents/noloong/weixin/accounts/`，文件权限会尽力设置为 `0600`。

## 启动

嵌入式 interaction server：

```bash
cargo run -p noloong -- weixin run \
  --profile-config examples/profile-configs/weixin-chatgpt-subscription.json \
  --weixin-account-id <account-id> \
  --weixin-allowed-users <weixin-user-id>
```

连接外部 interaction server：

```bash
cargo run -p noloong -- weixin bridge \
  --interaction-url ws://127.0.0.1:8787/jsonrpc/ws \
  --interaction-token <token> \
  --weixin-account-id <account-id> \
  --weixin-allowed-users <weixin-user-id>
```

开发 smoke 可以用 `--weixin-allow-all` 放开 DM allowlist；常驻运行应使用 `--weixin-allowed-users`。

## 环境变量

- `WEIXIN_ACCOUNT_ID`
- `WEIXIN_TOKEN`
- `WEIXIN_BASE_URL`
- `WEIXIN_CDN_BASE_URL`
- `WEIXIN_ALLOWED_USERS`
- `WEIXIN_ALLOW_ALL`
- `WEIXIN_LOCALE`
- `WEIXIN_FILE_INLINE_MAX_BYTES`
- `WEIXIN_FILE_MAX_DOWNLOAD_BYTES`
- `WEIXIN_FILE_MAX_UPLOAD_BYTES`
- `WEIXIN_FILE_DOWNLOAD_DIR`

显式 CLI 参数优先于环境变量；本地登录凭据可补全 token 和 base URL。

## 控制命令

命令必须以 `/` 或 `／` 开头。没有前缀的文本不会进入控制面。

- `/帮助`：显示命令帮助。
- `/状态`：查看当前会话。
- `/新会话`：为当前微信对话创建新会话。
- `/会话`：列出当前微信对话的会话。
- `/切换 1`：切换到第 1 个会话。
- `/删除 1`：删除第 1 个会话。
- `/运行配置`：查看 profile、工具、插件和消息数。
- `/队列`：查看 steering/follow-up 队列。
- `/队列 <文本>`：向当前 session 添加 follow-up。
- `/清空队列`：清空 steering 和 follow-up 队列。
- `/审批`：列出待审批工具调用。
- `/同意 1`：同意第 1 个待审批项。
- `/拒绝 1`：拒绝第 1 个待审批项。
- `/进程`：列出后台进程。
- `/进程 1`：查看第 1 个进程输出。
- `/进程 1 等待`：等待第 1 个进程。
- `/进程 1 终止`：终止第 1 个进程。
- `/子任务 <prompt>`：从当前会话创建子任务。

## 媒体能力

- 入站图片会作为 `MediaKind::Image` 进入 agent。
- 入站文件会作为 `MediaKind::File` 进入 agent。
- 入站语音如果 iLink 提供转写文本，则优先使用文本；否则降级为文件附件。
- 入站视频降级为文件附件。
- 出站图片走 iLink image item。
- 出站普通文件走 iLink file item。
- 出站语音/视频不生成微信原生气泡，默认按文件发送。
- CDN 下载只允许微信相关 CDN host，避免 SSRF。
- 媒体加解密使用 iLink CDN 的 AES-128-ECB + PKCS7。

超出 `WEIXIN_FILE_MAX_DOWNLOAD_BYTES` 或 `WEIXIN_FILE_MAX_UPLOAD_BYTES` 的文件会返回清晰错误，不会让 bridge 退出。

## 状态

微信运行状态使用统一 SQLite state database：

- account scoped `sync_buf`
- peer scoped `context_token`

每次收到用户消息时会保存最新 `context_token`。发送文本、媒体和 typing 时会读取该 token；如果 iLink 返回 stale/session expired，客户端会清理 token 并重试一次无 token 发送。

## 故障排查

- 扫码只得到一串文本：确认使用的是 `qrcode_img_content` 生成的 QR，而不是原始 `qrcode` token。
- `/状态` 显示没有会话：发送任意普通消息会自动创建或恢复会话。
- 重启后 `session already exists`：当前实现会先恢复已有 deterministic session，不应再重复创建。
- SQLite event `(run_id, sequence)` 冲突：registry restore 会推进 runtime run counter，避免复用旧 run id。
- 没有 typing：iLink `getconfig` 可能没有返回 `typing_ticket`，客户端会静默跳过 typing，不影响最终回复。
- 控制命令没有触发：确认命令带 `/` 或 `／` 前缀；裸中文会作为普通消息交给 agent。

## 真实 smoke 清单

1. 启动 `weixin run`。
2. 在微信发送 `/帮助`，确认返回分段命令帮助。
3. 发送 `/状态`，确认会话状态可读。
4. 发送普通文本 `状态`，确认它进入 agent 而不是控制面。
5. 发送一张图片，确认 agent 能看到图片输入或说明已接收媒体。
6. 发送一个小文件，确认 agent 能看到文件输入或说明已接收附件。
7. 发送 `/队列`、`/进程`，确认控制面可用。
8. 触发一次需审批工具调用，再发送 `/审批` 和 `/同意 1` 或 `/拒绝 1`。
9. 发送 `/子任务 回复 weixin subagent smoke ok`，确认子任务创建路径可用。
