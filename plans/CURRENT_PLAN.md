# Implementation Plan: Agent Core Conformance & CI Hardening

## Overview
把下一阶段目标从“继续加功能”切到“证明 `noloong-agent-core` 可靠”：建立可审计的 conformance matrix、系统化状态机/协议测试、CI gate，并发布到私有 GitHub 仓库 `m4n5ter/noloong` 后观察 GitHub Actions。真实 OpenRouter DeepSeek 测试保留为手动 gate，不进入默认 CI。

## Architecture Decisions
- Conformance 测试优先验证不变量，而不是重复现有 happy-path 测试。
- CI 默认只跑无外网、可重复、确定性的 gate；OpenRouter live test 作为手动命令和可选 workflow job。
- 新增测试不改变 agent core public API，除非为了测试可观测性必须暴露最小只读 helper。
- GitHub 目标仓库固定为私有 `m4n5ter/noloong`，默认分支 `main`。

## Task List

### Task 1: Replace `CURRENT_PLAN.md` With Conformance Plan
**Description:** 用本计划替换上一阶段已完成的 pi-like operational core 计划，明确当前阶段只做验证、CI 和发布闭环。  
**Acceptance criteria:**
- [ ] `plans/CURRENT_PLAN.md` 标题、目标、任务列表、checkpoint 全部更新为本阶段内容
- [ ] 旧计划中已完成的功能实现任务不再作为待办出现
- [ ] 明确 OpenRouter live test 是手动 gate，不是默认 CI gate
**Verification:**
- [ ] `sed -n '1,220p' plans/CURRENT_PLAN.md`
**Dependencies:** None  
**Files likely touched:** `plans/CURRENT_PLAN.md`  
**Estimated scope:** XS

### Task 2: Add Conformance Matrix
**Description:** 新增一份 prompt-to-artifact matrix，把 agent core 的能力、不变量、测试名、命令和覆盖缺口显式列出来。  
**Acceptance criteria:**
- [ ] Matrix 覆盖 runtime event sourcing、stateful agent、cancellation、queues、tool policies/hooks、JSON-RPC streaming、error/replay、OpenRouter live path
- [ ] 每项能力映射到至少一个具体测试名或标记为 explicit gap
- [ ] README 链接到 matrix，并说明如何更新
**Verification:**
- [ ] `rg -n "Conformance|agent_abort_cancels_active_run|stdio_model_stream_notifications_are_incremental|openrouter_deepseek" README.md plans crates/noloong-agent-core/tests`
**Dependencies:** Task 1  
**Files likely touched:** `plans/CONFORMANCE_MATRIX.md`, `README.md`  
**Estimated scope:** S

### Task 3: Add Runtime State Machine Conformance Tests
**Description:** 增加集中式 conformance tests，验证 event log 与 reducer 的核心不变量，而不是只检查单个场景输出。  
**Acceptance criteria:**
- [ ] 成功 run 必须满足 `reduce_events(events) == report.state`
- [ ] 失败 run 必须有 `PhaseFailed` 或 `RunFailed`，且 replay 后 `RunStatus::Failed`
- [ ] abort run 必须有 `RunAborted`，且 replay 后 `RunStatus::Aborted`
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test conformance runtime_`
**Dependencies:** Task 2  
**Files likely touched:** `crates/noloong-agent-core/tests/conformance.rs`  
**Estimated scope:** M

### Task 4: Add Event Ordering and Sink Conformance Tests
**Description:** 固化 event store append、state apply、subscriber notification 的顺序，避免实时事件实现后续被破坏。  
**Acceptance criteria:**
- [ ] sink 收到事件时，event store 中已包含该事件
- [ ] sink 失败后记录 `RunFailed`，且失败事件不再次通知同一个 failing sink
- [ ] model stream event 不重复 emit，returned events 与 pushed stream events 不产生双计数
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test conformance event_`
**Dependencies:** Task 3  
**Files likely touched:** `crates/noloong-agent-core/tests/conformance.rs`  
**Estimated scope:** M

### Task 5: Add Queue and Tool Policy Conformance Tests
**Description:** 扩展现有 unit tests，覆盖 queue mode 与 tool execution policy 的组合边界。  
**Acceptance criteria:**
- [ ] `QueueMode::OneAtATime` 多 follow-up 会产生多个后续 turn
- [ ] steering queue 在 tool batch 后注入，不在同一 tool batch 中途打断
- [ ] global sequential、per-tool sequential、parallel 三种模式都有 source-order commit 断言
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test conformance queue_ tool_`
**Dependencies:** Task 3  
**Files likely touched:** `crates/noloong-agent-core/tests/conformance.rs`  
**Estimated scope:** M

### Task 6: Add JSON-RPC Protocol Conformance Tests
**Description:** 建立 stdio JSON-RPC 协议级测试，专门覆盖 notification/request/terminal/error/cancel 的交错行为。  
**Acceptance criteria:**
- [ ] stream notification 可早于 request response 被 runtime 实时观察到
- [ ] `Finished` 可在无 response 时 settle run
- [ ] `Failed`、invalid JSON、extension stdout closed、request timeout、stream timeout 都有结构化失败断言
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test jsonrpc`
- [ ] `cargo test -p noloong-agent-core --test conformance jsonrpc_`
**Dependencies:** Task 4  
**Files likely touched:** `crates/noloong-agent-core/tests/conformance.rs`, `crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs`  
**Estimated scope:** M

### Task 7: Add CI Workflow
**Description:** 新增 GitHub Actions workflow，固化本地 green gate，并把 live OpenRouter 测试留作手动说明。  
**Acceptance criteria:**
- [ ] `.github/workflows/ci.yml` 在 push 和 PR 上运行
- [ ] CI steps: `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, `cargo test -p noloong-agent-core --examples`, three `node --check` commands
- [ ] Workflow 不要求 `OPENROUTER_API_KEY`
**Verification:**
- [ ] `gh workflow list`
- [ ] `gh run list --limit 5`
**Dependencies:** Tasks 3-6  
**Files likely touched:** `.github/workflows/ci.yml`, `README.md`  
**Estimated scope:** S

### Task 8: Add Manual Live Verification Documentation
**Description:** 明确真实模型测试的运行方式、约束和失败诊断，让 live gate 可重复执行。  
**Acceptance criteria:**
- [ ] README 写明 `OPENROUTER_API_KEY`、`deepseek/deepseek-v4-flash`、DeepSeek official provider-only、reasoning enabled
- [ ] README 写明 live test 命令和它为何不进入默认 CI
- [ ] Matrix 标记 live test 为 manual external gate
**Verification:**
- [ ] `cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture`
**Dependencies:** Task 2  
**Files likely touched:** `README.md`, `plans/CONFORMANCE_MATRIX.md`  
**Estimated scope:** S

### Task 9: Publish Private GitHub Repo and Observe Actions
**Description:** 将当前仓库发布到私有 GitHub 仓库 `m4n5ter/noloong`，推送 `main`，观察 CI 结果。  
**Acceptance criteria:**
- [ ] 若远程不存在，执行 `gh repo create m4n5ter/noloong --private --source=. --remote=origin`
- [ ] 若远程已存在，绑定 `origin` 到 `git@github.com:m4n5ter/noloong.git` 或 HTTPS 等价地址
- [ ] 提交信息使用 `Add agent core conformance gates`
- [ ] 推送 `main` 后 GitHub Actions 至少完成一次 run
**Verification:**
- [ ] `git remote -v`
- [ ] `git status --short`
- [ ] `gh run list --repo m4n5ter/noloong --limit 5`
- [ ] `gh run view --repo m4n5ter/noloong --log` 仅在失败时查看日志
**Dependencies:** Task 7  
**Files likely touched:** Git metadata only, no source files beyond prior tasks  
**Estimated scope:** S

## Checkpoints
### Checkpoint A: After Tasks 1-2
- [ ] Plan and conformance matrix accurately describe current repo capabilities
- [ ] Every existing test has an explicit purpose in the matrix

### Checkpoint B: After Tasks 3-6
- [ ] `cargo test -p noloong-agent-core --test conformance`
- [ ] `cargo test --workspace`
- [ ] No conformance item is marked as uncovered unless intentionally deferred

### Checkpoint C: After Tasks 7-9
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p noloong-agent-core --examples`
- [ ] `node --check` passes for all `.mjs` fixtures/examples
- [ ] GitHub Actions green on `m4n5ter/noloong`
- [ ] Manual OpenRouter live test passes locally

## Risks and Mitigations
| Risk | Impact | Mitigation |
|---|---:|---|
| Conformance tests duplicate existing unit tests | Medium | Focus on invariants and cross-feature interactions, keep existing scenario tests as supporting evidence |
| CI becomes flaky due external services | High | Keep OpenRouter live test manual and ignored by default |
| GitHub repo already exists with different history | Medium | Inspect `gh repo view m4n5ter/noloong` before creating/binding; do not force push |
| Test suite grows without clear ownership | Medium | Maintain `plans/CONFORMANCE_MATRIX.md` as the source of truth for capability-to-test mapping |

## Assumptions
- 下一阶段目标是 Conformance 验证与 CI hardening，不新增 agent runtime 功能，除非测试暴露必须修复的 bug。
- GitHub 私有仓库目标是 `m4n5ter/noloong`，默认分支是 `main`。
- 默认 CI 不使用 `OPENROUTER_API_KEY`；真实模型测试仍通过手动命令执行。
- 如发布时发现远程仓库已存在，不做破坏性历史重写，不使用 force push。
