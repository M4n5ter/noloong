# Implementation Plan: Host-first Evolvable Agent

## Overview

新增 `noloong-agent` 产品层，基于 `noloong-agent-core` 构建宿主机优先的自进化 Agent。v1 的主路径是后台命令工具：命令启动后不阻塞，Agent 可以跨 product turn 读取、等待、写入或终止进程；system prompt、tools、approval policy 通过 manifest patch 在审批后于下一 product turn 生效。

当前 `noloong-agent-core` 已提供：

- event-sourced runtime、phase graph、provider traits、tool permission/approval hooks。
- JSON-RPC extension bridge、context compaction、built-in model providers、SQLite event store。
- `AgentEffect::SetAvailableTools`、`ContextProvider`、`ToolProvider`、`ToolCallHook` 等产品层可复用扩展点。

本轮不把 host、shell、SSH、VMM、process manager 概念放进 `noloong-agent-core`。SSH/VMM 暂时是宿主机命令能力：Agent 可以通过 `host.exec.start` 启动 `ssh`、`lima`、`qemu`、`clone` 等命令自行操作。

参考资料：

- OpenAI Codex repository: <https://github.com/openai/codex>
- Codex exec server process lifecycle: <https://github.com/openai/codex/tree/main/codex-rs/exec-server>
- Linux `clone` future sandbox candidate: <https://github.com/unixshells/clone>

## Architecture Decisions

- 新 crate 命名为 `noloong-agent`，作为 product runtime；`noloong-agent-core` 继续保持 providerless kernel 边界。
- 后台命令采用 lifecycle tool group，而不是一个阻塞式 `exec`：`host.exec.start`、`host.exec.read`、`host.exec.wait`、`host.exec.write`、`host.exec.terminate`、`host.exec.list`。
- `host.exec.start` 使用 optimistic foreground window：命令若在配置的短等待窗口内完成，则直接返回完整结果；超过窗口才返回 running job handle，供后续 `read/wait/write/terminate` 使用。
- `host.exec.start` 接收 shell command string，并显式记录 shell；默认 shell 从宿主机推断，也允许显式选择 `sh`、`bash`、`zsh`、`powershell`、`cmd` 或 custom shell。
- 进程生命周期绑定 product session，不绑定单个 core run；session close 默认清理仍在运行的 job。
- 输出写 product spool/ring buffer；core event log 只记录 tool result、job lifecycle 摘要、cursor、truncation metadata，避免大输出污染 event store。
- 自进化采用 proposal + approval + next-turn rebuild：Agent 提交 manifest patch，审批通过后 supervisor 重建 core runtime，下一 product turn 生效。
- v1 manifest patch 支持 system prompt、enabled tools、approval policy；phase node 替换只保留 schema/documentation，不实际执行。
- 所有给模型看的 host context、tool description、approval prompt 使用 typed i18n catalog，默认支持 English 和 Chinese。
- locale 解析顺序：显式配置优先，其次宿主机 `LC_ALL` / `LC_MESSAGES` / `LANG`，最后 fallback 到 English。
- `crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md` 是 core 能力验证矩阵；product crate 后续如需独立矩阵，应放在 `crates/noloong-agent/docs/`，不再放入 `plans/`。

## Task List

### Phase 1: Product Crate Foundation

#### Task 1: Create product crate skeleton

**Description:** 新增 `crates/noloong-agent`，建立 product runtime 的模块边界，依赖 `noloong-agent-core`，并导出后续任务需要的最小 public API。

**Acceptance criteria:**

- [ ] workspace 包含 `crates/noloong-agent` member。
- [ ] crate 暴露 `AgentSession`、`AgentManifest`、`HostEnvironment` 的初始 public API。
- [ ] `noloong-agent-core` public API 不因 product crate 增加 host/process/VMM 概念。

**Verification:**

- [ ] `cargo check -p noloong-agent`
- [ ] `cargo check -p noloong-agent-core`
- [ ] `rg -n "HostEnvironment|host.exec|process manager|VMM|shell" crates/noloong-agent-core/src crates/noloong-agent-core/docs/ARCHITECTURE.md` only returns intentional architecture text if any.

**Dependencies:** None

**Files likely touched:**

- `Cargo.toml`
- `crates/noloong-agent/Cargo.toml`
- `crates/noloong-agent/src/lib.rs`

**Estimated scope:** Small

#### Task 2: Add host environment detection and typed i18n catalog

**Description:** 实现宿主机环境采集和 typed i18n catalog，为模型生成稳定的 host context、tool descriptions 和 approval prompts。

**Acceptance criteria:**

- [ ] `HostEnvironment` 包含 OS、arch、cwd、default shell、available shell hints、path style、locale。
- [ ] locale 支持 explicit override、host inference、English fallback。
- [ ] English/Chinese catalog key 完整性由测试保证，缺失 key 必须失败而不是静默 fallback。

**Verification:**

- [ ] `cargo test -p noloong-agent host_environment`
- [ ] `cargo test -p noloong-agent i18n_catalog`
- [ ] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/host.rs`
- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/host_environment.rs`
- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Medium

### Checkpoint: Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo check -p noloong-agent`
- [ ] `cargo check -p noloong-agent-core`
- [ ] `cargo test -p noloong-agent host_environment i18n_catalog`

### Phase 2: Background Command Runtime

#### Task 3: Implement `HostProcessManager`

**Description:** 实现 session 级后台进程管理器，负责 job id、process lifecycle、status、exit code、output cursor、spool/ring buffer、optimistic foreground window 和 session cleanup。

**Acceptance criteria:**

- [ ] `start` 支持 configurable foreground wait；窗口内完成时返回 completed snapshot，超时才返回 running job handle。
- [ ] `read`、`wait`、`list` 可在同一个 product session 内跨 turn 使用。
- [ ] session close 默认清理仍在运行的进程，并保留已完成 job 的摘要状态。

**Verification:**

- [ ] `cargo test -p noloong-agent host_process_manager_start_returns_completed_when_fast`
- [ ] `cargo test -p noloong-agent host_process_manager_start_returns_running_when_slow`
- [ ] `cargo test -p noloong-agent host_process_manager_read_wait_list`
- [ ] `cargo test -p noloong-agent host_process_manager_session_cleanup`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/process/mod.rs`
- `crates/noloong-agent/src/process/manager.rs`
- `crates/noloong-agent/tests/host_process_manager.rs`

**Estimated scope:** Medium

#### Task 4: Add process I/O and interactive command support

**Description:** 在 process manager 中加入 stdout/stderr/pty output buffering、stdin write、timeout 和 graceful terminate 行为，为交互式命令做基础。

**Acceptance criteria:**

- [ ] 支持 stdout/stderr 增量读取，cursor 顺序稳定。
- [ ] 支持 PTY 或 pipe stdin 写入；不支持的平台必须 fail fast 并给出结构化错误。
- [ ] `wait` timeout 不杀进程；`terminate` graceful timeout 后 kill。

**Verification:**

- [ ] `cargo test -p noloong-agent host_process_output_cursor_order`
- [ ] `cargo test -p noloong-agent host_process_interactive_write`
- [ ] `cargo test -p noloong-agent host_process_wait_timeout_does_not_kill`
- [ ] `cargo test -p noloong-agent host_process_terminate`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent/src/process/manager.rs`
- `crates/noloong-agent/src/process/io.rs`
- `crates/noloong-agent/tests/host_process_manager.rs`

**Estimated scope:** Medium

### Checkpoint: Process Runtime

- [ ] `cargo test -p noloong-agent host_process_manager`
- [ ] Manual smoke: start a long command, read partial output, wait, then inspect final status.
- [ ] Manual smoke: start an interactive command, write stdin, read response, terminate if still running.

### Phase 3: Command Lifecycle Tools

#### Task 5: Implement host command ToolProviders

**Description:** 将 `HostProcessManager` 暴露为 core `ToolProvider` 组，使模型通过工具调用启动、读取、等待、写入、终止和列出后台命令。

**Acceptance criteria:**

- [ ] 提供 `host.exec.start`、`host.exec.read`、`host.exec.wait`、`host.exec.write`、`host.exec.terminate`、`host.exec.list`。
- [ ] `host.exec.start` 在 foreground window 内完成时返回 completed output；超时返回 `jobId`、shell、cwd、status、initial output snapshot。
- [ ] 所有 tool output 使用稳定 structured `details`，包含 status、cursor、exit code、truncated/error metadata。

**Verification:**

- [ ] `cargo test -p noloong-agent host_exec_tools_start_and_read`
- [ ] `cargo test -p noloong-agent host_exec_tools_start_fast_path_returns_result`
- [ ] `cargo test -p noloong-agent host_exec_tools_wait_timeout`
- [ ] `cargo test -p noloong-agent host_exec_tools_write_and_terminate`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent/src/tools/host_exec.rs`
- `crates/noloong-agent/src/tools/mod.rs`
- `crates/noloong-agent/tests/host_exec_tools.rs`

**Estimated scope:** Medium

#### Task 6: Add command output audit summaries

**Description:** 确保大输出留在 product spool/ring buffer，core tool result 只提交摘要、cursor、cap 和 truncation metadata。

**Acceptance criteria:**

- [ ] 大 stdout/stderr 不作为完整 chunk 写入 core event log。
- [ ] `read` 支持 output cap，并明确返回 `truncated` 和 `nextCursor`。
- [ ] non-zero exit、stderr-only output、timeout、unknown job 都有稳定错误或状态表达。

**Verification:**

- [ ] `cargo test -p noloong-agent host_exec_large_output_is_spooled`
- [ ] `cargo test -p noloong-agent host_exec_output_cap_and_cursor`
- [ ] `cargo test -p noloong-agent host_exec_non_zero_and_unknown_job_details`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent/src/process/spool.rs`
- `crates/noloong-agent/src/tools/host_exec.rs`
- `crates/noloong-agent/tests/host_exec_tools.rs`

**Estimated scope:** Medium

### Checkpoint: Command Tools

- [ ] `cargo test -p noloong-agent host_exec_tools`
- [ ] Integration smoke: Agent starts long-running command, proceeds to another turn, then reads/waits result.
- [ ] Confirm `noloong-agent-core` event store does not contain unbounded command output.

### Phase 4: Manifest Evolution

#### Task 7: Implement `AgentManifest` and patch validation

**Description:** 定义 product manifest 和 manifest patch，支持 prompt、enabled tools、approval policy 的受控变更，并预留 phase profile schema。

**Acceptance criteria:**

- [ ] manifest 包含 locale、system prompt profile、enabled tools、approval policy、reserved phase profile。
- [ ] patch 支持 replace system prompt、enable/disable tool、update approval policy。
- [ ] invalid patch 被拒绝且不改变 manifest；phase patch v1 只能被记录为 unsupported/reserved。

**Verification:**

- [ ] `cargo test -p noloong-agent manifest_patch_applies_prompt_tools_policy`
- [ ] `cargo test -p noloong-agent manifest_patch_rejects_invalid_changes`
- [ ] `cargo test -p noloong-agent manifest_phase_patch_is_reserved`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/manifest.rs`

**Estimated scope:** Medium

#### Task 8: Add manifest patch proposal tool

**Description:** 将自进化入口实现为 tool：Agent 只能提交 manifest patch proposal，不能直接修改 live manifest。

**Acceptance criteria:**

- [ ] `agent.manifest.propose_patch` 返回 proposal id 和 patch summary。
- [ ] proposal 进入 approval path 前不会改变 manifest。
- [ ] proposal details 可审计，并能被 human 或 auto-review agent 使用。

**Verification:**

- [ ] `cargo test -p noloong-agent manifest_proposal_does_not_apply_without_approval`
- [ ] `cargo test -p noloong-agent manifest_proposal_tool_returns_auditable_details`

**Dependencies:** Task 7

**Files likely touched:**

- `crates/noloong-agent/src/evolution.rs`
- `crates/noloong-agent/src/tools/manifest.rs`
- `crates/noloong-agent/tests/manifest_evolution.rs`

**Estimated scope:** Medium

### Checkpoint: Manifest Evolution

- [ ] `cargo test -p noloong-agent manifest`
- [ ] Manual check: a proposed manifest patch is visible, auditable, and not applied until approved.

### Phase 5: Product Session Supervisor and Approval

#### Task 9: Implement `AgentSession` next-turn rebuild

**Description:** 实现 product supervisor：每个 product turn 使用当前 manifest 构造 core runtime；approved patch 在下一 product turn 前应用并重建 runtime，同时保留 session process manager。

**Acceptance criteria:**

- [ ] approved prompt/tool/policy patch 下一 product turn 生效。
- [ ] rejected patch 不影响下一 product turn。
- [ ] runtime rebuild 不丢失 `HostProcessManager` 中的后台 jobs。

**Verification:**

- [ ] `cargo test -p noloong-agent agent_session_prompt_patch_takes_effect_next_turn`
- [ ] `cargo test -p noloong-agent agent_session_tool_patch_takes_effect_next_turn`
- [ ] `cargo test -p noloong-agent agent_session_rebuild_preserves_background_jobs`

**Dependencies:** Task 6, Task 8

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/runtime_factory.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Medium

#### Task 10: Add approval reviewer integration

**Description:** 通过 `ToolCallHook` 统一处理 host command 和 manifest patch approval，并支持 human fallback 与可关闭的 auto-review agent。

**Acceptance criteria:**

- [ ] `host.exec.start`、`host.exec.write`、`host.exec.terminate` 和 manifest patch proposal 都进入 permission audit。
- [ ] human reviewer 可使用现有 pause/resume path。
- [ ] auto-review agent 可插拔、可关闭；关闭后需要 human decision。

**Verification:**

- [ ] `cargo test -p noloong-agent approval_host_exec_start_allow_deny`
- [ ] `cargo test -p noloong-agent approval_manifest_patch_allow_deny`
- [ ] `cargo test -p noloong-agent approval_auto_review_can_be_disabled`

**Dependencies:** Task 9

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

### Checkpoint: Evolvable Session

- [ ] `cargo test -p noloong-agent agent_session approval`
- [ ] End-to-end smoke: start background command, propose tool/policy change, approve it, observe next-turn runtime change while job remains readable.

### Phase 6: Documentation and Final Verification

#### Task 11: Document product architecture and examples

**Description:** 为 product crate 编写架构文档和 examples，解释 host-first execution、background command lifecycle、自进化 manifest、approval reviewer 和 i18n。

**Acceptance criteria:**

- [ ] docs 明确哪些能力在 `noloong-agent`，哪些能力留在 `noloong-agent-core`。
- [ ] example 展示 start long-running command、继续做别的事、再 read/wait。
- [ ] docs 明确 SSH/VMM v1 是宿主命令能力，不是 target abstraction。

**Verification:**

- [ ] `cargo test -p noloong-agent --examples`
- [ ] Manual check: docs mention lifecycle tool group and next-turn manifest rebuild.

**Dependencies:** Task 10

**Files likely touched:**

- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/examples/background_command.rs`
- `README.md`

**Estimated scope:** Small

### Final Checkpoint

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo nextest run --workspace --all-features`
- [ ] `cargo test -p noloong-agent --examples`
- [ ] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [ ] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| PTY behavior differs across platforms | High | Keep PTY behind process I/O abstraction; Unix path first, unsupported platform fail fast with structured error |
| Background output grows without bound | High | Use spool/ring buffer, output cap, truncation metadata, and lifecycle summaries |
| Runtime rebuild loses session state | High | Keep process manager, manifest store, and approval reviewer in `AgentSession`, not in core runtime |
| Approval auto-review makes unsafe decisions | High | Default to proposal + approval; auto-review can be disabled; every decision enters existing permission audit |
| Shell command strings are injection-prone | Medium | Treat command string as the user's explicit shell program, always record shell/cwd/env, and route through approval policy |
| Product crate accidentally leaks host concepts into core | Medium | Add regression audit and keep all host/process modules outside `noloong-agent-core` |
| Long-running jobs survive unexpectedly | Medium | Session close cleanup is default; docs must make lifecycle explicit |

## Parallelization Opportunities

- Task 2 and Task 3 can run in parallel after Task 1.
- Task 7 can run in parallel with Tasks 3-6 after Task 2 because manifest validation does not depend on process execution.
- Task 11 documentation can start after Task 5, but final examples should wait until Task 10 is complete.
