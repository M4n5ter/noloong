# Implementation Plan: Smart Built-In Approval Policy

## Overview

`ApprovalPolicy::RequireApproval` should become a smart gate: safe built-in operations are allowed immediately, risky operations ask for approval, and clearly unsafe operations can be denied without execution.

This follows the OpenAI Codex shape without copying it wholesale. Codex combines explicit exec policy rules, known-safe / dangerous command heuristics, sandbox escalation rules, and session approval caching. `noloong-agent` does not yet have a full sandbox boundary, so v1 is intentionally more conservative: unknown host commands require approval unless a rule or session approval cache explicitly allows them.

Reference implementation to read while implementing:

- `https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs`
- `https://github.com/openai/codex/blob/main/codex-rs/core/src/exec_policy.rs`
- `https://github.com/openai/codex/blob/main/codex-rs/shell-command/src/command_safety/is_safe_command.rs`
- `https://github.com/openai/codex/blob/main/codex-rs/shell-command/src/command_safety/is_dangerous_command.rs`
- `https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/sandboxing.rs`

## Architecture Decisions

- Repurpose `ApprovalPolicy::RequireApproval`; do not add a new public mode for v1.
- `RequireApproval` means "ask only when the built-in classifier says approval is required".
- `AutoReview` runs the reviewer only for calls classified as approval-required; safe calls bypass the reviewer.
- `AllowAll` still bypasses all built-in approval checks.
- No persistent exec policy files in v1; rules and caches are in-memory.
- No full sandbox in v1; unknown `host.exec.start` commands require approval by default.
- Deny only for clearly dangerous built-in host commands where running would be unreasonable without an explicit policy override.
- Preserve `noloong-agent-core` approval semantics. The smart policy is implemented in `noloong-agent` as built-in hook logic and session integration.

## Task List

### Phase 1: Approval Classification Foundation

#### Task 1: Add built-in classification model

**Description:** Add internal classification types that separate "policy evaluation" from "hook result rendering". The hook should first classify the tool call, then translate the classification into `ToolPermissionDecision` or `ToolApprovalRequestSpec`.

**Acceptance criteria:**

- [x] Internal result supports `Allow`, `NeedsApproval`, and `Deny`.
- [x] Classification metadata includes source, reason, tool name, and tool call id.
- [x] `BuiltInApprovalHook::before_tool_call` no longer maps `RequireApproval` directly to unconditional human approval.
- [x] Existing `AllowAll` and `AutoReview { fallback_to_human: false }` behavior remains covered by tests.

**Verification:**

- [x] `cargo test -p noloong-agent approval`
- [x] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

#### Task 2: Add policy evaluation flow

**Description:** Update `BuiltInApprovalHook` so policy modes consume the classification result consistently.

**Acceptance criteria:**

- [x] `AllowAll` always returns allow decision.
- [x] `RequireApproval` returns allow, approval, or deny based on classification.
- [x] `AutoReview` returns allow immediately for safe classifications.
- [x] `AutoReview` calls `ApprovalReviewer` only when classification is `NeedsApproval`.
- [x] If `AutoReview` needs approval and no reviewer exists, fallback behavior still follows `fallback_to_human`.

**Verification:**

- [x] Add tests for reviewer call count on safe and approval-required calls.
- [x] `cargo test -p noloong-agent approval`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

### Checkpoint: Approval Flow Foundation

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent approval`
- [x] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

### Phase 2: Built-In Tool Gate

#### Task 3: Classify non-command built-in tools

**Description:** Add deterministic classification for built-in tools that do not require shell command parsing.

**Acceptance criteria:**

- [x] `host.exec.read`, `host.exec.wait`, and `host.exec.list` are classified as `Allow`.
- [x] `host.exec.write` and `host.exec.terminate` are classified as `NeedsApproval`.
- [x] `agent.manifest.propose_patch` is classified as `NeedsApproval`.
- [x] Unknown tool names are classified as `NeedsApproval`, not auto-allowed.

**Verification:**

- [x] Tests cover each `BuiltInToolName` category.
- [x] Tests cover an unknown tool name.
- [x] `cargo test -p noloong-agent approval`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Small

#### Task 4: Attach approval metadata to tool specs

**Description:** Make built-in tool permission metadata useful to the classifier and audit logs. Keep provider-facing schemas unchanged.

**Acceptance criteria:**

- [x] Host exec tools expose permission metadata identifying host command capability.
- [x] Manifest patch tool exposes permission metadata identifying manifest mutation capability.
- [x] Classifier does not depend only on raw natural-language descriptions.
- [x] Permission metadata appears in `ToolPermissionRequested` audit events.

**Verification:**

- [x] Add or update tests asserting relevant `ToolSpec.permissions` metadata.
- [x] `cargo test -p noloong-agent`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent/src/tools/mod.rs`
- `crates/noloong-agent/src/tools/host_exec.rs`
- `crates/noloong-agent/src/tools/manifest.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

### Checkpoint: Built-In Tool Gate

- [x] `cargo test -p noloong-agent approval`
- [x] `cargo test -p noloong-agent`

### Phase 3: Host Command Safety Classifier

#### Task 5: Add host command parsing helpers

**Description:** Add a small internal command parser for approval classification. It should support direct command words and simple `sh` / `bash` / `zsh -lc` wrappers. It does not need to be a complete shell parser.

**Acceptance criteria:**

- [x] Parser extracts command segments from plain commands and simple shell wrappers.
- [x] Supported safe separators are `&&`, `||`, `;`, and `|`.
- [x] Unsupported shell syntax returns `Unknown`, which requires approval.
- [x] Parser never treats heredoc, command substitution, redirection, env assignment, or glob-heavy syntax as safe.

**Verification:**

- [x] Unit tests cover direct commands.
- [x] Unit tests cover `sh -lc`, `bash -lc`, and `zsh -lc`.
- [x] Unit tests cover unsupported syntax falling back to approval.

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/approval/command_safety.rs` if splitting the module is clearer
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

#### Task 6: Implement safe command allowlist

**Description:** Implement a conservative known-safe command classifier for read-only host commands. The allowlist should mirror Codex's intent, not its exact code.

**Acceptance criteria:**

- [x] Safe commands include `cat`, `pwd`, `ls`, `rg`, `grep`, `head`, `tail`, `wc`, and `sed -n {range}p`.
- [x] Safe git commands include `git status`, `git log`, `git diff`, `git show`, and read-only `git branch`.
- [x] Unsafe git global options such as `-C`, `-c`, `--git-dir`, `--work-tree`, and `--exec-path` prevent auto-allow.
- [x] A compound command is safe only when every segment is safe.

**Verification:**

- [x] Tests cover each safe command family.
- [x] Tests cover unsafe git global options.
- [x] Tests cover safe pipelines such as `rg foo src | head -n 20`.
- [x] `cargo test -p noloong-agent approval`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent/src/approval/command_safety.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

#### Task 7: Implement dangerous and unknown command handling

**Description:** Add explicit dangerous command detection and conservative unknown handling for `host.exec.start`.

**Acceptance criteria:**

- [x] `rm -f` and `rm -rf` are not auto-allowed.
- [x] `sudo rm -f` and `sudo rm -rf` are not auto-allowed.
- [x] Dangerous command segments inside supported shell wrappers are detected.
- [x] Unknown commands such as `python -c ...`, `node -e ...`, `curl ...`, or unrecognized binaries require approval.
- [x] Dangerous commands default to `NeedsApproval` unless the implementation introduces a separate internal deny threshold with tests documenting it.

**Verification:**

- [x] Tests cover direct dangerous commands.
- [x] Tests cover dangerous commands inside `bash -lc`.
- [x] Tests cover unknown commands requiring approval.
- [x] `cargo test -p noloong-agent approval`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent/src/approval/command_safety.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

### Checkpoint: Host Command Safety

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent approval`
- [x] `cargo test -p noloong-agent`
- [x] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

### Phase 4: Session Approval Cache

#### Task 8: Add in-memory approval cache

**Description:** Add session-scoped approval caching so repeated approval-required calls can be skipped after a user allows the same request for the current session. Because core does not notify hooks when a human approval is resolved, expose the cache through `AgentSession` APIs and wire it into the built-in hook.

**Acceptance criteria:**

- [x] `AgentSession` owns an in-memory approval cache.
- [x] `BuiltInApprovalHook` checks the cache before returning `NeedsApproval`.
- [x] Cache key for `host.exec.start` includes normalized shell, command, cwd, env changes, stdin mode, and relevant execution flags.
- [x] Cache key for `host.exec.write` and `host.exec.terminate` includes job id and operation.
- [x] Denials are not cached as allows.

**Verification:**

- [x] Unit tests cover stable key equality and inequality.
- [x] `cargo test -p noloong-agent approval`

**Dependencies:** Tasks 3, 7

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/approval.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Medium

#### Task 9: Add explicit cache recording API

**Description:** Add a small `AgentSession` API for the application layer to record an approval after it resumes a core `ToolApprovalRequest`. This avoids changing core hook semantics.

**Acceptance criteria:**

- [x] `AgentSession` exposes a method to record an approved `ToolApprovalRequest` and `ToolPermissionDecision`.
- [x] The method records only allow decisions that match the built-in approval hook metadata.
- [x] The method ignores unrelated external hook approvals.
- [x] Architecture docs explain that callers should record built-in approval resolutions when using the cache.

**Verification:**

- [x] Integration test records an approved request and verifies the same command is allowed later.
- [x] Integration test verifies a changed command still asks for approval.
- [x] `cargo test -p noloong-agent agent_session`

**Dependencies:** Task 8

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Medium

### Checkpoint: Session Cache

- [x] `cargo test -p noloong-agent approval`
- [x] `cargo test -p noloong-agent agent_session`
- [x] `cargo test -p noloong-agent`

### Phase 5: Documentation and Verification

#### Task 10: Update architecture documentation

**Description:** Document the new smart approval semantics and the differences from Codex.

**Acceptance criteria:**

- [x] Docs say `RequireApproval` is smart-gated, not every-call approval.
- [x] Docs describe the evaluation order: explicit built-in category, command safety, session cache, reviewer or human approval.
- [x] Docs state that unknown host commands require approval because v1 has no sandbox boundary.
- [x] Docs state persistent execpolicy files and full sandboxing are out of scope for this step.

**Verification:**

- [x] Review `crates/noloong-agent/docs/ARCHITECTURE.md`.
- [x] Search docs and this plan for obsolete all-calls approval wording; no stale claims remain.

**Dependencies:** Tasks 1-9

**Files likely touched:**

- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** Small

#### Task 11: Full validation

**Description:** Run the final workspace checks after implementation.

**Acceptance criteria:**

- [x] All new classifier and cache tests pass.
- [x] Existing agent/session approval tests pass.
- [x] Workspace clippy has no warnings.
- [x] No `#[allow(dead_code)]` is introduced.

**Verification:**

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent`
- [x] `cargo test -p noloong-agent-core --test agent`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo nextest run --workspace --all-features -j 1`
- [x] `git diff --check`

**Dependencies:** Tasks 1-10

**Files likely touched:**

- Test files only, if fixes are needed during validation.

**Estimated scope:** Small

## Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Unsafe command is misclassified as safe | High | Keep allowlist narrow, require every shell segment to be safe, and treat unsupported syntax as approval-required. |
| Unknown commands become too annoying | Medium | Session approval cache reduces repeated prompts; later persistent rules can improve ergonomics. |
| Cache records approvals incorrectly | High | Cache only built-in hook approvals with stable metadata and allow decisions. |
| API confusion around `RequireApproval` | Medium | Update docs and tests to lock the new smart-gate semantics. |
| Full Codex sandbox semantics are assumed | Medium | Docs explicitly state v1 does not include sandboxing or persistent execpolicy files. |

## Open Questions

None. Defaults are locked: Smart Gate scope, `ApprovalPolicy::RequireApproval` is repurposed, unknown host commands require approval, and session approval cache is in-memory only.
