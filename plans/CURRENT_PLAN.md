# 实施计划：为 Runtime Profile 增加 Event Store 配置

> 状态：已完成实现、本地验证、workspace 测试和 clippy。

## 概览

当前 `noloong-agent` 已经通过 `AgentSessionRegistryStore` 持久化 application session snapshot，包括 `AgentManifest`、`AgentState`、steering/follow-up queue、profile id、metadata 和时间戳；但 root `noloong` host 还没有把 `noloong-agent-core` 的 run-level `EventStore` 暴露成 profile 配置。core runtime 因此默认使用 `InMemoryEventStore`，普通会话内容可以通过 snapshot 恢复，但 paused approval resume、run event replay 和严格审计顺序在进程重启后还不够闭环。本轮目标是在 root profile config 中增加 `eventStore`，v1 先支持 memory 和 SQLite，并在文档中明确它与 `registryStore` 的职责边界。

## 架构决策

- `eventStore` 是 profile 级配置，不是全局配置；runtime 由 `AgentRuntimeProfile` 构建，session 恢复时通过 snapshot 中的 `profileId` 找回对应 profile，因此 event log 后端也应绑定到 profile。
- `registryStore` 不改变语义：它保存 session snapshot，用于 `session/list`、`session/get`、lazy restore 和 session 目录恢复。
- `eventStore` 保存 core `AgentEvent` append-only log，用于 run-level replay、approval resume、tool permission audit 和事件顺序审计。
- v1 只暴露 `memory` 和 `sqlite` event store；PostgreSQL/object event store 后续应作为 core `EventStore` backend 单独实现，不复用 registry store backend。
- `eventStore` 默认 `memory`，保持现有行为；需要跨进程恢复 paused approval 时必须使用持久 SQLite file URL，而不是 `sqlite::memory:`。
- root crate 需要启用 `noloong-agent-core/sqlite-store` feature，否则无法在 host 层构造 `SqliteEventStore`。

## 任务列表

### 阶段 1：配置模型与依赖

#### 任务 1：新增 profile 级 `eventStore` 配置模型

**描述：** 在 root `src/config.rs` 中新增 `ProfileEventStoreConfig`，并挂到 `RuntimeProfileConfig`。配置使用 `camelCase` wire format，默认值为 memory，SQLite 配置复用 core `SqliteEventStoreConfig` 的 URL 与 migration 语义。

**验收标准：**
- [x] `RuntimeProfileConfig` 新增 `event_store: ProfileEventStoreConfig`，serde 字段名为 `eventStore`。
- [x] `ProfileEventStoreConfig` 至少支持 `{"type":"memory"}` 和 `{"type":"sqlite","databaseUrl":"...","migrateOnConnect":true}`。
- [x] `eventStore` 缺省时等价于 memory。
- [x] `migrateOnConnect` 缺省为 `true`；配置为 `false` 时由 core SQLite store 执行现有 schema 校验。
- [x] 配置模型不影响已有 profile config 解析。

**验证：**
- [x] 单元测试：缺省 `eventStore` 解析为 memory。
- [x] 单元测试：SQLite event store config round-trip/parse 字段稳定。
- [x] 单元测试：`migrateOnConnect=false` 可以被正确解析。

**依赖：** 无

**预计涉及文件：**
- `src/config.rs`

**预计范围：** S

#### 任务 2：启用 root crate 的 core SQLite event store feature

**描述：** root `noloong` binary 需要能构造 `SqliteEventStore`，因此 workspace root 对 `noloong-agent-core` 的依赖要启用 `sqlite-store` feature。保持 library crates 的默认 feature 表面不扩大。

**验收标准：**
- [x] root `Cargo.toml` 中 `noloong-agent-core` 依赖启用 `sqlite-store`。
- [x] 不强制 `noloong-agent` 默认启用 core SQLite event store。
- [x] workspace clippy 不出现 feature 组合下的 unused/dead code 问题。

**验证：**
- [x] `cargo check -p noloong`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

**依赖：** 任务 1

**预计涉及文件：**
- `Cargo.toml`

**预计范围：** XS

### 检查点：配置基础

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong config::`
- [x] `cargo check -p noloong`

### 阶段 2：Host runtime wiring

#### 任务 3：构建并注入 profile event store

**描述：** 在 root `src/host.rs` 中为每个 runtime profile 构建一个 `Arc<dyn EventStore>`，并在 `RuntimeProfile::build_runtime` 中调用 `session.runtime_builder().with_event_store(...)`。profile 构建需要改为 async，以便 SQLite event store 可以在 build registry 时连接和迁移 schema。

**验收标准：**
- [x] `RuntimeProfile` 持有 `Arc<dyn EventStore>`。
- [x] `build_registry()` 为每个 profile 构建 provider、compaction、event store 后注册 profile。
- [x] `RuntimeProfile::build_runtime()` 在注册 model provider、compaction、plugins 前后都保持行为稳定，并注入同一个 profile event store。
- [x] memory event store 继续保持当前默认行为。
- [x] SQLite event store 构建失败时返回清晰 `HostBuildError`。

**验证：**
- [x] 单元测试：默认 memory event store 下现有 profile build 测试保持通过。
- [x] 单元测试：SQLite event store file URL 能构建 registry。
- [x] 单元测试：无效 SQLite URL 或缺失 schema 且 `migrateOnConnect=false` 返回可诊断错误。

**依赖：** 任务 1、任务 2

**预计涉及文件：**
- `src/host.rs`
- `src/config.rs`

**预计范围：** M

#### 任务 4：补充跨重建 event replay 验证

**描述：** 增加 host 层测试，证明 profile 配置的 SQLite event store 不只是能构建，还能跨重新连接读取已写入的 run events。这不要求自动恢复 running run，只验证 core event log 的持久化语义和 host wiring 正确。

**验收标准：**
- [x] 测试使用临时 SQLite 文件，第一次通过 profile event store append event。
- [x] 第二次重新 build registry 或重新 connect 同一路径后可以 load 同一 run id 的 event。
- [x] 测试清理 SQLite、WAL、SHM 临时文件。
- [x] 不依赖外部数据库或网络。

**验证：**
- [x] `cargo test -p noloong host::`
- [x] `cargo test -p noloong-agent-core --features sqlite-store --test sqlite_store`

**依赖：** 任务 3

**预计涉及文件：**
- `src/host.rs`

**预计范围：** S

### 检查点：runtime wiring

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong host::`
- [x] `cargo test -p noloong-agent --test interaction_registry`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

### 阶段 3：示例与文档

#### 任务 5：更新示例 profile config

**描述：** 更新至少一个 `examples/profile-configs` 文件，展示 `registryStore` 和 profile `eventStore` 同时配置。示例应避免真实 secret 和机器私有路径。

**验收标准：**
- [x] 示例中 `registryStore` 仍位于 host config 顶层。
- [x] 示例中 `eventStore` 位于具体 profile 下。
- [x] 示例使用安全占位路径或相对路径，不包含真实 token。
- [x] 示例配置 build test 覆盖新增字段。

**验证：**
- [x] `cargo test -p noloong config::`
- [x] `cargo test -p noloong host::`

**依赖：** 任务 1、任务 3

**预计涉及文件：**
- `examples/profile-configs/*.json`
- `src/config.rs`
- `src/host.rs`

**预计范围：** S

#### 任务 6：更新架构与交互文档

**描述：** 文档需要清楚说明 event store 的作用，以及它和 registry store 的区别，避免使用者误以为配置了 registry store 就具备完整 run-level replay 能力。

**验收标准：**
- [x] `crates/noloong-agent/docs/ARCHITECTURE.md` 明确两类 store 的职责、数据内容、恢复路径和限制。
- [x] `crates/noloong-agent/docs/INTERACTION.md` 在 Sessions and Profiles 部分说明 `eventStore` 对 paused approval resume 的影响。
- [x] root `README.md` 给出最小配置示例。
- [x] 文档说明 `running` session 仍不会自动 continuation；event store 只提供 replay/resume 所需事件，不改变保守恢复策略。
- [x] 文档说明 v1 event store 只支持 memory/SQLite，PostgreSQL/object 是后续演进。

**验证：**
- [x] 手工检查文档示例字段名与 serde wire format 一致。
- [x] 手工检查文档没有真实 secret、真实 API key、真实 bot token。

**依赖：** 任务 1、任务 3、任务 5

**预计涉及文件：**
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/docs/INTERACTION.md`
- `README.md`

**预计范围：** S

### 检查点：文档与示例

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong config::`
- [x] `cargo test -p noloong host::`
- [x] 手工检查示例配置和文档字段名。

### 阶段 4：收尾验证

#### 任务 7：全量验证与计划状态更新

**描述：** 完成实现后跑完整本地验证，并把本计划状态从待实现更新为已完成。确认没有新增 `#[allow(dead_code)]`，没有 secret 泄漏。

**验收标准：**
- [x] 所有任务验收项完成。
- [x] `plans/CURRENT_PLAN.md` 状态更新为已完成。
- [x] 没有新增 `#[allow(dead_code)]`。
- [x] 没有把真实 secret 写入仓库。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `git diff --check`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" crates src`
- [x] 使用已知敏感凭据片段执行仓库扫描，确认除扫描命令本身外无命中；计划文件不保留真实片段。

**依赖：** 任务 1-6

**预计涉及文件：**
- `plans/CURRENT_PLAN.md`

**预计范围：** XS

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 使用者误以为 registry store 等于完整运行时恢复 | 高 | 文档明确 snapshot store 和 event log store 分层职责 |
| SQLite event store 配了 `sqlite::memory:` 但期待跨进程恢复 | 中 | 文档和示例强调需要 file URL 才能跨进程持久化 |
| profile 构建改为 async 影响现有 build_registry 测试 | 中 | 保持 public `build_registry` async API 不变，只调整内部 profile 构建 |
| event store 与 registry store 使用不同生命周期导致 paused approval 无法恢复 | 中 | 文档说明同一 profile 必须稳定指向同一持久 event store |
| 过早扩展 PostgreSQL/object event store | 中 | v1 只接入已有 core SQLite event store，后续 backend 单独规划 |

## 并行化机会

- 任务 1 和任务 6 可以部分并行：先确定配置字段名后即可写文档草案。
- 任务 3 和任务 5 不能并行完成：示例 build test 依赖 host wiring。
- 任务 4 可以在任务 3 完成后独立验证，不影响文档更新。

## 开放问题

- 无需额外产品决策。默认采用 profile 级 `eventStore`、v1 memory/SQLite、`migrateOnConnect=true`、不改变 running session 保守恢复策略。
