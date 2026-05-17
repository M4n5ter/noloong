# Implementation Plan: Interaction Store 结构化优化

## Overview

本轮专门处理上一轮审查中适合单独推进的结构性问题：统一 SQLite URL 解析，整理 SQL registry store 的 metadata 查询流程，降低 Object store session list 的串行远端 IO 放大，并收敛 store 过滤语义。目标是不改变外部行为和 JSON-RPC wire shape，只优化内部实现的可靠性、复用性和性能边界。

## Implementation Status

完成于 2026-05-17。

- [x] Phase 1: Shared SQLite URL Parser
- [x] Phase 2: SQL Metadata Query Cleanup
- [x] Phase 3: Object Store IO Optimization
- [x] Phase 4: Regression and Review

已通过：

- [x] `cargo test -p noloong-agent sqlite_database_url`
- [x] `cargo test -p noloong-agent --features client-state-sqlite client_state`
- [x] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [x] `cargo test -p noloong-agent --features registry-store-postgres --test interaction_registry_store_postgres`
- [x] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [x] `cargo test -p noloong-agent --test interaction_registry`
- [x] `cargo test -p noloong schema`
- [x] `cargo fmt --all --check`
- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings`
- [x] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`
- [x] `cargo test --workspace`
- [x] `git diff --check`

## Architecture Decisions

- SQLite URL 解析统一放在 `noloong-agent` host/store 层，不下沉到 `noloong-agent-core`。
- SQL store 不引入 sqlite/postgres 两套 raw SQL 分叉；优先保持 Toasty 共用实现。
- Object store 的 record 仍是最终 truth；index 只负责候选收窄，最终结果必须经过 record 校验。
- Object store 并发读取使用内部常量，默认 `16`，本轮不新增 profile/config surface。
- 项目无兼容性负担；旧内部 parser 差异和旧 object index 形态可直接清理。

## Dependency Graph

```text
Shared SQLite URL parser
    ├── ClientStateStore SQLite backend
    ├── Registry SQL store location parser
    └── CLI state database parent validation

Session filter candidate semantics
    ├── SQL metadata candidate collection
    │       └── SQL list row loading and final record filtering
    └── Object index candidate collection
            ├── Path-based metadata candidate decode
            ├── Bounded concurrent index/record reads
            └── Stale index cleanup
```

## Task List

### Phase 1: Shared SQLite URL Parser

#### Task 1: 新增统一 SQLite URL parser

**Description:** 在 `noloong-agent` 新增共享 SQLite URL 解析类型，统一当前 client state、registry SQL store、CLI config 中重复的 memory/file/scheme 解析逻辑。该任务只新增 parser 和单元测试，不切换调用点。

**Acceptance criteria:**
- [ ] 新增 `SqliteDatabaseLocation` 或等价类型，至少包含 `Memory` 和 `File(PathBuf)`。
- [ ] 新增 parser，支持 `:memory:`、`sqlite::memory:`、`sqlite://memory`、`sqlite:<path>`、`sqlite://<path>` 和裸路径。
- [ ] parser 对空字符串、空 sqlite path、非 sqlite URL scheme 返回结构化错误。
- [ ] parser 不接受 postgres URL；postgres 仍由 registry SQL store 自己处理。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent sqlite_database_url`
- [ ] Tests pass: `cargo test -p noloong-agent --features client-state-sqlite client_state`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent/src/client_state.rs`
- `crates/noloong-agent/src/lib.rs`
- new shared module under `crates/noloong-agent/src/`

**Estimated scope:** S

#### Task 2: 将现有调用点切到统一 parser

**Description:** 删除 client state、registry SQL store、CLI config 中重复的 SQLite path parser。Client state 和 CLI config 直接复用 Task 1 的 parser；registry SQL store 继续保留 `SqlStoreLocation`，但其 sqlite 分支从共享 parser 映射得到。

**Acceptance criteria:**
- [ ] `SqliteClientStateStore::new` 使用统一 parser，行为与当前测试一致。
- [ ] `SqlAgentSessionRegistryStore` 的 sqlite URL 分支使用统一 parser，postgres 分支保持原样。
- [ ] `ensure_sqlite_database_parent` / CLI config 不再维护独立 sqlite parser。
- [ ] 仓库中不再存在多份 `sqlite_path_from_suffix` / `sqlite_database_path` 变体。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features client-state-sqlite client_state`
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] Tests pass: `cargo test -p noloong schema sqlite_database_path`
- [ ] Search check: `rg "sqlite_path_from_suffix|fn sqlite_database_path|fn sqlite_path\\("`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent/src/client_state.rs`
- `crates/noloong-agent/src/interaction/store/sql.rs`
- `src/config.rs`

**Estimated scope:** M

### Checkpoint: SQLite Parser

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --features client-state-sqlite client_state`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo test -p noloong schema`

### Phase 2: SQL Metadata Query Cleanup

#### Task 3: 收敛 SQL metadata candidate 构造

**Description:** 重写 SQL store 的 metadata candidate 构造流程。每个 `metadataEquals` 条件仍通过 `StoredAgentSessionMetadata` 查询候选 session id，但先收集所有条件候选集，按候选数量从小到大做交集，避免低选择性条件先放大内存集合。

**Acceptance criteria:**
- [ ] 空 `metadataEquals` 返回 `None`，保持无 metadata filter 路径。
- [ ] 非 scalar metadata filter 返回空候选集。
- [ ] 多条件 filter 按最小候选集优先交集，结果稳定排序。
- [ ] metadata candidate 逻辑不 decode session record JSON。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite metadata`
- [ ] Tests pass: `cargo test -p noloong-agent --test interaction_registry metadata`

**Dependencies:** Task 2

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/sql.rs`
- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`

**Estimated scope:** S

#### Task 4: 优化 SQL candidate session row 读取

**Description:** 替换 metadata 命中后的全表扫描路径。新增 `load_session_rows_for_filter`：无 metadata candidate 时走当前 all rows + row filter；有 candidate 时按 candidate id 点查，避免把全表 `record_json` 拉入内存。

**Acceptance criteria:**
- [ ] 小候选集仍只读取候选 session rows。
- [ ] 大候选集仍只读取 candidate session rows，不回退到全表 `record_json` 扫描。
- [ ] parent/profile/status row filter 在 decode record 前应用。
- [ ] 最终结果仍经过 `record_matches_session_list_filter` 校验。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-postgres --test interaction_registry_store_postgres`
- [ ] Tests pass: `cargo test -p noloong-agent --test interaction_registry registry_filters`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/sql.rs`
- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`

**Estimated scope:** M

#### Task 5: 明确 SQL filter 真值边界

**Description:** 将 SQL row filter 重命名并约束为 candidate prefilter，避免它被误认为最终过滤语义。最终 parent/profile/status/metadata 语义只由 `record_matches_session_list_filter` 决定。

**Acceptance criteria:**
- [ ] SQL row helper 名称体现 prefilter/candidate 语义。
- [ ] SQL row helper 不检查 metadata JSON 语义。
- [ ] `list(filter)` 的最终输出只在 `record_matches_session_list_filter` 通过后返回。
- [ ] 相关测试覆盖 row metadata drift 时不会返回错误 record。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] Tests pass: `cargo test -p noloong-agent --test interaction_registry metadata`

**Dependencies:** Task 4

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/sql.rs`
- `crates/noloong-agent/tests/interaction_registry_store_sqlite.rs`

**Estimated scope:** S

### Checkpoint: SQL Store

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo test -p noloong-agent --features registry-store-postgres --test interaction_registry_store_postgres`

### Phase 3: Object Store IO Optimization

#### Task 6: 从 object index path 解码 session id

**Description:** Object store 的 metadata candidate 查询不再读取每个 metadata index object body。新增 path decode helper，从 `session-metadata/{key}/{value}/{session}.json` 的文件名恢复 session id；metadata index body 可以保留但不参与 candidate 构造。

**Acceptance criteria:**
- [ ] metadata candidate 构造只 `list(prefix)`，不对每个 candidate 调用 `read(path)`。
- [ ] session id decode 使用现有 `URL_SAFE_NO_PAD` 规则。
- [ ] 非 `.json`、base64 无效或路径层级不匹配的 entry 被视为 stale entry 并跳过。
- [ ] candidate map 仍按 session id 稳定排序。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object metadata`
- [ ] Add test/fake assertion proving metadata candidate list does not require readable index body.

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**Estimated scope:** M

#### Task 7: 调整 Object store 写入顺序以避免不可见 record

**Description:** 让 Object store 的 insert/save 先写 index，再写 record，并在 record 写失败时清理新 index。这样 index 写失败不会留下“record 已存在但 list 不可见”的状态；record 是最终 truth，stale index 可在 list 时清理。

**Acceptance criteria:**
- [ ] insert：如果 session index 或 metadata index 写失败，不写 session record。
- [ ] insert：如果 session record 写失败，清理已写入的新 index。
- [ ] save：先加载 previous record；新 index 写失败时不改 record。
- [ ] save：record 写失败时尽力恢复 previous index 并清理 new index。
- [ ] remove：删除 record 后清理 session index 和 metadata index，现有行为保持。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [ ] Add failure-injection or focused unit test for insert index failure not leaving visible/hidden inconsistent record where feasible.

**Dependencies:** Task 6

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**Estimated scope:** M

#### Task 8: 为 Object store list 增加 bounded 并发读取

**Description:** Object store list 在读取 session-index 和 session record 时使用 bounded 并发，减少远端对象存储的串行 GET 放大。并发只用于 IO，最终结果仍按 `session_id` 排序。

**Acceptance criteria:**
- [ ] 新增内部常量 `OBJECT_STORE_LIST_READ_CONCURRENCY: usize = 16`。
- [ ] session-index body 读取使用 bounded 并发。
- [ ] record 读取使用 bounded 并发。
- [ ] 单个 NotFound stale index 被清理并跳过；其它 read/decode error 仍返回 store error。
- [ ] 最终 records 仍按 `session_id` 稳定排序。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [ ] Tests pass: `cargo test -p noloong-agent --test interaction_registry registry_lists_unloaded_stored_sessions`

**Dependencies:** Task 7

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/object.rs`

**Estimated scope:** M

#### Task 9: 收敛 Object store index prefilter 语义

**Description:** 将 `ObjectSessionIndexEntry::matches_filter` 收窄为 candidate prefilter，并确保 filter 不通过时能清理当前 filter 命中的错误 metadata index path。最终输出必须由 record 校验决定。

**Acceptance criteria:**
- [ ] object index helper 名称体现 candidate/prefilter 语义。
- [ ] index prefilter 不作为最终返回依据。
- [ ] stale metadata candidate path 在当前 filter 下不匹配时会被删除。
- [ ] session index stale 时会重写 session index 和 metadata index。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object metadata`
- [ ] Add test covering stale metadata candidate does not return wrong record and is removed.

**Dependencies:** Task 8

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/tests/interaction_registry_store_object.rs`

**Estimated scope:** S

### Checkpoint: Object Store

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

### Phase 4: Regression and Review

#### Task 10: 清理计划、命名和残留重复实现

**Description:** 做一次只针对本轮重构的静态清理，确保没有残留重复 parser、命名与实际语义不一致的 helper、或已经无用的 imports/tests。

**Acceptance criteria:**
- [ ] `rg "sqlite_path_from_suffix|fn sqlite_database_path|fn sqlite_path\\("` 只剩共享 parser 或预期 postgres/sql location 入口。
- [ ] SQL/Object filter helper 命名均体现 candidate/prefilter/final filter 边界。
- [ ] 无未使用 imports、无重复测试 fixture helper。
- [x] `plans/CURRENT_PLAN.md` 状态已同步为已实施。

**Verification:**
- [ ] Search check: `rg "sqlite_path_from_suffix|fn sqlite_database_path|fn sqlite_path\\("`
- [ ] Tests pass: `cargo fmt --all --check`
- [ ] Tests pass: `git diff --check`

**Dependencies:** Tasks 1-9

**Files likely touched:**
- `plans/CURRENT_PLAN.md`
- touched Rust files from earlier tasks

**Estimated scope:** S

#### Task 11: 全量回归

**Description:** 跑完整回归，确认 parser、SQL store、Object store 的结构优化没有破坏 client state、registry、Telegram/Weixin 和 schema 路径。

**Acceptance criteria:**
- [ ] 所有 checkpoint 命令通过。
- [ ] Workspace tests 通过。
- [ ] `noloong-agent` clippy 通过。
- [ ] 最终 diff 无 whitespace error。

**Verification:**
- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent --test interaction_registry`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo test -p noloong-agent --features registry-store-object --test interaction_registry_store_object`
- [ ] `cargo test -p noloong schema`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `git diff --check`

**Dependencies:** Task 10

**Files likely touched:**
- None expected beyond implementation files

**Estimated scope:** S

## Parallelization Opportunities

- Tasks 3-5 must stay sequential because they change the same SQL list path.
- Tasks 6-9 must stay sequential because they change Object store index/read invariants.
- After Task 2, SQL tasks and Object store tasks can be implemented by separate agents if their write sets stay disjoint.
- Task 10 can run after SQL and Object store tasks both finish.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Toasty lacks efficient batch query primitives | Medium | Use explicit point-lookup threshold and all-row fallback; do not introduce raw SQL split. |
| Object store index write ordering leaves stale index | Medium | Treat record as final truth, clean stale index during list, and add failure-path tests where feasible. |
| Path-based metadata candidate decode mishandles malformed entries | Low | Invalid paths are skipped and treated as stale; final record validation remains required. |
| Shared parser changes CLI error wording | Low | Keep current semantics and verify schema/config tests; exact wording is not public API. |
| Bounded concurrency changes result order | Medium | Sort final records by `session_id` after all concurrent reads complete. |

## Not Doing

- 不新增外部配置项或 profile 字段。
- 不做通用 object store cache 或后台 reindexer。
- 不引入 sqlite/postgres raw SQL 双实现。
- 不保留旧重复 parser 或旧内部 URL 解析差异。
- 不做旧 object index 兼容迁移；内部项目可以清库重建。

## Open Questions

- 无。默认选择：复用共享 SQLite parser、SQL 不分叉 raw query、Object store 使用 bounded 并发和 record final validation。
