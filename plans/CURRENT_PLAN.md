# 实施计划：产品级插件系统

> 状态：已完成实现、本地验证、workspace 测试和 clippy。

## 概览

当前仓库已经有语言无关的 stdio JSON-RPC extension 协议、Rust 侧 `StdioExtensionConfig`、runtime 注册入口、conformance runner，以及 TypeScript/Python 示例；但这还不是产品级插件系统。现在缺的是：插件不能作为 profile/session/manifest 的一等配置被声明、审批、持久化、启用、禁用和恢复；agent 也不能通过现有 `agent.manifest.propose_patch` 工具安全地提出“安装或启用某个插件”。本轮目标是补齐这个产品层插件系统，同时继续复用已有 extension wire contract，不重新设计一套协议。

## 架构决策

- 插件 v1 只支持 stdio JSON-RPC transport；这是当前最稳定、最低依赖、跨语言最清晰的边界。
- `noloong-agent-core` 继续负责 extension 进程连接、wire contract、capability 注册和 conformance；`noloong-agent` 负责插件声明、审批、manifest patch、session 恢复和产品安全策略。
- 插件声明进入 `AgentManifest`，这样 session snapshot 可以完整恢复插件状态；root profile config 只提供默认插件声明，创建 session 时通过 default manifest patches 或 profile defaults 落到 manifest。
- agent 不能直接安装插件，只能通过 `agent.manifest.propose_patch` 提出 `register_plugin`、`set_plugin_enabled`、`remove_plugin` 等 patch；真正生效仍然走 human approval。
- 插件命令使用直接 exec：`command + args + cwd`，不经过 shell，不支持内联脚本字符串。
- 插件环境默认隔离：不继承宿主环境；只能显式映射环境变量名，配置中不能保存 secret literal。
- 插件能力必须有显式 allowlist；extension 进程可以声明更多 capabilities，但 runtime 只注册插件声明允许的部分。
- v1 不做远程下载、签名校验、marketplace、版本锁定、MCP adapter、热插拔；这些保留为后续演进，避免把“接入插件”与“分发插件”混成一个大系统。

## 任务列表

### 阶段 1：插件声明与 manifest patch 基础

#### 任务 1：新增插件声明数据模型

**描述：** 在 `noloong-agent` 中定义产品级插件声明，挂到 `AgentManifest`。声明需要表达插件身份、显示信息、stdio transport、环境变量映射、允许的 capability、启用状态和加载失败策略。

**验收标准：**
- [x] `AgentManifest` 新增 `plugins: BTreeMap<String, AgentPluginDeclaration>`，默认空集合。
- [x] `AgentPluginDeclaration` 至少包含 `pluginId`、`displayName`、`description`、`transport`、`allowedCapabilities`、`enabled`、`onLoadFailure`。
- [x] `PluginTransport::Stdio` 支持 `command`、`args`、`cwd`、`env`、`requestTimeoutSecs`、`streamTimeoutSecs`。
- [x] `PluginEnv` 只允许把宿主环境变量名映射进插件环境，不允许 JSON 配置中保存 secret literal。
- [x] 所有新增 manifest 字段保持 `camelCase` wire format。

**验证：**
- [x] 单元测试：空 manifest 反序列化保持兼容。
- [x] 单元测试：插件声明 JSON round-trip 后字段稳定。
- [x] 单元测试：空 `pluginId`、空 `command`、重复/空 capability 被拒绝。

**依赖：** 无

**预计涉及文件：**
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**预计范围：** M

#### 任务 2：扩展 manifest patch 和 proposal summary

**描述：** 让 agent 可以通过现有 manifest proposal 工具提出插件变更。patch 本身需要严格校验，approval 摘要必须把将要启动的命令、参数、cwd、环境变量名、允许能力和启用状态展示清楚。

**验收标准：**
- [x] `ManifestPatch` 新增 `RegisterPlugin`、`SetPluginEnabled`、`RemovePlugin`。
- [x] `AgentManifest::apply_patch` 可以添加、启用、禁用和移除插件声明。
- [x] 注册已存在 `pluginId`、启用不存在插件、移除不存在插件时返回结构化错误。
- [x] `ManifestPatch::summary()` 和 i18n catalog 展示插件命令、能力 allowlist、env var names，不展示 secret value。
- [x] `agent.manifest.propose_patch` 的 input schema 能反映新增插件 patch 结构。

**验证：**
- [x] 单元测试：三类 plugin patch 的 apply 成功路径。
- [x] 单元测试：非法 patch 被拒绝且不会修改 manifest。
- [x] 单元测试：proposal summary 不包含环境变量真实值。

**依赖：** 任务 1

**预计涉及文件：**
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/src/tools/manifest.rs`
- `crates/noloong-agent/src/catalog.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**预计范围：** M

### 检查点：manifest 基础

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent manifest`
- [x] `cargo test -p noloong-agent agent_session`
- [x] 手工检查 manifest proposal 的 JSON schema 和 approval 文案不会误导用户授权未知命令。

### 阶段 2：core extension 加固与 capability allowlist

#### 任务 3：扩展 `StdioExtensionConfig` 的进程启动选项

**描述：** 让 core 的 stdio extension 启动配置支持产品层插件所需的最小进程控制：工作目录、环境变量、是否清空环境，以及可选的 capability allowlist。保持手动 Rust API 仍然可用。

**验收标准：**
- [x] `StdioExtensionConfig` 新增 `cwd`、`env`、`clearEnv`、`allowedCapabilities`。
- [x] 默认行为保持当前手动 API 兼容：不设置 `clearEnv` 时不破坏现有 extension tests。
- [x] 插件 loader 后续可以显式使用 `clearEnv = true`。
- [x] `Command` 启动时正确应用 cwd/env/env_clear，stderr 仍只作为日志通道。

**验证：**
- [x] 单元测试：config builder 能设置 cwd/env/clear env。
- [x] 集成测试：fixture extension 可以读取显式传入的 env，不能读取被隔离的 env。
- [x] 现有 JSON-RPC conformance tests 保持通过。

**依赖：** 无

**预计涉及文件：**
- `crates/noloong-agent-core/src/jsonrpc/mod.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**预计范围：** M

#### 任务 4：实现 capability allowlist 过滤

**描述：** extension 可以声明多个 capability，但插件声明必须决定哪些 capability 进入 runtime。过滤要在 duplicate validation 和注册前完成，避免未授权工具、hook、model provider 混入 runtime。

**验收标准：**
- [x] `AgentRuntimeBuilder::with_stdio_extension` 在注册前按 allowlist 过滤 capabilities。
- [x] allowlist 支持按 capability kind 和 id/tool name 精确匹配。
- [x] 插件未允许的 capability 不会注册，也不会影响 duplicate validation。
- [x] 当 allowlist 为空时，产品层插件默认注册零能力；手动 Rust API 可以继续选择“全部允许”的兼容路径。
- [x] 错误信息能区分“extension malformed”和“capability not allowed”。

**验证：**
- [x] 测试：fixture 同时声明 tool/model/hook，只 allow tool 时只注册 tool。
- [x] 测试：未授权 model provider 不能成为 default model provider。
- [x] 测试：未授权 capability 与已有内置能力同名时不会触发 duplicate error。

**依赖：** 任务 3

**预计涉及文件：**
- `crates/noloong-agent-core/src/runtime/builder.rs`
- `crates/noloong-agent-core/src/jsonrpc/mod.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**预计范围：** M

### 检查点：core 加固

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [x] `cargo test -p noloong-agent-core --test extension_conformance`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

### 阶段 3：session runtime 插件加载

#### 任务 5：实现 manifest 插件到 runtime 的装配

**描述：** 在 `AgentSession::runtime_builder` 或相邻模块中读取 manifest 中已启用插件，转换为 `StdioExtensionConfig` 并加载进 runtime。加载失败策略由插件声明控制。

**验收标准：**
- [x] 已启用插件会在 session runtime build 时启动并注册允许的 capabilities。
- [x] 已禁用插件不会启动进程。
- [x] `onLoadFailure = "disable_for_run"` 时，本轮 build 记录 warning/metadata，但 runtime 仍可继续构建。
- [x] `onLoadFailure = "fail_run"` 时，插件加载失败会阻止 runtime build 并返回结构化错误。
- [x] 插件进程生命周期仍由 core runtime 持有，runtime drop 时进程被清理。

**验证：**
- [x] 集成测试：manifest 中启用 fixture plugin 后，runtime 能看到插件 tool。
- [x] 集成测试：禁用插件不会启动 fixture process。
- [x] 集成测试：失败插件在两种 `onLoadFailure` 策略下行为不同。

**依赖：** 任务 1、任务 3、任务 4

**预计涉及文件：**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/plugin.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**预计范围：** M

#### 任务 6：明确恢复 live session 时的插件重建策略

**描述：** 已持久化 session 通过 registry store 恢复 live runtime 时，需要使用 snapshot 中的 manifest 插件声明重新启动插件，而不是依赖当前进程的临时 builder 状态。

**验收标准：**
- [x] session snapshot 中的 manifest 完整包含 plugins 字段。
- [x] live restore 时按 snapshot manifest 重建 enabled plugins。
- [x] 插件命令缺失、cwd 不存在、env var 缺失时错误可诊断。
- [x] read-only `session/list` / `session/get` 不启动插件进程。

**验证：**
- [x] registry restore 测试：只读读取不会构建 runtime。
- [x] registry restore 测试：恢复并运行时会启动 snapshot manifest 中的插件。
- [x] registry restore 测试：缺失 env var 返回明确错误。

**依赖：** 任务 5

**预计涉及文件：**
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/store/snapshot.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** M

### 检查点：runtime 装配

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent agent_session`
- [x] `cargo test -p noloong-agent interaction_registry`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

### 阶段 4：profile config、交互协议与示例

#### 任务 7：给 root profile config 增加默认插件声明

**描述：** root `RuntimeProfileConfig` 应允许配置默认插件，让 `noloong telegram`、`noloong serve interaction` 等入口启动 session 时天然带有一组 host-approved 插件声明。

**验收标准：**
- [x] `RuntimeProfileConfig` 支持 `plugins` 字段，结构复用 `AgentPluginDeclaration`。
- [x] profile build 时先校验默认插件声明，再把它们注入新 session manifest。
- [x] profile 默认插件和 `manifestPatches` 的顺序明确：先应用 profile defaults，再应用 manifest patches。
- [x] 示例配置中可以声明 TypeScript/Python stdio 插件，不包含 secret literal。

**验证：**
- [x] `cargo test -p noloong config::`
- [x] `cargo test -p noloong host::`
- [x] 示例 profile config build test 通过。

**依赖：** 任务 1、任务 2

**预计涉及文件：**
- `src/config.rs`
- `src/host.rs`
- `examples/profile-configs/plugin-stdio-example.json`

**预计范围：** M

#### 任务 8：在 interaction 文档中暴露插件工作流

**描述：** 不新增独立 plugin RPC；v1 复用 manifest proposal/apply 流程。需要把“agent 提议、human 审批、apply 后下一次 runtime build 生效”的流程写清楚，并给出 JSON 示例。

**验收标准：**
- [x] `INTERACTION.md` 增加 plugin manifest patch 示例。
- [x] 文档说明 read-only session 操作不会启动插件。
- [x] 文档说明插件启用/禁用后何时生效：下一次 runtime build/run 生效，不做 hot reload。
- [x] 文档说明外部 bridge 不能直接提交 provider credentials，只能触发 manifest proposal/approval 流程。

**验证：**
- [x] 文档中的 JSON 字段名与 serde wire format 一致。
- [x] 文档不出现真实 token、真实代理地址或机器私有路径。

**依赖：** 任务 2、任务 7

**预计涉及文件：**
- `crates/noloong-agent/docs/INTERACTION.md`
- `README.md`

**预计范围：** S

#### 任务 9：补充插件作者和使用者文档

**描述：** 现有 `EXTENSIONS.md` 面向 extension 作者；本任务补充产品层“如何被 noloong 作为插件加载”的文档，包括 manifest 声明、profile config、allowlist、安全模型和 conformance。

**验收标准：**
- [x] 新增或更新插件使用文档，区分 extension wire contract 与 product plugin declaration。
- [x] TypeScript/Python 示例说明如何通过 profile config 加载。
- [x] 文档给出 `noloong-extension-conformance` 的推荐运行方式。
- [x] 文档明确 v1 不支持 shell string、remote install、hot reload、secret literal。

**验证：**
- [x] 文档示例命令可在仓库根目录执行。
- [x] 示例 extension 的 README 与 root docs 不互相矛盾。

**依赖：** 任务 7、任务 8

**预计涉及文件：**
- `crates/noloong-agent-core/docs/EXTENSIONS.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `examples/extensions/typescript-conformance/README.md`
- `examples/extensions/python-conformance/README.md`
- `README.md`

**预计范围：** S

### 检查点：产品接入

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong`
- [x] `cargo test -p noloong-agent`
- [x] `cargo test -p noloong-agent-core`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`

### 阶段 5：端到端验证与收尾

#### 任务 10：增加端到端插件 smoke tests

**描述：** 用现有 TypeScript/Python conformance fixture 或新增最小 fixture，验证 profile 默认插件、manifest proposal 插件、禁用插件和 capability allowlist 的完整链路。

**验收标准：**
- [x] profile 默认插件可以被加载，并暴露一个允许的 test tool。
- [x] agent 提出的 `register_plugin` patch 经 approval/apply 后，下一轮 runtime build 能加载插件。
- [x] `set_plugin_enabled(false)` 后插件能力从 runtime 消失。
- [x] 未被 allowlist 允许的 tool/model/hook 不会注册。

**验证：**
- [x] `cargo test -p noloong-agent plugin`
- [x] `cargo test -p noloong-agent-core --test extension_language_examples`
- [x] TypeScript 示例在依赖可用时通过 strict conformance。
- [x] Python 示例通过 strict conformance。

**依赖：** 任务 5、任务 7

**预计涉及文件：**
- `crates/noloong-agent/tests/plugin.rs`
- `crates/noloong-agent-core/tests/extension_language_examples.rs`
- `examples/extensions/typescript-conformance/README.md`
- `examples/extensions/python-conformance/README.md`

**预计范围：** M

#### 任务 11：全量检查与架构文档更新

**描述：** 收尾阶段跑完整验证，更新架构文档中的后续演进方向，确保插件系统边界、非目标和未来扩展路径清晰。

**验收标准：**
- [x] `ARCHITECTURE.md` 描述 product plugin layer、extension layer、manifest approval layer 的关系。
- [x] `ARCHITECTURE.md` 的后续演进方向更新为 marketplace/signature/version lock/MCP adapter/hot reload，而不是继续写“接入插件”本身。
- [x] 没有新增 `#[allow(dead_code)]`。
- [x] 没有把 secret、真实 bot token、真实 API key 写入仓库。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `git diff --check`
- [x] 手工 review 文档与示例配置中的 secret 泄漏风险。

**依赖：** 任务 1-10

**预计涉及文件：**
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`
- 其它被实现任务实际触及的文件

**预计范围：** S

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 插件命令被 agent 轻易注册后执行任意代码 | 高 | agent 只能 proposal；human approval 必须展示 command/args/cwd/env/capabilities；默认不继承 env |
| extension 声明了超出预期的工具或 hooks | 高 | capability allowlist 在注册前过滤；默认产品插件不允许任何能力 |
| session 恢复时意外启动插件进程 | 中 | read-only descriptor 操作不构建 runtime；只有 run/mutation 恢复 live session 时启动 |
| 插件加载失败导致产品不可用 | 中 | 插件级 `onLoadFailure` 策略，默认 `disable_for_run`；关键插件可配置 `fail_run` |
| profile defaults、manifest patches、snapshot 恢复顺序混乱 | 中 | 明确顺序并加 registry/session restore 测试 |
| 为未来 marketplace 过早设计过多结构 | 中 | v1 只做本地 arbitrary command 插件声明；远程分发、签名、版本锁后置 |

## 并行化机会

- 任务 1 和任务 3 可以并行：一个在 `noloong-agent` 数据模型层，一个在 `noloong-agent-core` 进程配置层。
- 任务 8 和任务 9 可以在任务 2/7 的 schema 稳定后并行。
- 任务 10 的 TypeScript/Python fixture 验证可以与文档收尾并行，但必须等任务 5/7 的 runtime 装配完成。

## 已定决策

- v1 不新增 `noloong plugin list` / `noloong plugin validate` CLI；先通过 profile config 和 manifest proposal 暴露，保持表面积小。
- 插件 allowlist 不支持 wildcard，只做 capability kind + id/tool name 精确匹配，降低审批误解风险。
- 插件加载失败的 warning 记录在 `AgentSessionRuntimeBuilder` 本轮 build 结果中；`fail_run` 策略会返回结构化 build error。后续若需要 UI 主动展示，可把 warning 投射到 display/event 层。
