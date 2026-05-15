# noloong-agent 架构说明

`noloong-agent` 是基于 `noloong-agent-core` 的应用层 runtime。它负责宿主机环境认知、后台命令工具、自进化 manifest、approval reviewer 和面向模型的 i18n 文案；`noloong-agent-core` 继续作为不可自变异的 providerless kernel，不引入 host、shell、SSH、VMM 或 process manager 概念。

## 分层边界

```text
noloong-agent
  owns AgentSession
  owns AgentManifest
  owns HostProcessManager
  owns built-in approval reviewer
  builds AgentRuntime per application turn

noloong-agent-core
  owns event-sourced kernel
  owns phase graph
  owns provider traits and tool approval events
```

应用层通过 core 已有扩展点接入：

- `ContextProvider`：注入当前宿主机环境说明。内置 provider id 是 `noloong.builtin.host-context`。
- `ToolProvider`：暴露后台命令 lifecycle tools、模型感知文件编辑工具和 manifest patch proposal tool。
- `PhaseHook::before_model_request`：注入内置或自定义 system prompt。内置 hook id 是 `noloong.builtin.system-prompt`。
- `ToolCallHook`：统一处理命令执行、stdin 写入、终止命令、文件编辑和 manifest patch 的 approval。内置 approval hook id 是 `noloong.builtin.approval`。

## Immutable Host Self-Inspection

root `noloong` binary 在构建时内嵌一份 source snapshot 和 build-info manifest。该能力通过 `noloong build-info manifest`、`noloong build-info command`、`noloong build-info source list/cat/extract/archive` 暴露，目的是让 Agent 在没有原始 checkout 的环境里也能理解当前不可变 Rust host 的来源、构建 recipe 和源码内容。

source snapshot 遵循 `.gitignore`，并显式排除 `.git/`。因此 `.gitignore` 是安全边界：任何本地 token、数据库、日志、私钥或临时文件在进入 checkout 前，都应先确认会被 ignore。

该能力是自省和审计入口，不是常规自我改进入口。Noloong 不推荐 Agent 解包内置源码后修改并重新编译替换当前 binary；真正的自我改进应优先通过编写或更新插件完成，让不可变 Rust host 保持稳定，把演进放在可替换扩展层。

## Host-first Execution

v1 默认在宿主机执行。SSH、VMM、`clone`、Lima、QEMU 等不是统一 target abstraction，而是宿主机命令能力：Agent 可以通过 `host.exec.start` 启动这些命令，并在后续 turn 中读取或控制它们。

给模型的 host context 由 `HostEnvironment` 生成，包含：

- OS 和 CPU 架构。
- 当前目录。
- 默认 shell。
- 可用 shell hints。
- path style。
- locale。

这样可以避免模型在 PowerShell、`sh`、`bash`、`zsh` 等环境里混用错误命令。

## Built-in System Prompt

`AgentManifest::system_prompt` 是结构化配置，而不是一段裸字符串。默认值是 `{"source":"built_in"}`，表示使用应用层内置提示词；实际文本由当前 `locale`、built-in prompt profile 和默认模型共同决定。

Built-in prompt 支持 `profile`：

- `auto`：默认值。运行时根据当前默认模型选择 prompt profile；模型名包含 `gpt-5.5` 时选择 `gpt_5_5`，否则选择 `general`。
- `general`：通用 Agent prompt。
- `gpt_5_5`：面向 GPT-5.5 系列模型的 prompt profile。

自定义提示词使用 `{"source":"custom","prompt":"..."}`。切换 locale 或模型不会改变 custom prompt；只有 built-in prompt 会按 locale/profile 重新选择文本。

Built-in 和 custom prompt 都支持 `additions`。每个 addition 包含稳定 `id`、`text` 和 `enabled`，用于让 interaction channel、插件或 runtime profile 在基础系统提示词之上追加窄范围指令，而不是替换整段系统提示词。启用的 additions 会在最终 prompt 末尾以 `System Prompt Additions` 分节渲染；禁用的 additions 仍保留在 manifest 中，便于 UI 查询、重新启用和审计。

system prompt 通过 `noloong.builtin.system-prompt` before-model-request hook 注入为 `MessageRole::System`，并带有 `noloong.kind = "system_prompt"`、`noloong.source`、`noloong.configuredProfile`、`noloong.resolvedProfile` 和 `noloong.enabledAdditionIds` metadata。该消息只进入本次 provider request，不写入 core transcript，也不会污染 compaction、event store 或 registry snapshot 中的对话历史。hook 在每次请求前读取当前 manifest，因此替换提示词、切回内置提示词、修改 profile 或 additions 会影响下一次模型请求。

外部交互层可以通过 `manifest/system_prompt/get` 查询当前 session 的解析结果，包括 base text、additions、enabled addition ids、effective text、configured profile、resolved profile 和模型上下文。这个 API 是只读的，供 UI、插件管理器和 bridge 展示“当前实际发给模型的系统提示词”，避免只能从 manifest 结构自行推断。

## Background Command Lifecycle

后台命令不是单个阻塞式 `exec`，而是一组 lifecycle tools：

- `host.exec.start`
- `host.exec.read`
- `host.exec.wait`
- `host.exec.write`
- `host.exec.terminate`
- `host.exec.list`
- `agent.subagent.spawn`
- `agent.subagent.wait`
- `agent.subagent.result`
- `agent.subagent.list`

`host.exec.start` 使用 optimistic foreground window。命令若在 `foregroundWaitMs` 内完成，tool result 直接返回 completed status；超过窗口则返回 running job handle，Agent 可以继续做其它事，并在后续 turn 中调用 `read`、`wait`、`write` 或 `terminate`。

`agent.subagent.*` 是同一 built-in host tool 层的一部分，不属于 interaction JSON-RPC control plane 本身。`AgentSession` 只在宿主注入 `SubagentController` 且当前 depth 低于限制时装配这些工具；interaction registry 是第一个 controller provider。MVP 默认 `maxSubagentDepth = 1`，root session 可以创建和等待 direct child，child session 不再获得 subagent tools。

输出由应用层 spool/ring buffer 保存。core event log 只记录 tool result 中的摘要、cursor、cap 和 truncation metadata，不把大 stdout/stderr chunk 全量写入 event store。

## Background Completion Steering

后台命令进入终态后，`HostProcessManager` 会发布一次 `HostProcessEvent::JobCompleted`。事件在 child process 结束并且 stdout/stderr reader drain 之后生成，因此 completion preview 能包含最终输出尾部。

`AgentSession::attach_background_completion_steering(...)` 可以把这些 completion events 接到 core `Agent::steer(...)`。这条路径只排队 steering message，不做 auto continuation：它不会主动调用 `prompt`、`continue_run` 或模型 provider。Agent 空闲时，completion message 会等到下一次 run 开始，并在第一轮 model request 前注入；Agent 正在运行时，completion message 会在当前安全 turn 边界进入下一轮。

completion message 使用 `MessageRole::User`，不是 `MessageRole::ToolResult`。原因是后台完成事件是异步外部观察，不对应当前 provider transcript 中一个仍在等待结果的 assistant tool call；伪造 `ToolResult` 会破坏 chat completions、responses、anthropic messages 等 provider 的工具调用配对约束。

默认 completion preview 是 bounded tail output，上限 `16 KiB`。完整 stdout/stderr 历史仍由 `host.exec.read` 按 `jobId` 和 `afterSeq` cursor 拉取。

## Tool Output Overflow

`AgentSession::runtime_builder()` 默认注册 `BuiltInToolOutputOverflowHook`。该 hook 位于应用层，通过 core 的 `ToolCallHook::after_tool_call` 检查完整 `ToolOutput` 的 serialized byte size，hook id 是 `noloong.builtin.tool-output-overflow`。

默认 inline 上限是 `64 KiB`。未超限的 tool output 保持不变；超限时，hook 会把原始 `ToolOutput` 写入 `${TMPDIR}/noloong-agent/tool-output/{runId}-{turnId}-{toolCallId}.json`，并把 inline tool result 改写成短提示，包含文件路径、原始字节数、inline limit、tool name、tool call id，以及按模型可读 output content 生成的 head/tail preview。这样 core event log、模型上下文和后续 provider request 都只携带 bounded output，而完整结果仍可由 Agent 通过 host command tooling 读取。

如果写入临时 JSON 失败，hook 不会静默截断数据；它会返回 `is_error = true` 的 auditable tool output，并说明 overflow persistence failed。应用集成方可以通过 `AgentSessionBuilder::with_max_inline_tool_output_bytes(...)`、`with_tool_output_temp_dir(...)` 或 `with_tool_output_overflow_config(...)` 覆盖默认策略。

## Model-Aware File Editing

文件编辑工具是 session capability，不属于普通 `enabled_tools`。原因是 `write_file` 和 `apply_patch` 面向模型的操作风格不同，同时暴露会让模型在同一轮里混用两套编辑协议，增加错误率和审批面。

`write_file` 和 `apply_patch` 是 `AgentSession` 的保留工具名。`AgentSessionRuntimeBuilder::build()` 会先移除同名外部工具，再按 manifest policy 注入唯一内置工具，保证 provider request 里不会同时出现两套文件编辑协议。

`AgentManifest::file_edit_tool_policy` 控制暴露策略：

- `auto_by_model`：默认值。resolved model name 包含 `gpt`（大小写不敏感）时暴露 `apply_patch`；其它模型暴露 `write_file`。内置 Chat Completions、Responses API、Anthropic Messages provider 使用真实 config model；外部 provider 没有 `model_name()` 时回退到 provider id。
- `apply_patch`：强制只暴露 `apply_patch`。
- `write_file`：强制只暴露 `write_file`。
- `disabled`：不暴露内置文件编辑工具。

非 GPT 模型默认使用 `write_file`，因为很多模型对严格 patch grammar 的稳定性弱于直接写入/替换。`write_file` 不是只能做整文件覆盖：它支持两种互斥输入模式。

- `path + content`：写入或完整替换文本文件，缺失 parent directories 会自动创建。
- `path + oldString + newString`：对现有文本文件做严格字符串替换。默认要求 `oldString` 唯一命中；多处命中必须显式设置 `replaceAll = true`。

GPT 类模型默认使用 `apply_patch`。v1 支持严格 V4A-style patch：`*** Begin Patch` / `*** End Patch`，以及 `*** Add File`、`*** Update File`、`*** Delete File`、`*** Move File: old -> new` 和 `*** Move to:`。更新 hunk 必须严格匹配当前文件内容；所有操作先验证，验证失败不写入任何文件。

v1 明确不内置 Hermes fuzzy replace mode、read-file staleness tracking、read-dedup 或 auto-lint。后续如果需要这些能力，应作为额外阶段或 capability 接入，而不是让文件编辑工具隐式读写全局状态。

文件编辑统一通过 `FileEditManager` 做路径解析、敏感路径拒绝、parent directory 创建和 per-session path lock。相对路径基于 `HostEnvironment.cwd` 解析；`/etc`、`/boot`、systemd 目录、Docker socket path、macOS private system dirs 等敏感路径会在写入前拒绝。多文件 patch 按 resolved path 排序加锁，避免交叉 patch 死锁。

## Manifest Evolution

`AgentManifest` 描述 application session 的可变配置：

- locale。
- system prompt source：built-in 或 custom，以及 built-in profile、additions。
- enabled tools。
- file edit tool policy。
- approval policy。
- reserved phase profile。

Agent 不能直接修改 live manifest，只能通过 `agent.manifest.propose_patch` 提交 proposal。proposal 进入 approval path；审批通过后，由 `AgentSession` 应用到 session manifest 并持久化 snapshot。system prompt 和 locale 由 request-time hook/catalog 读取，下一次模型请求即可生效；工具、插件和文件编辑策略这类 runtime registration 变化需要下一次 runtime 构建后生效。

v1 真正支持的 patch 范围：

- replace system prompt。
- use built-in system prompt。
- set built-in system prompt profile。
- upsert/remove/enable/disable/reorder/clear system prompt additions。
- set locale。
- enable/disable tool。
- update approval policy。
- update file edit tool policy。

phase profile patch 只保留为 reserved schema，不执行。

## Approval Reviewer

应用层 approval 通过 `ToolCallHook` 实现，复用 core 的 permission audit 和 pause/resume 事件路径。

当前 policy：

- `AllowAll`：直接允许。
- `RequireApproval`：smart-gated approval。内置 hook 先分类工具调用；已知安全的内置只读操作直接允许，需要人工判断的操作才进入 human approval，明确不可接受的操作可直接 deny。
- `AutoReview`：复用同一套分类。安全调用直接允许；只有分类为 `NeedsApproval` 时才调用 auto-review agent。没有 reviewer 时可按配置回退 human approval 或 deny。

评估顺序：

1. `AllowAll` 直接 bypass 内置检查。
2. session approval cache 命中时直接 allow。cache 只记录当前 `AgentSession` 内由 `noloong.builtin.approval` 产生、带有内置 cache key，且 application 显式记录为 allow 的审批结果。
3. 内置工具类别分类：`host.exec.read`、`host.exec.wait`、`host.exec.list` 直接 allow；`host.exec.write`、`host.exec.terminate`、`write_file`、`apply_patch`、`agent.manifest.propose_patch` 进入 approval；未知工具名进入 approval。
4. `host.exec.start` 走命令安全分类器。已知只读命令允许；unsupported shell syntax、env assignment、redirection、command substitution、glob-heavy syntax 和未知命令都进入 approval；危险命令同样需要 approval。
5. 对 `NeedsApproval` 结果，`RequireApproval` 产生 core pause/resume approval request；`AutoReview` 调用 reviewer 或按 fallback 策略处理。

`AgentSession::record_tool_approval_resolution` 是 application 层接入 cache 的显式 API。调用方在用 core 的 `ToolApprovalResolution` resume 之前或之后，都可以把对应 `ToolApprovalRequest` 和 allow decision 传给 session；denial、外部 hook、缺少 built-in cache metadata 的 request 不会被记录。文件编辑工具没有 approval cache key，重复文件编辑不会因为上一次批准而自动放行。

v1 没有完整 sandbox 边界，也没有持久化 execpolicy 文件；因此 unknown host command 默认需要 approval。后续可以在不改变 core approval 语义的前提下，加入持久化规则、host sandbox/VMM policy 或更细粒度的 capability policy。

所有 approval decision 都进入 core 的 `ToolPermissionDecided` audit。进入 human approval 的请求也会保留 classification metadata；可缓存的请求还会保留 cache key，便于 application 层审计和记录 session cache。文件编辑 approval metadata 会包含 `host.file.write` capability 和可解析 target paths（如果参数可解析）。

## Interaction Control Plane

`noloong-agent` 内置 JSON-RPC 2.0 control plane，用于让任意第三方语言实现用户交互 bridge。它属于应用层，不进入 `noloong-agent-core`：core 仍只暴露 providerless `Agent`、event log、queue、approval pause/resume 和 runtime traits。

stdio line-delimited JSON-RPC 是默认、最低依赖、第三方 conformance baseline。启用 `interaction-http` feature 后，同一个 `InteractionControlHandler` 可以暴露为 HTTP/WebSocket transport：`POST /jsonrpc` 只处理单次 request/response，适合一次性 orchestration 调用；`GET /jsonrpc/ws` 是完整双向连接，同一 socket 承载 request、response 和 raw/display notification，适合 TS/Python 编写的 Telegram、WeChat/iLink 或 Web UI bridge。HTTP/WebSocket transport 使用 bearer token 做最小连接认证；具体方法权限仍由 `initialize` 后得到的 authority/UX grant 控制。

control plane 的核心对象是 `AgentSessionRegistry`。每个 registry entry 持有一个 `AgentSession`、一个 core `Agent`、runtime profile id、session metadata、可选 `parentSessionId` 和 `role`。subagent 仍是独立 session，只额外记录 parent/role/metadata；它不改变 parent session 的 run 状态。registry 通过弱引用注入 `SubagentController`，让模型可调用的 `agent.subagent.*` host tools 能创建 direct child、等待一组 child settled，并读取最终 assistant 输出，同时避免 registry -> session -> controller -> registry 的强引用环。

runtime 由 Rust host 注册 `AgentRuntimeProfile`。外部 bridge 只能选择 `profileId`，不能通过 JSON-RPC 传 provider credential 或任意模型配置。profile descriptor 可以携带默认 manifest patch；若创建 session 时没有传完整 manifest，registry 会先从 `AgentManifest::default()` 开始应用这些 patch，再应用 create/spawn request 携带的 `manifestPatches`。如果 request 传入完整 manifest，则以该 manifest 为 base，只追加 request-level patches。模型工具路径的 `agent.subagent.spawn` 会继承 parent profile/manifest 并投递 initial prompt；旧的 `subagent/spawn` JSON-RPC 方法保留为 bridge/control-plane API。

`AgentManifest::default()` 默认启用 `BuiltInToolName::ALL` 中的普通 Rust built-in tools，包括 `host.exec.*` 和 `agent.manifest.propose_patch`。关闭默认工具使用 `disable_tool` manifest patch；`enable_tool` 对默认已启用工具是幂等恢复操作。文件编辑工具不属于 `BuiltInToolName::ALL`：runtime build 阶段会先移除 `write_file` 和 `apply_patch`，再根据 `fileEditToolPolicy` 选择 exactly one，或禁用两者。

`AgentSessionRegistryStore` 持久化的是 application session snapshot，不是 `noloong-agent-core` 的 append-only event store。它保存 `AgentManifest`、`AgentState`、steering/follow-up queue、profile id、parent/role/metadata 和时间戳，用于让 interaction registry 在进程重启后恢复 session 目录和按需重建 live runtime。

profile 级 `eventStore` 是另一层：它保存 core `AgentEvent` append-only log，用于 run-level event replay、approval resume、tool permission audit 顺序和诊断。root `noloong` config 中，顶层 `registryStore` 选择 session snapshot backend；每个 profile 下的 `eventStore` 选择该 profile 构建 runtime 时注入的 core event log backend。两者不能互相替代：只有 registry store 时可以恢复 session descriptor 和 transcript snapshot，但 paused approval resume 这类需要 replay run events 的流程仍需要同一 profile 指向同一个持久 event store。

registry 支持 unloaded persisted session。`session/list` 和 `session/get` 可以直接从 store 中生成 descriptor，不会立即创建 provider、tool runtime 或后台进程管理器。只有 `agent/prompt`、`agent/continue`、queue/manifest mutation 等需要 live session 的操作才会触发 lazy restore：registry 用 snapshot 里的 `profileId` 查找当前 host 注册的 `AgentRuntimeProfile`，以 snapshot 中的 manifest 重建 `AgentRuntime`，再把 `AgentState` 和两类 queue 写回 core `Agent`。

恢复策略是显式保守的：

- `running` snapshot 说明上一进程在 run 中断，恢复时会被标记为 `failed`，并写回 store；不会自动继续调用模型或工具。
- `paused` snapshot 会保持 paused 状态，用于 approval 或人工流程恢复；若需要跨进程继续 approval，profile 必须配置持久 `eventStore`，例如 SQLite file URL。
- 如果当前 host 没有注册 snapshot 所需的 `profileId`，恢复会失败；这是 host/runtime profile 配置问题，不由外部 bridge 动态补 credential。
- profile 的默认 manifest patch 只在创建 session 时应用；恢复时使用 snapshot 中已持久化的 manifest。

内置 store backend：

- 默认 `InMemoryAgentSessionRegistryStore` 只适合测试或单进程临时 session。
- `registry-store-sqlite` 和 `registry-store-postgres` feature 启用 Toasty SQL backend。SQLite 支持 memory/file URL；PostgreSQL 支持 `postgres://` 和 `postgresql://` URL。SQL backend 适合需要强约束 session id、跨进程或多写者一致性的 registry。
- `registry-store-object` feature 启用 OpenDAL backend。每个 session 是 prefix 下的一个 JSON object，session id 使用 URL-safe base64 编码成 object key。该 backend 是 single-writer snapshot store，不承诺多进程同时写同一 registry 的强一致语义；需要多写者时应使用 SQL backend。

内置 event store backend：

- profile `eventStore` 默认是 memory，只适合当前进程内的 run event replay。
- root `noloong` v1 暴露 SQLite event store，配置形如 `{"type":"sqlite","databaseUrl":"sqlite:target/noloong-events.sqlite","migrateOnConnect":true}`。`migrateOnConnect` 默认 `true`；设为 `false` 时要求 schema 已存在。
- `sqlite::memory:` 仍是进程本地，不适合跨进程恢复 paused approval。
- PostgreSQL/object event store 不在 v1 中暴露；后续应作为 core `EventStore` backend 单独实现。

root profile config 的结构由 `schemas/profile-config.schema.json` 描述。该 schema 由 Rust 类型通过 `schemars` 生成，使用 `noloong profile-config schema --output schemas/profile-config.schema.json` 更新，使用 `noloong profile-config schema --check schemas/profile-config.schema.json` 检查 drift。CI 只信 checked-in artifact 与当前类型生成结果一致，不维护手写 schema。

profile config loader 支持 JSONC，但只放开注释和 trailing comma，用于让 profile 文件适合人工维护和编辑器 schema 提示。JSON-RPC extension protocol、provider payload、Telegram API payload 仍是严格 JSON。这里不使用 JSON5：unquoted key、single quote、hex number 等语法会扩大长期兼容面，而当前目标只是 JSON 配置文件的注释体验。

profile config 的 built-in provider 支持 typed `reasoning` 配置。Chat Completions 的 `reasoning.enabled` 会映射常见兼容开关，例如 `enable_thinking`、`thinking.type`、`reasoning.enabled`、`reasoning_split`、`chat_template_kwargs.enable_thinking`，`reasoning.effort` 会映射为 `reasoning_effort`。Responses 与 ChatGPT subscription profile 映射到 Responses API `reasoning` 和 encrypted reasoning include。Anthropic Messages 映射到当前 Claude API 的 `output_config.effort`，并可显式发送 `thinking: adaptive` 或 `thinking: disabled`。所有 provider 都先应用 typed mapping，再应用 `extraBody`，因此高级配置可以覆盖生成的 top-level 字段。

权限分为两层：

- authority capabilities：控制敏感动作，例如 `agent.run`、`agent.queue`、`approval.resolve`、`manifest.apply`、`process.control`、`subagent.spawn`、`session.delete`。
- UX capabilities：描述外部 bridge 的展示能力，例如 raw/display event、streaming text、message edit、Markdown 和 `maxMessageBytes`。

事件也分两层：

- raw event：直接发送 core `AgentEvent`，用于高级 bridge、审计或调试。
- display event：从 raw event 派生 UI 友好的 `DisplayEvent`，包括 run lifecycle、assistant delta/final、tool lifecycle 和 approval card。`streamText = false` 的 bridge 只需要渲染 final message；`maxMessageBytes` 会触发 bounded head/tail text truncation。

进程控制面不负责启动任意命令；后台命令仍由模型通过 `host.exec.start` 工具创建。bridge 只通过 `process/list`、`process/read`、`process/wait`、`process/write`、`process/terminate` 显示和控制已存在 job，并复用 `HostProcessManager` 的 cursor、spool、truncation 和 completion semantics。

协议细节见 `crates/noloong-agent/docs/INTERACTION.md`，验证矩阵见 `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`。

## First-party Telegram Client

Telegram 是第一个第一方 interaction client，但它仍然属于 `noloong-agent` 之外的 application/interaction 层，不进入 `noloong-agent-core`。实现位于 `crates/noloong-agent-telegram`，职责是 Telegram Bot API 适配、allowlist、chat/thread 到 session 的映射、文本批处理、Markdown/消息拆分、display event 投递和 inline approval button。

即使在 `noloong telegram` 单进程模式下，Telegram bridge 也通过 loopback WebSocket JSON-RPC 连接 host 的 `InteractionControlHandler`。这样单进程部署形态和分进程部署共享同一套协议路径，不给 Telegram 开后门，也确保第三方语言 bridge 能复用同样的 contract。

责任边界：

- root `noloong` binary 读取 profile config，构造 provider、runtime profile、registry store 和 interaction server。
- Telegram bridge 只接收 `interactionWsUrl`、bearer token、可选 `profileId`、Bot token、allowlist 和网络配置。
- Telegram config 不包含 provider credential、model name 或 registry store 配置。
- 未配置 allowed users/chats 且未显式 `allowAll` 时，Telegram bridge 拒绝启动。
- group/supergroup mention gating 使用配置的 bot username 判断 `@bot` 和 reply-to-bot。

当前支持三种启动形态：

- `noloong telegram`：同进程启动 host、loopback interaction WebSocket 和 Telegram long polling bridge。
- `noloong serve interaction`：只启动 interaction server，供 Telegram 或其它第三方 bridge 连接。
- `noloong telegram-bridge`：只启动 Telegram bridge，连接已有 interaction server。

Telegram Bot API 使用 direct `reqwest` adapter，而不是未使用的高层 Telegram framework。原因是 v1 需要精确控制 `getUpdates` offset、409 conflict、network retry、proxy、DoH/fallback IP 和 fake API 测试。

## i18n

所有给模型看的 application-generated natural-language text 都走 typed catalog，包括 host context、tool description、permission description、approval prompt/reason、tool input/process/manifest error、background completion steering message，以及 oversized tool output rewrite message。v1 默认支持 English 和 Chinese。

locale 解析顺序：

1. 显式配置。
2. 宿主机 `LC_ALL`、`LC_MESSAGES`、`LANG`。
3. English fallback。

catalog key 必须完整；缺失 key 应在测试中失败，而不是运行时静默 fallback。

## 后续演进方向

- 将 Telegram v1 扩展到 webhook、媒体输入输出和 per-chat/per-thread profile mapping，但这些仍应停留在 interaction/application 层。
- 将 display projection 拆成可插拔策略，使 WeChat/iLink、TUI 和 Web UI 能各自定义消息合并、编辑和 truncation 策略。
- 为 process control 增加可选 host sandbox/VMM policy，把当前 host-first tools 扩展到 SSH、Lima/QEMU 或其它隔离环境。
- 增强 manifest apply lifecycle，让 approved patch 能自动触发下一次 session runtime rebuild，并在 control plane 中暴露 rebuild/audit 结果。
- 为 interaction bridge 作者提供可复用 conformance runner，而不是只依赖 Rust 集成测试。
