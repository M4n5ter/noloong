# 实施计划：持久化 Agent Session Registry

> 状态：已实施并通过本地验证。PostgreSQL live round-trip 仍按计划由 `NOLOONG_POSTGRES_TEST_URL` 环境变量显式开启；当前环境未设置该变量，因此本轮执行的是 env-gated skip path 与 feature compile/clippy。

## 概览

当前 `AgentSessionRegistryStore` 只有内存实现，`AgentSessionRegistry::get` 遇到“store 中存在但 live map 中没有”的 session 会返回 internal error。下一步要把 registry 升级为可持久化、可恢复的 session registry：store 保存 session snapshot，registry 按需 lazy restore live runtime。

本轮不序列化 runtime、provider、credential、extension process handle 或 Rust closure。恢复 live session 时只使用 persisted `profileId` 在当前进程已注册的 `AgentRuntimeProfile` 中查找 profile，并用 persisted `AgentManifest` 和 `AgentState` 重建 `AgentSession + Agent`。上次状态为 `running` 的 session 恢复时标记为 `failed/interrupted`；`paused` session 保留 paused 状态，但能否继续 resume 取决于 runtime profile 是否重新接入同一个 durable core `EventStore`。

## 架构决策

- `noloong-agent-core` 只增加必要的 `AgentBuilder::with_initial_state` 和 queue mode getter，不引入 registry store、SQL、OpenDAL 或 application session 概念。
- `AgentSessionRegistryStore` 存的是 application session snapshot，不是 core event log；core 的 `EventStore` 仍然负责 approval resume 和 run replay。
- `session/list`、`session/get` 是 read-only descriptor path，可以从 persisted snapshot 直接返回，不触发 runtime profile rebuild。
- 需要实际操作 agent 的方法才走 live restore，例如 `agent/prompt`、`agent/continue`、`approval/resolve`、`event/subscribe`、`manifest/apply_approved`、`process/*` 控制类方法。
- persisted manifest 是恢复时唯一 manifest source of truth；恢复时不重新应用 profile default manifest patches，避免 profile 默认值漂移改变历史 session。
- `running` snapshot 恢复时写回为 `failed`，`lastError` 使用稳定的 interrupted 文案，并清空 `activePhase`；`paused` snapshot 保留 pending approvals。
- SQL backend 使用 Toasty 0.5，SQLite 和 PostgreSQL 共用同一模型；SQLite 提供文件和内存测试，PostgreSQL 提供 feature compile check 和 env-gated live test。
- OpenDAL object store backend 是单写者 snapshot store，不承诺多进程同时写同一个 registry；需要多进程强一致写入时使用 SQL backend。
- `mod.rs` 只做模块声明和 re-export；store 逻辑拆到独立文件，避免继续膨胀 `registry.rs`。

## 公开 API / 接口变化

- `noloong_agent_core::AgentBuilder`
  - 新增 `with_initial_state(state: AgentState) -> Self`。
- `noloong_agent_core::Agent`
  - 新增 `steering_queue_mode() -> QueueMode`。
  - 新增 `follow_up_queue_mode() -> QueueMode`。
- `noloong_agent::interaction::AgentSessionRecord`
  - 增加 `state: AgentState`。
  - 增加 `queues: AgentSessionQueueSnapshot`。
  - 增加 `schemaVersion`、`createdAtMs`、`updatedAtMs`。
- `noloong_agent::interaction::AgentSessionRegistryStore`
  - 将 `upsert(record)` 拆成 `insert(record)` 和 `save(record)`。
  - `insert` 必须在 duplicate `sessionId` 时返回 structured duplicate error。
  - 保留 `get(sessionId)`、`list()`、`remove(sessionId)`。
- 新增 optional store types：
  - `SqlAgentSessionRegistryStore`
  - `SqlAgentSessionRegistryStoreConfig`
  - `OpenDalAgentSessionRegistryStore`
  - `OpenDalAgentSessionRegistryStoreConfig`
- 新增 Cargo features：
  - `registry-store-sqlite`
  - `registry-store-postgres`
  - `registry-store-object`

## 任务列表

### Phase 1：Snapshot Contract 与 Core API

#### 任务 1：定义 persisted session snapshot 类型

**描述：** 扩展 interaction registry 的 persisted record，使 store 能保存足够的信息来恢复 descriptor 和 live runtime。snapshot 必须包括 manifest、state、queue 内容、queue mode、metadata 和时间戳。

**验收标准：**

- [ ] `AgentSessionRecord` 包含 `schemaVersion`、`sessionId`、`profileId`、`parentSessionId`、`role`、`manifest`、`state`、`queues`、`metadata`、`createdAtMs`、`updatedAtMs`。
- [ ] `AgentSessionQueueSnapshot` 同时保存 steering/follow-up messages 和各自 `QueueMode`。
- [ ] serde 使用 camelCase JSON，新增字段有清晰默认值或在创建时完整填充。
- [ ] `schemaVersion` 固定为 `1`，未知版本在 restore path 返回 structured error。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry snapshot_record_serde_round_trips`
- [ ] `cargo test -p noloong-agent --test interaction_registry snapshot_preserves_queues_and_state`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/store/snapshot.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 2：补齐 core Agent restore 所需 API

**描述：** 在 `noloong-agent-core` 中增加恢复 `AgentState` 和读取 queue mode 的最小 API。不要让 core 知道 registry store，也不要把 application snapshot 类型放进 core。

**验收标准：**

- [ ] `AgentBuilder::with_initial_state` 可以设置完整 `AgentState`，不只设置 initial messages。
- [ ] `Agent::steering_queue_mode` 和 `Agent::follow_up_queue_mode` 返回当前 queue mode。
- [ ] 现有 `with_initial_messages` 继续可用，并只影响 messages。
- [ ] core 不新增 SQL、OpenDAL、registry 或 profile 依赖。

**验证：**

- [ ] `cargo test -p noloong-agent-core --test agent agent_builder_restores_initial_state`
- [ ] `cargo test -p noloong-agent-core --test agent queued_modes_are_readable`
- [ ] `cargo clippy -p noloong-agent-core --all-targets -- -D warnings`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent-core/src/agent.rs`
- `crates/noloong-agent-core/tests/agent.rs`

**预计范围：** 小

#### 任务 3：拆分 registry store 模块

**描述：** 将当前 `registry.rs` 中的 store trait 和内存 store 拆出，建立后续 SQL/OpenDAL backend 的模块边界。`mod.rs` 只保留 re-export。

**验收标准：**

- [ ] `AgentSessionRegistryStore` 移入 `interaction/store/mod.rs` 或等价 store 模块。
- [ ] `InMemoryAgentSessionRegistryStore` 移入独立实现文件。
- [ ] `interaction/mod.rs` 只声明模块和 re-export，不承载业务逻辑。
- [ ] `registry.rs` 只保留 registry lifecycle、restore、descriptor 和 filtering 逻辑。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

**依赖：** 任务 1

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/mod.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/store/`

**预计范围：** 中

### Checkpoint：Snapshot 基础

- [ ] snapshot serde 与 queue/state round-trip 通过。
- [ ] core API 只做最小补强。
- [ ] 现有 interaction registry 测试仍通过。
- [ ] `cargo fmt --check`
- [ ] `cargo clippy -p noloong-agent-core --all-targets -- -D warnings`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

### Phase 2：Store Contract 与 Lazy Restore

#### 任务 4：重构 `AgentSessionRegistryStore` contract

**描述：** 把 store 从宽泛 upsert 改成明确的 create/update 语义。`insert` 负责 duplicate session id 防线，`save` 负责已存在 snapshot 的覆盖更新。

**验收标准：**

- [ ] trait 提供 `insert`、`save`、`remove`、`get`、`list`。
- [ ] `insert` 遇到 duplicate `sessionId` 返回 `InteractionError::invalid_params` 或等价 structured duplicate error。
- [ ] `save` 不创建新 session；不存在时返回 not found。
- [ ] in-memory store 使用同一行为，并保留并发 duplicate 测试。
- [ ] `create_session` 不依赖“先查再写”作为唯一正确性保证。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry interaction_registry_rejects_concurrent_duplicate_session_id`
- [ ] `cargo test -p noloong-agent --test interaction_registry store_insert_rejects_duplicate_session_id`
- [ ] `cargo test -p noloong-agent --test interaction_registry store_save_requires_existing_session`

**依赖：** 任务 3

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/store/`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 5：实现 read-only descriptor path

**描述：** 让 `session/list` 和 `session/get` 可以从 store 中的 unloaded session 返回 descriptor。该路径不能 build runtime，也不能把 session 加入 live map。

**验收标准：**

- [ ] `AgentSessionRegistry::list` 合并 live sessions 和 store records，live descriptor 覆盖同 id stored descriptor。
- [ ] `AgentSessionRegistry::get_descriptor` 对 unloaded stored session 返回 persisted descriptor。
- [ ] parent/profile/status filters 同时适用于 live 和 unloaded sessions。
- [ ] read-only path 不调用 `AgentRuntimeProfile::build_runtime`。
- [ ] missing profile 不影响 read-only descriptor。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry registry_lists_unloaded_stored_sessions`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_get_descriptor_does_not_restore_runtime`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_filters_unloaded_sessions`

**依赖：** 任务 4

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 6：实现 lazy live restore path

**描述：** 将需要操作 agent 的 registry path 改为“缺 live session 时，从 store 取 snapshot 并重建 live session”。恢复使用 persisted profile id、manifest、state 和 queues。

**验收标准：**

- [ ] action methods 通过统一 `get_live_session` 或等价 helper 取 live session。
- [ ] restore 使用当前注册的 `AgentRuntimeProfile`，profile 缺失时返回 structured not found/internal error，不修改 stored snapshot。
- [ ] restore 使用 persisted manifest，不重新应用 profile default manifest patches。
- [ ] restored `Agent` 使用 persisted `AgentState` 和 queue snapshot。
- [ ] 同一个 session 的并发 restore 只会 build 一次 runtime。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry registry_lazy_restores_session_for_prompt`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_uses_persisted_manifest`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_missing_profile_is_structured_error`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_rejects_concurrent_duplicate_restore`

**依赖：** 任务 2、任务 5

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 7：实现恢复状态规范化策略

**描述：** 在读取 persisted snapshot 准备 descriptor 或 live restore 前，统一处理异常状态。`running` 是进程崩溃或关闭前未完成的状态，恢复时必须变成 failed/interrupted；`paused` 保留。

**验收标准：**

- [ ] `running` snapshot 恢复后状态变为 `failed`。
- [ ] interrupted state 的 `lastError` 使用稳定英文文案，便于测试和 bridge 展示。
- [ ] interrupted state 清空 `activePhase`，保留 `runId`、messages、context、available tools。
- [ ] `paused` snapshot 保留 `pendingToolApprovals`。
- [ ] normalized record 写回 store，避免下一次 restart 仍看到 `running`。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_marks_running_session_failed`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_preserves_paused_session`
- [ ] `cargo test -p noloong-agent --test interaction_control interaction_control_get_reports_interrupted_session`

**依赖：** 任务 5、任务 6

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**预计范围：** 中

### Checkpoint：恢复语义

- [ ] unloaded session 的 read-only 查询可用。
- [ ] live restore 使用 profile id + persisted manifest + persisted state。
- [ ] `running` 和 `paused` 恢复策略均有测试。
- [ ] `cargo test -p noloong-agent --test interaction_registry --test interaction_control`

### Phase 3：Snapshot 持续更新

#### 任务 8：在 registry 中持久化 agent state 变化

**描述：** 为 registry-created/restored live session 注册 agent event listener，在 state-changing events 后保存最新 snapshot。不要在每个 model stream delta 上写 store。

**验收标准：**

- [ ] `RunStarted` 后保存 running snapshot。
- [ ] `EffectCommitted`、approval requested/resolved/expired、run terminal/pause events 后保存 snapshot。
- [ ] `ModelStreamEvent` 不触发 snapshot save。
- [ ] store save 失败会进入 agent event sink error path，并保留 audit failure。
- [ ] delete session 时取消或自然释放 listener，不保留悬挂引用。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry registry_persists_state_after_prompt`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_does_not_snapshot_model_deltas`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_snapshot_save_error_fails_run`

**依赖：** 任务 6

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 9：持久化 queue 与 manifest 变化

**描述：** queue control methods 和 manifest apply 不一定伴随 core event，因此 control handler 或 registry helper 必须在这些操作后保存 snapshot。

**验收标准：**

- [ ] `agent/steer`、`agent/follow_up` 保存 queue snapshot。
- [ ] `queue/edit`、`queue/clear`、`queue/set_mode` 保存 queue snapshot。
- [ ] `manifest/apply_approved` 保存新 manifest。
- [ ] snapshot 中的 queue mode 和 messages 在新 registry instance 中恢复。
- [ ] snapshot 中的新 manifest 在新 registry instance 中恢复并用于 runtime rebuild。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_control interaction_control_queue_changes_are_persisted`
- [ ] `cargo test -p noloong-agent --test interaction_control interaction_control_manifest_apply_is_persisted`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_replays_queue_snapshot`

**依赖：** 任务 8

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**预计范围：** 中

#### 任务 10：验证 paused approval resume 策略

**描述：** 用测试 profile 明确 paused session 的恢复边界：registry 能恢复 paused state 和 pending approvals，但真正 resume 需要 profile 构建出的 runtime 接入同一个 durable/shared core `EventStore`。

**验收标准：**

- [ ] 使用共享 `EventStore` 的 test profile 可以 restore paused session 并成功 `approval/resolve`。
- [ ] 使用新内存 `EventStore` 的 profile 会在 resume 时返回 core replay error，不被 registry 静默吞掉。
- [ ] 文档明确 paused resume 的必要条件是 runtime profile 的 durable event store 配置一致。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_paused_session_can_resume_with_shared_event_store`
- [ ] `cargo test -p noloong-agent --test interaction_registry registry_restore_paused_session_resume_fails_without_event_log`

**依赖：** 任务 8、任务 9

**可能涉及文件：**

- `crates/noloong-agent/tests/interaction_registry.rs`
- `crates/noloong-agent/docs/ARCHITECTURE.md`

**预计范围：** 中

### Checkpoint：持久化一致性

- [ ] state、queue、manifest 都能跨 registry instance 恢复。
- [ ] paused approval 的可恢复条件被测试锁定。
- [ ] snapshot 保存不会写入 model stream delta 热路径。
- [ ] `cargo test -p noloong-agent --test interaction_registry --test interaction_control`

### Phase 4：SQL Backed Registry Store

#### 任务 11：添加 SQL store 依赖和 feature gates

**描述：** 为 `noloong-agent` 添加 SQL registry store 所需的 optional dependencies。SQLite 和 PostgreSQL 使用独立 feature，避免默认构建引入数据库依赖。

**验收标准：**

- [ ] workspace 增加 `toasty-driver-postgresql` 依赖。
- [ ] `noloong-agent` 增加 optional `toasty`、`toasty-driver-sqlite`、`toasty-driver-postgresql`、`rusqlite`。
- [ ] `registry-store-sqlite` 启用 Toasty SQLite driver。
- [ ] `registry-store-postgres` 启用 Toasty PostgreSQL driver。
- [ ] 默认 features 仍为空，不影响默认 `cargo test --workspace` 的依赖面。

**验证：**

- [ ] `cargo check -p noloong-agent`
- [ ] `cargo check -p noloong-agent --features registry-store-sqlite`
- [ ] `cargo check -p noloong-agent --features registry-store-postgres`

**依赖：** 任务 4

**可能涉及文件：**

- `Cargo.toml`
- `crates/noloong-agent/Cargo.toml`

**预计范围：** 小

#### 任务 12：实现 Toasty SQL store model 与 config

**描述：** 新增 SQL backed registry store。模型使用单表 `stored_agent_sessions`，主键为 `session_id`，manifest/state/queues/metadata 以 JSON string 保存，常用 filter 字段拆列。

**验收标准：**

- [ ] SQL model 包含 `session_id`、`profile_id`、`parent_session_id`、`role`、`status`、`record_json`、`created_at_ms`、`updated_at_ms`。
- [ ] `SqlAgentSessionRegistryStoreConfig` 支持 `databaseUrl` 和 `migrateOnConnect`。
- [ ] SQLite 支持 `sqlite::memory:`、`sqlite://memory`、`sqlite:<path>`、`sqlite://<path>` 和直接文件路径。
- [ ] PostgreSQL 支持 `postgres://...` 和 `postgresql://...`。
- [ ] JSON decode failure 返回包含 `sessionId` 的 structured store error。

**验证：**

- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo check -p noloong-agent --features registry-store-postgres`

**依赖：** 任务 11

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/store/sql.rs`
- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`

**预计范围：** 中

#### 任务 13：验证 SQLite 跨 store instance 恢复

**描述：** 用临时 SQLite 文件模拟进程重启：第一个 registry 创建并运行 session，第二个 registry 用同一个 store 文件读取 descriptor 并 lazy restore。

**验收标准：**

- [ ] file SQLite store 中创建的 session 可被新 store instance list/get。
- [ ] 新 registry 的 read-only `session/get` 不 build runtime。
- [ ] 新 registry 第一次 action build runtime，并恢复 state/queues/manifest。
- [ ] `remove` 后新 store instance 不再列出该 session。

**验证：**

- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite sqlite_store_recovers_session_across_instances`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite sqlite_store_delete_persists`

**依赖：** 任务 12

**可能涉及文件：**

- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`

**预计范围：** 中

#### 任务 14：添加 PostgreSQL optional integration test

**描述：** 增加 env-gated PostgreSQL 测试，只在提供 `NOLOONG_POSTGRES_TEST_URL` 时运行真实 PostgreSQL。无环境变量时测试必须 ignored 或 early skip，不影响默认 CI。

**验收标准：**

- [ ] PostgreSQL feature compile check 通过。
- [ ] live test 使用唯一 table prefix 或临时 schema，避免污染共享数据库。
- [ ] live test 覆盖 insert/list/get/save/remove。
- [ ] 无 `NOLOONG_POSTGRES_TEST_URL` 时不会失败。

**验证：**

- [ ] `cargo check -p noloong-agent --features registry-store-postgres`
- [ ] `NOLOONG_POSTGRES_TEST_URL=postgres://... cargo test -p noloong-agent --features registry-store-postgres --test interaction_registry_store_postgres -- --ignored`

**依赖：** 任务 12

**可能涉及文件：**

- `crates/noloong-agent/tests/interaction_registry_store_postgres.rs`

**预计范围：** 小

### Checkpoint：SQL Store

- [ ] SQLite backend 可跨 store instance 恢复。
- [ ] PostgreSQL backend feature 编译通过。
- [ ] SQL duplicate `insert` 由主键或等价机制保证。
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo check -p noloong-agent --features registry-store-postgres`

### Phase 5：OpenDAL Object Store

#### 任务 15：添加 OpenDAL store 依赖和 key 编码

**描述：** 为 object-store backed registry 添加 optional OpenDAL dependency。public constructor 接收 `opendal::Operator`，使宿主应用可以自行决定 FS/S3/GCS/Azure 等 service 配置。

**验收标准：**

- [ ] `registry-store-object` feature 启用 `opendal` 和 `base64`。
- [ ] object key 使用 URL-safe no-pad base64 编码 `sessionId`，避免 slash、space、unicode 破坏路径。
- [ ] config 包含 `prefix`，所有对象写入同一 prefix 下。
- [ ] 文档明确该 backend 是 single-writer snapshot store。

**验证：**

- [ ] `cargo check -p noloong-agent --features registry-store-object`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object object_store_key_encoding_is_path_safe`

**依赖：** 任务 4

**可能涉及文件：**

- `Cargo.toml`
- `crates/noloong-agent/Cargo.toml`
- `crates/noloong-agent/src/interaction/store/object.rs`

**预计范围：** 小

#### 任务 16：实现 OpenDAL backed store

**描述：** 用 OpenDAL `Operator` 实现 `AgentSessionRegistryStore`。每个 session 一个 JSON object；`list` 通过 prefix scan 聚合 records。

**验收标准：**

- [ ] `insert` 在 object 已存在时返回 duplicate error。
- [ ] `save` 覆盖已有 object，不存在时返回 not found。
- [ ] `get` 读取并反序列化单个 record。
- [ ] `list` 只返回当前 prefix 下的 session records。
- [ ] `remove` 删除 session object。
- [ ] backend 不尝试实现跨进程 lock/CAS。

**验证：**

- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object object_store_insert_get_list_save_remove`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object object_store_prefix_isolation`

**依赖：** 任务 15

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**预计范围：** 中

#### 任务 17：验证 object store registry 恢复

**描述：** 用 OpenDAL memory 或 fs service 模拟 registry 重启，确保 object store backend 能完成与 SQLite 相同的 read-only descriptor 和 lazy restore 流程。

**验收标准：**

- [ ] object store 中创建的 session 可被新 registry list/get。
- [ ] read-only descriptor 不 build runtime。
- [ ] lazy restore 后可以继续 prompt。
- [ ] `running` snapshot 在 object store 中也会被 normalize 并写回 failed。

**验证：**

- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object object_store_registry_recovers_session_across_instances`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object object_store_running_restore_is_written_back`

**依赖：** 任务 16

**可能涉及文件：**

- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**预计范围：** 中

### Checkpoint：Object Store

- [ ] OpenDAL backend 满足同一 store behavior。
- [ ] key encoding 覆盖特殊 session id。
- [ ] object store 恢复流程和 SQL 恢复流程语义一致。
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`

### Phase 6：文档、矩阵与最终验证

#### 任务 18：更新架构文档和交互协议文档

**描述：** 将 registry 持久化和恢复策略写入文档，删除“后续演进方向”中已经完成的 registry store 项。文档必须让第三方 host 知道 profile id、durable event store 和 object store single-writer 的责任边界。

**验收标准：**

- [ ] `ARCHITECTURE.md` 说明 session registry store 与 core event store 的区别。
- [ ] `ARCHITECTURE.md` 说明 live restore 使用当前注册 `AgentRuntimeProfile` 重建 runtime。
- [ ] `ARCHITECTURE.md` 说明 `running` -> `failed/interrupted` 和 `paused` 保留策略。
- [ ] `INTERACTION.md` 说明 `session/list`/`session/get` 对 unloaded persisted sessions 的行为。
- [ ] `CONFORMANCE_MATRIX.md` 增加 registry store backend 和 restore semantics 覆盖项。

**验证：**

- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] 手动检查文档中没有把 object store 描述为强一致多写 backend。

**依赖：** 任务 7、任务 13、任务 17

**可能涉及文件：**

- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/docs/INTERACTION.md`
- `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`

**预计范围：** 小

#### 任务 19：最终 workspace 验证

**描述：** 跑完整质量门禁，确保默认构建和 feature-gated store 构建都通过。真实 PostgreSQL 测试只在用户提供 URL 时执行。

**验收标准：**

- [ ] 默认 workspace tests 通过。
- [ ] SQLite store feature tests 通过。
- [ ] Object store feature tests 通过。
- [ ] PostgreSQL feature compile check 通过。
- [ ] `clippy -D warnings` 无告警。
- [ ] 没有新增 `#[allow(dead_code)]`。

**验证：**

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [ ] `cargo check -p noloong-agent --features registry-store-postgres`

**依赖：** 所有任务

**可能涉及文件：**

- 无新增实现文件；只允许修复验证暴露的问题。

**预计范围：** 小

## 风险与缓解

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 把 registry snapshot 和 core event log 混为一谈 | paused approval resume 看似恢复但 replay 失败 | 文档和测试明确：registry 恢复 `AgentState`，approval resume 仍依赖 durable `EventStore` |
| 每个 token delta 都写 store | 性能明显下降，object store 成本过高 | 只在 state-changing events、queue/manifest 变更后保存 snapshot |
| profile 默认 manifest patch 漂移 | 老 session 恢复后行为被新 profile 默认值改变 | 恢复只使用 persisted manifest，不重新应用 default patches |
| object store 被误用于多进程写 | last-write-wins 或 duplicate race | backend 和文档明确 single-writer；多写者使用 SQL |
| PostgreSQL 测试依赖外部服务 | 默认 CI 不稳定 | Postgres live test env-gated，默认只做 feature compile check |
| `registry.rs` 继续膨胀 | 后续 store backend 难维护 | store/snapshot/sql/object 拆模块，`mod.rs` 只 re-export |

## 并行化机会

- 任务 1、2 可以并行；一个做 snapshot contract，一个做 core API。
- 任务 11、15 可以在 store trait 稳定后并行；分别准备 SQL 和 OpenDAL feature gates。
- 任务 13、17 可以在各自 backend 完成后并行；都复用同一 registry restore behavior。
- 任务 18 可以在恢复语义和 backend API 稳定后与最终测试并行更新。

## 明确不做

- 不实现自动 continuation 或 crash 后自动继续运行。
- 不序列化 provider credential、HTTP auth token、extension child process、closure 或 runtime object。
- 不让 JSON-RPC client 通过 persisted record 注入任意 provider 配置。
- 不实现 distributed session ownership、lease、lock 或 multi-writer object store consistency。
- 不把 OpenDAL object store 当成 core `EventStore` 的强一致 append log。
- 不为已有历史数据写 migration；当前没有兼容性负担。

## Open Questions

- 无阻塞问题。已选择默认策略：`running` 恢复为 `failed/interrupted`，`paused` 保留；OpenDAL object store 按 single-writer snapshot backend 实现。
