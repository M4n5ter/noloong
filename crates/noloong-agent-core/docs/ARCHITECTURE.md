# noloong-agent-core 架构说明

本文档描述 `noloong-agent-core` 当前的核心架构、运行流程、扩展边界和内置模型 provider 的工作方式。它关注的是 crate 内部的长期设计约束，而不是某一个应用层 agent 的具体产品形态。

## 设计定位

`noloong-agent-core` 是一个 event-sourced、providerless 的 Rust agent kernel。它不把某个模型厂商、某套工具协议、某个上下文策略或某种 agent loop 写死到核心里，而是把 agent loop 拆成可替换的 phase graph，并通过 typed events 和 effects 约束状态变更。

当前 crate 分为五层：

1. Kernel runtime：`AgentRuntime`、phase graph、event store、reducer。
2. Typed data model：`AgentEvent`、`AgentEffect`、`AgentState`、message/content/tool/thinking/model stream 类型。
3. Native extension traits：Rust 原生的 `ModelProvider`、`ToolProvider`、`ContextProvider`、`PhaseNode`、`PhaseHook`、`ToolCallHook`、`CompactionSummarizer`。
4. Process extension bridge：stdio JSON-RPC，把 JS/TS/Python 等外部进程适配为 Rust trait。
5. UX layer：`Agent`，在 kernel 之上提供持久状态、订阅、队列、abort、continue 等交互能力。

核心设计目标是：

- agent loop 的每个主要阶段都可以扩展、替换或插入。
- 状态变更必须可审计、可验证、可重放。
- 模型 provider、工具 provider、上下文 provider 不依赖具体厂商。
- 外部语言扩展与 Rust-native 扩展共享同一套核心类型语义。
- thinking/reasoning 不是强行降级成纯文本，而是保留结构化数据和 replay 信息。
- 图像、音频、视频、文件不是 provider-specific JSON，而是统一的 media block，由 provider adapter 映射到各自协议。

## 模块布局

主要模块如下：

- `src/lib.rs`：crate public API 的出口。
- `src/types.rs`：事件、状态、消息、工具、thinking、stream event 等核心类型。
- `src/providers.rs`：provider 和 hook traits，以及 request 类型。
- `src/phase.rs`：标准 phase graph 和默认 phase 实现。
- `src/runtime.rs`：`AgentRuntime`、builder、turn loop、事件记录、phase 执行。
- `src/reducer.rs`：event replay 和 effect validation。
- `src/store.rs`：`EventStore` trait 和 `InMemoryEventStore`。
- `src/agent.rs`：有状态 `Agent` UX layer。
- `src/jsonrpc.rs`：stdio JSON-RPC extension bridge。
- `src/chat_completions.rs`：内置 OpenAI-compatible Chat Completions provider。
- `src/responses.rs`：内置 OpenAI Responses API provider。
- `src/anthropic_messages.rs`：内置 Anthropic Messages provider。
- `src/compaction.rs`：context compaction 配置、planner、token estimator 和 summarizer 实现。

高层关系可以概括为：

```text
Agent
  owns persistent AgentState
  delegates runs to AgentRuntime

AgentRuntime
  owns EventStore
  owns PhaseNode graph
  owns ModelProvider / ToolProvider / ContextProvider / PhaseHook / ToolCallHook / CompactionSummarizer registries
  emits AgentEvent
  commits AgentEffect

PhaseNode
  receives PhaseContext
  returns PhaseOutput

Provider traits
  implement model streaming, tool execution, context preparation

Reducer
  replays committed events into AgentState
```

## Event-Sourced Kernel

核心状态不是由 phase 直接随意修改，而是由事件日志驱动。`AgentRuntime` 执行过程中持续写入 `AgentEvent`，并在写入后立即通过 reducer 应用到当前内存状态。

`AgentEvent` 包含：

- `sequence`：全局递增事件序号。
- `run_id`：运行 ID。
- `turn_id`：可选 turn ID。
- `phase`：可选 phase ID。
- `kind`：事件类型。

`AgentEventKind` 覆盖以下类别：

- run 生命周期：`RunStarted`、`RunCompleted`、`RunAborted`、`RunFailed`。
- turn 生命周期：`TurnStarted`、`TurnCompleted`。
- phase 生命周期：`PhaseStarted`、`PhaseCompleted`、`PhaseFailed`。
- effect 生命周期：`EffectProposed`、`EffectCommitted`、`EffectRejected`。
- model stream：`ModelStreamEvent`。
- tool 生命周期：`ToolCallResolved`、`ToolExecutionStarted`、`ToolExecutionUpdate`、`ToolExecutionCompleted`。
- extension event：`ExtensionEvent`。

真正能修改 `AgentState` 的是 `AgentEffect`：

- `AppendMessage`：追加一条消息。
- `PatchContext`：设置或删除 context key。
- `SetAvailableTools`：替换当前可用工具集合。
- `CompactMessages`：用一条 summary message 加 retained messages 替换当前消息历史。

effect 提交流程是：

```text
PhaseOutput.effects
  -> EffectProposed
  -> validate_effect
  -> EffectCommitted or EffectRejected
  -> reducer applies committed effect
```

这个设计让运行过程具备两个关键性质：

1. 可审计：模型输出、工具调用、工具结果、上下文修改和 phase 生命周期都有事件记录。
2. 可重放：`reduce_events(events)` 可以从事件日志恢复 `AgentState`。

`CompactMessages` 是 context compaction 的持久状态入口。它携带：

- `summary_message`
- `retained_message_ids`
- `dropped_message_ids`
- `tokens_before`
- `tokens_after`
- `metadata`

reducer 会校验 retained/dropped id 覆盖当前 messages，且 summary message id 是新的 system message。提交后，`AgentState.messages` 变为 `[summary_message] + retained_messages`。这让 compaction 后的 state 仍然可以从 event log 精确 replay，而不是在内存中偷偷裁剪历史。

当前默认 store 是 `InMemoryEventStore`，但 `EventStore` trait 只有 `append` 和 `load`，后续替换为 SQLite、PostgreSQL、object store 或 append-only log 都比较直接。

## AgentState 和消息模型

`AgentState` 当前包含：

- `run_id`
- `status`
- `messages`
- `context`
- `available_tools`
- `active_phase`
- `completed_turns`
- `last_error`

消息是 `AgentMessage`：

```rust
pub struct AgentMessage {
    pub id: MessageId,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    pub metadata: Map<String, Value>,
}
```

`MessageRole` 支持：

- `User`
- `Assistant`
- `ToolResult`
- `System`
- `Custom(String)`

内容块是 `ContentBlock`：

- `Thinking`
- `Media`
- `Text`
- `Json`
- `ToolCall`
- `ToolResult`

一个 assistant message 可以同时包含 thinking、visible text、media 和 tool calls，并且这些 block 保留模型流里的相对顺序边界。比如一个模型先输出 thinking，再输出一段正文，再输出媒体，再发起工具调用，最终会形成大致如下结构：

```text
AgentMessage(role=assistant)
  ContentBlock::Thinking
  ContentBlock::Text
  ContentBlock::Media
  ContentBlock::ToolCall
```

## Media 数据模型

图像、音频、视频和文件被建模为 provider-neutral media block，而不是 OpenAI、Anthropic 或其它 provider 的原始 content part。

`MediaBlock` 包含：

- `kind`：`image`、`audio`、`video`、`file` 或 `custom`。
- `source`：`Uri`、`Inline` 或 `Provider`。
- `data`：可选的编码 payload，用于保留 provider-hosted 输出资源同时流式返回 inline data 的场景。
- `mime_type`：可选 MIME type。
- `name`：可选文件名或展示名。
- `replay_descriptor`：描述如何在同 provider/model 下 replay provider-hosted media。
- `metadata`：附加信息，例如 transcript、expires_at、provider 原始字段。

`MediaSource` 的语义：

- `Uri`：事件日志保存引用，不下载也不内联。
- `Inline`：调用方已经提供的编码数据；v1 内置 provider 只消费 base64。
- `Provider`：provider-hosted 文件或输出资源，带 `provider_id` 和 provider-local id。

stream 侧对应 `MediaDelta`：

- `kind`
- `data_delta`
- `source`
- `mime_type`
- `name`
- `replay_descriptor`
- `metadata`
- `done`

当前实现故意不引入 `MediaStore`。这意味着 core 可以审计和重放 media 引用，但不负责 blob 生命周期、下载、上传、缓存、去重或加密存储。后续如果需要大体积 blob 管理，可以在不改变 message model 的前提下新增 store abstraction。

video 在 core 中是一等 `MediaKind::Video`。内置 Chat Completions provider 会把 video URI 或 inline base64 映射到 `video_url` content part；provider-hosted video file/ref 只有在调用方显式允许 provider video file media 时才会作为 file content part 透传。

## Thinking 数据模型

thinking/reasoning 被建模为结构化内容，而不是简单字符串。

`ThinkingBlock` 包含：

- `kind`：`raw`、`summary`、`redacted`、`encrypted` 或 `custom`。
- `text`：可展示的 thinking 文本或 summary。
- `raw`：provider 原始 thinking payload，可以是 string、object、array。
- `replay_descriptor`：描述如何在同 provider/model 下 replay 原始 thinking。
- `metadata`：附加信息。

stream 侧对应 `ThinkingDelta`：

- `kind`
- `text_delta`
- `raw_snapshot`
- `replay_descriptor`
- `metadata`

这个结构解决了 Chat Completions 兼容生态里的几个现实问题：

- 有的 provider 返回纯文本 reasoning。
- 有的 provider 返回 `reasoning_details` 数组。
- 有的 provider 返回 object，里面可能只有 summary，没有 raw text。
- 有的 provider 不允许或不应该暴露 raw thinking，只能返回 summary、redacted 或 encrypted placeholder。
- 同一个 thinking payload 可能只能在同 provider/model 内 replay，不能跨 provider 注入。

因此核心 API 不把 thinking 固定成 `String`，而是允许 provider 同时提供可展示文本、原始 payload、replay 描述和元数据。

## Provider Traits

核心 provider traits 位于 `providers.rs`。

`ModelProvider`：

```rust
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>>;
}
```

模型 provider 可以通过 `stream` sink 实时推送 `ModelStreamEvent`，也可以在返回值里返回事件集合。runtime 会记录 provider 已经通过 sink 发出的事件，并避免重复记录。

`ToolProvider`：

```rust
pub trait ToolProvider: Send + Sync {
    fn spec(&self) -> ToolSpec;

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput>;
}
```

`ContextProvider`：

```rust
pub trait ContextProvider: Send + Sync {
    fn id(&self) -> &str;

    fn prepare_context<'a>(
        &'a self,
        request: ContextRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<AgentEffect>>;
}
```

`ToolCallHook`：

```rust
pub trait ToolCallHook: Send + Sync {
    fn before_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeToolCallResult>>;

    fn after_tool_call<'a>(
        &'a self,
        context: AfterToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterToolCallResult>>;
}
```

hook 当前专注于工具调用前后：

- before hook 可以 block 工具调用，并返回可审计的 error tool output。
- after hook 可以改写工具输出的 content、details、is_error。

`PhaseHook`：

```rust
pub trait PhaseHook: Send + Sync {
    fn before_model_request<'a>(
        &'a self,
        context: BeforeModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeModelRequestHookResult>>;

    fn after_model_request<'a>(
        &'a self,
        context: AfterModelRequestHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterModelRequestHookResult>>;

    fn before_assistant_commit<'a>(
        &'a self,
        context: BeforeAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<BeforeAssistantCommitHookResult>>;

    fn after_assistant_commit<'a>(
        &'a self,
        context: AfterAssistantCommitHookContext<'a>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterAssistantCommitHookResult>>;
}
```

`PhaseHook` 是标准 phase 内的细粒度拦截点，适合在不替换完整 phase 的情况下调整 request、model events 或最终 assistant message。context 以借用形式暴露当前值，result 使用完整替换语义：

- `before_model_request` 替换 `ModelRequest`。
- `after_model_request` 替换后续 phase 使用的 `Vec<ModelStreamEvent>`。
- `before_assistant_commit` 替换折叠前的 `Vec<ModelStreamEvent>`。
- `after_assistant_commit` 替换最终 append 的 `AgentMessage`。

hooks 按注册顺序串行执行，后一个 hook 看到前一个 hook 的修改结果。任意 hook 返回 error 时，当前 phase 失败并走现有 `PhaseFailed` / run failure 路径。`after_model_request` 和 `before_assistant_commit` 修改的是后续 commit 输入，不会回写已经记录过的 raw provider stream events；最终 state 以 committed assistant message 为准。

`CompactionSummarizer`：

```rust
pub trait CompactionSummarizer: Send + Sync {
    fn id(&self) -> &str;

    fn summarize<'a>(
        &'a self,
        request: CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult>;
}
```

`CompactionSummarizer` 专门负责把将被裁剪的旧消息生成结构化 summary。它独立于 `ModelProvider`，所以 Rust 调用方可以用内置 model-backed summarizer，也可以通过 JSON-RPC 接入 JS/TS、Python 或其它模型 SDK。summary request 包含 previous summary、待摘要 messages、split-turn prefix messages、token budget 和 metadata。

选择扩展点时的边界是：

- 用 `PhaseNode` 替换完整 phase 或插入新的 phase。
- 用 `PhaseHook` 拦截标准 phase 的稳定边界。
- 用 `ToolCallHook` 专门处理 tool execution 前后的 block/rewrite。
- 用 `CompactionSummarizer` 替换 long-context 摘要生成策略。

## Phase Graph

phase 是 agent loop 的主要扩展边界：

```rust
pub trait PhaseNode: Send + Sync {
    fn id(&self) -> &str;
    fn run<'a>(&'a self, context: PhaseContext<'a>) -> BoxFuture<'a, PhaseOutput>;
}
```

`PhaseContext` 输入：

- `runtime`
- `run_id`
- `turn_id`
- `state`
- `scratch`
- `cancellation`
- `model_stream_sink`

`PhaseOutput` 输出：

- `scratch`
- `effects`
- `stream_events`
- `resolved_tool_calls`
- `tool_outputs`
- `completed_tool_outputs`

默认 phase 顺序是：

```text
input.ingest
context.prepare
model.request.prepare
model.stream
assistant.commit
tool.call.resolve
tool.execute
turn.decision
```

配置 context compaction 时，builder 会把标准 `context.compact` 自动插入在 `context.prepare` 和 `model.request.prepare` 之间；未配置时不会产生额外 no-op phase events。如果调用方显式替换或插入了同名 phase，builder 不会重复插入。

builder 支持：

- `replace_phase(phase_id, phase)`
- `insert_phase_after(after_phase_id, phase)`

这意味着核心 loop 的任意阶段都可以替换或插入。例如：

- 替换 `model.request.prepare` 来实现自定义 prompt assembly。
- 替换 `context.compact` 来实现完全自定义的压缩策略。
- 替换 `assistant.commit` 来实现更复杂的 output parser。
- 插入 phase 在 `tool.execute` 后做 tool result summarization。
- 替换 `turn.decision` 来实现 planner/executor、多 agent handoff 或 budget-aware loop。

## 默认 Turn 流程

一次 `AgentRuntime::run` 的高层流程是：

```text
RunStarted
  TurnStarted
    PhaseStarted(input.ingest)
    PhaseCompleted(input.ingest)
    ...
    PhaseStarted(turn.decision)
    PhaseCompleted(turn.decision)
  TurnCompleted
RunCompleted
```

失败时会记录：

```text
PhaseFailed
RunFailed
```

取消时会记录：

```text
RunAborted
```

默认 turn 内部数据通过 `PhaseScratch` 传递。它不是持久状态，而是当前 turn 的工作区：

- `input`
- `model_request`
- `request_messages_override`
- `model_events`
- `assistant_message`
- `tool_calls`
- `tool_outputs`
- `decision`

phase 之间共享 scratch，但只有 committed effects 会进入 `AgentState`。

### input.ingest

第一轮把用户输入 append 到 messages。后续 turn 不重复 append 初始 input。

### context.prepare

遍历所有 `ContextProvider`，允许它们返回 effects。典型用途：

- 注入检索上下文。
- 更新长期记忆引用。
- 计算当前任务 metadata。
- 调整可用工具集合。

### context.compact

`context.compact` 是 opt-in 标准 phase。配置后，phase 会：

1. 用 `TokenEstimator` 估算当前 `AgentState.messages` 的上下文大小。
2. 当估算值超过 `context_window_tokens - reserve_tokens` 时，从最新消息向前保留约 `keep_recent_tokens` 的上下文。
3. 只在 `User` 或 `Assistant` 边界裁剪，避免 retained history 以 `ToolResult` 开头。
4. 识别已有 `noloong.compaction` system summary，把它作为 `previous_summary` 做迭代更新。
5. 调用 `CompactionSummarizer` 生成新的结构化 summary。

写入模式有两种：

- `PersistentState`：提交 `CompactMessages` effect，持久裁剪 `AgentState.messages`。
- `RequestOnly`：只把 compacted messages 写入 `PhaseScratch.request_messages_override`，本轮 `ModelRequest` 使用裁剪后的消息，但最终 state 保留完整历史。

两者的核心差异是压缩后的 `[summary message] + retained messages` 写到哪里：

| 维度 | `PersistentState` | `RequestOnly` |
|------|-------------------|---------------|
| 写入目标 | `AgentState.messages` | 当前 turn 的 `PhaseScratch.request_messages_override` |
| 是否提交 effect | 是，提交 `CompactMessages` | 否 |
| replay 语义 | event log replay 后得到 compacted state | replay 后 state 仍是完整历史 |
| 后续 turn | 继续基于 summary + recent suffix | 重新从完整 state 判断是否需要压缩 |
| transcript 保留 | `AgentState.messages` 不再保留完整逐字历史 | `AgentState.messages` 保留完整历史 |
| 典型用途 | 长期运行 agent、需要状态持续瘦身、接受 event-sourced compaction | UI 或上层系统需要完整 transcript，只把 compaction 当成本次 request 的 prompt assembly 优化 |

`PersistentState` 更符合 core 的 event-sourced kernel 语义：压缩是一个明确、可审计、可重放的状态变更。它适合长会话和多 turn agent，但如果产品层需要展示完整历史，应从 event log 或外部 transcript store 获取，而不是依赖 compacted `AgentState.messages`。

`RequestOnly` 不改变状态事实，只影响本次模型请求。它适合上层已经维护完整历史、或者希望 compaction 只是 provider request 优化的场景。代价是 summary 不会持久化到 state，后续 turn 可能会重新摘要同一段旧历史。

summary message 使用 `MessageRole::System`，metadata 中包含 `noloong.compaction`，后续 compaction 会用它识别 previous summary。v1 的内置 estimator 是启发式 char/4 估算；需要 provider-specific tokenizer 时，可以替换 `TokenEstimator`。

### model.request.prepare

从当前 state 生成 `ModelRequest`：

- `messages`
- `context`
- `tools`
- `metadata`

这一步是 prompt assembly 的主要替换点。

### model.stream

调用默认 `ModelProvider`，把 provider stream 映射为核心 `ModelStreamEvent`。

provider 可以发：

- `Started`
- `ThinkingDelta`
- `TextDelta`
- `MediaDelta`
- `ToolCall`
- `Finished`
- `Failed`

如果 provider 返回 `Failed` event，后续 `assistant.commit` 会把它转成 phase error，让 run 进入 failed 状态。

### assistant.commit

把 `ModelStreamEvent` 折叠成一条 assistant message。

折叠规则：

- thinking delta 会累积成 `ThinkingBlock`。
- text delta 会累积成 `ContentBlock::Text`。
- media delta 会累积成 `ContentBlock::Media`。
- tool call 会形成 `ContentBlock::ToolCall`。
- thinking、text、media、tool call 之间会 flush，保留内容边界。
- 不同 `ThinkingKind` 之间会分开成不同 thinking block。
- 不同 media kind 或 source 会分开成不同 media block；inline base64 `data_delta` 会累积到当前 media block，直到 `done` 或其它内容类型开始。

### tool.call.resolve

从 assistant message 中提取所有 `ToolCall`，写入 scratch，并产生 `ToolCallResolved` 事件。

### tool.execute

执行工具调用。

默认执行模式是 parallel，但存在两个切换到 sequential 的条件：

- runtime 配置为 `ToolExecutionMode::Sequential`。
- 任意被调用工具的 `ToolSpec.execution_mode` 是 `Sequential`。

parallel 模式下，工具完成事件按实际完成顺序记录，但最终 append tool result message 时会恢复模型输出中的 source order。

工具执行错误的语义：

- 普通工具错误会变成 `ToolOutput { is_error: true }`，作为消息进入上下文，供模型下一轮处理。
- `AgentCoreError::Aborted` 会中止 run。
- missing tool 是 runtime 错误。

### turn.decision

默认策略很简单：

- 没有 tool call：stop。
- 达到 `max_turns`：stop。
- 否则 continue。

continue 后下一 turn 会把刚 append 的 tool result message 带进新的 model request。

## Runtime Queues 和 Agent UX Layer

`AgentRuntime` 是无状态 runner，`Agent` 是有状态 UX layer。

`Agent` 额外提供：

- persistent `AgentState`
- `prompt`
- `continue_run`
- `reset`
- `abort`
- `subscribe` / `unsubscribe`
- `wait_for_idle`
- steering queue
- follow-up queue

`Agent` 内部通过 event sink 实时 apply event 到自己的 state，同时通知 listeners。

queue 分两类：

- steering queue：每个 turn 结束后优先检查。如果有 steering message，会 append 后立即进入下一 turn。
- follow-up queue：只有当前 turn decision 是 stop 时检查。适合自然完成后追加后续输入。

queue mode：

- `OneAtATime`：每次 drain 一条。
- `All`：一次 drain 所有。

这个层的目标是提供长期交互体验，而不污染 kernel 的核心语义。

## Process Extension Bridge

stdio JSON-RPC bridge 允许外部语言扩展实现核心能力。

启动流程：

```text
spawn process
  -> initialize
  -> capabilities/list
  -> wrap capabilities into Rust trait adapters
```

当前支持的 capability：

- `ModelProvider { id }`
- `Tool { spec }`
- `ContextProvider { id }`
- `PhaseNode { id }`
- `PhaseHook { id }`
- `CompactionSummarizer { id }`

对应 JSON-RPC 方法：

- `model/stream`
- `tool/execute`
- `context/apply`
- `phase/run`
- `phase_hook/run`
- `compaction/summarize`

模型流事件通过 notification 发送：

```text
stream/event
```

如果 `stream/event` 在 `model/stream` response 前到达，bridge 会把它直接送进已注册的 `ModelStreamSink`。如果 response 返回时携带 buffered events，也会统一转换成 `ModelStreamEvent`。

`phase_hook/run` 使用 `hookId`、`hookPoint`、`runId`、`turnId`、`state` 和 hook-specific payload。返回值是统一 envelope：`modelRequest`、`modelEvents`、`assistantMessage` 都是可选字段；缺少对应字段表示 no-op，字段类型错误会让当前 phase 失败。

`compaction/summarize` 使用 `summarizerId` 加 typed summary request。request 包含 `runId`、`turnId`、`previousSummary`、`messagesToSummarize`、`turnPrefixMessages`、`tokenBudget` 和 `metadata`。response 至少要返回 `summary`，可选 `metadata` 会进入 `CompactionSummaryResult` 并最终写入 summary message / compaction effect metadata。缺失或类型错误会让 `context.compact` phase 失败。

`ContentBlock::Media` 和 `ModelStreamEvent::MediaDelta` 也走同一套 typed JSON contract。外部语言扩展不需要新的 bridge 方法；JS/TS/Python provider 只要按 serde JSON shape 发送 `media` content block 或 `media_delta` stream event，runtime 就会按 Rust-native provider 的同一语义处理。

这个设计的边界是：外部语言扩展不需要链接 Rust ABI，只需要实现 newline-delimited JSON-RPC 2.0。JS/TS 可以用 npm 生态，Python 可以用自己的 HTTP/model SDK，Rust core 只关心 typed JSON contract。

## Built-in Chat Completions Provider

内置 `ChatCompletionsProvider` 是一个普通 `ModelProvider` 实现。它没有特殊 runtime 权限，也不改变 agent loop。

配置项：

- `id`
- `base_url`
- `model`
- `api_key`
- `api_key_env`
- `headers`
- `extra_body`
- `max_completion_tokens`
- `temperature`
- `request_timeout`
- `stream_idle_timeout`
- `include_usage`
- `image_detail`
- `allow_provider_video_file_media`
- `output_modalities`
- `output_audio`

默认值：

- base URL：`https://api.openai.com/v1`
- API key env：`OPENAI_API_KEY`
- usage：默认打开 stream usage
- image detail：默认 `auto`
- output modalities：默认只请求 text

provider-specific 参数不应该写死在 core 里。兼容 OpenAI Chat Completions 的其它 provider 应该由调用方用以下字段组合：

- `base_url`
- `api_key_env`
- `headers`
- `extra_body`

例如 OpenRouter DeepSeek official route 的 live test 就在测试里通过 `extra_body` 注入 provider routing、reasoning 开关、include reasoning 等配置，而不是写进 provider 实现。

### 请求构造

`build_chat_payload` 负责把 `ModelRequest` 转成 Chat Completions body：

- `model`
- `messages`
- `stream: true`
- `stream_options.include_usage`
- `max_completion_tokens`
- `temperature`
- `tools`
- caller-owned `extra_body`

注意 `extra_body` 最后 merge，因此调用方可以为兼容 provider 添加或覆盖字段。

message 映射规则：

- `System` -> `{ role: "system", content }`
- `User` -> `{ role: "user", content }`
- `Custom(role)` -> `{ role, content }`
- `Assistant` -> `{ role: "assistant", content, tool_calls, reasoning? }`
- `ToolResult` -> one or more `{ role: "tool", tool_call_id, name, content }`

多模态输入映射规则：

- 纯文本 user/custom message 仍然使用 string content。
- 混合 text/media user/custom message 使用 content parts array。
- `MediaKind::Image + Uri` -> `image_url.url`。
- `MediaKind::Image + Inline(base64)` -> data URL 后进入 `image_url.url`。
- `MediaKind::Audio + Inline(base64)` -> `input_audio`，当前只接受 WAV/MP3 MIME type。
- `MediaKind::Video + Uri` -> `video_url.url`。
- `MediaKind::Video + Inline(base64)` -> data URL 后进入 `video_url.url`。
- `MediaKind::File + Provider` -> `file.file_id`。
- `MediaKind::File + Inline(base64)` -> `file.file_data`，可带 `filename`。
- audio URI、file URI、custom media kind、system media 默认报 provider error。
- provider video 默认报 provider error；只有 `allow_provider_video_file_media` 打开且 source 是同 provider file/ref 时，才作为 file content part 透传。

这个 provider 不做 URI 下载、不做 blob 上传，也不把 large media 内联进 event log。调用方需要先把媒体变成 URI、inline base64 或 provider-hosted id。

`ToolSpec` 映射为 Chat Completions function tool：

```json
{
  "type": "function",
  "function": {
    "name": "tool_name",
    "description": "tool description",
    "parameters": {}
  }
}
```

### Streaming

provider 发出 HTTP request 后，按 SSE frame 解析 response：

```text
data: {"choices":[{"delta":{"content":"hello"}}]}

data: [DONE]
```

SSE decoder 支持：

- multiline `data:`
- CRLF
- split CRLF across chunks
- final unfinished frame flush

每个 JSON chunk 进入 `ChatStreamState`：

- `delta.content` -> `TextDelta`
- `delta.reasoning_content` -> `ThinkingDelta`
- `delta.reasoning` -> `ThinkingDelta`
- `delta.reasoning_text` -> `ThinkingDelta`
- `delta.reasoning_details` -> `ThinkingDelta`
- `delta.tool_calls` -> partial tool call accumulator
- legacy `delta.function_call` -> partial tool call accumulator
- `delta.audio` -> `MediaDelta`
- `finish_reason` -> `StopReason`

audio output 的 transcript 保存在 media metadata 中，不强行变成 `TextDelta`。如果 provider 同时返回 visible text，则仍由 `delta.content` 进入 `TextDelta`。provider-hosted audio id 会进入 `MediaSource::Provider` 和 media replay descriptor；只有同 provider/model 的后续 assistant history 才能 replay。

stream 结束时，provider 会 emit 尚未发出的 tool calls，然后 emit `Finished`。

### Tool Call Accumulation

Chat Completions 的 tool call arguments 通常是流式字符串分片。provider 按 `index` 聚合：

```text
chunk 1: {"index":0,"id":"call-1","function":{"name":"lookup","arguments":"{\"query\":"}}
chunk 2: {"index":0,"function":{"arguments":"\"rust\"}"}}
```

结束时得到：

```json
{
  "id": "call-1",
  "name": "lookup",
  "arguments": {
    "query": "rust"
  }
}
```

如果 arguments 不是合法 JSON，会保留为 string，避免 provider bug 导致整个 stream 丢失。

### Thinking Extraction

Chat Completions 标准本身没有统一 thinking 字段，所以 provider 支持常见兼容字段：

- `reasoning_content`
- `reasoning`
- `reasoning_text`
- `reasoning_details`

提取策略：

- string：按文本 reasoning 处理。
- object：保留 raw object，并尝试从 `text` 或 `summary` 渲染可展示文本。
- array：通常来自 `reasoning_details`，按 `index` 或 `id` merge detail。
- arbitrary object：即使没有可展示文本，也保留 raw snapshot。

merge 策略：

- 对文本，如果 incoming 是 existing 的前缀扩展，则取 incoming，否则拼接。
- 对 details array，优先按 `index` 匹配，其次按 `id` 匹配。
- 对 summary array，如果 incoming 是 existing 的前缀扩展，则取 incoming，否则追加。

每个 thinking delta 都携带 replay descriptor：

```json
{
  "v": 1,
  "kind": "openai_chat_reasoning_replay",
  "providerId": "provider-id",
  "model": "model-name",
  "field": "reasoning"
}
```

历史 assistant message replay 时，只有 descriptor 的 provider id 和 model 都匹配当前 config，才会把 `ThinkingBlock.raw` 写回对应 Chat Completions reasoning 字段。这样可以避免跨 provider/model 注入不兼容 raw reasoning。

## Built-in Responses API Provider

内置 `ResponsesApiProvider` 也是普通 `ModelProvider`。它只负责 OpenAI Responses / OpenResponses wire format，不拥有 hidden conversation state，也不改变 runtime、phase graph 或 event sourcing 模型。

v1 采用 stateless full-history 模式：

- 每次从 `AgentState.messages` 构造完整 `input`。
- 默认 `store = false`。
- 不自动维护 `previous_response_id`。
- 如果未来需要 stateful Responses，应由 context/phase 扩展显式管理 response id，而不是让 provider 隐式持有会话状态。

配置项：

- `id`
- `base_url`
- `model`
- `api_key`
- `api_key_env`
- `headers`
- `extra_body`
- `max_output_tokens`
- `temperature`
- `request_timeout`
- `stream_idle_timeout`
- `store`
- `reasoning`
- `include_encrypted_reasoning`
- `native_tools`
- `function_tool_strict`
- `allow_file_data_url_input`

官方 OpenAI 默认值：

- base URL：`https://api.openai.com/v1`
- endpoint：`{base_url}/responses`
- API key env：`OPENAI_API_KEY`
- auth header：Bearer token
- `stream: true`
- `store: false`

Responses-compatible endpoint 仍由调用方配置。比如 OpenRouter Responses Beta 可以在测试或应用层使用：

- `base_url("https://openrouter.ai/api/v1")`
- `api_key_env("OPENROUTER_API_KEY")`
- `header("X-Title", "...")`
- `extra_body(...)`

core 不提供 OpenRouter、OpenAI model 或任何 vendor preset。

### Responses Request Mapping

`build_responses_payload` 负责把 `ModelRequest` 转成 Responses body：

- `model`
- `input`
- `stream: true`
- `store`
- `max_output_tokens`
- `temperature`
- `reasoning`
- `include`
- top-level `instructions`
- `tools`
- caller-owned `extra_body`

注意 `extra_body` 最后 merge，因此调用方可以为兼容 provider 添加或覆盖字段。

message 映射规则：

- `System` -> top-level `instructions`，不进入 `input` array。
- `User` -> `{ type: "message", role: "user", content: [...] }`。
- `Assistant` text/json -> completed assistant `message` item with `output_text`。
- `Assistant` tool calls -> `function_call` items。
- `Assistant` thinking -> 只有同 provider/model replay descriptor 匹配时，才渲染为 reasoning item。
- `ToolResult` -> `function_call_output` item。
- `Custom(role)` fail-fast，因为 Responses API 不接受 arbitrary role。

tool 映射规则：

- runtime `ToolSpec` -> Responses function tool。
- `function_tool_strict` 可配置，但不污染 core `ToolSpec`。
- `native_tools` 原样追加，用于 hosted tools pass-through。
- stream 中的 function call 仍回到 core `ToolCall`，由现有 tool phases 执行。

### Responses Media Input

Responses provider v1 支持：

- `MediaKind::Image + Uri` -> `input_image.image_url`。
- `MediaKind::Image + Inline(base64)` -> data URL，需要 `mime_type`。
- `MediaKind::Image + Provider` -> `input_image.file_id`，仅同 provider id。
- `MediaKind::File + Uri` -> `input_file.file_url`。
- `MediaKind::File + Provider` -> `input_file.file_id`，仅同 provider id。
- `MediaKind::File + Inline(base64)` -> `input_file.file_data`，只有 `allow_file_data_url_input(true)` 时允许。

audio、video、custom media kind、system media 和 assistant media replay 在 v1 中 fail-fast。这个 provider 不下载 URI、不上传 blob，也不管理文件生命周期。

### Responses Streaming

Responses SSE event model 不同于 Chat Completions。provider 复用 crate-private SSE framing decoder，但 parser 独立处理：

- `response.created` -> `Started`。
- `response.output_text.delta` -> `TextDelta`。
- OpenRouter-compatible `response.content_part.delta` text -> `TextDelta`。
- `response.output_item.added` with `function_call` -> 建立 partial tool call。
- `response.function_call_arguments.delta` -> 聚合 JSON fragment。
- `response.function_call_arguments.done` 或 function output item done -> emit `ToolCall`。
- `response.reasoning_summary_text.delta` -> `ThinkingDelta` with `ThinkingKind::Summary`。
- `response.reasoning_text.delta` -> `ThinkingDelta` with `ThinkingKind::Raw`。
- encrypted reasoning item -> `ThinkingDelta` with `ThinkingKind::Encrypted` and raw snapshot。
- `response.completed` / `response.done` -> emit pending tool calls and `Finished(Stop)`。
- `response.incomplete` with max-token reason -> `Finished(Length)`。
- `response.failed` / `response.error` / stream `error` -> `Failed`。

tool arguments 如果不是合法 JSON，会保留为 string，和其它 built-in provider 的 malformed tool arguments 策略一致。

### Responses Thinking Replay

Responses reasoning request config 是显式 opt-in：

- `ResponsesReasoningEffort::{Minimal, Low, Medium, High, XHigh, Custom}`
- `ResponsesReasoningSummary::{Auto, Concise, Detailed, None, Custom}`
- `include_encrypted_reasoning(true)` 会添加 `include: ["reasoning.encrypted_content"]`。

每个 Responses thinking delta 会携带 replay descriptor：

```json
{
  "v": 1,
  "kind": "openai_responses_reasoning_replay",
  "providerId": "provider-id",
  "model": "model-name"
}
```

历史 assistant message replay 时，只有 descriptor 的 provider id 和 model 都匹配当前 config，才会把 prior `ThinkingBlock.raw` 渲染为 Responses reasoning item。跨 provider 或跨 model 的 thinking 会被忽略。

## Built-in Anthropic Messages Provider

内置 `AnthropicMessagesProvider` 也是普通 `ModelProvider`。它只负责 Anthropic Messages wire format 和 SSE event model，不改变 runtime、phase graph 或 core message 类型。

配置项：

- `id`
- `base_url`
- `model`
- `api_key`
- `api_key_env`
- `auth_scheme`
- `headers`
- `extra_body`
- `max_tokens`
- `temperature`
- `request_timeout`
- `stream_idle_timeout`
- `anthropic_version`
- `beta_headers`
- `thinking`
- `allow_files_api_media`

官方 Anthropic 默认值：

- base URL：`https://api.anthropic.com`
- API key env：`ANTHROPIC_API_KEY`
- auth header：`x-api-key`
- version header：`anthropic-version: 2023-06-01`
- `max_tokens: 1024`

兼容 Anthropic Messages 的其它 endpoint 应由调用方配置，例如 OpenRouter：

- `base_url("https://openrouter.ai/api")`
- `api_key_env("OPENROUTER_API_KEY")`
- `auth_scheme(AnthropicAuthScheme::Bearer)`
- `without_anthropic_version()`

core 不提供 OpenRouter、Claude 或任何 vendor/model preset。

### Anthropic Request Mapping

`build_anthropic_payload` 负责把 `ModelRequest` 转成 Messages body：

- `model`
- `max_tokens`
- `stream: true`
- `temperature`
- `thinking`
- `tools`
- top-level `system`
- `messages`
- caller-owned `extra_body`

message 映射规则：

- `System` -> top-level `system` text blocks，不进入 `messages` array。
- `User` -> `{ role: "user", content: [...] }`。
- `Assistant` -> `{ role: "assistant", content: [...] }`。
- `ToolResult` -> `{ role: "user", content: [{ type: "tool_result", ... }] }`。
- `Custom(role)` fail-fast，因为 Anthropic Messages 不接受 arbitrary role。

tool 映射规则：

- `ToolSpec` -> Anthropic `tools` array，保留 `name`、`description`、`input_schema`。
- assistant `ContentBlock::ToolCall` -> `tool_use` block。
- tool result message -> user role `tool_result` block。
- tool result content 可包含 text/json/media；不支持的 media 会返回 provider error。

### Anthropic Media Input

Anthropic provider v1 支持：

- `MediaKind::Image + Inline(base64)` -> `image` base64 source，需要 `mime_type`。
- `MediaKind::Image + Uri` -> `image` URL source。
- `MediaKind::File + Inline(base64)` -> `document` base64 source，需要 `mime_type`，`name` 映射为 title。
- `MediaKind::File + Uri` -> `document` URL source。
- `MediaKind::File + Provider` -> opt-in Files API file source。只有 `allow_files_api_media(true)` 且 provider id 匹配当前 provider 时才允许，同时自动添加 Files API beta header。

Anthropic provider v1 不支持 audio/video/custom media kind，也不支持 system media 或 assistant media replay。它们会 fail-fast，而不是静默丢弃。

### Anthropic Streaming

Anthropic Messages 使用与 Chat Completions 不同的 SSE event model。provider 复用 crate-private SSE framing decoder，但 event parser 独立处理：

- `message_start` -> `Started`。
- `content_block_delta.text_delta` -> `TextDelta`。
- `content_block_start` with `tool_use` -> 按 content block index 建立 partial tool call。
- `content_block_delta.input_json_delta` -> 按 index 聚合 JSON fragment。
- `content_block_stop` -> emit `ToolCall`。
- `content_block_delta.thinking_delta` -> `ThinkingDelta` with `ThinkingKind::Raw`。
- `content_block_delta.signature_delta` -> 更新 thinking metadata/raw snapshot/replay descriptor。
- `message_delta.stop_reason` -> `StopReason`。
- `message_stop` -> `Finished`。
- stream `error` -> `Failed` event，后续 `assistant.commit` 会让 run failed。

tool `input_json_delta` 如果不是合法 JSON，会保留为 string，和 Chat Completions provider 的 malformed tool arguments 策略一致。

### Anthropic Thinking Replay

`enable_thinking(budget_tokens)` 才会在 request body 中发送 Anthropic extended thinking config。provider 默认不请求 thinking。

每个 Anthropic thinking delta 会携带 replay descriptor：

```json
{
  "v": 1,
  "kind": "anthropic_messages_thinking_replay",
  "providerId": "provider-id",
  "model": "model-name",
  "signature": "signature-if-seen"
}
```

历史 assistant message replay 时，只有 descriptor 的 provider id 和 model 都匹配当前 config，才会把 prior `ThinkingBlock` 渲染为 Anthropic `thinking` block，并带回 signature。跨 provider 或跨 model 的 thinking 会被忽略，避免把不兼容 raw reasoning 注入另一个 endpoint。

## Cancellation 和 Timeout

核心使用 `CancellationToken`，内部是 atomic flag 加 `Notify`。

主要可取消点：

- run/turn 开始前。
- context provider 调用前。
- model request 发送中。
- model stream chunk 等待中。
- tool execution 前。
- stdio JSON-RPC request 等待中。

built-in HTTP provider 有两层 timeout：

- `request_timeout`：等待初始 HTTP response。
- `stream_idle_timeout`：stream chunk 空闲超时。

stdio extension 也有：

- `request_timeout`
- `stream_timeout`

abort 的目标是尽快返回 `AgentCoreError::Aborted`，并记录 `RunAborted`。

## Error Semantics

错误大致分三类：

1. Kernel/runtime 错误：phase failed、missing provider、invalid effect、missing tool。
2. Provider/extension 错误：HTTP failure、JSON-RPC timeout、invalid extension response。
3. Tool execution business error：普通工具失败会转成 `ToolOutput { is_error: true }`。

设计上，普通工具失败不直接 fail run，因为模型通常应该能看到工具错误并继续修正。相反，provider failure、phase failure、invalid effect 等会 fail run。

event sink failure 是特殊情况：事件已经写入 store 后通知 sink。如果 sink 失败，runtime 会记录 `RunFailed`，但不会再次通知失败的 sink，避免递归失败。

## 验证矩阵

默认本地质量门：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p noloong-agent-core --examples
node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs
node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs
node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs
```

真实 provider 手动门：

```bash
cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture
cargo test -p noloong-agent-core --test anthropic_live openrouter_anthropic_messages -- --ignored --nocapture
cargo test -p noloong-agent-core --test responses_live -- --ignored --nocapture
```

真实 provider 测试当前覆盖：

- `OPENROUTER_API_KEY`
- `deepseek/deepseek-v4-flash`
- OpenRouter provider routing 限定 DeepSeek official provider
- `openrouter/free`
- OpenRouter free model router multimodal routing
- `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`
- OpenRouter provider routing 限定 NVIDIA provider for audio/video because `openrouter/free` currently has no input audio/video endpoints
- thinking enabled
- visible text
- tool call
- tool execution
- text+image+audio+video input payload acceptance
- Anthropic-compatible OpenRouter Messages endpoint
- Anthropic Messages provider text compatibility through `openrouter/free`
- OpenRouter Responses-compatible endpoint
- Responses provider text compatibility through `openrouter/free`
- optional Responses tool/reasoning gates through explicitly declared model env vars

这些测试保持 ignored，因为它们依赖外部网络、账户、模型可用性和 provider 当前行为。当前 Anthropic Messages 和 Responses 外部门只要求 OpenRouter；官方 Anthropic 测试作为显式 opt-in diagnostic 保留，只有设置 `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1` 且提供有效 `ANTHROPIC_API_KEY` 时才运行。

## 当前架构边界

当前实现刻意保持一些边界：

- core provider 不硬编码 OpenRouter、DeepSeek 或其它 vendor preset。
- `ChatCompletionsProvider` 是内置 provider，但仍只是 `ModelProvider`。
- `ResponsesApiProvider` 是内置 provider，但仍只是 `ModelProvider`。
- `AnthropicMessagesProvider` 是内置 provider，但仍只是 `ModelProvider`。
- phase graph 是主要 loop 扩展点，不把所有扩展都压进 callback。
- event log 是状态事实来源，`AgentState` 是 event replay 的结果。
- thinking replay 只在同 provider/model scope 内发生。
- media replay 只在同 provider/model scope 内发生，media URI/blob 生命周期不由 core 管理。
- 外部语言扩展通过 JSON-RPC typed payload，而不是 Rust ABI。

## 后续演进方向

比较自然的后续扩展方向：

1. 持久化 event store：SQLite/PostgreSQL/object store。
2. 多 model provider routing：按 phase、tool、context 或 budget 选择 provider。
3. Stateful Responses support：通过 context/phase 显式管理 `previous_response_id`。
4. 更严格的 JSON-RPC extension conformance suite。
5. `MediaStore`：大 blob 的持久化、去重、加密、权限和生命周期管理。
6. thinking redaction/encryption policy：将 raw thinking 的保存、暴露、replay 做成可配置策略。
7. tool permission model：把 before hook 扩展为可审计的 capability/approval 机制。

这些方向应该继续遵守当前核心原则：状态变更通过 effect，外部行为通过 trait 或 JSON-RPC，provider-specific 细节留在调用方配置或 provider 实现内，不泄漏到 runtime。
