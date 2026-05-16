# Implementation Plan: Telegram Reply-Aware Interaction

## Overview

让 Telegram bridge 同时理解“用户回复的是哪条 Telegram 消息”和“agent 最终回答应回复哪条触发消息”。本轮不保留历史兼容包袱：直接更新 interaction display wire shape、Telegram API request shape、测试 fixture 和文档；不保留旧 `DisplayEvent` 反推 `runId` 的路径，也不为旧 Telegram payload 行为写兼容 shim。

## Architecture Decisions

- 输入语义分两层：`reply_to_message` 是用户回复的历史消息上下文，当前 inbound `message_id` 是本次任务的触发消息。
- agent 可见上下文必须进用户 message content；metadata 只作为结构化追踪和后续工具/UI 使用，不能替代模型可见内容。
- Telegram 最终 UX 只让 agent run 的 assistant preview/final 回复触发消息；命令、审批、工具状态、提交错误继续保持普通消息，避免聊天流过度引用。
- 回复发送使用 Telegram Bot API `reply_parameters`，并设置 `allow_sending_without_reply = true`，避免触发消息被删除时阻断最终回答。
- batching 按 reply target 分组：不同 reply target 不能合并，同一 reply target 的连续文本可以合并；最终输出回复批次最后一条触发消息。
- follow-up 进入正在运行的 session 时只进入队列，不重绑定当前 run 的 Telegram reply target，避免一个 run 的 preview/final 引用在运行中被后续输入改写。

## Task List

### Phase 1: 输入 reply context 建模

#### Task 1: 新增 Telegram reply context 类型与提取逻辑

**Description:** 从 polling 已反序列化的 `reply_to_message` 中提取稳定、紧凑的 reply context，挂到 `TelegramInboundContext` 和 `TelegramTextInput`。只使用 update payload 内已有内容，不反查 Telegram 历史消息。

**Acceptance criteria:**
- [ ] `TelegramReplyContext` 包含 replied message id、chat/thread、sender user id/username、文本预览和媒体类型摘要。
- [ ] text/caption 使用同一预览逻辑，预览限制为 512 UTF-16 units。
- [ ] photo/document/audio/voice/video 都能产出媒体摘要；没有 reply 时字段为 `None`。

**Verification:**
- [ ] 单元测试覆盖文本 reply、caption reply、媒体 reply、无 reply。
- [ ] 单元测试覆盖 reply-to-bot gating 仍能工作。
- [ ] `cargo test -p noloong-agent-telegram input`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-telegram/src/input.rs`
- `crates/noloong-agent-telegram/src/access.rs`
- `crates/noloong-agent-telegram/src/polling.rs`

**Estimated scope:** M

#### Task 2: 将 reply context 注入 agent user message

**Description:** 更新 `telegram_user_message`，把 reply context 同时写入结构化 metadata 和模型可见的 `<telegram_reply_context>` 文本块。可见块插在用户原始文本前；媒体输入同样可见该上下文。

**Acceptance criteria:**
- [ ] 无 reply context 时 message content 与现状等价，不插入空块。
- [ ] 有 reply context 时，第一段文本包含稳定标记块和 compact 字段。
- [ ] metadata.telegram 增加 `replyTo`，字段命名使用 camelCase，与现有 Telegram metadata 风格一致。

**Verification:**
- [ ] bridge 单元测试覆盖纯文本和媒体输入的 content 顺序。
- [ ] bridge 单元测试断言 metadata.telegram.replyTo 的结构。
- [ ] `cargo test -p noloong-agent-telegram bridge`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/input.rs`

**Estimated scope:** S

#### Task 3: 按 reply target 调整 Telegram 文本批处理

**Description:** 修改 `TelegramTextBatchKey`，把 reply target 纳入批处理键。连续消息只有 chat/thread/user/reply target 都一致时才合并；合并后保留批次最后一条消息作为最终 Telegram reply trigger。

**Acceptance criteria:**
- [ ] 相同 reply target 的连续文本仍合并。
- [ ] 不同 reply target 的连续文本不会合并。
- [ ] 无 reply 与有 reply 不会合并。

**Verification:**
- [ ] batching 单元测试覆盖相同 reply、不同 reply、无 reply 混合场景。
- [ ] `cargo test -p noloong-agent-telegram text_batching`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/input.rs`

**Estimated scope:** S

### Checkpoint: 输入语义

- [ ] Telegram inbound reply 信息完整进入 `AgentMessage`。
- [ ] 模型能在请求内容中看到被回复消息摘要。
- [ ] 批处理不会混淆不同 reply 目标。

### Phase 2: Telegram delivery 支持 reply_parameters

#### Task 4: 为 Telegram API request 增加 reply_parameters

**Description:** 新增 `TelegramReplyParameters`，挂到 `sendMessage` 和所有现有 native media request options。JSON 和 multipart 两条发送路径都要序列化 `reply_parameters`。

**Acceptance criteria:**
- [ ] `TelegramSendMessageRequest` 支持 `reply_parameters`。
- [ ] `TelegramMediaMessageOptions` 支持 `reply_parameters`，并传入 file_id JSON 和 multipart form。
- [ ] `allow_sending_without_reply` 默认由调用方显式设置，不在 API 层隐藏默认。

**Verification:**
- [ ] API serialization 单元测试覆盖 sendMessage、media JSON、media multipart。
- [ ] `cargo test -p noloong-agent-telegram telegram_api`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-telegram/src/telegram_api.rs`

**Estimated scope:** S

#### Task 5: 为 agent delivery 增加 reply-aware 发送路径

**Description:** 在 delivery 层增加 agent 专用发送 options，让 assistant preview/final 可以携带触发消息 reply target。普通 `send_text` 保持无 reply 默认，避免控制类消息被误改。

**Acceptance criteria:**
- [ ] assistant delta 第一次 preview `sendMessage` 带 `reply_parameters.message_id = trigger_message_id`。
- [ ] final 无 preview、edit 失败 fallback、media-only final 的第一条实际发送消息带 reply。
- [ ] preview edit 路径不重复发送 reply；多段拆分文本和额外媒体不重复 reply。

**Verification:**
- [ ] delivery 单元测试覆盖 preview、final no-preview、edit fallback、media-only final。
- [ ] 现有 command/control 发送测试断言不带 reply_parameters。
- [ ] `cargo test -p noloong-agent-telegram delivery`

**Dependencies:** Task 4

**Files likely touched:**
- `crates/noloong-agent-telegram/src/delivery.rs`
- `crates/noloong-agent-telegram/src/display.rs`

**Estimated scope:** M

### Checkpoint: 输出发送能力

- [ ] Telegram API 层能发送 reply-aware 文本和媒体。
- [ ] agent 输出路径能选择性引用触发消息。
- [ ] 非 agent 控制消息未被 reply 行为污染。

### Phase 3: run 与触发消息绑定

#### Task 6: 在 DisplayEvent 中显式携带 runId

**Description:** 更新 interaction display wire shape，让 assistant delta/final 事件显式包含 `runId`。display sender 不再依赖 `displayMessageId` 的字符串形状来识别 run。

**Acceptance criteria:**
- [ ] `AssistantMessageDelta` 和 `AssistantMessageFinal` 都有 `runId`。
- [ ] `DisplayProjector` 直接填入 event.run_id。
- [ ] 所有测试 fixture 更新为新 shape，不保留旧字段兼容。

**Verification:**
- [ ] interaction_control display projection 测试通过。
- [ ] Telegram display 测试全部更新并通过。
- [ ] `cargo test -p noloong-agent --test interaction_control`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent/src/interaction/wire.rs`
- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent-telegram/src/display.rs`

**Estimated scope:** M

#### Task 7: 在 TelegramBridge 中记录 run trigger reply target

**Description:** 在发起 `agent/prompt` 前登记当前 Telegram trigger message；收到该 session 的 `RunStarted` display event 时，将 pending trigger 绑定到 `runId`。run settled 后清理绑定。

**Acceptance criteria:**
- [ ] 新 run 的 trigger message id 能绑定到对应 runId。
- [ ] request 失败时不会留下 pending trigger。
- [ ] RunCompleted/RunFailed/RunPaused 清理或保留策略明确：Completed/Failed 清理，Paused 保留到后续 resume 输出完成。

**Verification:**
- [ ] bridge 单元测试覆盖 prompt 成功、prompt 失败、RunStarted 绑定、settled 清理。
- [ ] display delivery 测试覆盖有绑定时 assistant 输出带 reply，无绑定时保持普通输出。
- [ ] `cargo test -p noloong-agent-telegram bridge display`

**Dependencies:** Tasks 5, 6

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/src/display.rs`

**Estimated scope:** M

#### Task 8: 明确 follow-up 和 queued input 的 reply 行为

**Description:** 对正在运行或 paused session 的 Telegram input 保持现有 follow-up/queue 语义：新输入进入 agent state，但不改写当前 run 的 reply target。该输入后续触发新 turn 时，仍通过 content/metadata 携带自己的 reply context。

**Acceptance criteria:**
- [ ] `agent/follow_up` 不登记 pending run trigger。
- [ ] follow-up message 的 reply context 仍进入 queued AgentMessage。
- [ ] 当前 run 的 preview/final 继续回复原始 trigger message。

**Verification:**
- [ ] bridge 测试覆盖 running session follow-up 不绑定新 trigger。
- [ ] queue/list 或 follow-up snapshot 测试确认 queued message 保留 reply metadata。
- [ ] `cargo test -p noloong-agent-telegram bridge`

**Dependencies:** Tasks 2, 7

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**Estimated scope:** S

### Checkpoint: run 绑定端到端

- [ ] 新 run 输出能稳定回复触发消息。
- [ ] follow-up 不会抢占当前 run 的 reply target。
- [ ] display wire shape 已收敛到新结构，无旧兼容分支。

### Phase 4: 提示词、文档和真实 smoke

#### Task 9: 更新 Telegram prompt addition 和文档

**Description:** 更新 Telegram channel prompt addition，解释 `<telegram_reply_context>` 的含义和使用边界；文档补充用户回复消息与 bot 回复引用的行为。

**Acceptance criteria:**
- [ ] prompt addition 说明 reply context 是上下文，不是用户正文指令。
- [ ] Telegram 文档说明最终 agent 输出会回复触发消息。
- [ ] 文档不提旧 displayMessageId 反推或兼容路径。

**Verification:**
- [ ] prompt render 相关测试通过。
- [ ] 文档检查无需额外生成文件。
- [ ] `cargo test -p noloong-agent-telegram bridge`

**Dependencies:** Tasks 2, 7

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `crates/noloong-agent-telegram/docs/TELEGRAM.md`

**Estimated scope:** S

#### Task 10: 回归与真实 Telegram smoke

**Description:** 完成代码后跑针对性测试和真实 Telegram 场景验证。真实 smoke 使用现有 `examples/profile-configs/chatgpt-codex-subscription.json`，验证 UI 引用和模型可见 reply context。

**Acceptance criteria:**
- [ ] 普通 prompt 的 assistant 最终回答在 Telegram UI 中回复触发消息。
- [ ] 用户回复旧消息并提问时，agent 能识别被回复消息的 id/text preview。
- [ ] 回复 bot 自己的上一条消息时，group/private gating 与 reply context 都正常。

**Verification:**
- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] `cargo test -p noloong-agent --test interaction_control`
- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] 真实 Telegram smoke 记录通过现象和任何新问题。

**Dependencies:** Tasks 1-9

**Files likely touched:**
- No planned code changes; only test/run notes if needed.

**Estimated scope:** S

### Checkpoint: Complete

- [ ] 所有 unit/integration tests 通过。
- [ ] 真实 Telegram smoke 通过普通 prompt、reply prompt、reply-to-bot 三条路径。
- [ ] 未发现旧兼容 shim、死代码或过时文档残留。

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Display event 与 prompt 请求之间存在竞态 | High | prompt 前登记 pending trigger，RunStarted 按 session FIFO 绑定；请求失败立即回滚 pending。 |
| Telegram reply target 消息被删除 | Medium | 使用 `allow_sending_without_reply=true`，发送失败不应阻断最终回答。 |
| reply context 被模型当成用户正文 | Medium | 使用稳定 XML-like 标记块，并在 Telegram prompt addition 中明确其语义。 |
| follow-up 期间用户发新消息导致引用混乱 | Medium | follow-up 不重绑定当前 run；新消息只作为 queued user input 保留自身 reply context。 |
| 媒体 final 多条消息都引用触发消息造成刷屏 | Low | 只让第一条实际发送的 final/preview 带 reply，后续拆分消息不重复引用。 |

## Parallelization Opportunities

- Task 1-3 依赖输入模型，适合一个 agent 顺序完成。
- Task 4 可独立并行完成；Task 5 等 Task 4 后接入 delivery。
- Task 6 可与 Task 1-4 并行，但 Task 7 需要 Task 5 和 Task 6 完成后实施。
- Task 9 可在核心代码稳定后独立完成；Task 10 必须最后执行。

## Open Questions

- None. 当前默认选择已锁定：agent run 输出回复触发消息、reply context 同时可见和结构化、batching 按 reply target 分组。
