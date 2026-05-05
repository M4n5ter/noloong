# Implementation Plan: Remove Legacy Terminology from `noloong-agent`

## Overview

`crates/noloong-agent` previously used a legacy wording family for crate-provided runtime, hooks, tools, and model-visible text. That wording was not a good long-term abstraction because it made application-layer responsibilities sound like a business-domain layer, and it had already entered public Rust APIs, hook/context ids, i18n text, errors, tests, and docs.

This plan performs a full rename. There are no compatibility aliases and no legacy hook id prefixes. Rust APIs use `BuiltIn*` for crate-provided capabilities; architecture prose uses `application layer` / `应用层` for the integration layer above core.

## Architecture Decisions

- No compatibility burden: remove old Rust type names and old hook id prefixes without deprecated aliases.
- Rust public API uses `BuiltIn*`: `BuiltInApprovalHook`, `BuiltInToolName`, and `BuiltInToolOutputOverflowHook`.
- Documentation uses application layer / 应用层 for architecture prose; `built-in` only describes crate-provided tools, hooks, and enums.
- Tool names stay unchanged: `host.exec.*` and `agent.manifest.propose_patch` are protocol names, not terminology labels.
- `noloong-agent-core` is only updated if it has stale references to the `noloong-agent` API; core behavior and public API stay unchanged.

## Task List

### Phase 1: Rust API Rename

#### Task 1: Rename public built-in types

**Description:** Rename the `noloong-agent` public Rust API to `BuiltIn*` names and update all internal references, re-exports, and test imports. This task handles Rust symbols only.

**Acceptance criteria:**

- [x] Approval hook type is `BuiltInApprovalHook`.
- [x] Built-in tool enum is `BuiltInToolName`.
- [x] Tool output overflow hook type is `BuiltInToolOutputOverflowHook`.
- [x] Private host context provider is `BuiltInHostContextProvider`.

**Verification:**

- [x] Rust symbol audit has no legacy type-name hits in `crates/noloong-agent/src` or `crates/noloong-agent/tests`.
- [x] `cargo test -p noloong-agent`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/tools/`
- `crates/noloong-agent/tests/`

**Estimated scope:** Medium

#### Task 2: Rename built-in hook and context ids

**Description:** Rename runtime-facing ids to the `noloong.builtin.*` prefix and update tests. Old ids are not retained.

**Acceptance criteria:**

- [x] Approval hook id is `noloong.builtin.approval`.
- [x] Host context provider id is `noloong.builtin.host-context`.
- [x] Tool output overflow hook id is `noloong.builtin.tool-output-overflow`.

**Verification:**

- [x] Hook id audit has no legacy prefix hits in `crates/noloong-agent`.
- [x] `cargo test -p noloong-agent approval tool_output_overflow`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/approval.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/tools/output_overflow.rs`
- `crates/noloong-agent/tests/`

**Estimated scope:** Small

### Checkpoint: API and ID Rename

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent`
- [x] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

### Phase 2: Text and Error Rename

#### Task 3: Rename i18n and model-visible wording

**Description:** Update `Catalog` text that can enter model context, approval requests, tool output, or errors. English uses application / built-in wording; Chinese uses 应用层 / 内置工具.

**Acceptance criteria:**

- [x] Approval-policy text uses application wording.
- [x] Manifest permission text uses application agent wording.
- [x] Unknown tool errors use built-in tool wording.
- [x] Source text audit has no legacy terminology hits in `crates/noloong-agent/src`.

**Verification:**

- [x] Source text audit passes for `crates/noloong-agent/src`.
- [x] `cargo test -p noloong-agent --test i18n`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/i18n.rs`
- `crates/noloong-agent/tests/manifest.rs`

**Estimated scope:** Medium

#### Task 4: Rename test data and assertions

**Description:** Update test imports, expected strings, and search assertions to verify the new terminology rather than avoiding the old strings.

**Acceptance criteria:**

- [x] Tests import `BuiltIn*` APIs.
- [x] Manifest error assertions expect built-in wording.
- [x] i18n tests cover the new English and Chinese wording.

**Verification:**

- [x] Test text audit has no legacy terminology hits in `crates/noloong-agent/tests`.
- [x] `cargo test -p noloong-agent`

**Dependencies:** Task 1, Task 3

**Files likely touched:**

- `crates/noloong-agent/tests/approval.rs`
- `crates/noloong-agent/tests/agent_session.rs`
- `crates/noloong-agent/tests/i18n.rs`
- `crates/noloong-agent/tests/manifest.rs`
- `crates/noloong-agent/tests/tool_output_overflow.rs`

**Estimated scope:** Medium

### Checkpoint: Text Rename

- [x] `cargo test -p noloong-agent`
- [x] Source and test text audit passes for `crates/noloong-agent`.

### Phase 3: Documentation and Plan Rename

#### Task 5: Update architecture documentation

**Description:** Update `crates/noloong-agent/docs/ARCHITECTURE.md` to use application layer wording, `BuiltIn*` type names, and `noloong.builtin.*` ids.

**Acceptance criteria:**

- [x] Overview no longer uses legacy wording.
- [x] Architecture diagram uses application / built-in wording.
- [x] tool output overflow, approval, manifest evolution, and i18n sections use the new terminology.

**Verification:**

- [x] Documentation text audit passes for `crates/noloong-agent/docs/ARCHITECTURE.md`.
- [x] Manual doc review confirms terminology is consistent with public API.

**Dependencies:** Tasks 1-4

**Files likely touched:**

- `crates/noloong-agent/docs/ARCHITECTURE.md`

**Estimated scope:** Small

#### Task 6: Update cross-doc references and current plan

**Description:** Ensure `plans/CURRENT_PLAN.md` and relevant cross-doc references do not preserve stale terminology. Do not change core public APIs.

**Acceptance criteria:**

- [x] `plans/CURRENT_PLAN.md` uses `BuiltIn*` / application layer terminology.
- [x] Any stale `noloong-agent` API references in core docs are removed.
- [x] Core public API is unchanged.

**Verification:**

- [x] Plan text audit has no legacy terminology hits.
- [x] `git diff --check`

**Dependencies:** Task 5

**Files likely touched:**

- `plans/CURRENT_PLAN.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`

**Estimated scope:** Small

### Final Checkpoint

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent`
- [x] `cargo test -p noloong-agent-core --test agent`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo nextest run --workspace --all-features -j 1`
- [x] Final terminology audit passes for `crates/noloong-agent` and `plans/CURRENT_PLAN.md`.
- [x] `git diff --check`

## Risks and Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Public API rename breaks downstream code | Medium | User chose no compatibility burden; keep rename complete and avoid aliases that prolong old terminology. |
| Stable id rename breaks persisted audit expectations | Medium | Update tests and docs in the same change; no migration path because old ids are intentionally removed. |
| Search misses stale lowercase text in docs or i18n | Medium | Final terminology audit covers source, tests, docs, and plan. |
| `BuiltIn` overused in prose | Low | Use `BuiltIn*` for Rust types, but `application layer` / `应用层` in architecture prose. |
| Core docs contain unrelated business-domain language | Low | Only update stale `noloong-agent` terminology; do not rewrite unrelated core architecture language unless it conflicts with new names. |

## Open Questions

None. Defaults are locked: full rename, no compatibility aliases, `BuiltIn*` Rust API, `noloong.builtin.*` ids, and application layer wording in documentation.
