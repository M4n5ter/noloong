# Implementation Plan: Goal and Trigger-Agnostic Automation

## Overview

Build two separate host-level capabilities in `noloong-agent`: **Goal** tracks one long-running objective for a session and audits it only at agent turn boundaries; **Automation** stores trigger-driven prompts and delivers them to target sessions when triggers fire. Automation must not be modeled as "cron only": the MVP implements a `time` trigger, but the data model and runner are trigger-agnostic so future webhook or external event triggers can reuse the same delivery path.

## Architecture Decisions

- Goal and Automation live in `noloong-agent` host/interaction layer, not in `noloong-agent-core`.
- Each session has at most one active goal. Setting a new goal replaces the previous active goal.
- Goal audit is turn-end only. There is no scheduled goal audit and no time-based goal wakeup.
- Goal audit uses the existing steering queue: a `TurnCompleted` listener injects a `goal_audit` observation, allowing the same run loop to continue into an audit turn.
- Goal status changes are explicit through a model-callable built-in tool and interaction API; free-text assistant output alone must not mark a goal complete.
- Automation is trigger-agnostic. MVP trigger kind is `time`, with `onceAtMs` and `intervalSeconds`; daily/weekly/cron/webhook are not in MVP.
- Automation delivery to an existing running/paused session uses steering observation, not follow-up. Idle/completed sessions may be prompted immediately.
- Pure automation sessions receive a system prompt addition that explains they are automation tasks and may be woken by triggers.
- Persistence uses the interaction registry store boundary, but goal and automation records are separate from `AgentSessionRecord`.

## Task List

### Phase 1: Shared Models and Persistence

#### Task 1: Define Goal and Automation domain models

**Description:** Add the typed records, enums, metadata keys, and store contracts that all later slices depend on. Keep trigger and target shapes extensible without coupling automation to time schedules.

**Acceptance criteria:**
- [x] `GoalRecord` supports `sessionId`, `objective`, `status`, optional `tokenBudget`, `lastAudit`, `createdAtMs`, and `updatedAtMs`.
- [x] `AutomationRecord` supports `automationId`, `status`, `target`, `trigger`, `prompt`, `metadata`, `lastFiredAtMs`, and `nextFireAtMs`.
- [x] `AutomationTrigger` is a tagged enum with MVP `time`; `AutomationTarget` distinguishes existing session vs new automation session.
- [x] Shared message metadata constants exist for `source.type = automation` and `source.type = goal_audit`.

**Verification:**
- [x] Unit tests cover serde round-trips for goal records, automation records, time triggers, and targets.
- [x] `cargo test -p noloong-agent goal automation`

**Dependencies:** None

**Files likely touched:**
- `crates/noloong-agent/src/interaction/goal.rs`
- `crates/noloong-agent/src/interaction/automation.rs`
- `crates/noloong-agent/src/interaction/store/traits.rs`

**Estimated scope:** M

#### Task 2: Persist Goal and Automation records in registry stores

**Description:** Extend memory, object, SQLite, and Postgres registry stores to persist goal and automation records through the same store abstraction used by session snapshots.

**Acceptance criteria:**
- [x] Memory store implements CRUD/list for goals and automations.
- [x] Object store writes records under stable prefixes such as `goals/` and `automations/`.
- [x] SQL store persists JSON payload plus indexed identifiers needed for lookup/list: id, session id, status, next fire time.
- [x] Store errors follow existing `InteractionError` conventions.

**Verification:**
- [x] Store tests cover insert/get/list/save/delete for memory, object, SQLite, and Postgres-gated tests.
- [x] `cargo test -p noloong-agent --test interaction_registry_store_object --test interaction_registry_store_sqlite --test interaction_registry_store_postgres`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent/src/interaction/store/memory.rs`
- `crates/noloong-agent/src/interaction/store/object.rs`
- `crates/noloong-agent/src/interaction/store/sql.rs`

**Estimated scope:** M

### Checkpoint: Persistence Foundation

- [x] Domain serde tests pass.
- [x] Registry store tests pass for enabled store backends.
- [x] No changes required in `noloong-agent-core`.

### Phase 2: Goal Lifecycle and Turn-End Audit

#### Task 3: Add Goal management APIs to interaction control

**Description:** Expose session-scoped goal lifecycle operations through JSON-RPC. This is the management surface that UI clients and future Telegram commands will wrap.

**Acceptance criteria:**
- [x] Add authority capability `goal.manage`.
- [x] Add methods `goal/set`, `goal/get`, `goal/pause`, `goal/resume`, `goal/clear`, and `goal/update`.
- [x] `goal/set` creates or replaces the active goal for a session.
- [x] Paused, cleared, achieved, unmet, and budget-limited goals are not considered pursuing.

**Verification:**
- [x] Interaction tests cover create, replace, read, pause, resume, clear, update, and missing session errors.
- [x] `cargo test -p noloong-agent --test interaction_control goal`

**Dependencies:** Tasks 1-2

**Files likely touched:**
- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent/src/interaction/wire.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**Estimated scope:** M

#### Task 4: Add model-callable Goal update tool

**Description:** Add a built-in host tool so the agent can explicitly update the current session's goal status during an audit turn. This avoids parsing free-form assistant text to decide whether a goal is complete.

**Acceptance criteria:**
- [x] Add built-in tool `agent.goal.update`, mounted only when the session has a pursuing goal and a goal controller is available.
- [x] Tool input supports `status: pursuing|achieved|unmet|budget_limited`, plus optional `summary` and `evidence`.
- [x] Tool can update only the current session's active goal.
- [x] Tool output returns the updated goal record as JSON.

**Verification:**
- [x] Tool unit tests cover valid update, invalid status, no active goal, and cross-session denial.
- [x] `cargo test -p noloong-agent --test goal_tools`

**Dependencies:** Task 3

**Files likely touched:**
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/src/tools/goal.rs`
- `crates/noloong-agent/tests/goal_tools.rs`

**Estimated scope:** M

#### Task 5: Inject turn-end Goal audit steering

**Description:** Wire goal audit into session runtime events. When a pursuing goal exists and a turn completes, inject a steering observation that asks the agent to audit progress and call `agent.goal.update` if the goal status changed.

**Acceptance criteria:**
- [x] `TurnCompleted` for sessions with pursuing goals injects one `goal_audit` steering observation.
- [x] Audit is skipped for paused, achieved, unmet, budget-limited, or cleared goals.
- [x] Audit message metadata includes `source.type`, `goalId/sessionId`, and `auditReason: turn_end`.
- [x] Audit injection does not create a timer and does not wake idle sessions by time.

**Verification:**
- [x] Registry/session tests prove a normal turn with a pursuing goal produces an audit turn.
- [x] Tests prove non-pursuing statuses do not inject audit.
- [x] `cargo test -p noloong-agent --test interaction_registry goal`

**Dependencies:** Task 4

**Files likely touched:**
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** M

### Checkpoint: Goal Path

- [x] `goal/set` plus one agent run can trigger a goal-audit turn.
- [x] Agent can update goal status through `agent.goal.update`.
- [x] No scheduled goal audit exists.
- [x] `cargo test -p noloong-agent --test interaction_control --test interaction_registry --test goal_tools`

### Phase 3: Automation CRUD and Delivery

#### Task 6: Add Automation management APIs

**Description:** Expose trigger-agnostic automation CRUD through JSON-RPC. The API should describe triggers generically even though MVP only ships `time`.

**Acceptance criteria:**
- [x] Add authority capability `automation.manage`.
- [x] Add methods `automation/create`, `automation/get`, `automation/list`, `automation/update`, `automation/delete`, and `automation/fire`.
- [x] `automation/create` validates target session/profile references and computes initial `nextFireAtMs` for `time` triggers.
- [x] `automation/fire` manually fires any active automation through the same delivery path as trigger fires.

**Verification:**
- [x] Interaction tests cover CRUD, manual fire, paused automation behavior, invalid target, and invalid trigger.
- [x] `cargo test -p noloong-agent --test interaction_control automation`

**Dependencies:** Tasks 1-2

**Files likely touched:**
- `crates/noloong-agent/src/interaction/control.rs`
- `crates/noloong-agent/src/interaction/automation.rs`
- `crates/noloong-agent/tests/interaction_control.rs`

**Estimated scope:** M

#### Task 7: Implement Automation delivery semantics

**Description:** Implement the shared delivery path that turns an automation record into a session message. Existing sessions and pure automation sessions must receive different context.

**Acceptance criteria:**
- [x] Existing idle/completed target sessions receive `Agent::prompt(AgentInput::Message)`.
- [x] Existing running/paused target sessions receive a steering observation and snapshot save.
- [x] Delivered messages include automation source metadata, automation id, trigger type, and fired timestamp.
- [x] Missing target sessions fail the fire attempt without corrupting automation state.

**Verification:**
- [x] Tests cover idle prompt, running steering, paused steering, missing session, and metadata shape.
- [x] `cargo test -p noloong-agent --test automation_delivery`

**Dependencies:** Task 6

**Files likely touched:**
- `crates/noloong-agent/src/interaction/automation.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/automation_delivery.rs`

**Estimated scope:** M

#### Task 8: Support pure automation sessions

**Description:** Let automation create or target a dedicated automation session. These sessions need explicit prompt context saying they are automation tasks that may be woken by triggers.

**Acceptance criteria:**
- [x] New automation session target can create a session using selected/default profile.
- [x] Automation session records include metadata marking automation ownership.
- [x] Automation sessions receive a system prompt addition explaining automation identity and trigger wakeups.
- [x] Existing non-automation sessions are not given the automation identity prompt.

**Verification:**
- [x] Tests cover session creation, metadata, manifest/system prompt addition, and normal session non-regression.
- [x] `cargo test -p noloong-agent --test interaction_registry automation_session`

**Dependencies:** Task 7

**Files likely touched:**
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/interaction_registry.rs`

**Estimated scope:** M

### Checkpoint: Manual Automation

- [x] Automation CRUD works through interaction JSON-RPC.
- [x] `automation/fire` works for existing and pure automation sessions.
- [x] Busy-session delivery uses steering observation, not follow-up.
- [x] `cargo test -p noloong-agent --test interaction_control --test automation_delivery --test interaction_registry`

### Phase 4: Time Trigger Runner

#### Task 9: Add trigger runner lifecycle

**Description:** Add a host-side runner that scans active automations, fires due triggers, and updates `lastFiredAtMs` / `nextFireAtMs`. The runner is generic over trigger kind even though only `time` can become due in MVP.

**Acceptance criteria:**
- [x] Runner starts with the interaction registry and can be disabled in tests/config.
- [x] Runner wakes on nearest `nextFireAtMs`, fires due active automations, and persists updated fire state.
- [x] Concurrent runner ticks do not double-fire the same automation.
- [x] Failed fire attempts are recorded in automation metadata or last error without disabling the automation.

**Verification:**
- [x] Time-controlled tests cover once, interval, paused, failure, and no double-fire.
- [x] `cargo test -p noloong-agent --test automation_runner`

**Dependencies:** Tasks 6-8

**Files likely touched:**
- `crates/noloong-agent/src/interaction/automation.rs`
- `crates/noloong-agent/src/interaction/registry.rs`
- `crates/noloong-agent/tests/automation_runner.rs`

**Estimated scope:** M

#### Task 10: Implement MVP time trigger calculations

**Description:** Implement deterministic calculation for MVP `time` triggers. Keep schedule logic isolated so future daily/weekly/cron/webhook work does not affect delivery.

**Acceptance criteria:**
- [x] `onceAtMs` fires once, then becomes completed or inactive.
- [x] `intervalSeconds` computes next fire from actual fire time, not from process wake time.
- [x] Invalid schedules are rejected at create/update.
- [x] Trigger calculation is pure and covered by unit tests.

**Verification:**
- [x] Unit tests cover due/not due, next fire, invalid interval, and once completion.
- [x] `cargo test -p noloong-agent automation_time`

**Dependencies:** Task 9

**Files likely touched:**
- `crates/noloong-agent/src/interaction/automation.rs`
- `crates/noloong-agent/tests/automation_runner.rs`

**Estimated scope:** S

### Checkpoint: Time Automation

- [x] Time-triggered automation fires without manual `automation/fire`.
- [x] Runner survives fire failures and keeps processing other automations.
- [x] No Goal audit is scheduled by the runner.
- [x] `cargo test -p noloong-agent --test automation_runner --test automation_delivery`

### Phase 5: Contracts, Docs, and Regression

#### Task 11: Update docs and schemas

**Description:** Document the new interaction methods, capabilities, trigger model, metadata conventions, and the explicit distinction between Goal and Automation.

**Acceptance criteria:**
- [x] Interaction docs list `goal/*` and `automation/*` methods with example payloads.
- [x] Conformance matrix includes Goal and Automation coverage.
- [x] Profile/config schema changes are included only if runner configuration is exposed.
- [x] Docs state that Goal has no scheduled audit.

**Verification:**
- [x] Documentation examples are syntactically valid JSON.
- [x] `cargo test -p noloong-agent --test interaction_control`

**Dependencies:** Tasks 3-10

**Files likely touched:**
- `crates/noloong-agent/docs/INTERACTION.md`
- `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`
- `schemas/profile-config.schema.json`

**Estimated scope:** S

#### Task 12: Full regression and live smoke hooks

**Description:** Run focused and workspace-level validation, then prepare smoke paths for future Telegram/CLI wrappers without implementing those wrappers in MVP.

**Acceptance criteria:**
- [x] Focused tests pass for goal, automation delivery, automation runner, and interaction control.
- [x] Workspace tests pass.
- [x] A manual smoke recipe exists: create session, set goal, run prompt, observe audit, create automation, manually fire, then interval-fire.

**Verification:**
- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test -p noloong-agent`
- [x] `cargo test --workspace`

**Dependencies:** Tasks 1-11

**Files likely touched:**
- `crates/noloong-agent/docs/INTERACTION.md`
- `plans/CURRENT_PLAN.md`

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Goal audit loops forever | High | Audit only for pursuing goals; `agent.goal.update` can mark achieved/unmet/budget-limited; tests assert no audit for terminal statuses. |
| Automation becomes cron-shaped | Medium | Keep `trigger` as tagged enum and isolate time calculations in trigger-specific code. |
| Busy session automation changes user intent | Medium | Deliver as steering observation, never follow-up, and mark source metadata clearly. |
| Store expansion becomes too broad | Medium | Add goal/automation CRUD alongside existing registry store patterns; avoid changing `AgentSessionRecord`. |
| Runner double-fires after restart or concurrent ticks | High | Persist `lastFiredAtMs` and `nextFireAtMs`; update fire state atomically per store as far as each backend supports. |

## Parallelization Opportunities

- Tasks 3-5 must be sequential after persistence because Goal API, tool, and audit depend on each other.
- Tasks 6-8 can run after persistence and mostly parallelize with Goal work if store contracts are stable.
- Tasks 9-10 must wait for Automation delivery.
- Task 11 docs can start once API shapes in Tasks 3 and 6 are fixed.

## Open Questions

- None for MVP. Daily/weekly schedules, cron/RRULE, webhook triggers, Telegram commands, and multi-goal sessions are explicitly deferred.
