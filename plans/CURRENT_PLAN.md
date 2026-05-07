# 实施计划：Profile Config JSON Schema 与 JSONC 支持

> 状态：已完成实现、本地验证、workspace 测试和 clippy。目标是为 root `noloong` profile config 提供完整、可生成、可校验的 JSON Schema，并让 profile config loader 支持 JSONC。明确不引入 JSON5。

## 概览

当前 root `noloong` profile config 只通过 `serde_json::from_str` 解析，使用者需要阅读示例和 Rust 类型才能知道完整配置结构。下一步要把 `HostProfileConfig` 及其引用到的 profile/provider/plugin/manifest patch/store/compaction 类型生成一份 checked-in JSON Schema，同时提供 CLI 生成入口，便于 IDE、文档、第三方配置工具和 CI 共享同一份 contract。配置文件输入层增加 JSONC 支持，只承诺 JSON 加注释和 trailing comma，不扩大到 JSON5 的 unquoted key、single quote、hex number 等额外语法。

## 架构决策

- 使用 `schemars = "1.2"` 生成标准 JSON Schema；schema 是 profile config 的 editor/tooling contract，不替代 serde 作为 runtime parse contract。
- 使用 `jsonc-parser = "0.32"` 支持 profile config JSONC；不使用 `json5`，避免把更宽的 JSON5 方言变成长期兼容承诺。
- checked-in schema 放在 `schemas/profile-config.schema.json`，由 CLI 生成，CI 负责检测漂移。
- CLI 入口采用 `noloong profile-config schema`，默认输出到 stdout；额外支持写入文件和 check 模式，便于本地更新和 CI 校验。
- `noloong-agent-core` 与 `noloong-agent` 中被 profile config 引用的类型通过可选 `json-schema` feature 派生 `JsonSchema`，避免让 library crates 的默认依赖面无意义扩大。
- schema 不应比 serde 更严格：如果 Rust 类型没有 `deny_unknown_fields`，schema 不主动设置全局 `additionalProperties: false`，避免 IDE 报错与运行时行为不一致。
- JSONC 支持只作用于 root profile config 文件加载；JSON-RPC、model provider protocol、extension conformance fixture、Telegram API payload 等仍保持严格 JSON。

## 任务列表

### 阶段 1：依赖与 schema feature 基础

#### 任务 1：添加 schema 与 JSONC 依赖

**描述：** 在 workspace 依赖中加入 `schemars`、`jsonc-parser`，并为 schema validation 测试加入 `jsonschema` dev-dependency。`schemars` 在 root crate 直接使用，在 core/agent 作为 optional feature 使用。

**验收标准：**
- [x] workspace 依赖包含 `schemars = "1.2"`、`jsonc-parser = "0.32"`。
- [x] root `noloong` package 可以直接使用 `schemars` 和 `jsonc-parser`。
- [x] `noloong-agent-core` 增加可选 feature `json-schema = ["dep:schemars"]`。
- [x] `noloong-agent` 增加可选 feature `json-schema = ["dep:schemars", "noloong-agent-core/json-schema"]`。
- [x] schema/example validation 测试可使用 `jsonschema = "0.46"`，但不进入 runtime dependency。

**验证：**
- [x] `cargo check -p noloong-agent-core --features json-schema`
- [x] `cargo check -p noloong-agent --features json-schema`
- [x] `cargo check -p noloong`

**依赖：** 无

**预计涉及文件：**
- `Cargo.toml`
- `crates/noloong-agent-core/Cargo.toml`
- `crates/noloong-agent/Cargo.toml`

**预计范围：** S

#### 任务 2：为 profile config 引用类型派生 `JsonSchema`

**描述：** 为 root `src/config.rs` 中的 config 类型派生 `JsonSchema`，并在 `noloong-agent-core`、`noloong-agent` 中只为 profile config 实际引用到的公共类型补充 feature-gated `JsonSchema` derive。重点覆盖 `ContextCompactionMode`、plugin declaration、capability selector、manifest patch、approval policy、file edit policy、locale 等链路。

**验收标准：**
- [x] `HostProfileConfig`、`RuntimeProfileConfig`、provider/store/compaction/auth config 类型都能生成 schema。
- [x] `AgentPluginDeclaration`、`PluginTransport`、`PluginEnvSource`、`PluginLoadFailurePolicy` 在 `json-schema` feature 下实现 `JsonSchema`。
- [x] `ManifestPatch` 及其引用的 manifest/policy enum 在 `json-schema` feature 下实现 `JsonSchema`。
- [x] `ContextCompactionMode` 和 `ExtensionCapabilitySelector` 在 `json-schema` feature 下实现 `JsonSchema`。
- [x] 不为无关 runtime internals 大面积添加 schema derive。

**验证：**
- [x] `cargo check -p noloong`
- [x] `cargo check -p noloong-agent --features json-schema`
- [x] `cargo check -p noloong-agent-core --features json-schema`

**依赖：** 任务 1

**预计涉及文件：**
- `src/config.rs`
- `crates/noloong-agent-core/src/compaction.rs`
- `crates/noloong-agent-core/src/types/extension.rs`
- `crates/noloong-agent/src/plugin.rs`
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/src/approval/policy.rs`

**预计范围：** M

### 检查点：schema 类型基础

- [x] `cargo fmt --all --check`
- [x] `cargo check -p noloong`
- [x] `cargo check -p noloong-agent --features json-schema`
- [x] `cargo check -p noloong-agent-core --features json-schema`

### 阶段 2：schema 生成入口与 checked-in artifact

#### 任务 3：新增 profile config schema 生成模块

**描述：** 新增 root schema helper，集中生成 `HostProfileConfig` 的 schema `serde_json::Value` 与 canonical pretty JSON。该模块不读取文件、不处理 CLI，只负责把 Rust 类型 contract 转换成稳定 JSON artifact。

**验收标准：**
- [x] 提供 `profile_config_schema_value()` 或等价函数。
- [x] 提供 canonical pretty JSON 输出函数，末尾换行稳定。
- [x] schema 顶层有清晰 title，并包含 `HostProfileConfig` 的完整 `$defs`。
- [x] schema generation 不依赖网络、环境变量或运行时 profile。

**验证：**
- [x] 单元测试：schema 是 JSON object，包含 `$schema`、`$defs`、`profiles`。
- [x] 单元测试：canonical 输出可被 `serde_json::from_str` 重新解析。

**依赖：** 任务 2

**预计涉及文件：**
- `src/schema.rs`
- `src/main.rs`

**预计范围：** S

#### 任务 4：增加 `noloong profile-config schema` CLI

**描述：** 在 root CLI 中增加 profile config 子命令，支持生成 schema 到 stdout、写入文件、以及检查 checked-in schema 是否与当前类型一致。该命令只生成 schema，不加载实际 profile config。

**验收标准：**
- [x] `noloong profile-config schema` 将 schema 输出到 stdout。
- [x] `noloong profile-config schema --output schemas/profile-config.schema.json` 可写入 schema 文件。
- [x] `noloong profile-config schema --check schemas/profile-config.schema.json` 在文件内容匹配时退出成功。
- [x] check 模式不匹配时返回非零错误，并给出可诊断信息。
- [x] CLI help 中能看到 profile-config 子命令。

**验证：**
- [x] 单元测试：CLI 能解析 `profile-config schema`。
- [x] 单元测试：check 模式对匹配/不匹配内容返回正确结果。
- [x] 手工验证：`cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`

**依赖：** 任务 3

**预计涉及文件：**
- `src/main.rs`
- `src/schema.rs`

**预计范围：** M

#### 任务 5：提交 checked-in schema artifact

**描述：** 新增 `schemas/profile-config.schema.json`，内容必须来自 CLI 生成结果。该文件是第三方使用者、IDE 和文档引用的稳定入口。

**验收标准：**
- [x] 新增 `schemas/profile-config.schema.json`。
- [x] 文件内容与 `noloong profile-config schema` 生成结果一致。
- [x] schema 覆盖 provider variants：`chat_completions`、`responses`、`anthropic_messages`、`chatgpt_responses`。
- [x] schema 覆盖 `registryStore`、profile `eventStore`、`compaction`、`plugins`、`manifestPatches`、`metadata`。
- [x] schema 文件不包含真实 secret、真实 token 或机器私有路径。

**验证：**
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `serde_json` parse schema artifact 成功。

**依赖：** 任务 4

**预计涉及文件：**
- `schemas/profile-config.schema.json`

**预计范围：** S

### 检查点：schema artifact 闭环

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong schema`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`

### 阶段 3：JSONC profile config loader

#### 任务 6：让 `HostProfileConfig::load` 支持 JSONC

**描述：** 把 profile config 文件加载从直接 `serde_json::from_str` 调整为 JSONC parse -> `serde_json::Value` -> `HostProfileConfig`。普通 `.json` 文件继续可用；`.jsonc` 或带注释/trailing comma 的配置也可用。错误信息需要保留“读文件失败”和“解析配置失败”的清晰边界。

**验收标准：**
- [x] `HostProfileConfig::load` 可以读取带 `//`、`/* */` 注释的 profile config。
- [x] `HostProfileConfig::load` 可以读取带 trailing comma 的 profile config。
- [x] 普通 JSON 文件行为保持不变。
- [x] JSONC 语法错误返回 `CliConfigError::ParseConfig`，错误信息包含足够定位信息。
- [x] 不把 JSONC 支持扩散到 JSON-RPC、provider payload 或 extension protocol。

**验证：**
- [x] 单元测试：普通 JSON profile config load 成功。
- [x] 单元测试：JSONC comments + trailing comma profile config load 成功。
- [x] 单元测试：非法 JSONC 返回 parse error。

**依赖：** 任务 1

**预计涉及文件：**
- `src/config.rs`

**预计范围：** S

#### 任务 7：增加 `.jsonc` 示例配置

**描述：** 在 `examples/profile-configs` 下新增一个不含 secret 的 JSONC 示例，展示 `$schema`、注释和 trailing comma 的使用方式。现有 `.json` 示例保持严格 JSON，继续服务自动测试和最小运行路径。

**验收标准：**
- [x] 新增 `examples/profile-configs/telegram-openrouter-free.jsonc` 或等价示例。
- [x] 示例顶部包含 `"$schema": "../../schemas/profile-config.schema.json"`。
- [x] 示例包含注释和至少一个 trailing comma。
- [x] 示例不包含真实 API key、Telegram bot token 或 user id。
- [x] 现有 `.json` 示例不被改成 JSONC。

**验证：**
- [x] 单元测试或集成测试：`.jsonc` 示例可通过 `HostProfileConfig::load` 加载并 `validate()` 通过。
- [x] 手工检查示例路径中的 `$schema` 相对位置正确。

**依赖：** 任务 5、任务 6

**预计涉及文件：**
- `examples/profile-configs/*.jsonc`
- `src/config.rs`

**预计范围：** S

### 检查点：JSONC 输入体验

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong config::`
- [x] `cargo test -p noloong schema`

### 阶段 4：schema validation、CI 与文档

#### 任务 8：用 JSON Schema 校验现有 profile 示例

**描述：** 增加测试，使用 generated schema 校验 `examples/profile-configs/*.json` 的 JSON 数据。JSONC 示例先通过 JSONC parser 转成 JSON value，再走同一份 schema 校验。

**验收标准：**
- [x] 所有现有 `.json` profile examples 通过 schema validation。
- [x] 新增 `.jsonc` profile example 通过 schema validation。
- [x] validation 使用当前 generated schema，而不是手写简化 schema。
- [x] 测试失败时能指出失败的示例文件名。

**验证：**
- [x] `cargo test -p noloong schema`
- [x] `cargo test -p noloong config::`

**依赖：** 任务 3、任务 6、任务 7

**预计涉及文件：**
- `src/schema.rs`
- `src/config.rs`

**预计范围：** S

#### 任务 9：把 schema drift 检查接入 CI

**描述：** 在 GitHub Actions 中加入 schema artifact drift 检查，确保 Rust 类型变更没有忘记更新 `schemas/profile-config.schema.json`。

**验收标准：**
- [x] `.github/workflows/ci.yml` 增加 schema check step。
- [x] CI step 使用 `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json` 或等价命令。
- [x] 该 step 不依赖 secret、网络服务或本地绝对路径。

**验证：**
- [x] 本地运行 CI step 命令成功。
- [x] `cargo test --workspace` 成功。

**依赖：** 任务 4、任务 5

**预计涉及文件：**
- `.github/workflows/ci.yml`

**预计范围：** XS

#### 任务 10：更新文档说明 schema 与 JSONC

**描述：** 更新 README 和相关 docs，说明 profile config schema 的位置、生成方式、CI 检查方式、JSONC 支持范围，以及为什么不支持 JSON5。

**验收标准：**
- [x] `README.md` 展示 `$schema` 用法和 `noloong profile-config schema` 命令。
- [x] 文档说明 `.json` 示例仍是严格 JSON，`.jsonc` 示例可用于带注释配置。
- [x] 文档明确 JSONC 只用于 profile config，不用于 extension JSON-RPC 或 provider payload。
- [x] 文档明确不使用 JSON5 的原因：避免扩大配置语言兼容面。
- [x] 文档没有真实 secret。

**验证：**
- [x] 手工检查文档命令和实际 CLI 一致。
- [x] 手工检查 schema path 和示例 `$schema` 相对路径一致。

**依赖：** 任务 4、任务 7

**预计涉及文件：**
- `README.md`
- `crates/noloong-agent/docs/INTERACTION.md`
- `crates/noloong-agent/docs/ARCHITECTURE.md`

**预计范围：** S

### 检查点：文档与 CI

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong schema`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] 手工检查文档与示例不包含真实 secret。

### 阶段 5：全量验证与收尾

#### 任务 11：全量验证与计划状态更新

**描述：** 完成实现后跑完整 workspace 验证，确认 schema artifact、JSONC loader、CI step 和文档都已闭环，再把本计划状态改为已完成。

**验收标准：**
- [x] 所有任务验收项完成。
- [x] `plans/CURRENT_PLAN.md` 状态更新为已完成。
- [x] 没有新增 `#[allow(dead_code)]`。
- [x] 没有真实 secret 写入仓库。
- [x] checked-in schema 与 CLI 生成结果一致。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `git diff --check`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" crates src`

**依赖：** 任务 1-10

**预计涉及文件：**
- `plans/CURRENT_PLAN.md`

**预计范围：** XS

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| schema 与 serde runtime 行为漂移 | 高 | schema 由 Rust 类型生成；checked-in artifact 由 CLI check 和 CI 防漂移 |
| schema 比 runtime 更严格导致 IDE 假阳性 | 中 | 不主动添加 `additionalProperties: false`，除非 serde 同步启用严格字段 |
| 为 schema derive 扩散修改过多 core/agent 类型 | 中 | 只覆盖 profile config 引用链，使用 `json-schema` optional feature |
| JSONC 被误解为 JSON5 | 中 | 文档和错误信息明确只支持 JSONC，不支持 JSON5 扩展语法 |
| JSONC parser 错误信息不够友好 | 中 | 在 `CliConfigError::ParseConfig` 中保留 parser 的位置/上下文；测试覆盖非法 JSONC |
| checked-in schema 体积较大影响 review | 低 | schema 单独提交或单独文件，CLI check 保证可再生成 |

## 并行化机会

- 任务 2 和任务 6 可以在任务 1 后并行：schema derive 与 JSONC loader 互不阻塞。
- 任务 10 可以在任务 4 的 CLI 形状确定后提前写草案。
- 任务 8 必须等待任务 3、6、7 完成，因为它依赖 generated schema 和 JSONC 示例。
- 任务 9 必须等待任务 4、5 完成，否则 CI 没有稳定 artifact 可检查。

## 开放问题

- 无需额外产品决策。采用 `schemars` + `jsonc-parser`，不引入 JSON5；schema checked in，同时提供 CLI 生成和 CI drift 检查。
