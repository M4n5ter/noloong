# Implementation Plan: Telegram Agent Cockpit

> 状态：Phase 1 的 Bot API、配置和 checkpoint 底座已完成；文件下载执行期错误映射会在 Phase 2 输入处理中补齐。后续继续推进多模态输入、native 输出和 cockpit UI。目标是把 `noloong-agent-telegram` 从“文本桥”升级为个人私聊优先的 Telegram-native Agent Cockpit：完整接入 Noloong interaction control plane，支持多模态输入输出、文件流、状态卡、审批卡、任务卡、队列/manifest/subagent/session 操作，并把移动端长任务体验打磨到可长期使用。

## Overview

当前 Telegram V1 已支持文本输入、文本 batching、display stream、流式编辑、最终回复、工具状态和 inline approval。短板是体验仍像最小 bridge：没有命令菜单、没有 cockpit 状态面、没有 Telegram 文件/媒体输入输出、没有后台 process 管理、没有 manifest/subagent/session/queue 的 Telegram UI，也没有重启 update checkpoint。第一阶段主场景锁定为个人私聊；群组/topic 继续保持现有 mention/thread 基础能力，但不做多人协作审批优化。

## Architecture Decisions

- Telegram bridge 仍是 application-layer interaction client，不进入 `noloong-agent-core`，也不持有 provider credentials。
- 第一阶段继续使用 long polling；不实现 webhook、Mini App、payments、inline mode、business connection 或 channel 管理。
- Telegram 初始化请求完整 authority：`agent.run`、`agent.queue`、`approval.resolve`、`manifest.apply`、`process.control`、`subagent.spawn`、`session.delete`；危险操作必须通过 callback confirmation。
- 文件策略采用 hybrid：小文件 inline base64，大文件下载到受控目录并以 `file://` URI 进入 `MediaBlock`，所有 Telegram `file_id`、MIME、文件名、大小保留到 metadata。
- 输出优先走 Telegram native media/file API；无法发送 native media 的 provider-only 内容降级为可读卡片。
- 体验模型采用 Agent Cockpit：聊天仍是主入口，但 commands、inline buttons、状态卡、审批卡、任务卡、文件卡是第一等交互面。

## Task List

### Phase 1: Telegram Bot API Foundation

#### Task 1: Expand Telegram API primitives

**Description:** 扩展 `TelegramApi` 抽象，让 bridge 具备 Telegram-native 的文件、媒体、chat action 和 command menu 能力。此任务只补 Bot API 能力层，不改变业务流。

**Acceptance criteria:**
- [x] `TelegramApi` 支持 `get_file`、download file bytes、`send_photo`、`send_document`、`send_audio`、`send_voice`、`send_video`、`send_chat_action`、`set_my_commands`。
- [x] send/media request 支持 `message_thread_id`、caption、reply markup；edit request 保持 Telegram Bot API 原生 `chat_id + message_id` 定位。
- [x] 新增 `TelegramInputFile`，支持 multipart upload path/bytes 与 Telegram `file_id` reuse。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram telegram_api`
- [x] fake API 覆盖 request serde、multipart-free JSON paths、Bot API error fallback。

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-telegram/src/telegram_api.rs`
- `crates/noloong-agent-telegram/src/polling.rs`

**Estimated scope:** M

#### Task 2: Add Telegram bridge file policy and startup update policy

**Description:** 增加 Telegram 文件生命周期配置和重启 update 处理策略。默认避免重启后误处理旧消息，同时为 media input/output 提供稳定落地目录。

**Acceptance criteria:**
- [x] `TelegramBridgeConfig` 增加 `file_policy`：inline 上限、最大下载大小、下载目录、保留时长。
- [x] `TelegramBridgeConfig` 增加 `startup_update_policy`，默认 `skip_pending_without_checkpoint`。
- [x] CLI/env 能配置下载目录、大小上限、是否跳过 pending updates。
- [ ] 下载目录创建失败、文件过大、未知 MIME 返回可本地化错误。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram config`
- [x] `cargo test -p noloong --bin noloong telegram_config`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/config.rs`
- `src/main.rs`

**Estimated scope:** M

#### Task 3: Persist long-polling offset checkpoints

**Description:** 让 Telegram polling 在正常处理 update 后记录 offset，重启时从 checkpoint 恢复；无 checkpoint 时按 startup policy 处理 pending updates。

**Acceptance criteria:**
- [x] 新增 offset store trait，并提供默认 file-backed implementation。
- [x] `TelegramPoller` 启动时可加载 initial offset。
- [x] 每个 update 成功处理后保存 `update_id + 1`。
- [x] `skip_pending_without_checkpoint` 会先 consume 当前 pending updates，再开始处理新消息。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram polling`
- [x] fake poller 覆盖重启不重复处理、handler 失败不推进 offset。

**Dependencies:** Task 2

**Files likely touched:**
- `crates/noloong-agent-telegram/src/polling.rs`
- `crates/noloong-agent-telegram/src/config.rs`

**Estimated scope:** M

### Checkpoint: Bot API Foundation

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] `cargo test -p noloong --bin noloong`

### Phase 2: Multi-modal Input

#### Task 4: Parse rich Telegram updates into bridge input

**Description:** 将 `TelegramUpdate` 从 text-only 扩展到 photo/document/audio/voice/video/caption，并保留 chat/thread/user/reply metadata。命令消息仍单独路由，不进入文本 batching。

**Acceptance criteria:**
- [x] `TelegramMessage` 反序列化 photo、document、audio、voice、video、caption、entities。
- [x] 新增 `TelegramInboundMessage`，可表达 text + media attachments。
- [x] 支持 caption 作为文本块，附件按 Telegram 顺序转入 media attachment model。
- [x] command detection 支持 `/cmd` 和 `/cmd@bot_username`。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram input`
- [x] serde fixtures 覆盖图片、文档、语音、视频、caption、bot mention。

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/polling.rs`
- `crates/noloong-agent-telegram/src/input.rs`
- `crates/noloong-agent-telegram/src/access.rs`

**Estimated scope:** M

#### Task 5: Convert Telegram attachments to Agent media

**Description:** 实现 Telegram attachment resolver：调用 `get_file` / download，根据 file policy 生成 `ContentBlock::Media`，并把 Telegram file metadata 写入 `MediaBlock.metadata`。

**Acceptance criteria:**
- [x] 小文件按 MIME/kind 生成 inline base64 media。
- [x] 大文件写入受控目录，并生成 `MediaSource::Uri { uri: "file://..." }`。
- [x] 图片映射 `MediaKind::Image`，语音/音频映射 `Audio`，视频映射 `Video`，其它 document 映射 `File`。
- [x] 超过最大下载大小时生成明确用户可见错误，不创建 agent prompt。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram media`
- [x] fake API 覆盖 file_id reuse、下载失败、MIME 缺失、大小超限。

**Dependencies:** Task 2, Task 4

**Files likely touched:**
- `crates/noloong-agent-telegram/src/input.rs`
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`

**Estimated scope:** M

#### Task 6: Prompt with rich AgentMessage

**Description:** 将 `handle_text_message` 升级为 `handle_inbound_message`。idle/completed 走 `agent/prompt`，running/paused 走 `agent/follow_up`，并确保媒体输入不会被文本 batching 破坏顺序。

**Acceptance criteria:**
- [x] 文本-only 消息继续保持 batching 行为。
- [x] 带附件消息立即形成一个 `AgentMessage`，不与其它消息合并。
- [x] running/paused 状态下用户输入进入 follow-up queue。
- [x] message id 使用稳定 Telegram chat/message id，避免重复提交。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram bridge`
- [x] fake interaction 覆盖 text-only、caption+media、running follow-up；media resolver 覆盖 media-only block 生成。

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/input.rs`
- `src/main.rs`

**Estimated scope:** M

### Checkpoint: Multi-modal Input

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-telegram input bridge`
- [ ] Manual smoke：私聊发送图片、文档、语音、视频，Agent 能看到对应 media block。

### Phase 3: Telegram-native Output and Display

#### Task 7: Send assistant media and files natively

**Description:** 扩展 delivery/rendering，将 assistant message 中的 `ContentBlock::Media` 发送为 Telegram 原生 photo/document/audio/video/voice。文本和媒体混合时使用 caption 或相邻消息，保证移动端可读。

**Acceptance criteria:**
- [x] inline base64 media 可上传为 Telegram file。
- [x] `file://` media 可从本地路径上传。
- [x] provider-only media 无本地数据时渲染为可读 fallback card。
- [x] 发送失败时降级为文本说明，不丢失最终回复。

**Verification:**
- [x] `cargo test -p noloong-agent-telegram delivery`
- [x] fake API 覆盖 send_photo/send_document/send_audio/send_video/send_voice、provider-only fallback、media send failure fallback。

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/render.rs`
- `crates/noloong-agent-telegram/src/telegram_api.rs`

**Estimated scope:** M

#### Task 8: Improve streaming UX with run cards and chat actions

**Description:** 将 display delivery 从“不断编辑一条文本”升级为状态清晰的 run card：typing/upload actions、流式预览、最终消息、失败/暂停状态均有稳定 UI。

**Acceptance criteria:**
- [ ] run started 发送或更新轻量状态卡。
- [ ] 文本流按节流编辑预览，最终回复替换或补发。
- [ ] 长回复 split 后保留首尾和 continuation 标识。
- [ ] `send_chat_action` 在长模型运行和文件上传时使用。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram display`
- [ ] fake display 覆盖 started/delta/final/failed/paused/chat action。

**Dependencies:** Task 7

**Files likely touched:**
- `crates/noloong-agent-telegram/src/display.rs`
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/text.rs`

**Estimated scope:** M

#### Task 9: Make tool and approval cards production-grade

**Description:** 将现有 tool status 和 approval button 打磨为可审计卡片：工具参数摘要、权限、reason、过期时间、审批后编辑原卡片，并支持 pending approval 列表。

**Acceptance criteria:**
- [ ] approval card 显示 tool、参数摘要、permissions、reason、expires_at。
- [ ] allow/deny 后编辑原消息并移除按钮。
- [ ] callback 非授权用户只收到 callback toast，不改变审批状态。
- [ ] `/approvals` 可列出并重新渲染当前 pending approvals。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram approval display`
- [ ] fake callback 覆盖 allow、deny、expired、unauthorized。

**Dependencies:** Task 8

**Files likely touched:**
- `crates/noloong-agent-telegram/src/approval.rs`
- `crates/noloong-agent-telegram/src/display.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`

**Estimated scope:** M

### Checkpoint: Native Output

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] Manual smoke：模型输出文本、图片/文件、审批请求，Telegram UI 可读且按钮状态正确。

### Phase 4: Agent Cockpit Commands

#### Task 10: Add command parser and command menu registration

**Description:** 新增 Telegram command layer，启动时注册本地化 bot commands，并将命令从普通 prompt 中分离。命令只操作当前 chat 的 active session，除非显式指定 session id。

**Acceptance criteria:**
- [ ] 支持 `/start`、`/help`、`/status`、`/new`、`/switch`、`/sessions`、`/profiles`、`/continue`、`/abort`、`/queue`、`/approvals`、`/processes`、`/process`、`/manifest`、`/subagent`、`/settings`。
- [ ] `set_my_commands` 在 bridge 初始化后执行，按 locale 注册描述。
- [ ] 命令解析支持 `/cmd@bot_username`。
- [ ] 未知命令返回本地化 help，不进入 agent prompt。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram commands`
- [ ] fake API 验证 command menu payload。

**Dependencies:** Task 1, Task 4

**Files likely touched:**
- `crates/noloong-agent-telegram/src/input.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`
- `src/main.rs`

**Estimated scope:** M

#### Task 11: Implement session and profile cockpit

**Description:** 让 Telegram 私聊可管理多个 sessions 与 profiles：创建新会话、切换 active session、查看当前状态、列出历史 sessions，并提供删除确认。

**Acceptance criteria:**
- [ ] `/profiles` 列出 profiles，并可用按钮切换默认 active profile。
- [ ] `/new` 创建新 session 并设为 active。
- [ ] `/sessions` 列出当前 chat 的 sessions，按钮可 switch/delete。
- [ ] `/status` 显示 active session profile、status、queues、manifest summary。
- [ ] session delete 必须二次确认，且 running session 需要 force abort 确认。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram session bridge`
- [ ] fake interaction 覆盖 profile/list、session/create/list/get/delete。

**Dependencies:** Task 10

**Files likely touched:**
- `crates/noloong-agent-telegram/src/session.rs`
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/display.rs`

**Estimated scope:** M

#### Task 12: Implement run and queue cockpit

**Description:** 暴露 run control 与 queue control。用户可 `/continue`、`/abort`、查看 follow-up/steering queue、清空或编辑队列中的用户输入。

**Acceptance criteria:**
- [ ] `/continue` 调用 `agent/continue`。
- [ ] `/abort` 调用 `agent/abort`，running 时需要确认。
- [ ] `/queue` 显示 steering/follow-up 队列摘要。
- [ ] queue card 支持 clear、set mode、将当前用户消息作为 follow-up。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram queue`
- [ ] fake interaction 覆盖 `queue/list`、`queue/clear`、`queue/set_mode`、`agent/continue`、`agent/abort`。

**Dependencies:** Task 11

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/display.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`

**Estimated scope:** M

#### Task 13: Implement process cockpit

**Description:** 将后台命令能力接入 Telegram：查看 jobs、读取输出、等待完成、写 stdin、终止进程。超长输出以 document 回传，短输出以卡片展示。

**Acceptance criteria:**
- [ ] `/processes` 调用 `process/list` 并显示 job status。
- [ ] `/process <job_id>` 显示 read/wait/write/terminate 按钮。
- [ ] `process/read` 支持增量读取和 max bytes。
- [ ] terminate/write 等操作必须通过 confirmation callback。
- [ ] 超长 stdout/stderr 作为 text file document 回传。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram process`
- [ ] fake interaction 覆盖 list/read/wait/write/terminate。

**Dependencies:** Task 11

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`

**Estimated scope:** M

#### Task 14: Implement manifest and subagent cockpit

**Description:** 暴露 manifest proposal 与 subagent spawn 能力。Telegram 侧只做安全 UI，不让用户直接编辑完整 manifest JSON。

**Acceptance criteria:**
- [ ] `/manifest` 显示 system prompt summary、enabled tools、pending proposals。
- [ ] pending proposal 可 approve，apply approved 必须确认。
- [ ] `/subagent` 可基于 active session spawn 子会话，并可带 initial prompt。
- [ ] subagent session 自动加入当前 chat 的 session list。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram manifest subagent`
- [ ] fake interaction 覆盖 `manifest/*` 与 `subagent/spawn`。

**Dependencies:** Task 11

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/session.rs`
- `crates/noloong-agent-telegram/src/i18n.rs`

**Estimated scope:** M

### Checkpoint: Cockpit Commands

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] Manual smoke：`/status`、`/new`、`/sessions`、`/queue`、`/processes`、`/manifest`、`/subagent` 都能在私聊中完成一条真实路径。

### Phase 5: Polish, Docs, and Live Verification

#### Task 15: Harden Telegram UX details

**Description:** 处理移动端体验细节：MarkdownV2 fallback、按钮 callback 过期、重复点击、message not modified、rate limit、错误卡片、命令帮助、i18n 完整性。

**Acceptance criteria:**
- [ ] 所有用户可见字符串都来自 `TelegramUiCatalog`。
- [ ] callback data 长度始终 <= 64 bytes。
- [ ] duplicate callback 不会重复执行危险操作。
- [ ] Telegram API 429 按 `retry_after` 退避。
- [ ] Markdown parse error fallback 保留按钮和重要内容。

**Verification:**
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] `rg -n '\"[^\"]*[\\u4e00-\\u9fff]|Run failed|Tool started|Approval' crates/noloong-agent-telegram/src`

**Dependencies:** Tasks 9-14

**Files likely touched:**
- `crates/noloong-agent-telegram/src/i18n.rs`
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/approval.rs`

**Estimated scope:** M

#### Task 16: Update docs and examples

**Description:** 更新 Telegram docs、interaction docs 和示例配置，明确 Agent Cockpit 能力、文件策略、安全默认值、长任务体验、命令列表和 live testing SOP。

**Acceptance criteria:**
- [ ] `TELEGRAM.md` 不再描述 V1 缺少 media/file/profile/process 等能力。
- [ ] docs 说明 personal-first、long-polling-first、hybrid file policy。
- [ ] 示例配置包含 Telegram file policy 和 startup update policy。
- [ ] 文档给出 ChatGPT subscription 与 OpenRouter free 两条 smoke 命令。

**Verification:**
- [ ] `rg -n "Telegram Agent Cockpit|filePolicy|startupUpdatePolicy|/processes|/manifest" crates/noloong-agent-telegram/docs crates/noloong-agent/docs examples/profile-configs`

**Dependencies:** Task 15

**Files likely touched:**
- `crates/noloong-agent-telegram/docs/TELEGRAM.md`
- `crates/noloong-agent/docs/INTERACTION.md`
- `examples/profile-configs/*.json*`

**Estimated scope:** S

#### Task 17: Full verification and live smoke

**Description:** 运行完整本地验证，并用测试 bot 做真实 Telegram smoke。真实 smoke 覆盖文字、多媒体、文件、审批、后台 process、commands 和最终输出。

**Acceptance criteria:**
- [ ] format check 通过。
- [ ] clippy 无 warning。
- [ ] workspace tests 通过。
- [ ] Telegram live smoke 通过：文本、图片、文档、语音、视频、审批按钮、process card、文件回传。
- [ ] ChatGPT subscription profile 与 OpenRouter free profile 都至少完成一次私聊 prompt。

**Verification:**
- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features`
- [ ] `cargo test --workspace`
- [ ] Manual Telegram live smoke with test bot credentials.

**Dependencies:** Task 16

**Files likely touched:**
- None, unless verification finds defects.

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Telegram Bot API rate limit / edit throttling | High | 保留 edit throttle，429 使用 `retry_after`，流式预览和最终消息分离。 |
| 文件下载放大磁盘和内存 | High | hybrid file policy、最大下载大小、受控目录、后续清理任务。 |
| 全 authority 暴露危险操作 | High | 交互层 policy 仍可限制；Telegram 端 destructive/process/manifest/session delete 全部二次确认。 |
| 多 session 路由错发 | Medium | active session 显式存储；display delivery 只按 session id -> Telegram route 投递。 |
| MarkdownV2 解析失败导致消息丢失 | Medium | 维持 plain text fallback，并确保 fallback 保留 reply markup。 |
| 群组行为未优化 | Medium | 第一阶段明确 personal-first；群组只保持现有 mention/thread gating。 |

## Parallelization Opportunities

- Task 1-3 必须优先顺序完成。
- Task 4-6 与 Task 7 可在 Task 1 后部分并行，但共享 `TelegramApi` contract 需先冻结。
- Task 11-14 可在 Task 10 完成后并行，写入范围需要按 session/queue/process/manifest 分开。
- Task 15-17 必须最后统一收口。

## Not Doing

- 不实现 webhook；long polling 先做到稳定、可恢复、可观测。
- 不实现 Telegram Mini App；复杂 UI 先用 command + inline keyboard + cards 完成。
- 不把 Telegram bot token、model credentials 或 provider config 放进 Telegram bridge。
- 不优先优化群组协作、多用户审批或 topic 自动管理。
