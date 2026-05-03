# Implementation Plan: Reusable Extension Conformance Runner

## Overview

把当前 crate 内部的 JSON-RPC extension conformance suite 提炼成第三方扩展作者也能运行的测试工具。交付形态同时包含 Rust public runner API 和 `noloong-extension-conformance` CLI。默认采用 hybrid profile：任意扩展都能运行通用 lifecycle / capability 校验；如果扩展实现标准 `conformance-*` capability，则额外运行完整 adapter payload、stream、tool hook 和 compaction 行为测试。

内部 fixture-only 的负向测试仍留在 crate tests 中，用于验证 core bridge 对 malformed result、duplicate capability、wrong response id、stdout close、timeout 等异常行为的健壮性；这些不要求第三方扩展实现。

## Architecture Decisions

- Runner API 和 CLI 共用同一套 case engine，避免 CLI 与内部测试逻辑漂移。
- `ExtensionConformanceProfile::Hybrid` 作为默认值：第三方接入成本低，同时能在实现标准 conformance capability 后获得强校验。
- `Strict` profile 要求完整标准 conformance capability，适合 extension SDK、template 和 CI gate。
- `Generic` profile 只校验 JSON-RPC lifecycle、typed capability decode、runtime registration 和 shutdown，适合任意现有扩展的基础健康检查。
- 不新增 `clap` 等 CLI 依赖；v1 用轻量手写 argv parser，保持 core crate 依赖面克制。
- Public runner 只暴露正向/协议契约测试；内部 bridge robustness tests 继续使用 dedicated fixture modes。

## Public API Contract

新增 public API 并从 `lib.rs` 导出：

- `ExtensionConformanceConfig`
- `ExtensionConformanceProfile::{Generic, Hybrid, Strict}`
- `ExtensionConformanceReport`
- `ExtensionConformanceCaseReport`
- `ExtensionConformanceCaseStatus::{Passed, Failed, Skipped}`
- `run_extension_conformance(config) -> Result<ExtensionConformanceReport>`

`ExtensionConformanceConfig` 持有 `StdioExtensionConfig`，并提供 builder：

- `new(stdio: StdioExtensionConfig)`
- `profile(ExtensionConformanceProfile)`
- `fail_fast(bool)`

新增 CLI binary：`noloong-extension-conformance`

- Invocation: `noloong-extension-conformance [--profile generic|hybrid|strict] [--json] [--fail-fast] -- <command> [args...]`
- Exit code `0` only when all non-skipped cases pass.
- Text output 包含 total / passed / failed / skipped 和失败 case message。
- `--json` 输出 `ExtensionConformanceReport`。

标准 full conformance capability ids：

- `conformance-model`
- `conformance_echo`
- `conformance-context`
- `conformance.phase`
- `conformance-hook`
- `conformance-tool-hook`
- `conformance-compaction`

## Dependency Graph

1. Runner report/config/profile public types
2. Shared positive conformance case engine
3. Existing internal suite refactor
4. CLI binary
5. Documentation and plan updates
6. Full verification gate

## Task List

### Phase 1: Runner Foundation

## Task 1: Add Public Conformance Types

**Description:** 新增 extension conformance runner 的 public report/config/profile/case status 类型，并从 crate root 导出。类型需要 serde-friendly，方便 CLI JSON 输出、第三方测试集成和内部 tests 断言。

**Acceptance criteria:**

- [x] `ExtensionConformanceProfile` 支持 `Generic`、`Hybrid`、`Strict`，默认行为为 `Hybrid`。
- [x] `ExtensionConformanceReport` 能表达 total、passed、failed、skipped、case reports 和 overall success。
- [x] `ExtensionConformanceCaseReport` 包含 case name、status、message、elapsed duration。
- [x] `ExtensionConformanceConfig` 持有 `StdioExtensionConfig`，支持 profile 和 fail-fast 配置。
- [x] Public API 从 `lib.rs` 导出，不暴露 crate-private test helper。

**Verification:**

- [x] `cargo test -p noloong-agent-core extension_conformance`
- [x] `cargo fmt --check`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/extension_conformance.rs`
- `crates/noloong-agent-core/src/lib.rs`

**Estimated scope:** M

## Task 2: Implement Shared Positive Case Engine

**Description:** 把当前 `tests/jsonrpc_conformance.rs` 中可复用的正向 conformance flow 抽成 runner case engine。Runner 负责连接扩展、读取 manifest/capabilities、按 profile 选择 case、构造 runtime 并验证标准 conformance capability 的行为。

**Acceptance criteria:**

- [x] `Generic` profile 校验 `initialize`、`capabilities/list`、typed capability decode、runtime registration、shutdown。
- [x] `Strict` profile 要求全部标准 `conformance-*` capability 存在，并运行完整行为 case。
- [x] `Hybrid` profile 在没有标准 conformance capability 时跳过 full cases。
- [x] `Hybrid` profile 在只实现部分标准 conformance capability 时 fail，并报告缺失 ids。
- [x] Case engine 支持 fail-fast；关闭 fail-fast 时收集所有 case 结果。
- [x] Runner 不要求第三方实现 malformed/duplicate/wrong-id 等 fixture-only modes。

**Verification:**

- [x] Existing fixture 在 `Strict` profile 下全部通过。
- [x] Model-only fixture mode 在 `Hybrid` profile 下 generic pass、full cases skipped。
- [x] Partial conformance fixture mode 在 `Hybrid` profile 下失败并报告 missing capability。

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent-core/src/extension_conformance.rs`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

### Checkpoint: Runner Core

- [x] Public runner API compiles and is exported.
- [x] Positive conformance cases can be executed without depending on test-only functions.
- [x] Existing fixture passes strict runner smoke test.

### Phase 2: Internal Suite Refactor

## Task 3: Preserve Internal Negative Bridge Tests

**Description:** 保留现有 bridge robustness coverage，但从内部 suite 中移除已经迁移到 runner 的 positive duplication。Fixture-only negative modes 继续验证 malformed result、duplicate capability、JSON-RPC error、wrong response id、stdout close、cancellation 和 stream timeout 等 core behavior。

**Acceptance criteria:**

- [x] `tests/jsonrpc_conformance.rs` 复用 runner 或 shared constants 验证 positive path。
- [x] `malformed-*` result tests 仍覆盖 active phase failure。
- [x] `duplicate-*` capability tests 仍覆盖 registration failure。
- [x] `wrong-response-id`、`stdout-close`、`stream-hangs`、`malformed-active-stream` 等 stream/request edge cases 仍保留。
- [x] Negative tests 不出现在 public runner 默认 case list 中。

**Verification:**

- [x] `cargo nextest run -p noloong-agent-core --test jsonrpc_conformance`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

## Task 4: Add Runner-Specific Integration Tests

**Description:** 新增 tests 覆盖 public runner profile semantics 和 report shape，确保第三方可依赖 API/CLI 的稳定行为。

**Acceptance criteria:**

- [x] `Strict` profile 对完整 fixture 返回 all passed。
- [x] `Hybrid` profile 对 model-only fixture 返回 generic passed、full skipped。
- [x] `Hybrid` profile 对 partial fixture 返回 failed report。
- [x] `fail_fast(true)` 在首个失败 case 后停止后续 case。
- [x] Report serde round-trip 保持 case status、message 和 counts。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test extension_conformance`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent-core/tests/extension_conformance.rs`
- `crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Estimated scope:** M

### Checkpoint: Test Coverage

- [x] Public runner profile semantics 有独立测试。
- [x] Internal negative suite 仍完整通过。
- [x] Positive logic 不再在多个 tests 中复制实现。

### Phase 3: CLI

## Task 5: Add `noloong-extension-conformance` Binary

**Description:** 在 `noloong-agent-core` crate 中新增 CLI binary，包装 public runner API，允许第三方扩展作者用命令行直接验证自己的 stdio JSON-RPC extension。

**Acceptance criteria:**

- [x] 支持 `--profile generic|hybrid|strict`。
- [x] 支持 `--json` 输出 machine-readable report。
- [x] 支持 `--fail-fast`。
- [x] 使用 `-- <command> [args...]` 分隔 runner args 与 extension command args。
- [x] Invalid argv 返回非零 exit code 并打印 concise usage。
- [x] Runner failure 返回非零 exit code；skipped cases 不导致失败。

**Verification:**

- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs --mode=all-capabilities,adapter-payloads,tool-hook-payloads`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile hybrid --json -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Dependencies:** Tasks 1-2

**Files likely touched:**

- `crates/noloong-agent-core/src/bin/noloong-extension-conformance.rs`
- `crates/noloong-agent-core/src/extension_conformance.rs`

**Estimated scope:** M

## Task 6: Add CLI Smoke Tests

**Description:** 增加 CLI-level smoke tests，验证 exit code、text summary、JSON output 和 invalid argv behavior。测试应复用现有 Node fixture，不依赖真实外部服务。

**Acceptance criteria:**

- [x] Strict fixture smoke exit code 为 `0`。
- [x] Hybrid model-only smoke exit code 为 `0`，并包含 skipped count。
- [x] Invalid profile 或缺少 `-- <command>` exit code 非 `0`。
- [x] JSON output 可反序列化为 `ExtensionConformanceReport`。

**Verification:**

- [x] `cargo test -p noloong-agent-core --test extension_conformance_cli`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent-core/tests/extension_conformance_cli.rs`

**Estimated scope:** M

### Checkpoint: CLI

- [x] CLI 可以被第三方扩展作者直接运行。
- [x] Text 和 JSON 输出都被测试覆盖。
- [x] CLI 和 Rust API 共享同一个 runner engine。

### Phase 4: Documentation and Final Verification

## Task 7: Update Architecture Documentation

**Description:** 更新 `ARCHITECTURE.md` 中 Process Extension Bridge 章节，把当前“内部 suite 不是 public runner”的描述替换为 public runner 设计，并说明 profile、CLI 示例和标准 conformance capability contract。

**Acceptance criteria:**

- [x] 文档解释 `Generic`、`Hybrid`、`Strict` 的区别。
- [x] 文档列出标准 `conformance-*` capability ids。
- [x] 文档给出 CLI text 和 JSON 模式示例。
- [x] “内部 suite 不是对第三方扩展作者暴露的 public runner” 这类过期描述被移除。
- [x] `## 后续演进方向` 中删除或改写该已完成待办。

**Verification:**

- [x] `rg "不是对第三方扩展作者暴露|可复用的 extension conformance runner" crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Dependencies:** Tasks 1-6

**Files likely touched:**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** S

## Task 8: Run Full Quality Gate

**Description:** 运行完整格式、lint、test 和 fixture syntax gate，确保 runner extraction 没有破坏 agent core、JSON-RPC bridge、provider、phase hook、compaction 或 SSE 行为。

**Acceptance criteria:**

- [x] Formatting clean。
- [x] Clippy clean。
- [x] Workspace tests pass。
- [x] Node fixture syntax valid。
- [x] CLI strict/hybrid smoke commands pass。

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo nextest run --workspace`
- [x] `node --check crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs --mode=all-capabilities,adapter-payloads,tool-hook-payloads`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile hybrid -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**Dependencies:** Tasks 1-7

**Files likely touched:**

- No new files expected beyond previous tasks

**Estimated scope:** S

### Checkpoint: Complete

- [x] Public runner API exists and is documented.
- [x] CLI exists and is tested.
- [x] Internal negative conformance suite still protects bridge robustness.
- [x] Full workspace quality gate passes.
- [x] `plans/CURRENT_PLAN.md` remains aligned with implemented behavior.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Public runner overfits current fixture | High | Split generic/hybrid/strict profiles; only strict requires standard conformance ids. |
| Negative bridge tests accidentally become third-party requirements | Medium | Keep malformed/duplicate/wrong-id modes internal and document the boundary. |
| CLI adds dependency bloat | Low | Use manual argv parser for v1. |
| Positive conformance logic is duplicated between runner and tests | Medium | Internal tests should call runner where possible and reserve custom code for negative cases. |
| Partial conformance capability creates confusing skips | Medium | Hybrid treats partial standard capability set as failure with explicit missing ids. |

## Parallelization Opportunities

- Tasks 1-2 are sequential foundation work.
- After Task 2, Task 3 and Task 4 can be done in parallel if they coordinate runner API names.
- Task 5 depends on runner API but can proceed before all negative suite cleanup is complete.
- Task 7 can begin after public API and CLI names are stable, then be finalized after tests pass.

## Open Questions

- None. Decisions locked: API + CLI, default `Hybrid` profile, no new CLI dependency, internal negative tests remain private.
