# Implementation Plan: Interaction Store 与 Client State 系统化整理

## Overview

本轮把交互客户端遗留的系统性状态问题收拢成两个清晰边界：`interaction/store` 负责可索引地查 session，`client state` 负责 Telegram/Weixin 这类客户端自己的持久状态。目标是删除全量 `session/list` 扫描、清理 Telegram 文件 offset 旧路径、让 Telegram/Weixin 共用统一 SQLite state，并只对 Weixin typing/dedup 做轻量运行态缓存优化。

## Architecture Decisions

- `AgentSessionListFilter` 增加顶层 metadata 精确匹配，查询值只支持 `string/number/bool` 标量；不做 JSON path、模糊匹配或范围查询。
- Registry store trait 改成 store 侧接收 session filter；registry 仍负责合并 loaded runtime session，并使用同一套过滤语义兜底。
- 新增统一 `ClientStateStore`，放在 host/client 层，不下沉到 `noloong-agent-core`。
- Telegram offset 和 Weixin sync/context/active session 全部迁入 `ClientStateStore`；删除 Telegram 文件 checkpoint 配置和旧 offset store。
- Weixin typing ticket 与 dedup 只做局部内存缓存，不抽通用 runtime cache 框架。
- 项目无兼容性负担：旧配置、旧路径和不再需要的类型直接删除。

## Dependency Graph

```text
Session metadata filter contract
    ├── Memory/SQL/Object store filtered list
    │       └── Registry + interaction JSON-RPC session/list
    │               ├── Weixin session query
    │               └── Telegram session query
    │
ClientStateStore contract
    ├── SQLite implementation
    │       ├── Weixin state facade
    │       └── Telegram offset facade
    │
Weixin runtime cache
    ├── typing ticket cache
    └── dedup cleanup cadence
```

## Task List

### Phase 1: Session Metadata Filter Foundation

#### Task 1: 定义 metadata exact-match filter 合约

**Description:** 扩展 session list filter 的公开 wire shape，新增 `metadataEquals` 字段，并把合法查询值限制为顶层标量。这个任务只定义类型、serde 和 validation，不改 store 查询实现。

**Acceptance criteria:**
- [x] `AgentSessionListFilter` 包含 `metadata_equals: Map<String, Value>`，serde 字段名为 `metadataEquals`。
- [x] validation 拒绝 object、array、null 类型的 metadata 查询值。
- [x] 空 `metadataEquals` 与未设置等价，不改变现有查询结果。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_registry metadata`
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_control session_list`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** M

**Implementation status:** Done. `AgentSessionListFilter` now exposes `metadataEquals`, validates scalar-only values, and treats empty metadata filters as no-op.

#### Task 2: 将 registry store session list 改为接收 filter

**Description:** 修改 `AgentSessionRegistryStore` 的 `list` 接口，让 store 层拿到 `AgentSessionListFilter`。Registry 对 stored records 使用 store 返回结果，对 loaded sessions 继续内存过滤并覆盖同 id descriptor。

**Acceptance criteria:**
- [x] Store trait 的 `list` 接口接收 `AgentSessionListFilter` 或等价内部 filter。
- [x] Registry 不再调用无参 `store.list()` 获取全量 session。
- [x] Loaded runtime session 与 stored session 对 parent/profile/status/metadata 使用一致过滤语义。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_registry registry_filters`
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_control interaction_control_initializes_and_lists_profiles`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/traits.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** M

**Implementation status:** Done. Registry store `list` now receives the filter, and registry applies the same fallback filtering for loaded runtime sessions.

#### Task 3: 实现 memory store filtered list

**Description:** 先在 in-memory store 里实现完整过滤语义，作为后续 SQL/Object store 的行为参考。

**Acceptance criteria:**
- [x] Memory store 按 parent/profile/status/metadata exact-match 返回 session records。
- [x] 返回结果按 `session_id` 稳定排序。
- [x] metadata value 使用 JSON 值语义比较，数字和字符串不互相匹配。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_registry in_memory`
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_registry metadata`

**Dependencies:** Tasks 1, 2

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/memory.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** S

**Implementation status:** Done. Memory store applies parent/profile/status/metadata filtering with stable session-id ordering.

### Checkpoint: Filter Contract

- [x] `cargo fmt -p noloong-agent --check`
- [x] `cargo test -p noloong-agent --test interaction_registry`
- [x] `cargo test -p noloong-agent --test interaction_control`

**Implementation status:** Done. Covered again by `cargo fmt --all --check` and full workspace regression.

### Phase 2: Durable Store Indexes

#### Task 4: 实现 SQL session metadata index

**Description:** 给 SQL registry store 增加 session metadata 索引表，insert/save/remove session 时同步维护，list 时优先用列过滤和 metadata index 缩小候选，再 decode record JSON。

**Acceptance criteria:**
- [x] SQL schema 包含 session metadata index，至少字段为 `session_id`、`key`、`value_json`。
- [x] `insert` 写入 session record 后同步写入顶层 metadata 标量索引。
- [x] `save` 更新 metadata 时清理旧索引并写入新索引。
- [x] `remove` 删除 session 时清理对应索引。
- [x] `list(filter)` 不需要反序列化全量 session JSON 才能应用 metadata filter。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [x] Tests pass: `cargo test -p noloong-agent --features registry-store-postgres --test interaction_registry_store_postgres`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/sql.rs`
- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`
- `crates/noloong-agent/tests/interaction_registry_store_postgres.rs`

**Estimated scope:** M

**Implementation status:** Done. SQL store maintains a scalar metadata index and uses indexed candidate filtering before record decode.

#### Task 5: 实现 Object store session metadata index

**Description:** 给 OpenDAL object store 增加轻量 session metadata index。查询时从 index 得到候选 session id，再读取候选 record 做一致性校验；如果 index stale，以 record 为准并在后续 save/remove 时修正。

**Acceptance criteria:**
- [x] Object store 为每个标量 metadata 建可列举的 index 路径。
- [x] `insert/save/remove` 同步维护 metadata index，并清理旧值。
- [x] `list(filter)` 对 metadata 查询只读取候选 records，不读取全部 session records。
- [x] stale index 不返回错误 record。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**Estimated scope:** M

**Implementation status:** Done. Object store maintains session summary and metadata candidate indexes, then validates candidate records to tolerate stale index entries.

#### Task 6: 更新 interaction JSON-RPC session/list 行为

**Description:** 确认 `session/list` 从 JSON-RPC 到 registry 到 store 都能透传 `metadataEquals`，并补控制面测试。

**Acceptance criteria:**
- [x] JSON-RPC `session/list` 接收 `metadataEquals`。
- [x] 非法 metadata 查询值返回 structured invalid params，不触发 store 查询。
- [x] session descriptor 的 metadata 与 filter 精确匹配时才返回。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_control session_list`
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_jsonrpc`

**Dependencies:** Tasks 4, 5

**Files likely touched:**
- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**Estimated scope:** S

**Implementation status:** Done. JSON-RPC `session/list` passes `metadataEquals` through validation and returns structured invalid params for unsupported values.

### Checkpoint: Store Indexes

- [x] `cargo fmt -p noloong-agent --check`
- [x] `cargo test -p noloong-agent --test interaction_registry`
- [x] `cargo test -p noloong-agent --test interaction_control`
- [x] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [x] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`

**Implementation status:** Done. SQLite/object store tests both cover metadata-filtered session listing.

### Phase 3: Unified Client State Store

#### Task 7: 新增 ClientStateStore 抽象和 SQLite 实现

**Description:** 新增统一客户端状态存储，使用同一个 state database，支持按 client/account/scope/key 读写 typed JSON value。SQLite schema 只初始化一次，后续操作不重复建表。

**Acceptance criteria:**
- [x] `ClientStateStore` 支持 get/set/delete JSON value。
- [x] Key 结构包含 `client`、`account`、`scope`、`key`。
- [x] SQLite store 构建时初始化 schema，单次操作不重复 `CREATE TABLE IF NOT EXISTS`。
- [x] 不把 ChatGPT token、Weixin credential 放入该 store。

**Verification:**
- [x] Tests pass: `cargo test -p noloong client_state`
- [x] Tests pass: `cargo test -p noloong-agent-telegram client_state`
- [x] Tests pass: `cargo test -p noloong-agent-weixin state`

**Dependencies:** None

**Files likely touched:**
- `src/config.rs`
- `src/main.rs`
- shared client-state module path chosen during implementation

**Estimated scope:** M

**Implementation status:** Done. Added host-side `ClientStateStore` plus SQLite implementation under `noloong-agent`, shared by Telegram and Weixin.

#### Task 8: 迁移 Weixin state facade 到 ClientStateStore

**Description:** 保留 Weixin 业务层需要的 typed facade，但底层不再自己维护独立 SQLite schema。`sync_buf`、`context_token`、active session id 全部映射到统一 client state。

**Acceptance criteria:**
- [x] Weixin state facade 使用 `ClientStateStore` 实现。
- [x] `sync_buf` 按 account fingerprint 隔离。
- [x] `context_token` 和 active session id 按 peer/chat kind 隔离。
- [x] 删除 Weixin state 中重复的 SQLite path/schema/open connection 逻辑。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-weixin state`
- [x] Tests pass: `cargo test -p noloong-agent-weixin bridge_restores_persisted_active_weixin_session`

**Dependencies:** Task 7

**Files likely touched:**
- `crates/noloong-agent-weixin/src/state.rs`
- `crates/noloong-agent-weixin/src/runtime.rs`
- `crates/noloong-agent-weixin/src/bridge.rs`

**Estimated scope:** M

**Implementation status:** Done. Weixin sync buffer, context token, and active session state now map onto unified client state while credentials remain separate.

#### Task 9: 迁移 Telegram offset 到 ClientStateStore

**Description:** 删除 Telegram 专属 file/sqlite offset store，改为通过统一 client state 保存 bot offset。CLI 不再暴露 `--telegram-offset-checkpoint` 和 `TELEGRAM_OFFSET_CHECKPOINT`。

**Acceptance criteria:**
- [x] Telegram poller 仍通过小 trait 读写 offset，但默认实现来自 `ClientStateStore`。
- [x] 删除 `FileTelegramOffsetStore`、`SqliteTelegramOffsetStore` 和对应 SQLite helper。
- [x] 删除 `--telegram-offset-checkpoint` CLI 参数和 `TELEGRAM_OFFSET_CHECKPOINT` env。
- [x] 启动时 checkpoint 恢复、skip pending 策略保持当前行为。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-telegram polling`
- [x] Tests pass: `cargo test -p noloong telegram_offset`
- [x] Tests pass: `cargo test -p noloong tests::cli_telegram_embeds_loopback_interaction_options`

**Dependencies:** Task 7

**Files likely touched:**
- `crates/noloong-agent-telegram/src/polling.rs`
- `src/main.rs`
- `src/config.rs`

**Estimated scope:** M

**Implementation status:** Done. Telegram offset now uses `ClientStateTelegramOffsetStore`; file/sqlite-specific offset stores and CLI/env checkpoint paths were removed.

### Checkpoint: Unified State

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent-telegram`
- [x] `cargo test -p noloong-agent-weixin`
- [x] `cargo test -p noloong telegram`
- [x] `cargo test -p noloong weixin`

**Implementation status:** Done. Full workspace regression covers the root Telegram/Weixin command tests.

### Phase 4: Client Query Integration

#### Task 10: Weixin 使用 metadata filter 查询 session

**Description:** 将 Weixin 会话恢复、列出、切换候选查询改为 `session/list` 的 metadata filter，不再拉全量 session 后本地过滤。

**Acceptance criteria:**
- [x] Weixin `list_sessions_for_chat` 使用 `metadataEquals` 查询 `channel/accountId/peerId/chatKind`。
- [x] 默认 deterministic `session/get` fast path 保留。
- [x] Weixin fake interaction 测试确认请求参数包含 metadata filter。
- [x] Weixin bridge 不再为了列出当前 peer session 调用空参数 `session/list`。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-weixin bridge`
- [x] Tests pass: `cargo test -p noloong-agent --test interaction_control session_list`

**Dependencies:** Task 6

**Files likely touched:**
- `crates/noloong-agent-weixin/src/bridge.rs`
- `crates/noloong-agent-weixin/src/runtime.rs`

**Estimated scope:** S

**Implementation status:** Done. Weixin bridge queries direct chat sessions with `metadataEquals` and retains deterministic get/ownership checks.

#### Task 11: Telegram 使用 metadata filter 查询 session

**Description:** 将 Telegram 当前 chat/thread 的 session 查询改为 metadata filter，避免未来多客户端或大量 session 时全量扫描。

**Acceptance criteria:**
- [x] Telegram session 查询使用 `metadataEquals` 查询 `channel/chatId`。
- [x] `threadId` 只在线程存在时加入 filter。
- [x] 旧的 session ownership 校验保留，用于防止 stale/错误 record。
- [x] 相关 fake interaction 测试确认 `session/list` 带 filter。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-telegram bridge`
- [x] Tests pass: `cargo test -p noloong telegram_sessions_command`

**Dependencies:** Task 6

**Files likely touched:**
- `crates/noloong-agent-telegram/src/bridge.rs`
- `src/main.rs`

**Estimated scope:** S

**Implementation status:** Done. Telegram bridge queries by channel/chat/thread metadata and keeps ownership validation for stale records.

### Checkpoint: Client Query Paths

- [x] `cargo test -p noloong-agent-telegram`
- [x] `cargo test -p noloong-agent-weixin`
- [x] `cargo test -p noloong-agent --test interaction_control`
- [x] `cargo test -p noloong-agent --test interaction_registry`

**Implementation status:** Done. Client query paths are covered by both crate-level and registry/control tests.

### Phase 5: Weixin Runtime Cache Cleanup

#### Task 12: Weixin typing ticket 轻量缓存

**Description:** 给 Weixin delivery 增加 peer/context-token scoped typing ticket cache。RunStarted 获取并缓存 ticket；terminal stop 优先复用缓存，缓存缺失时跳过 stop，不能阻塞最终消息。

**Acceptance criteria:**
- [x] typing cache key 包含 peer id 和当前 context token。
- [x] cache 有短 TTL，并在 session expired/stale token 时失效。
- [x] terminal final/fail/pause/completed 不等待 `get_config` 才发送用户可见消息。
- [x] `get_config` 失败只影响 typing，不影响最终消息发送。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-weixin display`
- [x] Tests pass: `cargo test -p noloong-agent-weixin delivery typing`

**Dependencies:** Task 8

**Files likely touched:**
- `crates/noloong-agent-weixin/src/delivery.rs`
- `crates/noloong-agent-weixin/src/display.rs`

**Estimated scope:** S

**Implementation status:** Done. Weixin delivery caches typing tickets by peer/context token with TTL and skips uncached stop calls instead of blocking final output.

#### Task 13: Weixin dedup 清理节流

**Description:** 优化 `MessageDeduplicator`，避免每条消息都对整个 seen map 做 `retain`。改成按 `next_cleanup_at` 间隔清理，行为保持 TTL 去重。

**Acceptance criteria:**
- [x] 每条消息只做 key lookup/insert，清理只在到达 cleanup deadline 后执行。
- [x] TTL 语义不变，过期 key 会被清理并允许重新接收。
- [x] 测试覆盖高频重复消息不会触发每条清理。

**Verification:**
- [x] Tests pass: `cargo test -p noloong-agent-weixin polling dedup`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-weixin/src/polling.rs`

**Estimated scope:** XS

**Implementation status:** Done. Weixin dedup now does key-specific expiry and throttles full cleanup by deadline.

### Checkpoint: Runtime Cache

- [x] `cargo test -p noloong-agent-weixin`
- [x] `cargo clippy -p noloong-agent-weixin --all-targets -- -D warnings`

**Implementation status:** Done. Weixin tests and clippy pass with the runtime cache changes.

### Phase 6: Cleanup, Docs, Regression

#### Task 14: 清理旧配置、文档和示例

**Description:** 删除过时 Telegram offset checkpoint 文档和配置引用，更新 README 和 Weixin/Telegram 运行说明，明确统一 state database 是唯一持久状态路径。

**Acceptance criteria:**
- [x] README 不再提 `TELEGRAM_OFFSET_CHECKPOINT` 或 file checkpoint。
- [x] CLI help 不再出现 `--telegram-offset-checkpoint`。
- [x] 文档说明 Telegram offset、Weixin sync/context/active session 都在 unified state DB。
- [x] 示例 profile/config 不包含旧字段。

**Verification:**
- [x] Tests pass: `cargo test -p noloong schema`
- [x] Manual check: `cargo run -- telegram --help` 不出现 offset checkpoint。

**Dependencies:** Tasks 8, 9

**Files likely touched:**
- `README.md`
- `crates/noloong-agent-weixin/docs/WEIXIN.md`
- `src/main.rs`

**Estimated scope:** S

**Implementation status:** Done. Removed old Telegram checkpoint CLI/env config and docs, renamed startup policy to `skip_pending_without_offset`, and verified CLI help has no checkpoint/offset option.

#### Task 15: 全量回归与审查收尾

**Description:** 跑完整回归，确认 store/state 改造没有破坏 automation、subagent、Telegram、Weixin 和 profile schema。最后整理计划实现状态，准备提交前审查。

**Acceptance criteria:**
- [x] 所有 checkpoint 命令通过。
- [x] `cargo test --workspace` 通过。
- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings` 通过。
- [x] `cargo clippy -p noloong-agent-telegram --all-targets -- -D warnings` 通过。
- [x] `cargo clippy -p noloong-agent-weixin --all-targets -- -D warnings` 通过。
- [x] `git diff --check` 通过。

**Verification:**
- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace`
- [x] `git diff --check`

**Dependencies:** Tasks 1-14

**Files likely touched:**
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

**Implementation status:** Done. Full regression, clippy, and diff whitespace checks pass.

## Parallelization Opportunities

- Task 4 and Task 5 can be parallelized after Task 3 because SQL/Object store indexes share the same contract but write to disjoint files.
- Task 8 and Task 9 can be parallelized after Task 7 because Weixin and Telegram facades are independent clients over the same state interface.
- Task 10 and Task 11 can be parallelized after Task 6 because Weixin and Telegram query paths touch different crates.
- Task 12 and Task 13 can be parallelized after Task 8 because they are independent Weixin runtime improvements.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Object store metadata index path design becomes too complex | Medium | Keep exact-match scalar only; read candidate records to validate index truth. |
| SQL metadata index drifts from record JSON | Medium | `save` always deletes old rows then rewrites from record metadata; tests cover metadata update/removal. |
| Deleting Telegram file checkpoint surprises local smoke runs | Low | No compatibility burden; document unified state DB and allow clearing DB in test workflows. |
| ClientStateStore ownership boundary grows too broad | Medium | Explicitly exclude credentials/tokens; only store client runtime state. |
| Typing cache uses stale ticket | Low | TTL is short; stale/session-expired errors invalidate cache and never block final delivery. |

## Not Doing

- 不做 JSON path metadata 查询、模糊查询、范围查询。
- 不做通用 runtime cache framework。
- 不迁移 ChatGPT token、Weixin login credentials 等敏感凭据。
- 不保留 Telegram file checkpoint 兼容路径。
- 不把 client state 下沉到 `noloong-agent-core`。

## Open Questions

- 无。当前默认选择为：正确和简洁优先、统一 `ClientStateStore`、metadata exact-match 索引、Weixin+Telegram 同轮迁移、运行态只做轻量内存缓存。
