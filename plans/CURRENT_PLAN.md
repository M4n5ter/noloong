# 实施计划：noloong-agent 交互控制面

## 概览

`crates/noloong-agent` 已经具备核心应用层能力：`AgentSession`、`AgentManifest`、后台命令 lifecycle、approval、manifest proposal、queue 和 event subscription。当前缺口不是 Rust API，而是缺少一个语言无关的交互控制面，让 TS/Python/Go 等第三方 bridge 可以连接 Telegram、微信/iLink、Web UI、CLI 或未来 subagent orchestration，并通过稳定协议驱动多个 agent session。

本计划在 `noloong-agent` 中新增 stdio JSON-RPC control server。外部 bridge 作为 client，通过 JSON-RPC 创建/管理 session、订阅 raw/display events、提交用户输入、处理 approval、控制后台进程、应用 manifest proposal，以及创建 subagent session。`noloong-agent-core` 继续保持 providerless/event-sourced kernel，只允许做必要 serde 补强，不引入产品交互语义。

## 架构决策

- 交互控制面属于 `noloong-agent`，不进入 `noloong-agent-core` 的 extension capability；core 已有 `Agent`、`RuntimeQueues`、approval pause/resume 和 event sink 足够作为底座。
- transport v1 只实现 line-delimited stdio JSON-RPC 2.0；后续 WebSocket/HTTP 可以复用同一 handler，不在本轮实现。
- 一个 control server 维护一个 session registry，可管理多个 `AgentSession + Agent`；每个 subagent 也是独立 session，并带 `parentSessionId`、`role`、`metadata`。
- registry store 使用 trait 抽象，默认 `InMemoryAgentSessionRegistryStore`；SQLite/file 持久化不是 v1 必需实现，但接口不能阻碍后续落地。
- runtime/profile 走 Rust host 注册的 `AgentRuntimeProfile` trait；RPC 只选择 profile，不允许外部 bridge 任意注入 provider credential。
- capability 分两类：authority capabilities 控制敏感方法，UX capabilities 描述展示能力。Telegram 可声明 `streamText + editMessage`，微信/iLink 可声明 final-only。
- event surface 同时提供 raw `AgentEvent` 和派生 `DisplayEvent`。raw 用于高级 bridge/审计，display 用于 UI adapter 直接渲染。
- 所有方法和 wire type 使用 camelCase JSON；错误使用 JSON-RPC structured error，不靠字符串约定。

## 任务列表

### Phase 1：协议与领域模型

#### 任务 1：定义 interaction wire types 和 capability 模型

**描述：** 新增 `noloong_agent::interaction` 模块，定义 JSON-RPC control plane 的请求/响应、session 描述、profile 描述、capability grant、display event 和错误类型。此任务只建立类型和校验逻辑，不启动 server。

**验收标准：**

- [ ] `InteractionAuthorityCapability` 覆盖 `agent.run`、`agent.queue`、`approval.resolve`、`manifest.apply`、`process.control`、`subagent.spawn`、`session.delete`。
- [ ] `InteractionUxCapability` 覆盖 `rawEvents`、`displayEvents`、`streamText`、`editMessage`、`markdown`、`maxMessageBytes`。
- [ ] `InteractionClientInfo` 在 initialize 阶段携带 requested authority/UX capabilities。
- [ ] server policy 能把 requested capabilities 过滤成 granted capabilities。
- [ ] 未授权 method 能映射为稳定 JSON-RPC error code。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_capability_grants`
- [ ] `cargo test -p noloong-agent interaction_wire_serde`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/mod.rs`
- `crates/noloong-agent/src/interaction/wire.rs`
- `crates/noloong-agent/src/lib.rs`
- `crates/noloong-agent/tests/interaction.rs`

**预计范围：** 中

#### 任务 2：实现 JSON-RPC stdio substrate

**描述：** 在 `noloong-agent` 内实现面向 control server 的 line-delimited JSON-RPC 读写层。不要复用 core 的 `StdioExtension` 对外 API，因为本方向是外部 bridge 调用 Rust server；但 wire 风格应与现有 extension 文档保持一致。

**验收标准：**

- [ ] stdin 每行解析一个 JSON-RPC request。
- [ ] stdout 每行输出一个 JSON-RPC response 或 notification。
- [ ] stderr 保留给日志，不参与协议。
- [ ] invalid JSON、unknown method、invalid params、handler error 都返回结构化 error。
- [ ] request handler 支持 async，且单个请求失败不关闭 server。
- [ ] `shutdown` 可优雅退出。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_jsonrpc_invalid_input`
- [ ] `cargo test -p noloong-agent interaction_jsonrpc_shutdown`
- [ ] `node --check examples/interaction/telegram-bridge/bridge.mjs`（示例存在后）
- [ ] `python3 -m py_compile examples/interaction/python_cli_bridge/bridge.py`（示例存在后）

**依赖：** 任务 1

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/jsonrpc.rs`
- `crates/noloong-agent/src/interaction/server.rs`
- `crates/noloong-agent/tests/interaction_jsonrpc.rs`

**预计范围：** 中

### Checkpoint：协议基础

- [ ] interaction 类型 serde roundtrip 通过。
- [ ] JSON-RPC substrate 能处理正常请求、错误请求和 `shutdown`。
- [ ] `cargo fmt --check`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

### Phase 2：Session Registry 与 Runtime Profile

#### 任务 3：实现 session registry 和 store trait

**描述：** 新增 `AgentSessionRegistry` 管理多个 session。每个 registry entry 持有 `AgentSession`、`Agent`、runtime profile id、session metadata、parent/role 信息和订阅状态。默认 store 是内存实现，但 API 必须为后续持久化留出边界。

**验收标准：**

- [ ] `session/create` 可创建 root session。
- [ ] `session/list` 支持按 `parentSessionId`、`profileId`、`status` 过滤。
- [ ] `session/get` 返回 manifest、state summary、profile id、parent/role、metadata。
- [ ] `session/delete` 拒绝删除 running/paused session，除非请求显式 `forceAbort` 且具备 `session.delete` authority。
- [ ] registry store trait 不暴露 `Agent` 内部锁或 runtime implementation detail。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_registry_create_list_get`
- [ ] `cargo test -p noloong-agent interaction_registry_delete_running_requires_force`
- [ ] `cargo test -p noloong-agent interaction_registry_parent_filter`

**依赖：** 任务 1

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/store.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**预计范围：** 中

#### 任务 4：实现 `AgentRuntimeProfile` 注册与选择

**描述：** 引入 runtime profile 抽象，让 Rust host 控制可用模型、provider、tools、context compaction 和 extensions。RPC 只通过 `profileId` 选择 profile，避免第三方 bridge 直接传 credential 或任意 provider 配置。

**验收标准：**

- [ ] `AgentRuntimeProfile` 能基于 `AgentManifest` 构建 `AgentRuntime`。
- [ ] `profile/list` 返回 profile id、display name、description、metadata 和默认 manifest patch。
- [ ] `session/create` 未指定 profile 时使用 server default profile。
- [ ] unknown profile id 返回 structured error。
- [ ] profile 构建失败不会污染 registry。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_profile_list`
- [ ] `cargo test -p noloong-agent interaction_session_create_uses_profile`
- [ ] `cargo test -p noloong-agent interaction_session_create_unknown_profile_fails`

**依赖：** 任务 3

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/profile.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/interaction_profile.rs`

**预计范围：** 中

#### 任务 5：实现 subagent session spawn

**描述：** 在 registry 层实现 `subagent/spawn`。v1 的 subagent 是带 parent metadata 的独立 session，可选择立即 prompt，但不新增模型可调用 subagent tool，也不自动调度任务。

**验收标准：**

- [ ] `subagent/spawn` 需要 `subagent.spawn` authority。
- [ ] child session 必须记录 `parentSessionId`、`role` 和 caller-provided metadata。
- [ ] child 可继承 parent profile，也可显式选择其它 profile。
- [ ] 可选 `initialPrompt` 会创建 session 后立即启动 child agent run。
- [ ] parent session 不会因为 child run 失败而自动失败。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_subagent_spawn_metadata`
- [ ] `cargo test -p noloong-agent interaction_subagent_spawn_initial_prompt`
- [ ] `cargo test -p noloong-agent interaction_subagent_spawn_requires_capability`

**依赖：** 任务 3、任务 4

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/interaction/handlers.rs`
- `crates/noloong-agent/tests/interaction_subagent.rs`

**预计范围：** 中

### Checkpoint：多 Session 基础

- [ ] root session 和 child session 都能创建、查询、删除。
- [ ] profile 选择和构建失败路径有测试。
- [ ] registry 不依赖具体 transport。
- [ ] `cargo test -p noloong-agent interaction_registry interaction_profile interaction_subagent`

### Phase 3：Agent 操作与事件订阅

#### 任务 6：实现 agent run 和 queue 方法

**描述：** 暴露 `Agent` 已有的 prompt、continue、steer、follow-up、abort、wait idle 和 queue edit API。方法必须尊重现有 active-run exclusivity、steering/follow-up intent 和 user input 特殊路由语义。

**验收标准：**

- [ ] `agent/prompt` 支持 text 和完整 `AgentMessage` 输入。
- [ ] `agent/continue` 复用 core 的 continuation validation。
- [ ] `agent/steer` 支持 `observation` 和 `userInput` intent。
- [ ] `agent/follow_up` 默认使用 `userInput` intent。
- [ ] `queue/list`、`queue/edit`、`queue/clear`、`queue/set_mode` 支持 steering/follow-up 两类队列。
- [ ] active run 中重复 `agent/prompt` 返回 busy error，不隐式排队。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_agent_prompt_events`
- [ ] `cargo test -p noloong-agent interaction_agent_steer_user_input`
- [ ] `cargo test -p noloong-agent interaction_queue_edit`
- [ ] `cargo test -p noloong-agent interaction_agent_busy_error`

**依赖：** 任务 3、任务 4

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/handlers.rs`
- `crates/noloong-agent/src/interaction/wire.rs`
- `crates/noloong-agent/tests/interaction_agent.rs`

**预计范围：** 中

#### 任务 7：实现 raw event subscription

**描述：** 将 `Agent::subscribe` 暴露给 JSON-RPC client。外部 bridge 可按 session 订阅 raw `AgentEvent`，也可取消订阅。通知必须包含 `sessionId`，避免多 session bridge 混淆事件来源。

**验收标准：**

- [ ] `event/subscribe` 支持 raw events。
- [ ] `event/unsubscribe` 能停止后续通知。
- [ ] raw event notification 方法名固定为 `agent/event`。
- [ ] 每条通知包含 `sessionId`、`subscriptionId`、`event`。
- [ ] listener 失败不会破坏 agent state；JSON-RPC writer 关闭时 server 能停止派发。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_raw_event_subscribe`
- [ ] `cargo test -p noloong-agent interaction_raw_event_unsubscribe`
- [ ] `cargo test -p noloong-agent interaction_raw_event_multi_session`

**依赖：** 任务 6

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/events.rs`
- `crates/noloong-agent/src/interaction/server.rs`
- `crates/noloong-agent/tests/interaction_events.rs`

**预计范围：** 中

#### 任务 8：实现 display event projection

**描述：** 从 raw `AgentEvent` 派生 UI 友好的 `DisplayEvent`，让不想理解 core event log 的 bridge 也能直接渲染。投影必须根据 UX capabilities 调整：Telegram-like client 可收到可编辑消息更新，微信/iLink-like client 可收到 final-only 输出。

**验收标准：**

- [ ] `display/subscribe` 订阅 display events，通知方法名固定为 `display/event`。
- [ ] assistant text delta 聚合为 `assistantMessageDelta` / `assistantMessageFinal`。
- [ ] tool lifecycle 投影为 `toolStarted`、`toolUpdated`、`toolCompleted`。
- [ ] approval request 投影为 `approvalRequested` display card。
- [ ] run status 投影为 `runStarted`、`runCompleted`、`runFailed`、`runPaused`。
- [ ] `maxMessageBytes` 超限时做 bounded truncation，并保留 truncation metadata。
- [ ] 不支持 `streamText` 的 client 只收到 final display message。
- [ ] 支持 `editMessage` 的 client 收到稳定 `displayMessageId`，可用于外部消息编辑。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_display_streaming_with_edit`
- [ ] `cargo test -p noloong-agent interaction_display_final_only`
- [ ] `cargo test -p noloong-agent interaction_display_approval_card`
- [ ] `cargo test -p noloong-agent interaction_display_truncation`

**依赖：** 任务 7

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/display.rs`
- `crates/noloong-agent/src/interaction/events.rs`
- `crates/noloong-agent/tests/interaction_display.rs`

**预计范围：** 中

### Checkpoint：可交互闭环

- [ ] 外部 JSON-RPC client 可创建 session、订阅事件、prompt、收到最终 assistant 输出。
- [ ] 支持 raw-only 和 display-only 两种 bridge 风格。
- [ ] streaming/edit capability 对通知行为有可测影响。
- [ ] `cargo test -p noloong-agent interaction_agent interaction_events interaction_display`

### Phase 4：Approval、Manifest 和 Process 控制

#### 任务 9：实现 approval control methods

**描述：** 暴露 tool approval 查询和恢复能力。control plane 需要与 `AgentSession::record_tool_approval_resolution` 集成，让 allow decision 可进入 session approval cache。

**验收标准：**

- [ ] `approval/list` 返回 session 当前 pending approvals。
- [ ] `approval/resolve` 需要 `approval.resolve` authority。
- [ ] allow/deny decision wire shape 复用 core `ToolPermissionDecision`。
- [ ] allow 且符合内置 cache 条件时调用 `AgentSession::record_tool_approval_resolution`。
- [ ] `approval/resume_timeouts` 可触发已过期 approval deny。
- [ ] unknown approval id 返回 structured error。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_approval_list_resolve_allow`
- [ ] `cargo test -p noloong-agent interaction_approval_resolution_records_cache`
- [ ] `cargo test -p noloong-agent interaction_approval_timeout_resume`
- [ ] `cargo test -p noloong-agent interaction_approval_requires_capability`

**依赖：** 任务 6、任务 8

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/handlers.rs`
- `crates/noloong-agent/src/interaction/wire.rs`
- `crates/noloong-agent/tests/interaction_approval.rs`

**预计范围：** 中

#### 任务 10：实现 manifest control methods

**描述：** 暴露 manifest 查询、proposal 查询、proposal approve 和 apply approved patch。外部 bridge 可展示 agent 自进化提案，并在用户确认后应用。

**验收标准：**

- [ ] `manifest/get` 返回当前 manifest。
- [ ] `manifest/proposals/list` 返回 pending proposals。
- [ ] `manifest/proposals/approve` 将 proposal 移入 approved queue。
- [ ] `manifest/apply_approved` 需要 `manifest.apply` authority，并调用 `AgentSession::apply_approved_manifest_patches`。
- [ ] apply 后后续 runtime build 使用新 manifest。
- [ ] reserved phase profile patch 仍然不可执行。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_manifest_get`
- [ ] `cargo test -p noloong-agent interaction_manifest_proposal_approve_apply`
- [ ] `cargo test -p noloong-agent interaction_manifest_apply_requires_capability`
- [ ] `cargo test -p noloong-agent interaction_manifest_reserved_patch_rejected`

**依赖：** 任务 3、任务 6

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/handlers.rs`
- `crates/noloong-agent/tests/interaction_manifest.rs`

**预计范围：** 小

#### 任务 11：实现 process control methods

**描述：** 暴露 `HostProcessManager` 的只读和控制能力，使外部 bridge 可以显示后台任务、拉取输出、写 stdin、等待或终止进程。敏感操作必须按 authority gate 区分。

**验收标准：**

- [ ] `process/list` 不需要 `process.control`，但需要基础 session access。
- [ ] `process/read` 可按 `jobId`、`afterSeq`、`maxBytes`、`waitMs` 拉取输出。
- [ ] `process/wait`、`process/write`、`process/terminate` 需要 `process.control` authority。
- [ ] 输出保持现有 head/tail/cursor/truncation 语义，不复制完整 spool 到 JSON-RPC event。
- [ ] unknown job id 返回 structured error。

**验证：**

- [ ] `cargo test -p noloong-agent interaction_process_list_read`
- [ ] `cargo test -p noloong-agent interaction_process_wait_write_terminate`
- [ ] `cargo test -p noloong-agent interaction_process_control_requires_capability`

**依赖：** 任务 3

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/handlers.rs`
- `crates/noloong-agent/tests/interaction_process.rs`

**预计范围：** 中

### Checkpoint：完整控制面

- [ ] prompt、event、approval、manifest、process 和 subagent 方法都经过 capability gate。
- [ ] display projection 能覆盖 approval 和 tool/process 状态。
- [ ] `cargo test -p noloong-agent interaction_approval interaction_manifest interaction_process`

### Phase 5：文档、示例和验证矩阵

#### 任务 12：编写 interaction 协议文档

**描述：** 新增面向第三方 bridge 作者的文档，说明 stdio transport、initialize/capabilities、session registry、事件订阅、approval、manifest、process 和 subagent 方法。文档要能让 TS/Python 作者不读 Rust 源码也能实现 client。

**验收标准：**

- [ ] 新增 `crates/noloong-agent/docs/INTERACTION.md`。
- [ ] 每个 JSON-RPC method 都有 params/result 示例。
- [ ] 文档明确 raw event 与 display event 的差异。
- [ ] 文档明确 Telegram-like 和 WeChat/iLink-like UX capabilities 示例。
- [ ] 文档明确 authority capability 的安全边界。
- [ ] 更新 `crates/noloong-agent/docs/ARCHITECTURE.md` 的后续演进方向。

**验证：**

- [ ] `rg "session/create|agent/prompt|display/event|approval/resolve|subagent/spawn" crates/noloong-agent/docs/INTERACTION.md`
- [ ] `cargo test -p noloong-agent --test interaction_docs_contract`

**依赖：** 任务 1 至任务 11

**可能涉及文件：**

- `crates/noloong-agent/docs/INTERACTION.md`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/tests/interaction_docs_contract.rs`

**预计范围：** 中

#### 任务 13：提供 TypeScript 和 Python bridge 示例

**描述：** 增加最小第三方语言 client 示例，证明外部 bridge 可以启动/连接 control server，创建 session，订阅 display events，发送 prompt，并处理 approval。

**验收标准：**

- [ ] TypeScript/Node 示例包含 client wrapper、session create、prompt、display event render、approval resolve。
- [ ] Python 示例使用标准库或最少依赖实现同等流程。
- [ ] 示例不包含真实 Telegram/微信 SDK token；只模拟 adapter 行为。
- [ ] README 解释如何替换成 Telegram edit-message 或微信 final-only 策略。

**验证：**

- [ ] `node --check examples/interaction/typescript-bridge/*.mjs`
- [ ] `python3 -m py_compile examples/interaction/python-bridge/*.py`
- [ ] `cargo test -p noloong-agent --test interaction_examples`

**依赖：** 任务 12

**可能涉及文件：**

- `examples/interaction/typescript-bridge/`
- `examples/interaction/python-bridge/`
- `crates/noloong-agent/tests/interaction_examples.rs`

**预计范围：** 中

#### 任务 14：补充 conformance 和 workspace gate

**描述：** 为 interaction control plane 增加 agent-level conformance 入口和验证矩阵，确保后续新增 method 或 wire type 时必须更新测试证据。

**验收标准：**

- [ ] 新增 `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md` 或在现有 agent 文档中加入 interaction capability matrix。
- [ ] matrix 覆盖 session registry、capability gate、agent run、queue、raw/display events、approval、manifest、process、subagent。
- [ ] 默认 gate 明确包含 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、TS/Python 示例检查。
- [ ] docs contract 测试会在新增公开 method 但未更新文档时失败。

**验证：**

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `node --check examples/interaction/typescript-bridge/*.mjs`
- [ ] `python3 -m py_compile examples/interaction/python-bridge/*.py`

**依赖：** 任务 12、任务 13

**可能涉及文件：**

- `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`
- `crates/noloong-agent/tests/interaction_docs_contract.rs`
- `README.md`

**预计范围：** 小

### Checkpoint：完成

- [ ] 第三方语言 bridge 可完整走通 create session -> subscribe display -> prompt -> approval -> final response。
- [ ] 多 session 和 subagent registry 行为可测。
- [ ] raw/display event 两层文档与示例完整。
- [ ] 所有 sensitive methods 都有 capability gate 测试。
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 控制面一次性变成远程 daemon 平台 | 高 | v1 只做 stdio JSON-RPC；transport 与 handler 分层，WebSocket/HTTP 后续再接。 |
| capability gate 过粗导致 bridge 权限过大 | 高 | authority 与 UX capability 分离；每个敏感 method 有明确 gate 测试。 |
| display projection 变成第二套 agent state | 中 | display event 只从 raw event 派生，不作为事实来源；事实仍是 core event log 和 `AgentState`。 |
| subagent 语义过早绑定未来设计 | 中 | v1 subagent 只做 session registry parent/role 和 spawn，不做模型可调用 subagent tool。 |
| runtime profile 过度限制扩展性 | 中 | profile 用 Rust trait，而不是枚举；外部 provider 能通过 host 自定义 profile 或现有 stdio extension 注入。 |
| 文档与 wire contract 漂移 | 中 | 增加 docs contract 测试和 conformance matrix。 |

## 明确不做

- 不把 interaction control plane 加入 `noloong-agent-core` 的 `ExtensionCapability`。
- 不内置 Telegram、微信、iLink 或 Web UI SDK。
- 不实现 WebSocket/HTTP transport。
- 不实现 SQLite registry store，只保留 pluggable store trait 和 in-memory default。
- 不允许 JSON-RPC client 直接提交 provider credential 或任意模型配置。
- 不新增模型可调用的 subagent tool；本轮只实现 control plane 级 subagent session。
