# Implementation Plan: Stateful and Stateless Responses API

> 状态：已完成实现、本地验证、schema check、clippy、workspace tests，以及 ChatGPT subscription streaming/compact live smoke。目标是以 OpenAI Codex 的 Responses 实现为准，完整支持 `stateful` 和 `stateless` 两种 Responses API 运行模式，并修复 `store=false` 场景下回放非持久化 item id 导致的 404。

## Overview

当前 `ResponsesApiProviderConfig` 只有裸 `store: bool`，且 `/responses/compact` 返回的 raw Responses item 会直接写入 provider payload。ChatGPT subscription 默认 `store=false` 时，如果后续请求回放了 `rs_...` 这类未持久化 reasoning item id，OpenAI 后端会返回 “Items are not persisted when `store` is set to false”。本计划将 `store` 上升为清晰的 `stateMode`：默认 `stateless` 使用 full input-array chaining、`store=false` 和 encrypted reasoning replay；显式 `stateful` 使用 `store=true`，允许保留服务端 item id。常规链路不引入 `previous_response_id` / Conversations API，因为 Codex 普通 Responses 请求也不依赖它们。

## Architecture Decisions

- 默认 `stateMode` 为 `stateless`，避免隐式依赖服务端持久化。
- `stateful` 必须显式 opt-in，并等价于内置 provider 请求体中的 `store=true`。
- `store` 与 `include` 由 `stateMode` 和 reasoning config 管理，`responses.extraBody` 不允许覆盖这两个字段。
- `stateless + reasoning.enabled=true` 自动请求 `reasoning.encrypted_content`；显式 `includeEncrypted: false` 在该组合下是配置错误。
- `stateless` 回放 raw Responses item 时必须规范化：移除不该回放的顶层 `id`，只允许 encrypted reasoning / compaction 这类安全 item。
- `/responses/compact` 请求继续保持 Codex 行为：不发送普通 request 的 `store`，也不额外发送普通 request 的 `include`。
- 本轮不把 core history 全量改成 Codex 的 `ResponseItem` 历史模型，也不实现 `previous_response_id` / Conversations API；这两个方向后续可作为独立 provider mode 设计。

## Task List

### Phase 1: Core State Mode Foundation

#### Task 1: Add first-class Responses state mode

**Description:** 在 `noloong-agent-core` 中引入 `ResponsesStateMode`，让 request rendering 通过语义化 mode 控制 `store`，而不是让调用方直接维护裸布尔值。保留现有 `.store(bool)` builder 作为兼容入口，但内部映射到 `stateMode`。

**Acceptance criteria:**
- [x] 新增公开枚举 `ResponsesStateMode::{Stateless, Stateful}`，默认值为 `Stateless`。
- [x] `ResponsesApiProviderConfig` 与 `ResponsesApiRequestRenderConfig` 都包含 `state_mode`。
- [x] `.stateless()`、`.stateful()`、`.with_state_mode(...)` builder 可用。
- [x] 现有 `.store(true)` 等价于 `.stateful()`，`.store(false)` 等价于 `.stateless()`。
- [x] 请求体在 `stateless` 下渲染 `store: false`，在 `stateful` 下渲染 `store: true`。

**Verification:**
- [x] `cargo test -p noloong-agent-core --test responses state_mode`
- [x] `cargo test -p noloong-agent-core --test responses renders_store`

**Dependencies:** 无

**Files likely touched:**
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

#### Task 2: Make stateless reasoning include automatic and reserved fields strict

**Description:** 将 encrypted reasoning include 的默认行为与 `stateMode` 绑定。stateless reasoning 必须自动携带 `reasoning.encrypted_content`，避免后续无法安全回放 reasoning state。禁止 `extraBody` 覆盖 `store` 和 `include`，避免绕过 mode 语义。

**Acceptance criteria:**
- [x] `stateless + reasoning.is_some()` 自动渲染 `include: ["reasoning.encrypted_content"]`。
- [x] `stateful + reasoning.is_some()` 不自动强制 include，除非显式 `include_encrypted_reasoning=true`。
- [x] 显式 `include_encrypted_reasoning=true` 在两种 mode 下都渲染 encrypted reasoning include。
- [x] `extraBody` 包含 `store` 或 `include` 时，renderer 返回 provider error。
- [x] `extraBody` 的其它字段保持现有扩展能力。

**Verification:**
- [x] `cargo test -p noloong-agent-core --test responses encrypted_reasoning`
- [x] `cargo test -p noloong-agent-core --test responses rejects_reserved_extra_body`

**Dependencies:** Task 1

**Files likely touched:**
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** S

### Checkpoint: Core Request Semantics

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent-core --test responses`

### Phase 2: Profile Config and Host Wiring

#### Task 3: Expose `stateMode` in root profile config

**Description:** 在 `responses` 与 `chatgpt_responses` built-in provider config 中增加 `stateMode` 字段。默认值为 `stateless`，并由 `src/host.rs` 映射到 core provider 和 ChatGPT Responses provider。

**Acceptance criteria:**
- [x] `responses` provider 支持 `stateMode: "stateless" | "stateful"`。
- [x] `chatgpt_responses` provider 支持相同字段。
- [x] 省略 `stateMode` 时默认 `stateless`。
- [x] host builder 正确把 `stateMode` 传给 `ResponsesApiProviderConfig`。
- [x] `chatgpt-codex-subscription` 示例显式使用 `stateMode: "stateless"`。

**Verification:**
- [x] `cargo test -p noloong --lib config`
- [x] `cargo test -p noloong --lib host`

**Dependencies:** Task 1

**Files likely touched:**
- `src/config.rs`
- `src/host.rs`
- `examples/profile-configs/chatgpt-codex-subscription.json`
- `schemas/profile-config.schema.json`

**Estimated scope:** M

#### Task 4: Validate profile reasoning conflicts

**Description:** profile 层需要区分 `includeEncrypted` 是未配置还是显式 `false`。`stateless + reasoning.enabled=true + includeEncrypted:false` 应在启动阶段失败，避免用户得到一个看似可运行但迟早在 replay 时坏掉的配置。

**Acceptance criteria:**
- [x] profile reasoning config 的 `includeEncrypted` 可表达 omitted / true / false。
- [x] `stateless + reasoning.enabled=true + includeEncrypted:false` 返回清晰配置错误。
- [x] `stateless + reasoning.enabled=true` 且省略 `includeEncrypted` 时自动启用 encrypted reasoning。
- [x] `stateful + reasoning.enabled=true + includeEncrypted:false` 允许启动。
- [x] schema 中 `includeEncrypted` 仍是普通 boolean 字段。

**Verification:**
- [x] `cargo test -p noloong --lib config`
- [x] `cargo test -p noloong --lib host`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`

**Dependencies:** Task 2, Task 3

**Files likely touched:**
- `src/config.rs`
- `src/host.rs`
- `src/schema.rs`
- `schemas/profile-config.schema.json`

**Estimated scope:** M

### Checkpoint: Profile to Provider Wiring

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong --lib config`
- [x] `cargo test -p noloong --lib host`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`

### Phase 3: Replay Policy and Compaction Compatibility

#### Task 5: Add Responses replay item normalization

**Description:** 在 core 中提供统一的 Responses item replay policy，供普通 request history 渲染和 OpenAI compact output 过滤复用。该 helper 是防止 stateless 模式错误回放 `rs_...` id 的核心。

**Acceptance criteria:**
- [x] 新增公开枚举 `ResponsesReplayItemSource::{RequestHistory, CompactOutput}`。
- [x] 新增公开函数 `normalize_responses_replay_item(...) -> Result<Option<Value>>`。
- [x] `stateful` 下原样保留 item。
- [x] `stateless` 下保留安全 item 并移除顶层 `id`。
- [x] `stateless` 下 `reasoning` 必须包含 `encrypted_content`。
- [x] `stateless` 下 `compaction` / `context_compaction` 必须包含 `encrypted_content`。
- [x] `RequestHistory` 来源遇到 unsafe item 返回错误。
- [x] `CompactOutput` 来源遇到 unsafe item 返回 `Ok(None)`。

**Verification:**
- [x] `cargo test -p noloong-agent-core --test responses replay_item`
- [x] `cargo test -p noloong-agent-core --test responses stateless_replay`

**Dependencies:** Task 1, Task 2

**Files likely touched:**
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/src/lib.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** M

#### Task 6: Apply replay normalization while rendering provider payload history

**Description:** 当前 `ContentBlock::ProviderPayload { provider: "openai.responses", kind: "response_item" }` 会被原样塞进 Responses request input。这个路径必须根据 `stateMode` 规范化，避免 stateless 模式回放不可解析的服务端 item id。

**Acceptance criteria:**
- [x] `openai.responses` / `response_item` provider payload 在 request rendering 时经过 `normalize_responses_replay_item(..., RequestHistory)`。
- [x] `stateless` 下规范化后的 item 不包含顶层 `id`。
- [x] `stateless` 下未加密 reasoning provider payload 会让 request rendering 失败。
- [x] `stateful` 下原始 item id 会被保留。
- [x] 非 Responses provider payload 的现有错误语义不变。

**Verification:**
- [x] `cargo test -p noloong-agent-core --test responses provider_payload`
- [x] `cargo test -p noloong-agent-core --test responses stateless_rejects_unencrypted_reasoning`

**Dependencies:** Task 5

**Files likely touched:**
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**Estimated scope:** S

#### Task 7: Filter `/responses/compact` output by state mode

**Description:** `OpenAiResponsesCompactor` 需要知道目标 `stateMode`。stateless compact output 写回 replacement history 前必须过滤 unsafe item；stateful compact output 可以保留完整 raw item。compact 请求本身继续移除 `store`，保持 Codex parity。

**Acceptance criteria:**
- [x] `OpenAiResponsesCompactorConfig` 增加 `state_mode`，默认 `Stateless`。
- [x] host 构建 ChatGPT compact provider 时传入 profile `stateMode`。
- [x] compact 请求 payload 不包含 `store`。
- [x] compact 请求 payload 不包含普通 request 自动 include。
- [x] stateless compact output 会丢弃 unsafe item。
- [x] stateless compact output 保留 encrypted `reasoning` / `compaction` / `context_compaction` item。
- [x] 过滤后没有任何可回放 item 时返回明确 provider error。
- [x] stateful compact output 保留原始 item id。

**Verification:**
- [x] `cargo test -p noloong-openai --test compact`
- [x] `cargo test -p noloong --lib host`

**Dependencies:** Task 3, Task 5

**Files likely touched:**
- `crates/noloong-openai/src/compact.rs`
- `crates/noloong-openai/tests/compact.rs`
- `src/host.rs`

**Estimated scope:** M

### Checkpoint: Replay and Compaction Safety

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent-core --test responses`
- [x] `cargo test -p noloong-openai --test compact`
- [x] `cargo test -p noloong --lib host`

### Phase 4: Documentation, Examples, and End-to-End Verification

#### Task 8: Update schema, examples, and docs

**Description:** 文档需要把 Responses `stateMode`、event store、registry store、Responses service-side state 这三者区分清楚。示例配置应展示推荐的 stateless ChatGPT subscription 用法，并给出 stateful opt-in 示例。

**Acceptance criteria:**
- [x] JSON schema 包含 `stateMode` 字段和枚举说明。
- [x] `chatgpt-codex-subscription.json` 显式使用 `stateMode: "stateless"`。
- [x] 新增或更新一个 stateful opt-in 示例。
- [x] README 或架构文档说明 event store 不等于 Responses service-side state。
- [x] 文档说明 registry store 负责 session registry，event store 负责 agent event replay，`stateMode` 只影响 provider request/replay。
- [x] 文档说明 `previous_response_id` / Conversations API 不在本轮内置路径中。

**Verification:**
- [x] `cargo run -p noloong -- profile-config schema --output schemas/profile-config.schema.json`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `rg -n "stateMode|stateless|stateful|event store|registry store" README.md crates/noloong-agent-core/docs crates/noloong-openai README.md examples/profile-configs`

**Dependencies:** Task 3, Task 4, Task 7

**Files likely touched:**
- `README.md`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `crates/noloong-openai/README.md`
- `examples/profile-configs/*.json*`
- `schemas/profile-config.schema.json`

**Estimated scope:** M

#### Task 9: Run full local verification

**Description:** 完成所有代码和文档改动后跑完整 workspace 验证，确保 public API、schema、compact、provider rendering 和 profile wiring 没有回归。

**Acceptance criteria:**
- [x] format check 通过。
- [x] clippy 无 warning。
- [x] workspace tests 通过。
- [x] schema check 通过。
- [x] 无新增 `#[allow(dead_code)]`。

**Verification:**
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo test --workspace`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" src crates`

**Dependencies:** Task 1-8

**Files likely touched:**
- 全部已修改文件

**Estimated scope:** S

#### Task 10: Run real provider smoke tests

**Description:** 使用现有 ChatGPT subscription token file 验证真实 provider 行为，重点覆盖 ChatGPT Responses streaming 和 Codex `responses/compact`。真实测试只在已有凭证可用时执行，不把凭证写入仓库。

**Acceptance criteria:**
- [x] ChatGPT Responses streaming live smoke 通过。
- [x] ChatGPT Codex compact live smoke 通过。
- [x] compact live smoke 未出现 `Items are not persisted when store is false`。
- [x] `stateMode=stateful` 的 `store=true` 行为由本地 request rendering 测试覆盖。

**Verification:**
- [x] `NOLOONG_OPENAI_LIVE_CHATGPT=1 NOLOONG_CHATGPT_LIVE_MODEL=gpt-5.4-mini NOLOONG_CHATGPT_TOKEN_FILE="$HOME/.agents/noloong/chatgpt/token.json" cargo test -p noloong-openai --test live_chatgpt -- --ignored --nocapture`
- [x] `cargo test -p noloong-agent-core --test responses state_mode`

**Dependencies:** Task 1-9

**Files likely touched:**
- 无，除非真实测试暴露需要修复的问题

**Estimated scope:** S

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---:|---|
| stateless replay policy 过宽，仍可能回放不可解析 item | 高 | 默认 strict：request history 遇到 unsafe item 直接报错；compact output 只允许明确安全 item |
| stateless replay policy 过窄，丢失 compact 后的有效上下文 | 中 | 明确保留 encrypted reasoning、`compaction`、`context_compaction` 与 assistant message；测试覆盖 compact replacement history |
| `extraBody.store` / `extraBody.include` 被用户依赖 | 中 | 本项目无兼容性负担；通过配置错误把冲突显式暴露，避免运行时隐式坏掉 |
| ChatGPT compact endpoint 返回新 item type | 中 | compact output 对 unknown item 在 stateless 下 drop；stateful 保留；文档注明需要后续扩展 classifier |
| `.store(bool)` 与 `stateMode` 双入口造成混淆 | 低 | `.store(bool)` 仅作为 builder compatibility adapter，文档和 profile 只推荐 `stateMode` |

## Open Questions

无。本计划按以下默认决策执行：`stateless` 为默认模式；`stateful` 显式 opt-in；不引入 `previous_response_id` / Conversations API；compact 请求保持 Codex parity。

## Completion Criteria

- [x] `responses` 和 `chatgpt_responses` profile 都支持 `stateMode`。
- [x] 默认 stateless 不再回放非持久化 `rs_...` id。
- [x] stateful 请求体正确使用 `store=true` 并允许 raw item id replay。
- [x] stateless reasoning 自动包含 encrypted reasoning replay 所需 include。
- [x] compact output 在 stateless 下经过安全过滤。
- [x] schema、examples、README、架构文档与实现一致。
- [x] `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features`、`cargo test --workspace` 全部通过。
