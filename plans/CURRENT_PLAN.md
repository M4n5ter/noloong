# Implementation Plan: SQLite 运行状态统一化

## Overview

将 noloong 的可恢复运行状态统一落到本地 SQLite，默认使用 `~/.agents/noloong/state.sqlite`。本轮不保留历史兼容包袱：旧的默认 memory store、Telegram offset JSON 默认路径、示例里的显式 memory 配置都清理掉；显式非 SQLite backend 只在仍有当前价值时保留。ChatGPT token、Models.dev cache、Telegram 下载媒体、tool overflow 临时文件不是运行状态，不迁入 SQLite。

## Architecture Decisions

- 默认运行状态库是统一 SQLite 文件：`~/.agents/noloong/state.sqlite`。
- `NOLOONG_STATE_DATABASE_URL` 是唯一的全局默认状态库覆盖入口，接受现有 SQLite URL 形状。
- 省略 `registryStore` 和 profile `eventStore` 时都使用统一 SQLite；显式配置才走 memory/postgres/object。
- core `runId` 必须 session 命名空间化，否则多个 session 共用同一个 event table 时都会从 `run-1` 开始并产生主键冲突。
- Telegram polling offset 是可恢复运行状态，默认进统一 SQLite；删除默认 JSON checkpoint 逻辑。
- 没有兼容负担：旧默认、旧示例和不再需要的配置分支应直接删除或收敛，不新增兼容 shim。
- Toasty 相关依赖同步升级到 `0.6.0`；`rusqlite` 保持当前最新 `0.39.0`，除非 resolver 要求调整。

## Task List

### Phase 1: SQLite 默认状态库基础

#### Task 1: 定义统一状态数据库解析

**Description:** 增加 host 侧状态数据库解析入口，负责从默认路径或 `NOLOONG_STATE_DATABASE_URL` 得到 SQLite URL，并确保文件型 SQLite 的父目录存在。这个入口会被 registry、event store 和 Telegram offset 复用。

**Acceptance criteria:**
- [ ] 未设置环境变量时返回 `sqlite:~/.agents/noloong/state.sqlite` 展开后的文件 URL。
- [ ] 设置 `NOLOONG_STATE_DATABASE_URL` 时完全使用该值。
- [ ] 文件型 SQLite 连接前会创建父目录。

**Verification:**
- [ ] 单元测试覆盖默认路径、环境变量覆盖、空环境变量回退、父目录创建。
- [ ] `cargo test -p noloong config state_database`

**Dependencies:** None

**Files likely touched:**
- `src/config.rs`
- `src/host.rs`

**Estimated scope:** S

#### Task 2: 将 registryStore 默认改为 SQLite

**Description:** 把 root profile config 的 `registryStore` 从必填/默认 memory 收敛为“省略即统一 SQLite”。显式 memory 仍允许用于测试，但示例不再默认使用 memory。

**Acceptance criteria:**
- [ ] `HostProfileConfig` 省略 `registryStore` 时构建 SQLite registry store。
- [ ] 显式 `registryStore.type = memory/sqlite/postgres/object_fs` 仍按配置构建。
- [ ] 普通示例配置删除显式 memory registry store。

**Verification:**
- [ ] 配置测试覆盖省略 registryStore、显式 memory、显式 sqlite。
- [ ] SQLite registry reload 测试证明 session/goal/automation 可跨 registry rebuild 恢复。
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`

**Dependencies:** Task 1

**Files likely touched:**
- `src/config.rs`
- `src/host.rs`
- `examples/profile-configs/*.json`

**Estimated scope:** M

#### Task 3: 将 profile eventStore 默认改为 SQLite

**Description:** 把 profile `eventStore` 的默认值从 memory 改为统一 SQLite。事件日志是 paused approval resume、事件 replay 和诊断的运行状态，默认不能再是进程本地。

**Acceptance criteria:**
- [ ] 省略 profile `eventStore` 时构建统一 SQLite event store。
- [ ] 显式 memory 仅作为显式测试/临时运行选择保留。
- [ ] 现有 SQLite event store schema 初始化行为保持清晰，不新增旧 memory fallback。

**Verification:**
- [ ] profile build 测试覆盖省略 eventStore 后写入并 reload event。
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`
- [ ] `cargo test -p noloong-agent openai_wiring`

**Dependencies:** Task 1

**Files likely touched:**
- `src/config.rs`
- `src/host.rs`
- `schemas/profile-config.schema.json`

**Estimated scope:** M

### Checkpoint: 默认状态库基础

- [ ] 省略 registryStore/eventStore 的 profile 能启动。
- [ ] session snapshot 和 core event 都写入同一个 SQLite 文件。
- [ ] 示例配置不再把正常运行路径固定到 memory。

### Phase 2: Run ID 命名空间化

#### Task 4: 给 AgentRuntime 增加 run id 前缀

**Description:** 在 core runtime builder 增加 run id prefix 配置，并让 `next_run_id()` 生成带 session 命名空间的 id，例如 `run-<session-fingerprint>-1`。这是共享 SQLite event store 的前置安全条件。

**Acceptance criteria:**
- [ ] 默认 builder 在未设置 prefix 时仍能生成有效 run id。
- [ ] 设置 prefix 后所有新 run id 都包含该 prefix。
- [ ] run id 只包含 provider/tool/event/display 安全字符。

**Verification:**
- [ ] core runtime 单测覆盖默认 run id 和带 prefix run id。
- [ ] 事件 append/load 使用新 run id 成功。

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent-core/src/runtime/builder.rs`
- `crates/noloong-agent-core/src/runtime/run_loop.rs`

**Estimated scope:** S

#### Task 5: 从 AgentSession 注入稳定 run id 前缀

**Description:** host session 在构建 runtime 时基于 `sessionId` 生成稳定 fingerprint，并注入 core runtime。这样不同 session 即使共用同一个 event store，也不会在 `(run_id, sequence)` 主键上冲突。

**Acceptance criteria:**
- [ ] 同一个 session 重建后生成相同 run id prefix。
- [ ] 不同 session 的 run id prefix 不同。
- [ ] approval id、display id、goal audit metadata 等自然使用新 run id，不保留旧 `run-1` 假设。

**Verification:**
- [ ] registry 测试中两个 session 各跑一次，不出现 event store 主键冲突。
- [ ] paused approval resume 使用新 run id 能恢复。
- [ ] `cargo test -p noloong-agent --test interaction_registry`

**Dependencies:** Task 4

**Files likely touched:**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** M

### Checkpoint: 共享 event store 安全性

- [ ] 多 session 共用统一 SQLite event store 无 run id 冲突。
- [ ] approval resume、goal audit、display event 不依赖旧 run id 形状。
- [ ] `cargo test -p noloong-agent --test interaction_registry --test interaction_control`

### Phase 3: Telegram offset 迁入 SQLite

#### Task 6: 实现 SQLite Telegram offset store

**Description:** 为 Telegram polling offset 增加 SQLite store，表结构为 `telegram_offsets(bot_fingerprint primary key, offset, updated_at_ms)`。offset 属于 bridge 运行恢复状态，应和其他 host 状态保存在同一个 DB。

**Acceptance criteria:**
- [ ] SQLite offset store 支持 load/save。
- [ ] store 以 bot token fingerprint 为 key，不保存 bot token 明文。
- [ ] schema 初始化和统一状态 DB 使用同一连接策略。

**Verification:**
- [ ] 单元测试覆盖新 offset store 的空读、写入、覆盖、重建后读取。
- [ ] `cargo test -p noloong-agent-telegram telegram_offset`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-telegram/src/polling.rs`
- `src/main.rs`

**Estimated scope:** M

#### Task 7: 删除默认 JSON offset checkpoint 路径

**Description:** 默认 bridge 不再生成 `~/.agents/noloong/telegram/*.offset.json`。删除默认路径函数和相关文档。若仍保留 `FileTelegramOffsetStore`，只作为显式 CLI/env 诊断路径使用；如果实现后没有当前用途，则直接删除。

**Acceptance criteria:**
- [ ] 未传 `--telegram-offset-checkpoint` 时使用 SQLite offset store。
- [ ] 不再存在默认 JSON checkpoint path。
- [ ] 文档不再建议默认 JSON checkpoint。

**Verification:**
- [ ] bridge config 测试覆盖默认 SQLite offset 和显式 checkpoint 行为。
- [ ] `cargo test -p noloong-agent-telegram`

**Dependencies:** Task 6

**Files likely touched:**
- `src/main.rs`
- `crates/noloong-agent-telegram/docs/TELEGRAM.md`

**Estimated scope:** S

### Checkpoint: Telegram 恢复状态

- [ ] Telegram bridge 默认 offset 存入统一 SQLite。
- [ ] 重启 bridge 不会 replay 已处理 update。
- [ ] 不再创建默认 `.offset.json` 文件。

### Phase 4: 依赖升级和清理

#### Task 8: 升级 Toasty 相关依赖

**Description:** 将 Toasty 生态升级到当前版本，并按最小必要改动适配 SQL registry store 和 SQLite event store。不要顺带做无关重构。

**Acceptance criteria:**
- [ ] `toasty`, `toasty-driver-sqlite`, `toasty-driver-postgresql` 升到 `0.6.0`。
- [ ] `Cargo.lock` 只包含升级所需变更。
- [ ] SQL store 和 event store 编译通过。

**Verification:**
- [ ] `cargo update -p toasty -p toasty-driver-sqlite -p toasty-driver-postgresql`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`

**Dependencies:** None

**Files likely touched:**
- `Cargo.toml`
- `Cargo.lock`
- SQL store files only if Toasty 0.6 API requires it.

**Estimated scope:** S

#### Task 9: 清理配置 schema、文档和示例

**Description:** 把文档从“memory 默认、SQLite 可选”改成“SQLite 默认、memory 显式临时”。删除历史妥协措辞和过时示例，确保 schema 与新默认一致。

**Acceptance criteria:**
- [ ] profile schema 反映 `registryStore` 和 `eventStore` 可省略。
- [ ] architecture/interaction/telegram 文档说明统一 state DB。
- [ ] 示例配置不保留无意义 memory 默认。

**Verification:**
- [ ] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [ ] `cargo test -p noloong schema`

**Dependencies:** Tasks 2, 3, 7

**Files likely touched:**
- `schemas/profile-config.schema.json`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent-telegram/docs/TELEGRAM.md`

**Estimated scope:** M

### Checkpoint: 完整回归

- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`
- [ ] `cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite`
- [ ] `cargo test -p noloong-agent --test interaction_registry --test interaction_control`
- [ ] `cargo test -p noloong-agent-telegram`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| 多 session 共享 event store 后 run id 冲突 | High | 先完成 run id 命名空间化，再启用默认 SQLite eventStore。 |
| Toasty 0.6 API 破坏 SQL store 编译 | Medium | 依赖升级独立成任务，先跑最小 SQL/event 测试再扩大回归。 |
| 默认 SQLite 目录权限或路径不可用 | Medium | 统一入口创建父目录并给出明确错误，不回退 memory。 |
| Telegram offset 迁移遗漏导致 update replay | Medium | 默认 SQLite offset store 必须有重启读取测试和一次真实 Telegram smoke。 |
| 示例仍显式 memory 误导后续使用 | Low | 清理普通示例，只在测试/隔离 smoke 中保留 memory。 |

## Parallelization Opportunities

- Task 8 依赖升级可和 Task 1-3 并行，但最终需要统一回归。
- Task 6 的 SQLite offset store 可在 Task 1 的状态 DB helper 定稿后并行实现。
- Task 9 文档/schema 必须等实现形状稳定后再做。

## Open Questions

- 无。当前决策是：没有兼容负担，默认统一 SQLite，旧默认 memory/JSON checkpoint 路径直接清理。
