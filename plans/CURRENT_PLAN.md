# Implementation Plan: Built-in Tool Description Improvements

## Overview

Improve model-facing descriptions for the built-in `noloong-agent` tools and related permissions. The goal is to make each description useful to an LLM at tool-selection time: what the tool does, when to use it, which arguments materially affect behavior, and what kind of result or follow-up action to expect.

This is intentionally a text and test update. It must not change tool names, schemas, runtime behavior, approval policy, provider serialization, or permission enforcement.

## Architecture Decisions

- Keep built-in tool and permission descriptions in the existing i18n catalog keyed by `MessageKey`.
- Update English and Chinese catalog entries together so the two locales remain semantically equivalent.
- Write operational descriptions, not tutorials: one to three concise sentences per description, except `apply_patch`, which should include one compact multi-operation patch example.
- Mention only arguments that materially change correct tool use, such as `foregroundWaitMs`, `afterSeq`, `maxBytes`, `pipeStdin`, `content`, `oldString`, `newString`, and `replaceAll`.
- Permission descriptions remain capability-level summaries. They should explain the broad authority being granted without duplicating every tool argument.
- Do not add keyword-based description content tests. They are brittle and low signal; use catalog completeness plus spec propagation tests instead.

## Task List

### Phase 1: Description Contract

#### Task 1: Define the description rubric

**Description:** Establish the minimal contract for model-facing tool descriptions before rewriting individual catalog entries.

**Acceptance criteria:**

- [ ] The rubric covers purpose, correct usage, critical constraints, and return or follow-up behavior.
- [ ] The rubric explicitly requires English and Chinese entries to carry the same semantics.
- [ ] The task produces no schema, runtime, permission, or provider behavior changes.

**Verification:**

- [ ] Review the rubric against the current built-in tool surfaces.

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Small

### Phase 2: Built-in Tool Description Updates

#### Task 2: Improve host exec lifecycle descriptions

**Description:** Rewrite the host command descriptions so the model understands the background-job lifecycle and knows which follow-up tool to call.

**Acceptance criteria:**

- [ ] `host.exec.start` explains that commands run as background jobs, `foregroundWaitMs` may return fast results inline, and longer jobs return a job handle such as `jobId`.
- [ ] `host.exec.read` explains cursor-based, non-destructive stdout/stderr polling with `afterSeq` and `maxBytes`.
- [ ] `host.exec.wait` explains waiting for completion and that timeout does not kill the job.
- [ ] `host.exec.write` explains that stdin writes only work for jobs started with `pipeStdin`.
- [ ] `host.exec.terminate` explains termination as an explicit request and that the latest job status is returned.
- [ ] `host.exec.list` explains that it lists current session jobs and their status.
- [ ] English and Chinese catalog entries are updated together.

**Verification:**

- [ ] `cargo test -p noloong-agent i18n`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Small

#### Task 3: Improve file editing tool descriptions

**Description:** Rewrite file editing descriptions so the model can choose and use the active editing tool correctly.

**Acceptance criteria:**

- [ ] `write_file` explains both full-file `content` writes and strict `oldString` / `newString` replacement mode.
- [ ] `write_file` explains the effect of `replaceAll`.
- [ ] `apply_patch` explains the strict V4A patch format, required Begin/End markers, add/update/delete/move support, and includes one minimal multi-operation example.
- [ ] The wording does not imply fuzzy replacement or unsupported read-staleness behavior.
- [ ] English and Chinese catalog entries are updated together.

**Verification:**

- [ ] `cargo test -p noloong-agent i18n`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Small

#### Task 4: Improve manifest and permission descriptions

**Description:** Rewrite manifest patch and permission descriptions so the model understands approval boundaries and broad capabilities.

**Acceptance criteria:**

- [ ] `agent.manifest.propose_patch` explains that it proposes manifest changes for a future turn and does not apply them immediately.
- [ ] The manifest patch description mentions approval before application.
- [ ] `host.command` permission explains authority to start and control host processes.
- [ ] `host.file.write` permission explains authority to write, modify, move, or delete host filesystem paths through file editing tools.
- [ ] `agent.manifest.patch` permission explains authority to change future agent session behavior after approval.
- [ ] English and Chinese catalog entries are updated together.

**Verification:**

- [ ] `cargo test -p noloong-agent i18n`

**Dependencies:** Task 1

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Small

### Checkpoint: Description Text Complete

- [ ] Built-in tool descriptions are operational and concise.
- [ ] Permission descriptions describe capabilities without becoming argument tutorials.
- [ ] English and Chinese entries are semantically aligned.
- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent i18n`

### Phase 3: Regression Coverage

#### Task 5: Add non-keyword i18n propagation tests

**Description:** Add tests that verify built-in tool specs source descriptions from the i18n catalog without asserting keyword lists or prose fragments.

**Acceptance criteria:**

- [ ] Tests keep catalog completeness coverage for English and Chinese.
- [ ] Tests verify host exec tool specs use the expected catalog description keys.
- [ ] Tests verify file edit tool specs use the expected catalog description keys.
- [ ] Tests verify manifest patch tool specs use the expected catalog description keys.
- [ ] Tests verify permission descriptions are propagated from the expected catalog keys.
- [ ] No tests assert keyword lists, prose fragments, or complete description strings for content quality.

**Verification:**

- [ ] `cargo test -p noloong-agent i18n`

**Dependencies:** Tasks 2, 3, 4

**Files likely touched:**

- `crates/noloong-agent/tests/i18n.rs`

**Estimated scope:** Small

#### Task 6: Audit generated tool specs for description propagation

**Description:** Confirm the improved catalog text is what the runtime exposes through built-in tool specifications.

**Acceptance criteria:**

- [ ] Host exec tool specs source descriptions from the updated catalog keys.
- [ ] File edit tool specs source descriptions from the updated catalog keys.
- [ ] Manifest patch tool specs source descriptions from the updated catalog keys.
- [ ] No hard-coded duplicate description strings are introduced.

**Verification:**

- [ ] `rg -n "Description|description\\(" crates/noloong-agent/src/tools crates/noloong-agent/src/i18n.rs`
- [ ] `cargo test -p noloong-agent i18n`

**Dependencies:** Tasks 2, 3, 4

**Files likely touched:**

- `crates/noloong-agent/src/tools/host_exec.rs`
- `crates/noloong-agent/src/tools/file_edit.rs`
- `crates/noloong-agent/src/tools/manifest.rs`
- `crates/noloong-agent/src/i18n.rs`

**Estimated scope:** Small

### Checkpoint: Complete

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent i18n`
- [ ] `cargo test -p noloong-agent`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---:|---|
| Descriptions become too long and crowd model context | Medium | Keep descriptions concise; allow only the `apply_patch` multi-operation example because its grammar is otherwise easy to misuse. |
| Tests become brittle against harmless wording changes | Medium | Avoid keyword-based description content tests; only verify catalog completeness and spec propagation. |
| English and Chinese entries drift | Medium | Update both locales in the same tasks and test both catalogs where practical. |
| Descriptions imply unsupported behavior | High | Tie wording only to existing schemas and observed runtime behavior. |
| Permission descriptions over-specify implementation details | Medium | Keep permission text capability-level and leave argument details in tool descriptions. |

## Open Questions

- None. The default scope is to improve all built-in model-facing tool and permission descriptions without changing APIs or behavior.
