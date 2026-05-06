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
- `ToolCallHook`：统一处理命令执行、stdin 写入、终止命令、文件编辑和 manifest patch 的 approval。内置 approval hook id 是 `noloong.builtin.approval`。

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

## Background Command Lifecycle

后台命令不是单个阻塞式 `exec`，而是一组 lifecycle tools：

- `host.exec.start`
- `host.exec.read`
- `host.exec.wait`
- `host.exec.write`
- `host.exec.terminate`
- `host.exec.list`

`host.exec.start` 使用 optimistic foreground window。命令若在 `foregroundWaitMs` 内完成，tool result 直接返回 completed status；超过窗口则返回 running job handle，Agent 可以继续做其它事，并在后续 turn 中调用 `read`、`wait`、`write` 或 `terminate`。

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
- system prompt profile。
- enabled tools。
- file edit tool policy。
- approval policy。
- reserved phase profile。

Agent 不能直接修改 live manifest，只能通过 `agent.manifest.propose_patch` 提交 proposal。proposal 进入 approval path；审批通过后，由 `AgentSession` 在下一 application turn 前应用 patch 并重建 core runtime。

v1 真正支持的 patch 范围：

- replace system prompt。
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

`noloong-agent` 内置 stdio JSON-RPC 2.0 control plane，用于让任意第三方语言实现用户交互 bridge。它属于应用层，不进入 `noloong-agent-core`：core 仍只暴露 providerless `Agent`、event log、queue、approval pause/resume 和 runtime traits。

control plane 的核心对象是 `AgentSessionRegistry`。每个 registry entry 持有一个 `AgentSession`、一个 core `Agent`、runtime profile id、session metadata、可选 `parentSessionId` 和 `role`。subagent 在 v1 中也是独立 session，只额外记录 parent/role/metadata；它不改变 parent session 的 run 状态，也不引入模型可调用的 subagent tool。

runtime 由 Rust host 注册 `AgentRuntimeProfile`。外部 bridge 只能选择 `profileId`，不能通过 JSON-RPC 传 provider credential 或任意模型配置。profile descriptor 可以携带默认 manifest patch；若创建 session 时没有传完整 manifest，registry 会先从 `AgentManifest::default()` 开始应用这些 patch。

`AgentSessionRegistryStore` 持久化的是 application session snapshot，不是 `noloong-agent-core` 的 append-only event store。它保存 `AgentManifest`、`AgentState`、steering/follow-up queue、profile id、parent/role/metadata 和时间戳，用于让 interaction registry 在进程重启后恢复 session 目录和按需重建 live runtime。core event store 仍然负责 run-level event replay、approval resume 和审计顺序；两者不能互相替代。

registry 支持 unloaded persisted session。`session/list` 和 `session/get` 可以直接从 store 中生成 descriptor，不会立即创建 provider、tool runtime 或后台进程管理器。只有 `agent/prompt`、`agent/continue`、queue/manifest mutation 等需要 live session 的操作才会触发 lazy restore：registry 用 snapshot 里的 `profileId` 查找当前 host 注册的 `AgentRuntimeProfile`，以 snapshot 中的 manifest 重建 `AgentRuntime`，再把 `AgentState` 和两类 queue 写回 core `Agent`。

恢复策略是显式保守的：

- `running` snapshot 说明上一进程在 run 中断，恢复时会被标记为 `failed`，并写回 store；不会自动继续调用模型或工具。
- `paused` snapshot 会保持 paused 状态，用于 approval 或人工流程恢复。
- 如果当前 host 没有注册 snapshot 所需的 `profileId`，恢复会失败；这是 host/runtime profile 配置问题，不由外部 bridge 动态补 credential。
- profile 的默认 manifest patch 只在创建 session 时应用；恢复时使用 snapshot 中已持久化的 manifest。

内置 store backend：

- 默认 `InMemoryAgentSessionRegistryStore` 只适合测试或单进程临时 session。
- `registry-store-sqlite` 和 `registry-store-postgres` feature 启用 Toasty SQL backend。SQLite 支持 memory/file URL；PostgreSQL 支持 `postgres://` 和 `postgresql://` URL。SQL backend 适合需要强约束 session id、跨进程或多写者一致性的 registry。
- `registry-store-object` feature 启用 OpenDAL backend。每个 session 是 prefix 下的一个 JSON object，session id 使用 URL-safe base64 编码成 object key。该 backend 是 single-writer snapshot store，不承诺多进程同时写同一 registry 的强一致语义；需要多写者时应使用 SQL backend。

权限分为两层：

- authority capabilities：控制敏感动作，例如 `agent.run`、`agent.queue`、`approval.resolve`、`manifest.apply`、`process.control`、`subagent.spawn`、`session.delete`。
- UX capabilities：描述外部 bridge 的展示能力，例如 raw/display event、streaming text、message edit、Markdown 和 `maxMessageBytes`。

事件也分两层：

- raw event：直接发送 core `AgentEvent`，用于高级 bridge、审计或调试。
- display event：从 raw event 派生 UI 友好的 `DisplayEvent`，包括 run lifecycle、assistant delta/final、tool lifecycle 和 approval card。`streamText = false` 的 bridge 只需要渲染 final message；`maxMessageBytes` 会触发 bounded head/tail text truncation。

进程控制面不负责启动任意命令；后台命令仍由模型通过 `host.exec.start` 工具创建。bridge 只通过 `process/list`、`process/read`、`process/wait`、`process/write`、`process/terminate` 显示和控制已存在 job，并复用 `HostProcessManager` 的 cursor、spool、truncation 和 completion semantics。

协议细节见 `crates/noloong-agent/docs/INTERACTION.md`，验证矩阵见 `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`。

## i18n

所有给模型看的 application-generated natural-language text 都走 typed catalog，包括 host context、tool description、permission description、approval prompt/reason、tool input/process/manifest error、background completion steering message，以及 oversized tool output rewrite message。v1 默认支持 English 和 Chinese。

locale 解析顺序：

1. 显式配置。
2. 宿主机 `LC_ALL`、`LC_MESSAGES`、`LANG`。
3. English fallback。

catalog key 必须完整；缺失 key 应在测试中失败，而不是运行时静默 fallback。

## 后续演进方向

- 在不改变 `InteractionControlHandler` 的前提下增加 WebSocket/HTTP transport；stdio JSON-RPC 继续作为最低依赖 conformance transport。
- 将 display projection 拆成可插拔策略，使 Telegram、WeChat/iLink、TUI 和 Web UI 能各自定义消息合并、编辑和 truncation 策略。
- 为 process control 增加可选 host sandbox/VMM policy，把当前 host-first tools 扩展到 SSH、Lima/QEMU 或其它隔离环境。
- 增强 manifest apply lifecycle，让 approved patch 能自动触发下一次 session runtime rebuild，并在 control plane 中暴露 rebuild/audit 结果。
- 为 interaction bridge 作者提供可复用 conformance runner，而不是只依赖 Rust 集成测试。
