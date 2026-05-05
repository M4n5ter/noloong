# Implementation Plan: Model-Aware Exclusive File Editing Tools

## Overview

Add built-in file editing support to `noloong-agent`, inspired by Hermes `write_file_tool` / `patch_tool` behavior but not copied wholesale. Runtime must expose exactly one editing tool by default: `apply_patch` for models whose resolved model name contains `gpt` case-insensitively, and `write_file` for all others.

Reference implementation to read while implementing:

- `https://github.com/NousResearch/hermes-agent/blob/main/tools/file_tools.py`
- `https://github.com/NousResearch/hermes-agent/blob/main/tools/file_operations.py`
- `https://github.com/NousResearch/hermes-agent/blob/main/tools/patch_parser.py`

## Architecture Decisions

- Add `ModelProvider::model_name() -> Option<&str>` in `noloong-agent-core`, defaulting to `None`.
- Built-in Chat Completions, Responses API, and Anthropic Messages providers return their config `model`.
- External providers fall back to `provider.id()` for `auto_by_model`.
- Add `FileEditToolPolicy` to `AgentManifest`.
- Default policy is `AutoByModel`: `gpt` model name selects `apply_patch`, all other model names select `write_file`.
- Explicit overrides are `ApplyPatch`, `WriteFile`, and `Disabled`.
- File edit tools are session capabilities, not ordinary manifest `enabled_tools`, because they are mutually exclusive.
- Replace `AgentSession::runtime_builder()` with a noloong-agent wrapper builder that mirrors common `AgentRuntimeBuilder` methods and injects the selected file edit tool at `build()`.
- Tool names are exactly `apply_patch` and `write_file`.
- V1 `apply_patch` supports V4A-style patch input only; do not implement Hermes fuzzy replace mode yet.
- V1 does not add `read_file`, `search_files`, auto-lint, read-dedup, or read-staleness tracking.
- V1 does not cache approvals for file edits.

## Task List

### Phase 1: Model-Aware Selection Foundation

#### Task 1: Add model name metadata to model providers

**Description:** Add an optional true model-name accessor to `ModelProvider` and implement it for built-in provider types.

**Acceptance criteria:**

- [ ] `ModelProvider` has `model_name() -> Option<&str>` with a default `None` implementation.
- [ ] `ChatCompletionsProvider`, `ResponsesApiProvider`, and `AnthropicMessagesProvider` return their config `model`.
- [ ] Existing custom test providers compile without implementing the new method.
- [ ] No behavior changes to model streaming.

**Verification:**

- [ ] `cargo test -p noloong-agent-core chat_completions responses anthropic_messages`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent-core/src/providers.rs`
- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/anthropic_messages.rs`

**Estimated scope:** Small

#### Task 2: Add manifest file edit policy

**Description:** Add manifest-level configuration for selecting which built-in file edit tool may be exposed.

**Acceptance criteria:**

- [ ] `AgentManifest` includes `file_edit_tool_policy`.
- [ ] `FileEditToolPolicy` serde shape uses `snake_case`.
- [ ] Default policy is `AutoByModel`.
- [ ] Explicit policies `ApplyPatch`, `WriteFile`, and `Disabled` round-trip through JSON.
- [ ] Existing manifest defaults remain otherwise unchanged.

**Verification:**

- [ ] `cargo test -p noloong-agent manifest`

**Dependencies:** None

**Files likely touched:**

- `crates/noloong-agent/src/manifest.rs`
- `crates/noloong-agent/tests/manifest.rs`

**Estimated scope:** Small

#### Task 3: Introduce model-aware session runtime builder

**Description:** Add an `AgentSessionRuntimeBuilder` wrapper so `AgentSession` can defer model-aware file tool selection until model providers are registered.

**Acceptance criteria:**

- [ ] `AgentSession::runtime_builder()` returns the wrapper builder.
- [ ] Existing call shape still works: `session.runtime_builder().with_model_provider(...).build()`.
- [ ] Wrapper supports pass-through for common runtime builder methods currently used by tests and examples.
- [ ] Wrapper tracks model providers and default model provider id.
- [ ] At `build()`, wrapper resolves the selected edit tool from manifest policy and model name.
- [ ] If a provider has no `model_name()`, fallback uses `provider.id()`.

**Verification:**

- [ ] `cargo test -p noloong-agent agent_session`
- [ ] `cargo test -p noloong-agent`

**Dependencies:** Tasks 1, 2

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/agent_session.rs`
- `crates/noloong-agent/examples/background_command.rs`

**Estimated scope:** Medium

### Checkpoint: Selection Foundation

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core chat_completions responses anthropic_messages`
- [ ] `cargo test -p noloong-agent manifest`
- [ ] `cargo test -p noloong-agent agent_session`

### Phase 2: File Edit Tool Core

#### Task 4: Add shared file edit manager

**Description:** Add shared infrastructure for path resolution, sensitive-path rejection, parent directory creation, and per-path locking.

**Acceptance criteria:**

- [ ] Relative paths resolve against `HostEnvironment.cwd`.
- [ ] Sensitive paths are rejected before any write.
- [ ] Sensitive path blocks include `/etc`, `/boot`, systemd dirs, Docker socket paths, and macOS private system dirs.
- [ ] Parent directories are created for write operations.
- [ ] Per-session locks serialize edits touching the same resolved path.
- [ ] Multi-file patch locks are sorted by resolved path before acquisition.
- [ ] Tool errors are returned as `ToolOutput { is_error: true, ... }`, not panics.

**Verification:**

- [ ] Unit tests cover relative resolution.
- [ ] Unit tests cover sensitive path rejection.
- [ ] Unit tests cover multi-path lock ordering helper behavior.

**Dependencies:** Task 2

**Files likely touched:**

- `crates/noloong-agent/src/tools/file_edit.rs`
- `crates/noloong-agent/src/tools/mod.rs`
- `crates/noloong-agent/tests/file_edit.rs`

**Estimated scope:** Medium

#### Task 5: Implement `write_file`

**Description:** Implement direct file editing with two mutually exclusive modes: whole-file replacement and strict `oldString` / `newString` replacement.

**Acceptance criteria:**

- [ ] Tool name is `write_file`.
- [ ] Input schema requires either `path` + `content` or `path` + `oldString` + `newString`.
- [ ] `content` mode fully replaces existing file content.
- [ ] `oldString` / `newString` mode updates an existing file with strict matching.
- [ ] Multiple `oldString` matches require explicit `replaceAll`.
- [ ] Missing parent directories are created for `content` mode.
- [ ] Directories and sensitive paths are rejected.
- [ ] Output includes `mode`, `path`, `resolvedPath`, `bytesWritten`, and `createdParentDirs`.
- [ ] Replacement output includes `replacements`.
- [ ] Tool spec permission capability is `host.file.write`.
- [ ] Tool execution mode is sequential.

**Verification:**

- [ ] Integration test writes a new file in a temp dir.
- [ ] Integration test overwrites an existing file.
- [ ] Integration test creates missing parents.
- [ ] Integration test replaces a unique old string.
- [ ] Integration test rejects ambiguous replacement unless `replaceAll` is true.
- [ ] Integration test rejects directory targets and sensitive paths.
- [ ] `cargo test -p noloong-agent file_edit`

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent/src/tools/file_edit.rs`
- `crates/noloong-agent/tests/file_edit.rs`

**Estimated scope:** Medium

#### Task 6: Implement `apply_patch` parser and validator

**Description:** Implement strict V4A-style patch parsing and validation before any filesystem writes.

**Acceptance criteria:**

- [ ] Tool name is `apply_patch`.
- [ ] Input schema requires `patch`.
- [ ] Parser recognizes `*** Begin Patch` and `*** End Patch`.
- [ ] Parser supports `*** Add File`, `*** Update File`, `*** Delete File`, and move syntax.
- [ ] Update hunks use strict context matching against current file contents.
- [ ] All operations validate before any operation writes.
- [ ] Validation failure leaves all files unchanged.
- [ ] Tool spec permission capability is `host.file.write`.
- [ ] Tool execution mode is sequential.

**Verification:**

- [ ] Tests cover add file.
- [ ] Tests cover update file.
- [ ] Tests cover delete file.
- [ ] Tests cover move file.
- [ ] Tests cover successful multi-file patch.
- [ ] Tests cover malformed patch.
- [ ] Tests cover non-matching update context as no-op failure.
- [ ] Tests verify validation failure does not partially write.

**Dependencies:** Task 4

**Files likely touched:**

- `crates/noloong-agent/src/tools/file_edit.rs`
- `crates/noloong-agent/tests/file_edit.rs`

**Estimated scope:** Medium

### Checkpoint: File Edit Tools

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent file_edit`
- [ ] `cargo clippy -p noloong-agent --all-targets --all-features -- -D warnings`

### Phase 3: Runtime Integration and Approval

#### Task 7: Register exactly one edit tool at runtime

**Description:** Wire file edit selection into the session runtime builder.

**Acceptance criteria:**

- [ ] `gpt-5.5-mini` selects `apply_patch`.
- [ ] `GPT-*` casing still selects `apply_patch`.
- [ ] `deepseek-*` selects `write_file`.
- [ ] `claude-*` selects `write_file`.
- [ ] Explicit `ApplyPatch` policy selects `apply_patch` regardless of model.
- [ ] Explicit `WriteFile` policy selects `write_file` regardless of model.
- [ ] Explicit `Disabled` policy selects neither.
- [ ] `ModelRequest.tools` never contains both `apply_patch` and `write_file`.

**Verification:**

- [ ] Session tests inspect `runtime.tool("apply_patch")`.
- [ ] Session tests inspect `runtime.tool("write_file")`.
- [ ] Session tests inspect captured `ModelRequest.tools`.
- [ ] `cargo test -p noloong-agent agent_session`

**Dependencies:** Tasks 3, 5, 6

**Files likely touched:**

- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Medium

#### Task 8: Update approval classification

**Description:** Make file edit tools require approval and emit useful permission metadata.

**Acceptance criteria:**

- [ ] `write_file` and `apply_patch` are classified as `NeedsApproval`.
- [ ] Permission metadata includes `builtIn`, `capability`, and `tool`.
- [ ] Approval request metadata includes parseable target paths when available.
- [ ] Session approval cache does not auto-allow repeated file edits.
- [ ] Unknown or malformed file edit arguments still require approval rather than auto-allowing.

**Verification:**

- [ ] Approval tests cover both file edit tools.
- [ ] Audit tests cover `host.file.write` permission metadata.
- [ ] Cache tests confirm file edit approvals are not cached.
- [ ] `cargo test -p noloong-agent approval`
- [ ] `cargo test -p noloong-agent agent_session`

**Dependencies:** Task 7

**Files likely touched:**

- `crates/noloong-agent/src/approval/classification.rs`
- `crates/noloong-agent/src/approval/cache.rs`
- `crates/noloong-agent/tests/approval.rs`
- `crates/noloong-agent/tests/agent_session.rs`

**Estimated scope:** Small

#### Task 9: Update i18n and docs

**Description:** Add user-visible tool descriptions and document the model-aware exclusive editing behavior.

**Acceptance criteria:**

- [ ] English and Chinese catalogs include descriptions for `write_file` and `apply_patch`.
- [ ] Architecture docs explain why only one edit tool is exposed.
- [ ] Docs state non-GPT models get `write_file` by default because many models are less reliable with patch grammar.
- [ ] Docs state v1 omits Hermes fuzzy replace mode, read-file staleness tracking, and auto-lint.
- [ ] Current plan remains aligned with implemented scope.

**Verification:**

- [ ] `cargo test -p noloong-agent i18n`
- [ ] Review docs for stale claims.
- [ ] `rg -n "write_file|apply_patch|FileEditToolPolicy|file_edit_tool_policy" crates/noloong-agent/docs plans`

**Dependencies:** Tasks 7, 8

**Files likely touched:**

- `crates/noloong-agent/src/i18n.rs`
- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** Small

### Checkpoint: Complete

- [ ] `cargo fmt --check`
- [ ] `cargo test -p noloong-agent-core chat_completions responses anthropic_messages`
- [ ] `cargo test -p noloong-agent manifest`
- [ ] `cargo test -p noloong-agent file_edit`
- [ ] `cargo test -p noloong-agent approval`
- [ ] `cargo test -p noloong-agent agent_session`
- [ ] `cargo test -p noloong-agent`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---:|---|
| Model name cannot be discovered for external providers | Medium | Use `model_name()` for built-ins and provider id fallback for external providers; document the fallback. |
| Wrapper builder misses existing pass-through methods | Medium | Search tests/examples for `runtime_builder()` chaining and add only currently used pass-through methods in v1. |
| Both edit tools accidentally register | High | Centralize selection in one resolver and add tests against runtime tools plus `ModelRequest.tools`. |
| Patch parser applies partial changes | High | Validate every operation before writes and test validation failure no-op behavior. |
| File edits touch sensitive system paths | High | Centralize path validation and apply it before lock acquisition and before writes. |
| Strict patch matching frustrates non-GPT models | Medium | Non-GPT default is `write_file`, which supports both whole-file write and strict old/new replacement; explicit `ApplyPatch` remains available for callers that want it. |

## Open Questions

- None. Defaults are locked: model-aware exclusive tools, GPT gets `apply_patch`, non-GPT gets `write_file`, explicit override is available, and v1 omits Hermes fuzzy replace/read-staleness features.
