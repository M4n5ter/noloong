# Implementation Plan: Extension Wire Contract Documentation

## Overview

当前扩展文档已经说明了 Noloong stdio JSON-RPC extension 的能力模型，但对插件开发者仍不够可执行：每个 method、phase hook、tool hook 的输入结构、返回结构、可省略字段和 no-op 语义没有形成完整 contract。这个计划将 `EXTENSIONS.md` 升级为插件作者可以直接照着实现 handler 的 wire contract，并把 TS/Python 示例 README 与 conformance runner 说明对齐。

## Architecture Decisions

- `EXTENSIONS.md` 作为插件作者的权威 wire contract；`ARCHITECTURE.md` 只保留架构意图并链接到 contract 文档。
- 文档采用真实 JSON-RPC payload 形状，而不是 Rust trait 术语；字段命名必须匹配当前 serde wire shape。
- 每个 method 使用同一模板：调用时机、params、result、no-op 语义、失败语义、最小 handler 示例。
- 暂不引入 machine-readable JSON Schema 文件；先用 Markdown contract、具体 JSON snippet、TS/Python 示例和 conformance runner 保持开发体验闭环。
- 示例 helper 仍保持 example-local SDK skeleton，不承诺 npm/PyPI 稳定 SDK。

## Dependency Graph

1. 从 Rust serde types 和 JSON-RPC bridge 固化真实 wire shapes。
2. 重写 `EXTENSIONS.md` 的公共结构与 method contracts。
3. 补齐 phase hook、tool hook、compaction 的逐 hook-point 输入/输出说明。
4. 更新 TS/Python 示例 README 与架构文档链接。
5. 增加轻量文档覆盖测试，防止后续新增 hook/method 后文档再次缺失。
6. 运行 conformance、语言示例、Rust workspace 检查。

## Task List

### Phase 1: Contract Inventory

## Task 1: Inventory Actual Wire Shapes

**Description:** 对照 `jsonrpc.rs`、`types.rs`、`providers.rs`、`phase.rs` 和 `compaction.rs`，整理当前所有 extension-facing JSON shape。重点确认 method params 是否使用 wrapper、flatten payload、enum tag、`camelCase` 字段和 `snake_case` variant。

**Acceptance criteria:**

- [x] 列出所有 JSON-RPC method：`initialize`、`capabilities/list`、`model/stream`、`stream/event`、`tool/execute`、`context/apply`、`phase/run`、`phase_hook/run`、`tool_hook/run`、`compaction/summarize`、`shutdown`。
- [x] 列出所有 hook point：`before_model_request`、`after_model_request`、`before_assistant_commit`、`after_assistant_commit`、`before_tool_call`、`after_tool_call`。
- [x] 确认 common shapes 覆盖 `AgentState`、`AgentMessage`、`ContentBlock`、`ModelStreamEvent`、`ToolSpec`、`ToolCall`、`ToolOutput`、`AgentEffect`、`PhaseScratch`、`PhaseOutput`、`ToolPermissionDecision`、`CompactionSummaryRequest`。

**Verification:**

- [x] `rg -n "request\\(|phase_hook/run|tool_hook/run|compaction/summarize" crates/noloong-agent-core/src/jsonrpc.rs`
- [x] `rg -n "struct ModelRequest|enum ContentBlock|enum ModelStreamEvent|struct PhaseOutput|struct CompactionSummaryRequest" crates/noloong-agent-core/src`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/docs/EXTENSIONS.md`

**Estimated scope:** S

### Checkpoint: Inventory

- [x] 所有 public extension methods 和 hook points 都有真实 wire shape 来源。
- [x] 后续文档不需要靠推测补字段。

### Phase 2: Method Contracts

## Task 2: Rewrite Core Extension Method Contracts

**Description:** 重写 `EXTENSIONS.md` 的 lifecycle、capability、model、tool、context、phase 和 shutdown 章节，让插件作者能直接知道每个 handler 接收什么、必须返回什么、返回空对象代表什么。

**Acceptance criteria:**

- [x] `initialize` 明确 `protocolVersion` 输入和 `manifest` 输出。
- [x] `capabilities/list` 明确所有 capability variant 的 JSON shape。
- [x] `model/stream` 明确 `providerId`、`streamId`、`request`、inline `events` 与 `stream/event` notification 的关系。
- [x] `tool/execute` 明确 `toolName` wrapper 与内部 `request` 的完整字段。
- [x] `context/apply` 与 `phase/run` 明确 effects、scratch、stream events、tool outputs 的返回语义。
- [x] `shutdown` 明确应返回 `{}`，并说明可在响应后退出进程。

**Verification:**

- [x] `rg -n "## .*model/stream|providerId|streamId|tool/execute|phase/run|shutdown" crates/noloong-agent-core/docs/EXTENSIONS.md`
- [x] 手动检查每个 method section 都包含 params 和 result JSON snippet。

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/docs/EXTENSIONS.md`

**Estimated scope:** M

## Task 3: Document Hook Point Contracts

**Description:** 为 `phase_hook/run` 和 `tool_hook/run` 增加逐 hook-point contract。每个 hook point 必须写清楚公共 envelope 字段、专属 payload 字段、允许返回字段、字段省略时的 no-op 语义，以及 malformed response 的影响。

**Acceptance criteria:**

- [x] `before_model_request` 说明输入 `modelRequest`，返回 `modelRequest` 可重写 request。
- [x] `after_model_request` 说明输入 `modelRequest` 和 `modelEvents`，返回 `modelEvents` 可重写 stream result。
- [x] `before_assistant_commit` 说明输入 `modelEvents`，返回 `modelEvents` 可重写 commit source。
- [x] `after_assistant_commit` 说明输入 `assistantMessage`，返回 `assistantMessage` 可重写已构造消息。
- [x] `before_tool_call` 说明输入 `toolCall`、`toolSpec`、`permissions`，返回 `decision` 会进入 audit。
- [x] `after_tool_call` 说明输入 `toolCall`、`output`，返回 `content`、`details`、`isError` 可重写 tool output。

**Verification:**

- [x] `rg -n "before_model_request|after_model_request|before_assistant_commit|after_assistant_commit|before_tool_call|after_tool_call" crates/noloong-agent-core/docs/EXTENSIONS.md`
- [x] 手动检查每个 hook point 至少有一个 params snippet 和一个 result snippet。

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/docs/EXTENSIONS.md`

**Estimated scope:** M

### Checkpoint: Method and Hook Contracts

- [x] 插件作者不读 Rust 源码也能实现所有 method handler。
- [x] 每个 hook point 的输入和输出都能从文档直接复制为 JSON 对照。

### Phase 3: Shared Shapes and Examples

## Task 4: Add Common JSON Shape Reference

**Description:** 在 `EXTENSIONS.md` 中增加 common shapes 参考，覆盖所有 method 和 hook 会复用的数据结构。文档应突出 required/optional 字段、tag enum 写法、thinking/media/tool-call 的特殊结构。

**Acceptance criteria:**

- [x] `AgentMessage`、`ContentBlock`、`ThinkingBlock`、`MediaBlock`、`ToolCall`、`ToolSpec`、`ToolOutput` 均有最小 JSON 示例。
- [x] `ModelStreamEvent` 覆盖 `started`、`thinking_delta`、`text_delta`、`media_delta`、`tool_call`、`finished`、`failed`。
- [x] `AgentEffect` 覆盖 `append_message`、`patch_context`、`set_available_tools`、`compact_messages`。
- [x] `PhaseScratch` 和 `PhaseOutput` 说明 tuple-like tool output 在 JSON 中的表现。
- [x] 文档明确 optional 字段省略、空数组和空对象的推荐写法。

**Verification:**

- [x] `rg -n "AgentState|AgentMessage|ContentBlock|ModelStreamEvent|PhaseScratch|PhaseOutput|AgentEffect" crates/noloong-agent-core/docs/EXTENSIONS.md`
- [x] 手动检查 JSON snippet 的 field casing 与 Rust serde attributes 一致。

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent-core/docs/EXTENSIONS.md`

**Estimated scope:** M

## Task 5: Align TS/Python Example Documentation

**Description:** 更新 TypeScript 和 Python conformance 示例 README，让它们从“如何运行示例”升级为“如何按 contract 写 handler”。README 需要列出 handler method mapping，并指向 `EXTENSIONS.md` 的 contract 章节。

**Acceptance criteria:**

- [x] TS README 列出每个 handler key 对应的 JSON-RPC method。
- [x] Python README 列出每个 function 对应的 JSON-RPC method。
- [x] 两个 README 都说明 stdout/stderr 分离、`stream/event` notification 和 strict conformance 命令。
- [x] README 不把 example-local helper 描述成稳定 SDK。

**Verification:**

- [x] `rg -n "phase_hook/run|tool_hook/run|compaction/summarize|EXTENSIONS.md" examples/extensions/typescript-conformance examples/extensions/python-conformance`
- [x] `npm run check` in `examples/extensions/typescript-conformance`
- [x] `python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py`

**Dependencies:** Task 4

**Files likely touched:**

- `examples/extensions/typescript-conformance/README.md`
- `examples/extensions/python-conformance/README.md`

**Estimated scope:** S

### Checkpoint: Author Usability

- [x] `EXTENSIONS.md` 是完整 contract。
- [x] TS/Python 示例 README 能把 contract 映射到具体 handler。

### Phase 4: Drift Guard and Final Verification

## Task 6: Add Documentation Coverage Guard

**Description:** 增加轻量测试，确保 `EXTENSIONS.md` 至少覆盖当前所有 extension method、standard hook point、strict conformance capability id 和核心 shared shape 名称。这个测试不替代人工审阅，但能防止未来新增协议点后文档完全遗漏。

**Acceptance criteria:**

- [x] 测试断言所有 JSON-RPC method 名称都出现在 `EXTENSIONS.md`。
- [x] 测试断言所有 hook point 名称都出现在 `EXTENSIONS.md`。
- [x] 测试断言 strict conformance ids 都出现在 `EXTENSIONS.md` 或示例 README 中。
- [x] 测试失败信息能指出缺失的具体文档 token。

**Verification:**

- [x] `cargo test -p noloong-agent-core extension_docs_cover_current_contract`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent-core/tests/extension_language_examples.rs`
- or `crates/noloong-agent-core/tests/extension_docs_contract.rs`

**Estimated scope:** S

## Task 7: Run Full Extension Verification Gate

**Description:** 运行文档和示例相关的最终验证，确保 contract 文档没有破坏现有 conformance 示例，也没有引入格式或 lint 问题。

**Acceptance criteria:**

- [x] Python example 通过 strict conformance。
- [x] TypeScript example 通过 typecheck 和 strict conformance。
- [x] Rust integration tests 覆盖语言示例和文档 coverage guard。
- [x] Workspace format、clippy、nextest 均通过。

**Verification:**

- [x] `python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py`
- [x] `npm run check` in `examples/extensions/typescript-conformance`
- [x] `npm run conformance` in `examples/extensions/typescript-conformance`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- python3 examples/extensions/python-conformance/full_conformance_extension.py`
- [x] `cargo test -p noloong-agent-core --test extension_language_examples`
- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo nextest run --workspace`
- [x] `git diff --check`

**Dependencies:** Task 6

**Files likely touched:**

- None

**Estimated scope:** S

### Checkpoint: Complete

- [x] `EXTENSIONS.md` contains implementation-grade wire contracts for every extension method and hook point.
- [x] TS/Python examples point plugin authors to the exact contract and still pass strict conformance.
- [x] Rust tests include a guard against obvious documentation drift.
- [x] Full workspace verification passes.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| 文档 JSON shape 与 serde wire shape 漂移 | High | 从 Rust source inventory 开始，并加入 docs coverage guard |
| 文档过长但仍难用 | Medium | 每个 method 使用统一模板，并在 hook point 处给最小 params/result snippet |
| README 与 `EXTENSIONS.md` 重复导致维护成本上升 | Medium | README 只保留 handler mapping 和运行方式，完整字段 contract 只放 `EXTENSIONS.md` |
| 文档测试过弱 | Low | 本阶段只防遗漏；完整行为仍由 strict conformance 示例验证 |

## Open Questions

- None.
