# Implementation Plan: Noloong Skills 与 MCP 插件机制

## Overview

本轮为 noloong 增加原生 plugin 机制的第一版可用能力：一个 plugin 可以提供 Skills、MCP servers，以及现有 noloong stdio extension。Skills 行为参考 `openai/codex` 的实际实现：默认只把 skill metadata 和可读取路径注入模型上下文；显式 mention 时 host 自动读取并注入 `SKILL.md` 正文；其它语义匹配场景由 agent 根据 metadata 中的 path 自行读取。MCP 使用官方 `rmcp`，支持 stdio 和 streamable HTTP，不支持 legacy SSE。项目没有兼容性包袱，旧 plugin wire shape 直接替换，不保留迁移分支。

## Implementation Status

- [x] Phase 1 Plugin Component Foundation：已将旧顶层 plugin transport 替换为 `components`，并完成 manifest schema、summary、i18n 相关测试与 stdio extension runtime 迁移。
- [x] Phase 2 Skills Loader and Metadata Context：已实现 skills loader、2% input window metadata budget、默认 metadata 注入、`$skill` / `skill://...` / structured selection 的 turn-local `SKILL.md` 正文注入。
- [x] Phase 3 MCP Runtime：已接入 `rmcp`，支持 stdio 与 streamable HTTP client，MCP tools 映射为 `ToolProvider`，并覆盖 tool filters、secret redaction、真实 stdio/HTTP tool call 测试。
- [x] Phase 4 Docs, Examples, and Regression：已更新 plugin 示例、增加 skills/MCP 示例、更新 extension/interaction 文档，并完成格式、测试与 clippy 回归。

## Architecture Decisions

- 插件是 `noloong-agent` 的 host/runtime 层能力，不下沉到 `noloong-agent-core`；core 继续只认 `ToolProvider`、`ContextProvider` 和 stdio extension bridge。
- 旧 `AgentPluginDeclaration.transport` 改成 `components`，组件类型为 `skills`、`mcp`、`noloong_extension`；现有 stdio extension 能力保留为 `noloong_extension` 组件。
- Skills 不是 built-in tools；不新增 `agent.skill.read/search/list`。模型通过注入的 metadata path 使用现有 host 文件/命令能力读取 skill 资源。
- Skills metadata 预算为当前主模型真实 input window 的 2%；超预算策略按 Codex 风格优先压缩路径和 description，而不是按当前 query 做相关性筛选。
- 显式 skill mention 采用 Codex 同款 host 注入：`$skill`、`skill://...` 或结构化 skill selection 命中时，host 在本 turn 自动读取 `SKILL.md` 正文并注入。
- MCP 使用 `rmcp` 官方客户端；v1 支持 stdio 和 streamable HTTP，明确拒绝 legacy SSE 配置。
- streamable HTTP headers 支持静态值和 host env secret 映射；manifest summary、日志、warning 不输出 secret value。

## Target Config Shape

```json
{
  "pluginId": "local-dev",
  "displayName": "Local Dev",
  "enabled": true,
  "onLoadFailure": "disable_for_run",
  "components": [
    {
      "type": "skills",
      "roots": ["./skills"]
    },
    {
      "type": "mcp",
      "serverId": "filesystem",
      "transport": {
        "type": "stdio",
        "command": "node",
        "args": ["./servers/filesystem.js"],
        "cwd": ".",
        "env": {}
      },
      "enabledTools": ["read_file", "list_directory"],
      "disabledTools": [],
      "toolNamePrefix": "fs",
      "requestTimeoutSecs": 30
    },
    {
      "type": "mcp",
      "serverId": "remote-docs",
      "transport": {
        "type": "streamable_http",
        "url": "https://example.com/mcp",
        "headers": {
          "Authorization": {
            "type": "host_env",
            "name": "REMOTE_DOCS_MCP_TOKEN",
            "prefix": "Bearer "
          }
        },
        "connectTimeoutSecs": 10,
        "requestTimeoutSecs": 30
      },
      "toolNamePrefix": "docs"
    },
    {
      "type": "noloong_extension",
      "transport": {
        "type": "stdio",
        "command": "node",
        "args": ["./extension.mjs"],
        "cwd": ".",
        "env": {}
      },
      "allowedCapabilities": [
        { "type": "tool", "name": "conformance_echo" }
      ]
    }
  ]
}
```

## Dependency Graph

```text
Plugin component schema
    ├── Manifest patch/schema/i18n updates
    ├── Noloong extension component compatibility-in-current-code
    ├── Skills component loader
    │       ├── Skills metadata renderer and 2% budget
    │       └── Explicit skill injection into model request
    └── MCP component loader
            ├── rmcp transport clients
            ├── MCP tool provider adapter
            └── Approval and output mapping

Runtime plugin loader
    ├── with_manifest_plugins component dispatch
    ├── plugin load warning diagnostics
    └── docs, examples, regression tests
```

## Task List

### Phase 1: Plugin Component Foundation

#### Task 1: 重构 plugin declaration 为 components

**Description:** 将 `AgentPluginDeclaration` 从单一 `transport` 改为 `components` 列表。新增 `PluginComponent` tagged enum，包含 `skills`、`mcp`、`noloong_extension` 三类。旧顶层 `transport` 形态直接删除，不做兼容。

**Acceptance criteria:**
- [ ] `AgentPluginDeclaration` 必须包含非空 `pluginId`、`displayName` 和至少一个 component。
- [ ] `PluginComponent` 使用 `type` tagged enum，serde 字段为 camelCase。
- [ ] `noloong_extension` 组件复用现有 stdio transport 和 `allowedCapabilities` 语义。
- [ ] 旧 `transport` 顶层字段不再出现在类型、schema、示例和测试中。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test manifest plugin`
- [ ] Search check: `rg "\"transport\"|PluginTransport|to_stdio_extension_config" crates/noloong-agent examples/profile-configs`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent/src/plugin.rs`
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/manifest.rs`

**Estimated scope:** M

#### Task 2: 更新 manifest tool schema、summary 和 i18n

**Description:** 更新 `agent.manifest.propose_patch` 的 JSON schema、patch summary 和中英文 i18n，使注册插件时展示 component 类型、数量和安全摘要。secret/env source 只能显示来源名，不能显示解析后的值。

**Acceptance criteria:**
- [ ] manifest patch schema 接受新 component shape。
- [ ] `RegisterPlugin` summary 包含 plugin id、enabled、onLoadFailure、component summaries。
- [ ] env/header secret source 在 summary 和错误文案中只显示 source name。
- [ ] 中英文 i18n 测试覆盖 plugin register/enable/remove 仍完整。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent i18n`
- [ ] Tests pass: `cargo test -p noloong-agent --test manifest manifest_plugin`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent/src/tools/manifest.rs`
- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/manifest.rs`

**Estimated scope:** S

#### Task 3: 迁移现有 stdio extension plugin runtime

**Description:** 将 `AgentSessionRuntimeBuilder::with_manifest_plugins()` 从“每个 plugin 启动一个 stdio extension”改成“遍历 enabled plugin components”。本任务只接通 `noloong_extension`，确保现有 conformance extension 测试在新 schema 下继续成立。

**Acceptance criteria:**
- [ ] disabled plugin 不加载任何 component。
- [ ] `noloong_extension` 组件会启动 stdio extension 并同步 core metadata。
- [ ] `DisableForRun` 记录 plugin/component warning 并继续构建 runtime。
- [ ] `FailRun` 在 component 加载失败时终止 runtime 构建。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test plugin`
- [ ] Tests pass: `cargo test -p noloong-agent --test agent_session`

**Dependencies:** Task 2

**Files likely touched:**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/plugin.rs`
- `crates/noloong-agent/tests/plugin.rs`

**Estimated scope:** M

### Checkpoint: Plugin Foundation

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --test manifest`
- [ ] `cargo test -p noloong-agent --test plugin`
- [ ] `cargo test -p noloong-agent --test agent_session`

### Phase 2: Skills Loader and Metadata Context

#### Task 4: 新增 Skills loader 与 metadata model

**Description:** 新增 agent 层 skills 模块，按 plugin `skills.roots` 递归发现 `SKILL.md`。解析 YAML frontmatter 的 `name`、`description`，记录 canonical `path`、`pluginId`、`scope`。隐藏目录跳过，扫描深度和目录数量有硬限制，避免错误目录拖垮 session 构建。

**Acceptance criteria:**
- [ ] 支持 root 直接包含 `SKILL.md` 和 root 子目录包含多个 `SKILL.md`。
- [ ] `name` 和 `description` 缺失、为空或超长时产生结构化 load error。
- [ ] skill path 使用 canonical absolute path；canonicalize 失败时使用原 absolute path。
- [ ] 同一路径重复发现时去重；同名 skill 允许共存。
- [ ] `onLoadFailure` 控制 skills component load error 是 warning 还是 fail run。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test skills loader`
- [ ] Tests pass: `cargo test -p noloong-agent --test plugin skills`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/src/skills.rs`
- `crates/noloong-agent/src/plugin.rs`
- `crates/noloong-agent/tests/skills.rs`

**Estimated scope:** M

#### Task 5: 实现 2% skills metadata 渲染预算

**Description:** 实现 Codex 风格的 available skills 渲染器。默认预算为 `floor(inputLimitTokens * 0.02)`；使用当前主模型真实 input limit。渲染时先尝试 absolute path，超预算再尝试 skill root alias，仍超预算则均匀截断 description，最后只保留能放下的最小 skill 行。

**Acceptance criteria:**
- [ ] 低于预算时完整渲染所有 `name/description/path`。
- [ ] path 太长导致超预算时，生成 `Skill roots` alias table 并使用短路径。
- [ ] description 太长导致超预算时，保留所有 skill 行并均匀截断 description。
- [ ] 最小行也超预算时，按稳定顺序保留能放下的行并产生 warning。
- [ ] 启用 skills 但无法解析主模型 input limit 时，runtime 构建失败并提示显式配置模型窗口。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test skills render`
- [ ] Tests pass: `cargo test -p noloong-agent --test agent_session skills_context`

**Dependencies:** Task 4

**Files likely touched:**
- `crates/noloong-agent/src/skills.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/skills.rs`

**Estimated scope:** M

#### Task 6: 将 available skills 注入模型请求

**Description:** 在 system prompt/runtime context 注入路径中加入 `<skills_instructions>`，只进入本次 provider request，不写入持久 transcript、event store 或 compaction 历史。文案包含 skill discovery、trigger rules、progressive disclosure、resources 读取规则和安全 fallback。

**Acceptance criteria:**
- [ ] 无 enabled skills 时不注入 `<skills_instructions>`。
- [ ] 有 enabled skills 时每次模型请求都注入 metadata 和 how-to-use。
- [ ] 注入内容包含路径读取说明，不暗示存在 `agent.skill.*` 工具。
- [ ] skills warning 可进入 plugin load warnings 或 runtime diagnostics。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test agent_session skills_context`
- [ ] Tests pass: `cargo test -p noloong-agent --test manifest`

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/system_prompt.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** S

#### Task 7: 实现显式 skill mention 的正文注入

**Description:** 对 `$skill`、`skill://<absolute-path>` 和未来 bridge 可传入的 structured skill selection 做 host 侧解析。命中 enabled skill 后，在本 turn 自动读取对应 `SKILL.md` 正文并作为 turn-local skill instructions 注入模型请求。

**Acceptance criteria:**
- [ ] `$name` 只有唯一 enabled skill 匹配时才注入；同名冲突时不按 name 注入。
- [ ] `skill://path` 按 canonical path 精确匹配 enabled skill。
- [ ] disabled skill 不会被正文注入。
- [ ] skill 正文读取失败时发送 warning，模型请求继续使用 metadata fallback。
- [ ] 注入的 skill 正文不写入长期 session transcript。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --test skills injection`
- [ ] Tests pass: `cargo test -p noloong-agent --test agent_session skill`

**Dependencies:** Task 6

**Files likely touched:**
- `crates/noloong-agent/src/skills.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/skills.rs`

**Estimated scope:** M

### Checkpoint: Skills

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --test skills`
- [ ] `cargo test -p noloong-agent --test agent_session`
- [ ] Manual prompt snapshot check: skills metadata appears in model request, `SKILL.md` body appears only for explicit mention.

### Phase 3: MCP Runtime

#### Task 8: 引入 rmcp 并定义 MCP component config

**Description:** 在 `noloong-agent` 增加 `mcp` feature 和 `rmcp` 依赖。定义 MCP component、stdio transport、streamable HTTP transport、tool allow/deny、timeout 和 header/env source 类型。legacy SSE 类型不进入 schema。

**Acceptance criteria:**
- [ ] `mcp` feature 编译时启用 rmcp stdio 和 streamable HTTP client capability。
- [ ] `McpPluginComponent` 支持 `serverId`、`transport`、`enabledTools`、`disabledTools`、`toolNamePrefix`、`requestTimeoutSecs`。
- [ ] streamable HTTP 支持 `url`、static headers、host env headers、`connectTimeoutSecs`。
- [ ] `type = "sse"` 或未知 transport 被 serde/validate 拒绝。
- [ ] secret header/env value 不进入 Debug、summary 或 error 文案。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test manifest mcp`
- [ ] Tests pass: `cargo check -p noloong-agent --features mcp`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/Cargo.toml`
- `Cargo.toml`
- `crates/noloong-agent/src/plugin.rs`

**Estimated scope:** M

#### Task 9: 实现 MCP connection loader

**Description:** 在 runtime 构建时为 enabled MCP component 建立 rmcp client connection，初始化 server 并拉取 tool list。加载失败按 `onLoadFailure` 处理。MCP server 生命周期绑定到 runtime，runtime drop 时关闭连接。

**Acceptance criteria:**
- [ ] stdio MCP server 可启动、initialize、list_tools。
- [ ] streamable HTTP MCP server 可 initialize、list_tools。
- [ ] timeout、认证失败、server crash 产生带 pluginId/serverId/component 信息的 warning/error。
- [ ] duplicate `serverId` 或 duplicate exposed tool name 被拒绝或按 `onLoadFailure` 降级。
- [ ] runtime 结束时不遗留 stdio child process。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test mcp_plugin stdio`
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test mcp_plugin streamable_http`

**Dependencies:** Task 8

**Files likely touched:**
- `crates/noloong-agent/src/mcp.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/mcp_plugin.rs`

**Estimated scope:** M

#### Task 10: 将 MCP tools 适配为 ToolProvider

**Description:** 为每个 MCP tool 生成 noloong `ToolProvider`。工具名采用确定性前缀，输入 schema 来自 MCP tool schema，调用时把 noloong tool args 转发给 MCP `call_tool`，并把 MCP response 映射回现有 `ToolOutput`。

**Acceptance criteria:**
- [ ] 默认工具名为 `mcp.<pluginId>.<serverId>.<toolName>`，配置 `toolNamePrefix` 后使用短前缀。
- [ ] `enabledTools` 为空时默认暴露全部未禁用工具。
- [ ] `enabledTools` 与 `disabledTools` 同时命中时 disabled 优先。
- [ ] MCP text content 映射为文本输出；structured/resource/image content 映射为 JSON/metadata 输出。
- [ ] MCP `isError` 或 call error 标记为 tool error，不伪装成成功。
- [ ] 大输出继续复用现有 tool output overflow hook。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test mcp_plugin tool_call`
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test agent_session mcp`

**Dependencies:** Task 9

**Files likely touched:**
- `crates/noloong-agent/src/mcp.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/mcp_plugin.rs`

**Estimated scope:** M

#### Task 11: 接入 MCP approval 与 runtime diagnostics

**Description:** 为 MCP tools 生成清晰的 permission description，复用现有 approval hook。运行配置和 manifest 输出中展示 MCP server/tool 数、transport 和加载状态。

**Acceptance criteria:**
- [ ] 非 built-in MCP tools 默认走现有 unknown tool approval 路径或明确的 MCP permission classification。
- [ ] approval request 文案包含 plugin display name、serverId、tool name。
- [ ] `/manifest` 或 runtime config 能看到 MCP server 和 tool count。
- [ ] approval pause/resume 后 MCP tool call 可继续完成。

**Verification:**
- [ ] Tests pass: `cargo test -p noloong-agent --features mcp --test mcp_plugin approval`
- [ ] Tests pass: `cargo test -p noloong-agent --test interaction_registry manifest`

**Dependencies:** Task 10

**Files likely touched:**
- `crates/noloong-agent/src/approval/hook.rs`
- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/mcp_plugin.rs`

**Estimated scope:** S

### Checkpoint: MCP

- [ ] `cargo fmt -p noloong-agent --check`
- [ ] `cargo test -p noloong-agent --features mcp --test mcp_plugin`
- [ ] `cargo test -p noloong-agent --features mcp --test agent_session`
- [ ] `cargo clippy -p noloong-agent --all-targets --features mcp -- -D warnings`

### Phase 4: Examples, Docs, and Final Regression

#### Task 12: 更新示例 profile 和文档

**Description:** 更新 plugin stdio 示例为新 component schema，新增 skills 示例和 MCP stdio/streamable HTTP 示例。文档明确 noloong 原生 plugin 与 Codex plugin manifest 不兼容，legacy SSE MCP 不支持。

**Acceptance criteria:**
- [ ] `examples/profile-configs/plugin-stdio-example.json` 使用 `noloong_extension` component。
- [ ] 新增或更新示例展示 skills roots、MCP stdio、MCP streamable HTTP。
- [ ] architecture/interaction docs 说明 plugin components、skills metadata budget、explicit skill injection 和 MCP transport。
- [ ] 文档中不承诺 Codex plugin 兼容或 computer-use plugin 支持。

**Verification:**
- [ ] JSON examples parse through profile/config tests.
- [ ] Search check: `rg "transport\"|legacy SSE|Codex plugin" crates/noloong-agent/docs crates/noloong-agent-core/docs examples`

**Dependencies:** Task 11

**Files likely touched:**
- `examples/profile-configs/plugin-stdio-example.json`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/docs/INTERACTION.md`

**Estimated scope:** S

#### Task 13: 全量回归和 smoke

**Description:** 跑完整回归，确认新 plugin schema、skills 注入和 MCP feature 不破坏现有 interaction、subagent、goal、automation、Telegram/Weixin 客户端路径。对 skills 做一次真实 agent smoke：显式 mention skill 和非显式 metadata 查找各跑一次。

**Acceptance criteria:**
- [ ] 所有计划内测试通过。
- [ ] `--all-features` clippy 通过。
- [ ] 显式 `$skill` smoke 中模型请求包含 `SKILL.md` 正文。
- [ ] 非显式 smoke 中模型请求只包含 metadata/path，agent 能通过 host 文件能力读取 skill。
- [ ] MCP stdio fake server smoke 可真实调用 tool。
- [ ] MCP streamable HTTP fake server smoke 可真实调用 tool。

**Verification:**
- [ ] `cargo fmt --all --check`
- [ ] `cargo test -p noloong-agent --test manifest`
- [ ] `cargo test -p noloong-agent --test agent_session`
- [ ] `cargo test -p noloong-agent --test plugin`
- [ ] `cargo test -p noloong-agent --test skills`
- [ ] `cargo test -p noloong-agent --features mcp --test mcp_plugin`
- [ ] `cargo test -p noloong-agent --features mcp`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`
- [ ] `git diff --check`

**Dependencies:** Task 12

**Files likely touched:**
- Tests and docs only unless regressions require fixes.

**Estimated scope:** M

### Checkpoint: Complete

- [ ] New plugin component schema is the only supported schema.
- [ ] Skills metadata and explicit injection match the intended Codex-style behavior.
- [ ] MCP stdio and streamable HTTP both work through `rmcp`.
- [ ] Legacy SSE and Codex plugin manifest compatibility are explicitly out of scope.
- [ ] Final regression and smoke complete.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Skills metadata bloats model request | High | Enforce 2% input-window budget with alias/truncation/minimum-line fallback. |
| Skill body injection pollutes long-term state | Medium | Inject as request-local context only; do not persist to transcript or compaction history. |
| MCP transport lifecycle leaks child processes | High | Bind rmcp client handles to runtime lifetime and add stdio drop/shutdown tests. |
| MCP tool names collide after provider name normalization | Medium | Generate deterministic names and reject collisions during runtime build. |
| Secret headers leak in logs or summaries | High | Keep secret values out of `Debug`, summaries, warnings, and errors; test with env secret. |
| Component schema change misses manifest proposal schema | Medium | Update `agent.manifest.propose_patch` schema in the same foundation phase. |

## Parallelization Opportunities

- After Task 3, Task 4-7 (skills) and Task 8-11 (MCP) can be implemented in parallel if write sets stay separate.
- Documentation examples can start after Task 1 schema stabilizes, but final docs should wait for Task 11 behavior.
- Regression/smoke should remain sequential after all feature tasks are merged.

## Open Questions

- None. The current product decision is fixed: no Codex plugin compatibility, no Codex computer-use plugin support, no legacy SSE MCP.
