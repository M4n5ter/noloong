# Implementation Plan: Toasty-backed SQLite EventStore

## Overview

本轮实现一个内置的 SQLite 持久化 `EventStore`，用于替代仅内存的 `InMemoryEventStore`，并为后续 PostgreSQL backend 留出顺滑迁移路径。v1 明确采用 SQL-first，而不是 OpenDAL-first：OpenDAL 更适合作为未来 object archive/object backend 的统一访问层，不承担强一致 append event log 的主存储语义。

当前仓库状态：

- `EventStore` 只有 `append(event)` 和 `load(run_id)` 两个方法。
- `InMemoryEventStore` 当前用 `BTreeMap<RunId, Vec<AgentEvent>>` 保存事件。
- runtime 依赖事件可按 `sequence` replay，并且 paused approval resume 会从 store load event log 后继续追加事件。
- `AgentEvent` 已完整 `Serialize` / `Deserialize`，适合先以 JSON payload 持久化。

Toasty 参考资料：

- GitHub: <https://github.com/tokio-rs/toasty/tree/main>
- docs.rs: <https://docs.rs/toasty>
- Toasty guide: <https://tokio-rs.github.io/toasty/nightly/guide/>
- Database setup guide: <https://tokio-rs.github.io/toasty/nightly/guide/database-setup.html>
- Tokio release blog: <https://tokio.rs/blog/2026-04-03-toasty-released>
- Local exploration option: `git clone https://github.com/tokio-rs/toasty.git /tmp/toasty`
- Crates.io check already observed: `toasty = "0.5.0"`, `toasty-driver-sqlite = "0.5.0"`, `toasty-driver-postgresql = "0.5.0"`.

## Architecture Decisions

- v1 只实现 SQLite event store；PostgreSQL 是后续 task，不在本轮实现。
- 使用 Toasty，依赖 `toasty` 和 `toasty-driver-sqlite`，通过 feature gate 控制。
- 新 feature 命名为 `sqlite-store`；默认 build 不拉入 SQLite/Toasty。
- `SqliteEventStore` 是内置 backend，不改变 `EventStore` trait 签名。
- SQLite 持久化完整 `AgentEvent` JSON，同时拆出少量索引列：`run_id`、`sequence`、`turn_id`、`phase`、`kind_type`、`created_at_ms`。
- 强 append 一致性以 Toasty composite primary key `(run_id, sequence)` 实现；重复 append 必须失败，不能 upsert 或静默覆盖。
- `load(run_id)` 必须按 `sequence ASC` 返回事件，空 run 继续返回空 vec。
- 不把 OpenDAL 引入本轮依赖；未来如果做 object store，需要单独设计 manifest/lock/compaction 协议。

## Task List

### Phase 1: Foundation

#### Task 1: Add Toasty dependency plan and feature gates

**Description:** 在 workspace dependency 层加入 Toasty SQLite 所需依赖，并通过 `sqlite-store` feature 隔离持久化 backend，保证默认 core 仍保持轻量。

**Acceptance criteria:**

- [ ] `Cargo.toml` workspace dependencies 包含 Toasty 相关依赖，版本使用 `0.5` 系列。
- [ ] `crates/noloong-agent-core/Cargo.toml` 新增 `sqlite-store` feature。
- [ ] 默认 `cargo check -p noloong-agent-core` 不启用 Toasty/SQLite backend。

**Verification:**

- [ ] `cargo check -p noloong-agent-core`
- [ ] `cargo check -p noloong-agent-core --features sqlite-store`

**Dependencies:** None

**Files likely touched:**

- `Cargo.toml`
- `crates/noloong-agent-core/Cargo.toml`

**Estimated scope:** Small

#### Task 2: Split store module and add store error variant

**Description:** 将当前 `store.rs` 拆成 facade + memory backend，为 SQLite backend 提供明确模块边界，并添加持久化错误类型入口。

**Acceptance criteria:**

- [ ] `EventStore` 和 `InMemoryEventStore` 仍从 `noloong_agent_core` 原 public path 导出。
- [ ] `AgentCoreError::Store(String)` 可表达 SQLite/Toasty/schema/constraint 错误。
- [ ] `InMemoryEventStore` 行为不变。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test runtime_core`
- [ ] `cargo test -p noloong-agent-core --test conformance runtime_success_replay_matches_report_state`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/store.rs`
- `crates/noloong-agent-core/src/store/mod.rs`
- `crates/noloong-agent-core/src/store/memory.rs`
- `crates/noloong-agent-core/src/error.rs`

**Estimated scope:** Small

### Checkpoint: Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo check -p noloong-agent-core`
- [ ] `cargo check -p noloong-agent-core --features sqlite-store`

### Phase 2: SQLite EventStore

#### Task 3: Define Toasty event model and schema initialization

**Description:** 新增 SQLite event row model，并实现 `SqliteEventStoreConfig` 和 `SqliteEventStore::connect`。连接时按配置初始化 schema，默认支持 `sqlite::memory:` 和 `sqlite://path/to/db.sqlite`。

**Acceptance criteria:**

- [ ] `SqliteEventStoreConfig` 至少包含 `database_url` 和 `migrate_on_connect`。
- [ ] `SqliteEventStore::connect(config)` 返回可 clone/share 的 store 实例。
- [ ] Schema 使用 `(run_id, sequence)` composite primary key，提供唯一性和按 run replay 的稳定顺序。
- [ ] 若 `migrate_on_connect = false` 且 schema 不存在，connect 或首次访问必须返回清晰 `AgentCoreError::Store`。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store connect_in_memory_sqlite_store`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store connect_file_sqlite_store`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/src/store/sqlite.rs`
- `crates/noloong-agent-core/src/store/mod.rs`
- `crates/noloong-agent-core/tests/sqlite_store.rs`

**Estimated scope:** Medium

#### Task 4: Implement append/load contract

**Description:** 实现 `EventStore` for `SqliteEventStore`，把完整 `AgentEvent` JSON 持久化，同时用索引列保证 replay 顺序和错误可诊断性。

**Acceptance criteria:**

- [ ] `append` 单事件写入，不 upsert。
- [ ] Duplicate `(run_id, sequence)` 返回 `AgentCoreError::Store`。
- [ ] `load(run_id)` 按 `sequence ASC` 返回完整事件。
- [ ] 反序列化失败时返回包含 run id 和 sequence 的 store error。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store append_and_load_orders_by_sequence`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store duplicate_sequence_is_rejected`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test core event_log_replays_to_report_state`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/src/store/sqlite.rs`
- `crates/noloong-agent-core/tests/sqlite_store.rs`

**Estimated scope:** Medium

### Checkpoint: SQLite EventStore

- [ ] `cargo fmt --check`
- [ ] `cargo clippy -p noloong-agent-core --all-targets --features sqlite-store -- -D warnings`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`

### Phase 3: Runtime Persistence Scenarios

#### Task 5: Test durable replay across store instances

**Description:** 验证 SQLite store 不是只在同一个 handle 内可用：同一个 DB 文件由第二个 store 实例重新连接后，仍能 load 并 replay 原 run。

**Acceptance criteria:**

- [ ] Runtime A 使用 file SQLite store 完成一个普通 run。
- [ ] Runtime B 重新连接同一 DB 文件后 load 同一 run。
- [ ] `reduce_events(events)` 与 Runtime A 的 report state 一致。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store durable_replay_survives_store_reconnect`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent-core/tests/sqlite_store.rs`

**Estimated scope:** Small

#### Task 6: Test approval resume crash recovery with SQLite

**Description:** 用 SQLite store 覆盖当前最关键的持久化恢复路径：tool approval pause 后，换一个 runtime 实例 resume 并完成。

**Acceptance criteria:**

- [ ] Runtime A 跑到 `RunStatus::Paused`，pending approval 写入 SQLite。
- [ ] Runtime B 使用同一 SQLite DB 调用 `resume_tool_approvals`。
- [ ] replay state 包含 approval resolved/expired、run resumed、tool output 和 run completed。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store approval_resume_survives_runtime_restart`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test tool_flow tool_approval_pauses_and_crash_recovers_on_resume`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent-core/tests/sqlite_store.rs`

**Estimated scope:** Medium

### Checkpoint: Runtime Persistence

- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test conformance`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test tool_flow`

### Phase 4: Documentation and Final Verification

#### Task 7: Update architecture docs and roadmap

**Description:** 更新架构文档，记录 SQL-first 决策、SQLite backend 的一致性 contract，以及 OpenDAL 的非 v1 定位。

**Acceptance criteria:**

- [ ] `ARCHITECTURE.md` 描述 `SqliteEventStore` 和强 append contract。
- [ ] “后续演进方向” 将 PostgreSQL 标记为下一步 SQL backend。
- [ ] 文档明确 OpenDAL 未来可用于 object archive/object backend，但不作为 v1 primary event store。
- [ ] Toasty 参考链接出现在文档或计划中，便于实现者查 API。

**Verification:**

- [ ] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [ ] Manual check: docs mention SQL-first and Toasty references.

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** Small

### Final Checkpoint

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test conformance`
- [ ] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Toasty 0.x API churn | Medium | Keep SQLite backend isolated behind `sqlite-store`; use official guide/docs links before implementation |
| Toasty schema API may not expose the exact migration primitive needed | Medium | First implementation task must inspect Toasty examples/source; if needed, keep schema init in a small SQL-specific helper behind the same backend |
| Duplicate sequence handling differs across drivers | High | Treat duplicate append as required behavior; add explicit test now so PostgreSQL must match later |
| Runtime event counter is process-local | Medium | Keep current runtime behavior for v1; SQLite enforces duplicate `(run_id, sequence)` instead of silently accepting conflicts |
| JSON payload grows with large tool/media events | Medium | Accept for v1; object archive/media store is a later design, not part of SQLite event store |
| Tests may need temporary DB files | Low | Use temp directories and unique DB paths; avoid checked-in artifacts |

## Parallelization Opportunities

- Task 1 and Toasty source/docs exploration can happen together.
- Task 5 and Task 6 tests should wait until Task 4 append/load is green.
- Documentation can be drafted after Task 3, but final wording should wait until SQLite behavior tests are stable.

## Open Questions

- None. Direction is locked to Toasty-backed SQLite v1 with strong append consistency.
