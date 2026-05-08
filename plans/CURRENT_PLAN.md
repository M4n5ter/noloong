# 实施计划：默认启用内置工具与 Provider Reasoning 配置

> 状态：已完成实现、本地验证、schema check、clippy 和 workspace tests。目标是让 `noloong-agent` 的 Rust built-in tools 默认可用，同时为 root profile config 增加 provider-aware `reasoning` 配置，避免用户为了常见推理开关与推理强度手写 `extraBody`。

## 概览

实施前 `AgentManifest::default()` 的 `enabledTools` 是空集合，导致普通内置工具默认不会进入模型上下文；文件编辑工具因为不走 `BuiltInToolName` 集合，而是在 runtime build 阶段按 `fileEditToolPolicy` 特殊挂载，所以看起来已经被单独启用。

本实施已将普通 built-in tools 改为默认启用，并保留文件编辑工具现有特殊处理：`apply_patch` / `write_file` 永远不会同时暴露，默认仍按模型名自动选择。Provider 配置层新增 typed `reasoning` 字段：Chat Completions 负责映射常见 thinking extra body 与 `reasoning_effort`，Responses / ChatGPT Responses 映射到已有 Responses reasoning config，Anthropic Messages 按当前 Claude API 使用 `output_config.effort`，不把已不推荐的 `budget_tokens` 作为 profile 主路径。

## 架构决策

- “默认启用 built-in tools” 只覆盖 `BuiltInToolName::ALL` 中的 Rust product-layer tools；第三方 plugins 仍保持 opt-in。
- `enabledTools` 缺省时使用默认 built-ins；显式配置空数组时表示关闭全部普通 built-ins。
- `ManifestPatch::DisableTool` 是关闭默认内置工具的方式；`EnableTool` 对默认已启用工具保持幂等。
- 文件编辑工具继续由 `fileEditToolPolicy` 控制，不加入 `BuiltInToolName::ALL`。
- Root profile config 暴露 provider-aware `reasoning`，而不是在 `noloong-agent-core` 中硬编码 OpenRouter、DeepSeek、Anthropic model preset。
- `extraBody` 仍是最高优先级 escape hatch：先应用 typed `reasoning` 映射，再应用用户显式 `extraBody`，同名 top-level 字段由 `extraBody` 覆盖。
- Anthropic Messages 使用官方当前推荐的 `output_config.effort`；`budget_tokens` 只保留为 core provider 低层 legacy/manual thinking 能力，不从 root profile 主配置暴露。参考：`https://platform.claude.com/docs/en/build-with-claude/effort`。

## 任务列表

### 阶段 1：默认启用普通内置工具

#### 任务 1：定义默认 built-in tool 集合

**描述：** 为 `BuiltInToolName` 增加默认启用集合 helper，并让 `AgentManifest::new()`、`AgentManifest::default()`、serde 缺省反序列化都使用该集合。显式 `enabledTools: []` 必须保留关闭普通 built-ins 的语义。

**验收标准：**
- [x] `AgentManifest::default().enabled_tools` 包含 `BuiltInToolName::ALL` 的所有成员。
- [x] 缺省反序列化 manifest 时 `enabledTools` 使用默认集合。
- [x] 显式反序列化 `{"enabledTools":[]}` 时集合保持为空。
- [x] `FileEditToolPolicy::AutoByModel` 仍是默认值，且不受普通 built-in tool 默认集合影响。

**验证：**
- [x] `cargo test -p noloong-agent --test manifest`
- [x] `cargo test -p noloong-agent --test agent_session`

**依赖：** 无

**预计涉及文件：**
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/manifest.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**预计范围：** S

#### 任务 2：更新 session/runtime 默认工具行为

**描述：** 更新依赖旧行为的 session 测试和 manifest proposal 测试。默认 runtime 应能直接看到普通 built-in tools；测试“patch 下一轮生效”时改用先 `DisableTool` 再 `EnableTool`，或选择其它能体现状态变化的工具。

**验收标准：**
- [x] 默认 runtime 暴露 `host.exec.start/read/wait/write/terminate/list` 和 `agent.manifest.propose_patch`。
- [x] `DisableTool` 后重建 runtime 会隐藏目标工具。
- [x] `EnableTool` 后重建 runtime 会恢复目标工具。
- [x] `write_file` / `apply_patch` 仍由 runtime builder 移除后按策略重新选择，never-both 测试继续覆盖。

**验证：**
- [x] `cargo test -p noloong-agent --test agent_session`
- [x] `cargo test -p noloong-agent --test interaction_control`

**依赖：** 任务 1

**预计涉及文件：**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/agent_session.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**预计范围：** M

### 检查点：默认工具语义稳定

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent --test manifest`
- [x] `cargo test -p noloong-agent --test agent_session`

### 阶段 2：Provider reasoning 配置模型

#### 任务 3：为 root profile config 增加 typed reasoning 类型

**描述：** 在 root config 层为每类 built-in provider 增加 `reasoning` 字段。字段 shape 按 provider 类型区分，但都保持 JSON schema 可生成、JSONC 可编辑、缺省不改变 provider 行为。

**验收标准：**
- [x] `chat_completions.reasoning` 支持 `enabled: bool` 和 `effort: low|medium|high|xhigh`。
- [x] `responses.reasoning` 和 `chatgpt_responses.reasoning` 支持 `enabled`、`effort: minimal|low|medium|high|xhigh`、`summary: auto|concise|detailed|none`、`includeEncrypted`。
- [x] `anthropic_messages.reasoning` 支持 `effort: low|medium|high|xhigh|max` 与 `thinking: adaptive|disabled|omit`。
- [x] 省略 `reasoning` 时保持现有请求体行为。
- [x] `enabled=false` 时不发送 Responses typed reasoning；Chat Completions 发送 common disable switches；Anthropic 仅在 `thinking=disabled` 时发送 disabled thinking。

**验证：**
- [x] `cargo test -p noloong --lib config`
- [x] `cargo test -p noloong --lib schema`

**依赖：** 无

**预计涉及文件：**
- `src/config.rs`
- `src/schema.rs`
- `schemas/profile-config.schema.json`

**预计范围：** M

#### 任务 4：扩展 Anthropic Messages core provider 的 effort 支持

**描述：** 在 `noloong-agent-core` 的 Anthropic Messages provider 中增加一等 `output_config.effort` 支持，并把 thinking config 从只支持 manual `budget_tokens` 扩展为 adaptive / disabled / manual。Root profile 主路径只使用 effort 与 adaptive/disabled/omit。

**验收标准：**
- [x] `AnthropicMessagesProviderConfig` 可设置 `output_config.effort`。
- [x] request body 正确渲染 `output_config: {"effort": "medium"}`。
- [x] adaptive thinking 渲染为 `thinking: {"type":"adaptive"}`。
- [x] disabled thinking 渲染为 `thinking: {"type":"disabled"}`。
- [x] legacy manual thinking 仍可通过 core builder 渲染 `thinking: {"type":"enabled","budget_tokens": N}`，但 root profile 不新增 `budgetTokens`。

**验证：**
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`

**依赖：** 无

**预计涉及文件：**
- `crates/noloong-agent-core/src/anthropic_messages.rs`
- `crates/noloong-agent-core/tests/anthropic_messages.rs`

**预计范围：** M

### 阶段 3：Provider 映射与 host wiring

#### 任务 5：实现 Chat Completions reasoning extra body 映射

**描述：** 在 root host provider 构建路径中，将 `chat_completions.reasoning` 转换为 common compatibility extra body，再叠加用户 `extraBody`。不要把 provider/model 名字写死到 core provider。

**验收标准：**
- [x] `enabled=true` 写入 `enable_thinking=true`。
- [x] `enabled=true` 写入 `thinking: {"type":"enabled"}`。
- [x] `enabled=true` 写入 `reasoning: {"enabled": true}`。
- [x] `enabled=true` 写入 `reasoning_split=true`。
- [x] `enabled=true` 写入 `chat_template_kwargs: {"enable_thinking": true}`。
- [x] `enabled=false` 写入对应 false / disabled 值。
- [x] `effort` 写入 top-level `reasoning_effort`，支持 `low|medium|high|xhigh`。
- [x] 用户 `extraBody` 可覆盖 typed mapping 的同名 top-level 字段。

**验证：**
- [x] `cargo test -p noloong --lib host`
- [x] `cargo test -p noloong-agent-core --test chat_completions`

**依赖：** 任务 3

**预计涉及文件：**
- `src/host.rs`
- `src/config.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`

**预计范围：** M

#### 任务 6：实现 Responses、ChatGPT Responses、Anthropic 映射

**描述：** 将 root profile typed reasoning 映射到已有 provider config。Responses 与 ChatGPT Responses 复用 `ResponsesReasoningConfig`；Anthropic 使用新加的 `output_config.effort` 和 thinking mode。

**验收标准：**
- [x] Responses provider 根据 `effort` / `summary` 渲染 `reasoning` object。
- [x] Responses provider 根据 `includeEncrypted` 渲染 encrypted reasoning include。
- [x] ChatGPT Responses provider 使用同一套 Responses reasoning 映射，并保留 Codex compact auth/provider 行为。
- [x] Anthropic provider 根据 `effort` 渲染 `output_config.effort`。
- [x] Anthropic provider 根据 `thinking` 渲染 adaptive / disabled / omit。
- [x] `extraBody` 在所有 provider 中仍最后叠加。

**验证：**
- [x] `cargo test -p noloong --lib host`
- [x] `cargo test -p noloong-agent-core --test responses`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`

**依赖：** 任务 3、任务 4

**预计涉及文件：**
- `src/host.rs`
- `crates/noloong-openai/src/provider.rs`
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/anthropic_messages.rs`

**预计范围：** M

### 检查点：Provider reasoning 请求体可验证

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong --lib host`
- [x] `cargo test -p noloong-agent-core --test chat_completions`
- [x] `cargo test -p noloong-agent-core --test responses`
- [x] `cargo test -p noloong-agent-core --test anthropic_messages`

### 阶段 4：示例、文档与完整验证

#### 任务 7：更新 profile examples 与 schema

**描述：** 更新 checked-in profile examples，让默认工具启用后的配置更简洁，并展示新的 typed reasoning 用法。重新生成 JSON schema，确保编辑器提示覆盖新字段。

**验收标准：**
- [x] `telegram-openrouter-free.jsonc` 移除冗余 `enable_tool` patches。
- [x] `telegram-openrouter-free.jsonc` 展示 Chat Completions `reasoning.enabled` 和 `reasoning.effort`。
- [x] `chatgpt-codex-subscription.json` 展示 Responses-style `reasoning.effort`。
- [x] `schemas/profile-config.schema.json` 与 Rust 类型一致。
- [x] 所有 profile examples 通过 schema validation。

**验证：**
- [x] `cargo run -p noloong -- profile-config schema --output schemas/profile-config.schema.json`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `cargo test -p noloong --lib schema`
- [x] `cargo test -p noloong --lib config`

**依赖：** 任务 1、任务 3、任务 5、任务 6

**预计涉及文件：**
- `examples/profile-configs/telegram-openrouter-free.jsonc`
- `examples/profile-configs/telegram-openrouter-free.json`
- `examples/profile-configs/chatgpt-codex-subscription.json`
- `schemas/profile-config.schema.json`

**预计范围：** S

#### 任务 8：更新架构文档与 README

**描述：** 文档需要明确默认内置工具、文件编辑工具例外、provider reasoning 映射、`extraBody` 覆盖关系，以及 Anthropic 当前推荐使用 effort 而不是 profile-level `budgetTokens`。

**验收标准：**
- [x] README profile config 段落包含 `reasoning` 示例。
- [x] `crates/noloong-agent/docs/ARCHITECTURE.md` 说明默认工具与 manifest patch disable 语义。
- [x] `crates/noloong-agent-core/docs/ARCHITECTURE.md` 的 Anthropic provider 章节说明 `output_config.effort` 与 adaptive thinking。
- [x] 文档说明 Chat Completions common thinking switches 属于 root profile convenience mapping，不是 core vendor preset。
- [x] 文档说明 `extraBody` 覆盖 typed mapping。

**验证：**
- [x] `rg -n "reasoning|reasoning_effort|output_config|default.*tool|enabledTools" README.md crates/noloong-agent/docs crates/noloong-agent-core/docs`

**依赖：** 任务 1、任务 3、任务 4、任务 5、任务 6

**预计涉及文件：**
- `README.md`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**预计范围：** S

#### 任务 9：最终验证与 clippy

**描述：** 跑完整 workspace 验证，确保默认工具行为、schema、provider payload 和 docs 更新没有引入 lint warning 或测试回归。

**验收标准：**
- [x] workspace format check 通过。
- [x] workspace clippy 无 warning。
- [x] workspace tests 通过。
- [x] schema check 通过。
- [x] 无新增 `#[allow(dead_code)]`。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo test --workspace`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" src crates`

**依赖：** 任务 1-8

**预计涉及文件：**
- 全部已修改文件

**预计范围：** S

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---:|---|
| 默认启用 `host.exec.write` / `host.exec.terminate` 增加模型可见工具面 | 高 | 依赖现有 approval classifier，危险或控制类操作仍进入 approval；测试覆盖默认可见但不可绕过审批 |
| 缺省 `enabledTools` 与显式空数组语义混淆 | 中 | serde 使用自定义 default helper；测试同时覆盖字段缺省和显式空数组 |
| Chat Completions unknown fields 被少数 provider 拒绝 | 中 | typed mapping 只在用户显式配置 `reasoning` 时启用；`extraBody` 可覆盖或移除；文档说明兼容性边界 |
| Anthropic `thinking=disabled` 对部分新模型可能被拒绝 | 中 | 不把 `enabled=false` 自动映射为 disabled thinking；只有用户显式 `thinking=disabled` 才发送 |
| `extraBody` 深层 merge 语义复杂化 | 低 | v1 只承诺 top-level override，避免引入难以解释的深层合并规则 |

## 完成标准

- [x] 普通 built-in tools 默认进入新 session runtime。
- [x] 文件编辑工具仍按现有策略特殊选择，永不同时暴露。
- [x] Root profile config 可 typed 配置 Chat Completions / Responses / ChatGPT Responses / Anthropic reasoning。
- [x] Chat Completions 支持 `reasoning_effort` 的 `low|medium|high|xhigh` 映射。
- [x] Anthropic profile 主路径使用 `output_config.effort`，不新增 profile-level `budgetTokens`。
- [x] Schema、examples、README、架构文档与实现一致。
- [x] `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features`、`cargo test --workspace` 全部通过。
