# Implementation Plan: Context Compaction Phase

## Overview

为 `noloong-agent-core` 增加内置 context compaction 能力：在长上下文场景下自动估算上下文大小、选择安全裁剪点、生成结构化摘要，并支持两种写入模式：

- `PersistentState`：通过 event-sourced effect 裁剪 `AgentState.messages`，summary 作为 `System` message 保留。
- `RequestOnly`：只裁剪当前 `ModelRequest.messages`，不修改持久 `AgentState`。

设计参考 `pi-mono` 的 compaction 思路，但不复制其 session tree、branch summary、file operation tracking 等 coding-agent 特化结构。Noloong v1 以通用 `AgentState.messages` 为输入，摘要能力通过专用 trait 和 JSON-RPC 扩展开放给 Rust、JS/TS、Python 等实现。

## Architecture Decisions

- compaction 是 opt-in 能力；未启用时默认 runtime 不插入 `context.compact`，避免每 turn 产生 no-op phase events。
- 新增标准 phase `context.compact`，启用 compaction 时自动插入在 `context.prepare` 和 `model.request.prepare` 之间。
- 新增 `CompactionSummarizer` trait，而不是复用 `ModelProvider` 作为公共扩展边界；内置 model-backed summarizer 只是一个普通实现。
- 新增 `TokenEstimator` trait，内置 `HeuristicTokenEstimator`，v1 不引入 tokenizer 依赖。
- 新增 `AgentEffect::CompactMessages`，让 persistent compaction 仍满足 event-sourced replay。
- `RequestOnly` 模式通过 `PhaseScratch` 中的 message override 影响本轮 `ModelRequest`，不改变 `AgentState.messages`。
- summary message 使用 `MessageRole::System`，metadata 写入 `noloong.compaction`，便于后续识别 previous summary 并做迭代更新。

## Dependency Graph

1. Public compaction types and config validation
2. Event-sourced message compaction effect and reducer support
3. Token estimation and cut point planner
4. `context.compact` phase and runtime/builder registration
5. Model-backed summarizer
6. JSON-RPC summarizer bridge
7. Documentation and quality gates

## Task List

### Phase 1: Foundation

## Task 1: Define Compaction Public API

**Description:** 新增 compaction 相关公共类型，建立配置、摘要请求、摘要结果、token estimator 和 summarizer 的稳定边界。

**Acceptance criteria:**

- [x] `ContextCompactionConfig` 包含 `context_window_tokens`、`reserve_tokens`、`keep_recent_tokens`、`mode`、`metadata`。
- [x] `ContextCompactionMode` 支持 `PersistentState` 和 `RequestOnly`。
- [x] `CompactionSummaryRequest` 包含 `run_id`、`turn_id`、`previous_summary`、`messages_to_summarize`、`turn_prefix_messages`、`token_budget`、`metadata`。
- [x] `CompactionSummaryResult` 至少包含 `summary`，并支持可选 `metadata`。
- [x] `CompactionSummarizer` 是 `Send + Sync`，方法返回 crate 现有 `BoxFuture<Result<...>>` 风格。
- [x] `TokenEstimator` 是可替换 trait，内置 `HeuristicTokenEstimator`。
- [x] config validation 拒绝无法成立的窗口配置，例如 `context_window_tokens <= reserve_tokens`。

**Verification:**

- [x] `cargo test -p noloong-agent-core compaction_config`
- [x] serde round-trip tests cover config, request, and result types.

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/providers.rs`
- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**Estimated scope:** M

## Task 2: Add Event-Sourced Message Compaction Effect

**Description:** 新增 `AgentEffect::CompactMessages` 及 reducer 支持，让 persistent compaction 可以裁剪持久 messages，同时保持事件日志可 replay。

**Acceptance criteria:**

- [x] `AgentEffect::CompactMessages { compaction: MessageCompaction }` 支持 serde round-trip。
- [x] `MessageCompaction` 包含 summary message、retained message ids、dropped message ids、tokens before/after estimate、metadata。
- [x] reducer 将当前 messages 替换为 `[summary_message] + retained_messages`。
- [x] validation 拒绝 empty summary id、unknown retained/dropped id、retained/dropped overlap。
- [x] replay compacted event log 能重建 compacted state。

**Verification:**

- [x] `cargo test -p noloong-agent-core compact_messages_effect`
- [x] `cargo test -p noloong-agent-core event_log_replays_to_report_state`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/src/reducer.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**Estimated scope:** M

### Checkpoint: Foundation

- [x] Public API compiles.
- [x] Reducer tests pass.
- [x] Existing event replay tests still pass.

### Phase 2: Planner and Phase Integration

## Task 3: Implement Token Estimation and Cut Point Planner

**Description:** 实现 provider-neutral compaction planner，用启发式 token 估算决定是否 compact、裁剪哪些消息、保留哪些 recent messages。

**Acceptance criteria:**

- [x] token estimator 覆盖 `Text`、`Json`、`Thinking`、`Media`、`ToolCall`、`ToolResult`。
- [x] tool result 序列化摘要输入时默认截断，避免摘要请求被工具输出撑爆。
- [x] planner 在 `estimated_tokens <= context_window_tokens - reserve_tokens` 时返回 skip。
- [x] planner 从最新消息向前累计 `keep_recent_tokens`，选择第一个安全 cut point。
- [x] cut point 只能落在 `User` 或 `Assistant` 边界，永不以 `ToolResult` 开始 retained history。
- [x] planner 识别已有 `noloong.compaction` system summary，并将其作为 `previous_summary`，不重复摘要旧 summary。
- [x] 如果 cut point 落在一个 user turn 中间，planner 生成 `turn_prefix_messages`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test compaction planner`
- [x] Tests cover no-op threshold, user-boundary cut, assistant-boundary cut, tool-result safety, previous summary update, split-turn prefix.

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/compaction.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**Estimated scope:** M

## Task 4: Add `context.compact` Standard Phase

**Description:** 将 compaction planner 和 summarizer 接入标准 phase graph，并支持 persistent/request-only 两种模式。

**Acceptance criteria:**

- [x] 新增 `PHASE_CONTEXT_COMPACT` 和 `StandardPhase::ContextCompact`。
- [x] 启用 compaction 时 phase order 为 `input.ingest -> context.prepare -> context.compact -> model.request.prepare -> ...`。
- [x] 未配置 compaction 时不插入默认 `context.compact`，避免 recurring no-op phase events。
- [x] `PersistentState` 模式提交 `CompactMessages` effect，后续 `model.request.prepare` 使用 compacted state。
- [x] `RequestOnly` 模式写入 scratch message override，后续 `model.request.prepare` 使用 override。
- [x] summarizer error 或 cancellation 会让 `context.compact` fail，且不会调用 model provider。
- [x] runtime 和 high-level agent builder 暴露 `with_context_compaction(config, summarizer)`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test compaction phase`
- [x] Tests cover persistent mode, request-only mode, no-op mode, error path, cancellation path.

**Dependencies:** Tasks 1-3

**Files likely touched:**

- `crates/noloong-agent-core/src/phase.rs`
- `crates/noloong-agent-core/src/runtime.rs`
- `crates/noloong-agent-core/src/agent.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**Estimated scope:** M

### Checkpoint: Core Integration

- [x] 启用 compaction 后 `context.compact` appears in runtime phase event order.
- [x] Persistent and request-only runtime tests pass.
- [x] Existing phase hook and provider tests still pass.

### Phase 3: Summarizer Implementations

## Task 5: Implement Model-Backed Compaction Summarizer

**Description:** 提供一个内置 summarizer，实现通用 LLM 摘要能力，但保持它只是 `CompactionSummarizer` 的一个实现，不给 core provider 加特权路径。

**Acceptance criteria:**

- [x] `ModelBackedCompactionSummarizer` 接收 `Arc<dyn ModelProvider>` 和 summarizer-specific config。
- [x] summarizer 将旧 messages 序列化为单条 user prompt，不让模型继续原对话。
- [x] summary prompt 使用固定结构：goal、constraints/preferences、progress、key decisions、next steps、critical context。
- [x] 有 `previous_summary` 时使用 update prompt，要求 preserve and update。
- [x] split-turn 场景生成 history summary 和 turn prefix summary，并合并为一条 summary。
- [x] 不传 tools；只使用 summary provider 返回的 text deltas 生成 summary。
- [x] `Failed` stream event 或空 summary 返回 error。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test compaction model_backed`
- [x] Mock provider tests cover initial summary, previous summary update, split-turn summary, failed event, empty summary.

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent-core/src/compaction.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**Estimated scope:** M

## Task 6: Add JSON-RPC Compaction Summarizer

**Description:** 让外部语言扩展通过 JSON-RPC 提供 compaction summary，实现 JS/TS、Python、其它 LLM SDK 的 summarizer 接入。

**Acceptance criteria:**

- [x] `ExtensionCapability::CompactionSummarizer { id }` 可在 capabilities 中声明。
- [x] runtime discovery 将该 capability 注册为 `CompactionSummarizer`。
- [x] 新增 JSON-RPC method `compaction/summarize`。
- [x] request 包含 `summarizerId`、`runId`、`turnId`、`previousSummary`、`messagesToSummarize`、`turnPrefixMessages`、`tokenBudget`、`metadata`。
- [x] response 至少包含 `summary`；可选 `metadata` 会写入 `CompactionSummaryResult`。
- [x] missing summary、wrong field type、extension error 都会让当前 phase fail。
- [x] stdio fixture 增加 compaction summarizer mode。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test jsonrpc compaction`
- [x] `node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

**Dependencies:** Tasks 1 and 4

**Files likely touched:**

- `crates/noloong-agent-core/src/jsonrpc.rs`
- `crates/noloong-agent-core/src/types.rs`
- `crates/noloong-agent-core/tests/jsonrpc.rs`
- `crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`

**Estimated scope:** M

### Checkpoint: Extension Support

- [x] Native and JSON-RPC summarizers share the same request/result semantics.
- [x] Malformed extension output fails deterministically.
- [x] Existing JSON-RPC provider/tool/context/phase/hook tests still pass.

### Phase 4: Documentation and Quality Gate

## Task 7: Update Architecture Documentation

**Description:** 更新架构文档，说明 compaction phase、summarizer trait、effect replay、两种 mode 和 JSON-RPC 扩展契约。

**Acceptance criteria:**

- [x] `ARCHITECTURE.md` 中 `context compaction phase` 从 future work 移到已支持能力。
- [x] 文档说明 `PersistentState` 和 `RequestOnly` 的差异。
- [x] 文档说明 `CompactMessages` effect 的 replay 语义。
- [x] 文档说明 summary message metadata 和 previous summary 识别方式。
- [x] 文档说明 `compaction/summarize` JSON-RPC wire contract。

**Verification:**

- [x] Manual doc review confirms examples match exported types.

**Dependencies:** Tasks 1-6

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** S

## Task 8: Final Quality Gate

**Description:** 对完整实现运行格式、lint、单测和全量测试，确保 compaction 不破坏已有 provider/runtime/extension 行为。

**Acceptance criteria:**

- [x] `cargo fmt --check` passes.
- [x] `cargo clippy --workspace --all-targets --all-features` passes with no warnings.
- [x] `cargo nextest run --workspace` passes.
- [x] Targeted compaction tests pass.
- [x] Existing provider live ignored tests remain ignored unless explicitly invoked.

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo nextest run --workspace`
- [x] `cargo test -p noloong-agent-core --test compaction`
- [x] `cargo test -p noloong-agent-core --test jsonrpc compaction`

**Dependencies:** Tasks 1-7

**Files likely touched:**

- No implementation files beyond fixes discovered by verification.

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Persistent compaction breaks event replay | High | Model it as explicit `CompactMessages` effect and add reducer replay tests. |
| Tool call/result pairing is broken by cut point | High | Planner never starts retained history at `ToolResult`; add cut point safety tests. |
| Summary loses critical prior context | High | Use structured summary prompt, previous summary update path, and retain recent token suffix. |
| Request-only mode diverges from state unexpectedly | Medium | Name mode explicitly and test final state remains unchanged while provider request is compacted. |
| Token estimate is inaccurate | Medium | Keep estimator replaceable; default heuristic is conservative and validated by threshold tests. |
| JSON-RPC contract becomes ambiguous | Medium | Use typed request/result envelope; missing/wrong summary is a phase error. |
| Summary provider recursively triggers compaction | Medium | Model-backed summarizer constructs a direct `ModelRequest` for summary only and does not enter runtime phase graph. |

## Parallelization Opportunities

- Tasks 1-4 should be sequential because they establish shared API, reducer semantics, planner output, and phase integration.
- Task 5 and Task 6 can proceed in parallel after Task 4 if the `CompactionSummarizer` contract is stable.
- Task 7 can start after public type names settle, then be finalized after Tasks 5-6.
- Task 8 must run after all implementation tasks.

## Open Questions

- None for v1. Defaults chosen:
  - Built-in compaction is opt-in.
  - Default mode is `PersistentState`.
  - `RequestOnly` is supported for callers that need full persisted transcript.
  - Summary messages use `MessageRole::System` plus `noloong.compaction` metadata.
  - v1 does not implement pi-mono session tree branch summarization or file-operation extraction.
