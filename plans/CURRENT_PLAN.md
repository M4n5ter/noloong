# Implementation Plan: 微信 iLink 交互客户端

## Overview

将微信 iLink 做成 noloong 的第二个交互式客户端。第一版不复制 Telegram：微信侧没有可靠消息编辑、没有 inline button、群聊能力不稳定、出站依赖 `context_token`、媒体走 iLink CDN + AES。目标是做一个 **DM-first 微信 cockpit + media-capable final-only delivery**：个人私聊里能发文本、图片、文件给 agent，agent 用可靠的开始提示和最终结果回复；控制面用“编号卡片 + 短命令”代替按钮。

## Architecture Decisions

- 新增 `noloong-agent-weixin` crate，不把 Telegram crate 抽成通用 channel framework；等第三个客户端出现再提炼共享抽象。
- 微信客户端默认个人 DM-first；群聊第一版默认禁用，不作为验收主线。
- display 采用 final-only：不实现消息编辑、不实现 streaming preview，不模拟 Telegram 的 inline keyboard。
- 控制交互使用编号文本协议：列表展示编号，用户发送 `/同意 1`、`/拒绝 1`、`/切换 2`、`/删除 2` 等完成操作。
- 媒体 MVP 包含图片和文件的入站/出站；语音和视频先降级为文件附件或已有转写文本，不追求原生微信语音气泡。
- iLink 运行状态进入现有 state SQLite：`sync_buf`、peer `context_token`、登录账号元数据按 account fingerprint 分区；不新增默认 JSON checkpoint。
- QR 登录和环境变量并存：`noloong weixin login` 提供人体工程学，`WEIXIN_*` 仍支持服务器部署。
- 没有兼容性负担：命令、schema、文档只描述当前最优形态，不保留过时别名或迁移 shim。

## Task List

### Phase 1: iLink 基础与登录

#### Task 1: 新增 Weixin crate 与基础配置

**Description:** 新增 `crates/noloong-agent-weixin`，接入 workspace、主 CLI 依赖和基础配置类型。配置只覆盖当前需要的 iLink 字段、访问控制、文件策略、locale 和 interaction 连接信息。

**实现状态:** 已实现。新增 crate、workspace 依赖、CLI 接入和配置校验均已落地；相关 config 测试已通过。

**Acceptance criteria:**
- [x] workspace 包含 `noloong-agent-weixin`，主包能引用该 crate。
- [x] `WeixinBridgeConfig` 支持 account/token/base URL/CDN URL、interaction URL/token、profile、locale、DM allowlist、文件下载策略。
- [x] 配置校验拒绝缺失 token/account/interaction URL、空 allowlist、非法文件大小策略。

**Verification:**
- [x] `cargo check -p noloong-agent-weixin`
- [x] 配置 serde/validation 单元测试覆盖 env-equivalent 字段和非法配置。

**Dependencies:** None

**Files likely touched:**
- `Cargo.toml`
- `crates/noloong-agent-weixin/Cargo.toml`
- `crates/noloong-agent-weixin/src/config.rs`
- `src/main.rs`

**Estimated scope:** M

#### Task 2: 实现 iLink API client 与错误模型

**Description:** 实现 iLink HTTP API 的最小客户端：`getupdates`、`sendmessage`、`sendtyping`、`getconfig`、`getuploadurl`、QR login 相关 GET。请求头、`base_info`、超时和错误分类按参考实现落地为 Rust 强类型。

**实现状态:** 已实现。已对齐参考实现的 snake_case wire shape、headers/base_info、typing、QR 和上传下载相关端点；真实 smoke 中修复了 QR 字段解析问题。

**Acceptance criteria:**
- [x] API client 自动附加 `base_info.channel_version`、iLink app headers、Authorization。
- [x] API 错误区分 HTTP、decode、iLink ret/errcode、session expired、rate limited。
- [x] `ret=-2/errcode=-2 + errmsg="unknown error"` 作为 stale session 处理。

**Verification:**
- [x] fake HTTP/client 测试请求 path/header/body。
- [x] 错误解析测试覆盖 session expired、stale session、rate limited、普通错误。

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-weixin/src/ilink_api.rs`

**Estimated scope:** M

#### Task 3: 实现 QR 登录与凭据加载

**Description:** 增加 `noloong weixin login`，拉取 QR、终端展示扫码内容、轮询确认结果，并以 `0600` 保存 account/token/base URL。运行时加载优先级为显式 CLI/env 覆盖本地登录凭据。

**实现状态:** 已实现并真实验证。已支持终端 QR、PNG QR 和本地账号保存；真实微信扫码登录已通过。

**Acceptance criteria:**
- [x] `noloong weixin login` 能输出可扫码 URL/ASCII QR，并处理 wait/scaned/redirect/expired/confirmed。
- [x] confirmed 后保存 account_id、token、base_url、user_id、saved_at；文件权限尽力设为 `0600`。
- [x] `noloong weixin run/bridge` 可从 env 或保存凭据解析账号信息；缺失时给出明确错误。

**Verification:**
- [x] QR login fake/test 覆盖 confirmed、expired、redirect host、timeout。
- [x] CLI parse/config 测试覆盖 env 覆盖和本地凭据加载。
- [x] 真实微信 QR login smoke 通过。

**Dependencies:** Task 2

**Files likely touched:**
- `crates/noloong-agent-weixin/src/login.rs`
- `src/main.rs`

**Estimated scope:** M

### Checkpoint: 基础可启动

- [x] `cargo check -p noloong-agent-weixin`
- [x] `cargo test -p noloong-agent-weixin login ilink_api config`
- [x] CLI 能解析 `weixin login`、`weixin bridge`、`weixin run`。

### Phase 2: 状态、轮询与入站消息

#### Task 4: 将 iLink sync/context 状态写入 SQLite

**Description:** 实现 SQLite 状态存储，保存 account scoped `sync_buf` 和 peer scoped `context_token`。这是微信可靠收发的基础，不使用 JSON checkpoint。

**实现状态:** 已实现。状态使用统一 SQLite，不新增 JSON checkpoint；真实路径已保存和复用 `context_token`。

**Acceptance criteria:**
- [x] `sync_buf` 按 account fingerprint 保存和读取。
- [x] `context_token` 按 account fingerprint + peer user id 保存、读取、删除。
- [x] stale token 重试后能清理对应 peer token。

**Verification:**
- [x] SQLite store 单元测试覆盖空读、写入、覆盖、删除、重建后读取。
- [x] 不保存 token 明文到 fingerprint key；敏感值只作为 value 存在本地 state DB。

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-weixin/src/state.rs`

**Estimated scope:** S

#### Task 5: 实现 long-poll 与消息去重

**Description:** 实现 iLink `getupdates` polling loop，恢复 `sync_buf`，处理 long-poll timeout、连续失败 backoff、session expired 暂停、message id/content fingerprint 去重。

**实现状态:** 已实现并真实验证。真实微信 smoke 中 polling 能持续运行，handler 错误不会直接退出 bridge。

**Acceptance criteria:**
- [x] poller 启动时读取 `sync_buf`，成功响应后持久化新的 `get_updates_buf`。
- [x] 单条消息 handler 失败不终止 polling；recoverable API 错误继续重试。
- [x] 重复 message id 和短时间重复文本不会重复投递给 agent。

**Verification:**
- [x] fake iLink/poller 测试覆盖 sync_buf 推进。
- [x] polling 测试覆盖 handler error 和 dedup。
- [x] 真实微信 polling smoke 通过。

**Dependencies:** Tasks 2, 4

**Files likely touched:**
- `crates/noloong-agent-weixin/src/polling.rs`

**Estimated scope:** M

#### Task 6: 解析入站文本、引用和基础媒体

**Description:** 将 iLink `item_list` 转换成 noloong `AgentMessage` 输入。支持文本、引用消息摘要、图片、文件、语音/视频降级附件。引用上下文使用模型可见 `<weixin_reply_context>` JSON 块，同时写入 metadata。

**实现状态:** 已实现并真实验证。文本、引用上下文、边界转义和媒体识别已落地；真实文本、图片和 docx 文件输入均已进入模型输入。

**Acceptance criteria:**
- [x] 文本消息生成 `AgentMessage`，metadata 包含 account、peer、message id、chat kind、context token 状态。
- [x] `ref_msg` 生成安全转义的 `<weixin_reply_context>`，被引用文本/媒体摘要进入模型可见内容。
- [x] 图片和文件生成 `MediaBlock`；语音/视频生成文件类 `MediaBlock` 或使用 iLink 已有 voice text。

**Verification:**
- [x] 单元测试覆盖纯文本、引用文本、引用媒体、语音转写等基础路径。
- [x] 真实微信图片/文件输入 smoke。
- [x] JSON escaping 针对 `</weixin_reply_context>` 的专门测试。

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent-weixin/src/input.rs`
- `crates/noloong-agent-weixin/src/media.rs`

**Estimated scope:** M

### Checkpoint: 入站可用

- [x] fake iLink text update 能提交 `agent/prompt`。
- [x] fake iLink media update 能形成 `MediaBlock`。
- [x] `cargo test -p noloong-agent-weixin polling input state`
- [x] 真实图片/文件输入继续验收。

### Phase 3: 出站文本与媒体交付

#### Task 7: 实现微信文本渲染与 final-only display

**Description:** 实现微信适配的文本渲染、拆分和 display delivery。微信不支持可靠 edit，因此只处理 run started 简短提示、assistant final、run failed、approval card 和必要状态消息。

**实现状态:** 已实现。真实 smoke 后将 `RunStarted` 改为 iLink typing，并将控制输出改为段落式文本以适配微信显示。

**Acceptance criteria:**
- [x] assistant delta 不发送 preview，不留下游标或半成品消息。
- [x] assistant final 按微信长度限制拆分，保留代码块和复制友好的换行。
- [x] run started 使用 iLink typing；run completed 静默；run failed 发清晰错误。

**Verification:**
- [x] display 单元测试覆盖 delta ignored、final sends、run failed、text split。
- [x] 渲染测试覆盖标题、表格、代码块、长行 wrapping。
- [x] 真实微信 `/帮助`、`/状态`、普通文本回复 smoke 通过。

**Dependencies:** Tasks 2, 6

**Files likely touched:**
- `crates/noloong-agent-weixin/src/render.rs`
- `crates/noloong-agent-weixin/src/delivery.rs`
- `crates/noloong-agent-weixin/src/display.rs`

**Estimated scope:** M

#### Task 8: 实现 iLink CDN 媒体收发

**Description:** 实现微信 CDN AES-128-ECB + PKCS7 下载/上传路径。入站媒体从 `encrypt_query_param/full_url` 下载并解密；出站图片/文件先加密上传 CDN，再通过 `sendmessage.item_list` 发送。

**实现状态:** 已实现并真实验证。代码路径已覆盖 AES、CDN allowlist、入站下载解密和出站上传发送；真实图片和 docx 文件入站已通过。最新真实 smoke 暴露出 API 成功但微信客户端未显示图片的问题已按参考实现从根因修复：出站媒体 `sendmessage` payload 明确携带 `media.encrypt_type = 1`，并避免输出未使用字段的 `null` 值；复验中微信客户端已显示图片气泡。

**Acceptance criteria:**
- [x] AES padding/encrypt/decrypt 与参考实现兼容。
- [x] 下载只允许微信 CDN allowlist host，防 SSRF。
- [x] 出站图片走 image item，普通文件走 file item；语音/视频默认作为文件附件发送。
- [x] 超出文件策略大小时给用户发送可理解的失败说明，而不是让 bridge 崩溃。

**Verification:**
- [x] AES 向量和 padded size 单元测试。
- [x] allowlist 测试拒绝非微信 CDN host 和非 http/https scheme。
- [x] fake CDN 更完整覆盖 download decrypt、upload ciphertext、sendmessage media item。
- [x] 真实微信媒体出站 smoke：不仅 API 成功，微信客户端必须可见图片/文件气泡。

**Dependencies:** Tasks 2, 6, 7

**Files likely touched:**
- `crates/noloong-agent-weixin/src/media.rs`
- `crates/noloong-agent-weixin/src/delivery.rs`

**Estimated scope:** M

#### Task 9: 实现 context_token 出站策略

**Description:** 每次向 peer 发送消息时使用最新 `context_token`；遇到 session expired/stale token 时清理 token 并重试一次无 token 发送，保证自动化或长时间未互动后的推送仍尽力可达。

**实现状态:** 已实现。真实文本和控制输出都已带 `context_token` 发送；stale token 清理和重试有单测。

**Acceptance criteria:**
- [x] 收到用户消息时保存最新 `context_token`。
- [x] 发送文本/媒体/typing 时读取 peer token 并放入 payload。
- [x] session expired/stale token 错误只触发一次 tokenless retry，并清理 SQLite 中该 peer token。

**Verification:**
- [x] fake iLink server 测试 sendmessage 带 token。
- [x] stale token 测试覆盖清理和 tokenless retry。
- [x] rate limit/stale 分类测试覆盖。

**Dependencies:** Tasks 4, 7, 8

**Files likely touched:**
- `crates/noloong-agent-weixin/src/state.rs`
- `crates/noloong-agent-weixin/src/delivery.rs`

**Estimated scope:** S

### Checkpoint: 出站可用

- [x] fake iLink E2E：文本 prompt -> final text reply。
- [x] fake iLink E2E：图片/文件输入进入 agent。
- [x] fake iLink E2E：agent 图片/文件输出回微信。
- [x] `cargo test -p noloong-agent-weixin delivery media display`
- [x] 真实微信文本出站 smoke 通过。
- [x] 真实微信媒体出站 smoke。

### Phase 4: 微信 cockpit 与 interaction 接线

#### Task 10: 实现 WeixinBridge 与 session 生命周期

**Description:** 复用 noloong interaction JSON-RPC 控制面，实现微信 DM 到 session 的映射、profile 选择、prompt/follow-up、display subscription 和 system prompt addition。

**实现状态:** 已实现并修复真实重启恢复问题。真实 smoke 暴露的 `session already exists`、event `(run_id, sequence)` 冲突，以及旧 profile deterministic session 导致 `/状态` 静默失败的问题已从 registry/runtime/bridge 根因修复；新 session record 会持久化唯一 `runIdPrefix`，同一 session id 删除重建也不会复用旧 run id。

**Acceptance criteria:**
- [x] DM peer id 映射稳定 session id，支持 active session、new session、switch session。
- [x] running/paused session 的新输入进入 follow-up，不抢占当前 run。
- [x] system prompt addition 说明微信 channel、final-only、无按钮、引用上下文和媒体降级行为。
- [x] bridge 重启后能恢复已有 deterministic session。
- [x] bridge 重启后遇到当前 profile 配置中不存在的旧 deterministic session，会清理旧 session 并回到当前 profile 的新会话路径。

**Verification:**
- [x] bridge 单元测试覆盖 create/prompt/session restore。
- [x] bridge 单元测试覆盖旧 profile deterministic session 清理。
- [x] registry restore run counter 回归测试。
- [x] 真实微信重启后 `/状态` 恢复既有 session。

**Dependencies:** Tasks 6, 7

**Files likely touched:**
- `crates/noloong-agent-weixin/src/bridge.rs`
- `src/main.rs`
- `crates/noloong-agent-core/src/runtime/run_loop.rs`
- `crates/noloong-agent/src/interaction/registry.rs`

**Estimated scope:** M

#### Task 11: 实现编号卡片与短命令控制面

**Description:** 用微信文本协议替代 Telegram inline buttons。所有列表类控制面输出编号，用户用带 `/` 或 `／` 前缀的短命令选择对象和动作。

**实现状态:** 已实现。根据真实体验已收紧为必须带 `/` 或 `／` 前缀，并补了 i18n/help 和段落式微信输出；编号选择现在绑定到当前 peer 最近控制卡，过期或不匹配时会重发当前列表。

**Acceptance criteria:**
- [x] 支持 `/帮助`、`/状态`、`/新会话`、`/会话`、`/切换 N`、`/删除 N`、`/运行配置`、`/审批`、`/同意 N`、`/拒绝 N`、`/队列`、`/清空队列`、`/进程`、`/进程 N`、`/子任务 <prompt>`。
- [x] 列表编号只对当前 peer 的最近控制卡有效，过期或不匹配时返回当前列表和提示。
- [x] 命令支持中文主路径和 slash alias；普通文本不被误判为命令。
- [x] 控制输出支持 i18n，并适配微信对单换行的显示行为。

**Verification:**
- [x] parser 单元测试覆盖中文命令、slash prefix、编号选择、普通文本不误判。
- [x] 真实微信 smoke 验证 `/帮助`、`/状态`、普通 `状态`、`/队列`、`/进程`。
- [x] fake bridge 测试继续补 approval resolve、session switch/delete、queue clear、process read、subagent spawn。

**Dependencies:** Task 10

**Files likely touched:**
- `crates/noloong-agent-weixin/src/input.rs`
- `crates/noloong-agent-weixin/src/runtime.rs`
- `crates/noloong-agent-weixin/src/i18n.rs`

**Estimated scope:** M

#### Task 12: 接入 CLI run/bridge 和嵌入式 interaction server

**Description:** 增加 `noloong weixin bridge` 和 `noloong weixin run`。`bridge` 连接外部 interaction server；`run` 像 Telegram 一样内嵌 interaction server 并启动微信 bridge。

**实现状态:** 已实现。`weixin run` 已用于真实微信 smoke。

**Acceptance criteria:**
- [x] `weixin bridge` 支持外部 `--interaction-url/--interaction-token`。
- [x] `weixin run` 能加载 profile config，创建 loopback interaction server，并启动微信 bridge。
- [x] 默认 profile 媒体策略适配微信：图片/文件保留，语音/视频降级为文件。

**Verification:**
- [x] CLI parse 测试覆盖 `weixin run`。
- [x] config 测试覆盖 env 和 CLI 参数优先级。
- [x] 真实 `weixin run` smoke 通过文本与控制命令路径。

**Dependencies:** Tasks 10, 11

**Files likely touched:**
- `src/main.rs`
- `src/config.rs`

**Estimated scope:** M

### Checkpoint: 交互闭环

- [x] 文本 DM 可以创建 session、发起 run、收到 final。
- [x] 编号命令可以完成审批和 session 控制。
- [x] `cargo test -p noloong-agent-weixin`
- [x] `cargo test -p noloong`

### Phase 5: 文档、示例和真实 smoke

#### Task 13: 更新文档和示例配置

**Description:** 增加微信 iLink 文档和最小 profile 示例。文档只描述当前实现，不保留 Telegram 兼容说法或历史妥协。

**实现状态:** README、独立 `docs/WEIXIN.md` 和微信专用 ChatGPT 订阅示例 profile 已补齐。

**Acceptance criteria:**
- [x] README 包含 QR 登录、env 配置、DM allowlist、编号命令语法。
- [x] 文档包含完整媒体限制和故障排查。
- [x] 示例 profile 可用于 ChatGPT 订阅的微信 smoke。
- [x] 文档明确群聊默认不支持、编辑/按钮不支持、语音/视频当前降级。

**Verification:**
- [x] 示例 profile 通过配置加载和 validate 校验。
- [x] README 中的命令与 CLI/help 输出一致。

**Dependencies:** Tasks 1-12

**Files likely touched:**
- `README.md`
- `crates/noloong-agent-weixin/docs/WEIXIN.md`
- `examples/profile-configs/weixin-*.json`
- `schemas/profile-config.schema.json`（仅当配置 schema 需要更新）

**Estimated scope:** S

#### Task 14: 完整回归与真实微信 smoke

**Description:** 完成所有代码后跑完整回归，并在真实微信 iLink 环境验证核心路径。真实 smoke 以 DM-first 为准。

**实现状态:** 已完成。文本、图片/文件入站、媒体出站、控制命令、typing、session restore、审批和 subagent 已在真实微信通过；subagent 最终输出已验证会回写父会话上下文。

**Acceptance criteria:**
- [x] QR login 可获取凭据，`weixin run` 能启动 polling。
- [x] 真实微信 DM 文本 prompt 能收到最终回答。
- [x] 真实微信 DM 图片/文件能进入 agent，agent 能引用其内容或说明媒体已接收。
- [x] 真实微信编号命令能完成一次审批或 session 切换。
- [x] bridge 运行期间 iLink recoverable error 不会退出进程。

**Verification:**
- [x] `cargo fmt --all --check`
- [x] `cargo check -p noloong`
- [x] `cargo clippy -p noloong-agent-weixin --all-targets -- -D warnings`
- [x] `cargo clippy -p noloong --all-targets -- -D warnings`
- [x] `cargo test -p noloong-agent-weixin`
- [x] `cargo test -p noloong-agent-telegram display`
- [x] `cargo test -p noloong-agent --test interaction_registry`
- [x] `cargo test -p noloong cli_weixin`
- [x] `cargo test -p noloong weixin_config`
- [x] `cargo test -p noloong profile_config_loads_weixin_chatgpt_example`
- [x] 真实微信媒体出站 smoke 记录通过现象和遗留问题。
- [x] 真实微信审批和 subagent smoke 通过；subagent child final 能显示到微信，并能作为父会话下一轮上下文读取。

**Dependencies:** Tasks 1-13

**Files likely touched:**
- No planned code changes; only docs/test notes if needed.

**Estimated scope:** S

### Checkpoint: Complete

- [x] 关键 unit/integration tests 通过。
- [x] 真实微信 DM 文本、媒体、编号控制三条路径全部通过。
- [x] 当前计划中的过时 Telegram 内容已清理。
- [x] 未新增默认 JSON 状态文件、旧兼容 shim 或无用抽象。

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| iLink API 行为与参考实现不完全一致 | High | 先做 fake server 和最小真实 smoke；API 错误保留 raw ret/errcode/errmsg 日志。 |
| iLink `context_token` 过期导致出站失败 | High | 保存最新 token，session expired/stale 时清理并重试一次无 token 发送。 |
| 媒体 CDN/AES 实现细节错误 | High | 早做 AES/CDN fake tests；图片/文件先验收，语音/视频明确降级。 |
| 微信没有按钮导致控制体验笨重 | Medium | 使用编号卡片 + 短命令；控制卡保持最近选择上下文，失败时重发当前列表。 |
| 群聊能力不可控 | Medium | 第一版 DM-only，群聊默认 disabled，文档明确不是 bug。 |
| 长文本在微信里难复制/阅读 | Medium | final-only 输出做 block-aware split、保留代码块、长行软换行。 |
| 登录凭据泄漏 | Medium | 凭据文件 `0600`，日志只打印短 fingerprint，不打印 token。 |

## Parallelization Opportunities

- Task 2 API client 和 Task 4 SQLite state 可以并行。
- Task 6 入站解析和 Task 7 文本渲染可在 API contract 稳定后并行。
- Task 8 媒体 CDN/AES 可由独立 agent 实现，避免影响 bridge 控制面。
- Task 11 编号命令 parser/i18n 可与 Task 10 bridge 接线并行，但编号 action record 格式需先约定。
- Task 13 文档可在 Task 10-12 稳定后并行补齐。

## Open Questions

- 无。当前默认决策：个人 DM-first、QR setup + env、编号卡片 + 短命令、图片/文件进入 MVP、语音/视频降级、final-only delivery、SQLite 状态存储。
