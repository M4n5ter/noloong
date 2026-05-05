# Implementation Plan: Split Approval Module With Thin `mod.rs`

## Overview

Refactor `crates/noloong-agent/src/approval.rs` into a normal `approval/` module tree. `approval/mod.rs` must be an entrypoint only: module declarations plus public/internal re-exports, with no policy logic, helper functions, constants, trait definitions, or tests. This is a behavior-preserving cleanup.

## Architecture Decisions

- Remove the mixed `approval.rs` plus `approval/` layout.
- Keep `approval/mod.rs` structural only: `mod ...;` declarations and `pub use` / `pub(crate) use` exports.
- Preserve existing public paths:
  - `noloong_agent::{ApprovalPolicy, ApprovalReviewer, BuiltInApprovalHook}`
  - `noloong_agent::approval::{allow_decision, deny_decision}`
- Keep behavior, serde shape, approval metadata, approval cache semantics, and command safety classification unchanged.
- Do not add `#[allow(dead_code)]`; the workspace-level `dead_code = "deny"` lint must stay clean.

## Task List

### Phase 1: Module Entry Foundation

#### Task 1: Replace the mixed module layout

**Description:** Remove `crates/noloong-agent/src/approval.rs` as the approval module body and introduce `crates/noloong-agent/src/approval/mod.rs` as the module entrypoint.

**Acceptance criteria:**

- [ ] `crates/noloong-agent/src/approval.rs` no longer exists.
- [ ] `crates/noloong-agent/src/approval/mod.rs` exists.
- [ ] `approval/mod.rs` contains no function bodies, type definitions, constants, tests, or policy logic.
- [ ] `crates/noloong-agent/src/lib.rs` continues to use `pub mod approval;` unchanged.

**Verification:**

- [ ] `cargo check -p noloong-agent`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/approval/mod.rs`

**Estimated scope:** Small

#### Task 2: Split public approval API types

**Description:** Move externally visible approval policy and reviewer definitions out of the module entrypoint into focused files.

**Acceptance criteria:**

- [ ] `ApprovalPolicy` lives in `approval/policy.rs` with the same derives and serde attributes.
- [ ] `ApprovalReviewer` lives in `approval/reviewer.rs` with the same trait signature.
- [ ] `BuiltInApprovalHook` remains publicly reachable through `noloong_agent::BuiltInApprovalHook`.
- [ ] `ApprovalPolicy` and `ApprovalReviewer` remain publicly reachable through `noloong_agent::{ApprovalPolicy, ApprovalReviewer}`.

**Verification:**

- [ ] `cargo test -p noloong-agent approval`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/approval/mod.rs`
- `crates/noloong-agent/src/approval/policy.rs`
- `crates/noloong-agent/src/approval/reviewer.rs`
- `crates/noloong-agent/src/lib.rs`

**Estimated scope:** Small

### Checkpoint: Public API Still Compiles

- [ ] `cargo check -p noloong-agent`
- [ ] `cargo test -p noloong-agent approval`

### Phase 2: Approval Logic Split

#### Task 3: Move hook orchestration into `hook.rs`

**Description:** Move `BuiltInApprovalHook`, its constructors, internal cache injection, and `ToolCallHook` implementation into `approval/hook.rs`.

**Acceptance criteria:**

- [ ] `BuiltInApprovalHook` lives in `approval/hook.rs`.
- [ ] `ToolCallHook for BuiltInApprovalHook` lives in `approval/hook.rs`.
- [ ] Hook behavior is unchanged for `AllowAll`, `RequireApproval`, and `AutoReview`.
- [ ] Internal visibility stays narrow; do not expose hook internals publicly just to satisfy imports.

**Verification:**

- [ ] `cargo test -p noloong-agent approval`

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent/src/approval/hook.rs`
- `crates/noloong-agent/src/approval/mod.rs`
- `crates/noloong-agent/tests/approval.rs`

**Estimated scope:** Medium

#### Task 4: Move classification logic into `classification.rs`

**Description:** Move internal approval classification state and built-in tool classification helpers into `approval/classification.rs`.

**Acceptance criteria:**

- [ ] `ApprovalClass` and `ApprovalClassification` live in `approval/classification.rs`.
- [ ] `classify_host_exec_start` and built-in tool classification glue live in `approval/classification.rs`.
- [ ] `approval/command_safety.rs` remains focused on command parsing and safety classification.
- [ ] Classification metadata keys and values remain unchanged.

**Verification:**

- [ ] `cargo test -p noloong-agent approval`
- [ ] `cargo test -p noloong-agent agent_session`

**Dependencies:** Task 3

**Files likely touched:**

- `crates/noloong-agent/src/approval/classification.rs`
- `crates/noloong-agent/src/approval/hook.rs`
- `crates/noloong-agent/src/approval/command_safety.rs`

**Estimated scope:** Medium

#### Task 5: Move constants and decision helpers

**Description:** Move approval constants and generic decision constructors into files that can be imported by cache, hook, classification, and tests without putting logic in `mod.rs`.

**Acceptance criteria:**

- [ ] `BUILT_IN_APPROVAL_HOOK_ID` and `APPROVAL_CACHE_KEY_METADATA` live in `approval/constants.rs`.
- [ ] `allow_decision` and `deny_decision` live in `approval/decisions.rs`.
- [ ] `noloong_agent::approval::{allow_decision, deny_decision}` still works.
- [ ] `approval/cache.rs` imports constants from `constants.rs`, not from `super`.

**Verification:**

- [ ] `cargo test -p noloong-agent approval`
- [ ] `cargo test -p noloong-agent agent_session`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent/src/approval/constants.rs`
- `crates/noloong-agent/src/approval/decisions.rs`
- `crates/noloong-agent/src/approval/cache.rs`
- `crates/noloong-agent/src/approval/mod.rs`

**Estimated scope:** Small

### Checkpoint: Approval Behavior Preserved

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent approval`
- [ ] `cargo test -p noloong-agent agent_session`

### Phase 3: Cleanup and Verification

#### Task 6: Tighten imports and visibility

**Description:** Clean up module imports after the split and keep visibility scoped to the smallest level that compiles cleanly.

**Acceptance criteria:**

- [ ] No unused imports remain.
- [ ] No `pub` visibility is introduced unless required by current public API.
- [ ] Internal sibling-module items use `pub(super)` or `pub(crate)` only where needed.
- [ ] No `#[allow(dead_code)]` is added.

**Verification:**

- [ ] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

**Dependencies:** Task 5

**Files likely touched:**

- `crates/noloong-agent/src/approval/*.rs`

**Estimated scope:** Small

#### Task 7: Run full workspace checks

**Description:** Verify the refactor did not affect other crates or integration paths.

**Acceptance criteria:**

- [ ] All `noloong-agent` tests pass.
- [ ] Workspace clippy passes with warnings denied.
- [ ] `git diff` shows only approval module organization changes unless a test import needs adjustment.

**Verification:**

- [ ] `cargo test -p noloong-agent`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

**Dependencies:** Task 6

**Files likely touched:**

- `crates/noloong-agent/src/approval/*.rs`
- `crates/noloong-agent/tests/approval.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Small

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---:|---|
| Visibility widens during the split | Medium | Prefer private items, then `pub(super)`, then `pub(crate)` only when cross-module or session integration requires it. |
| Public import paths regress | Medium | Keep `mod.rs` re-exports and run existing approval/session tests. |
| Behavior changes accidentally while moving code | Medium | Move code mechanically first, then clean imports; do not alter logic in the same pass. |

## Open Questions

- None. This plan assumes a behavior-preserving refactor with `mod.rs` kept structural only.
