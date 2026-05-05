# 实施计划：ChatGPT 订阅与 OpenAI Provider Integration

## 概览

为 Noloong 增加 ChatGPT 订阅登录、认证刷新、OpenAI-compatible provider 装配，以及可选的 OpenAI `/responses/compact` 接入。最重要的边界是：`noloong-agent-core` 只提供通用扩展 substrate；OpenAI/ChatGPT 私有协议、OAuth、Codex backend URL 和 compact endpoint 全部放在 core 外的 OpenAI integration crate 中。

最终形态：

- `noloong-agent-core`：定义 `HttpAuthProvider`、replacement compaction、JSON-RPC extension wire contract 和 built-in Responses/Chat Completions provider 的通用接入点。
- `noloong-openai`：实现 ChatGPT OAuth/device-code login、token storage/refresh、ChatGPT-backed Responses provider helper、OpenAI `/responses/compact` compactor。
- `noloong-agent`：只做可选 product wiring，不承载 core abstraction，也不把 OpenAI 私有协议变成默认 agent 行为。
- 第三方 TS/Python 扩展：可以通过 JSON-RPC 实现同等的 auth provider 和 context compactor。

## 当前实施状态

- 已完成 Phase 1 的 core HTTP auth substrate：Rust trait、built-in provider 接入、结构化 HTTP status、401 refresh/retry、JSON-RPC `http_auth_provider` adapter 和 TS/Python conformance 示例。
- 已完成 Phase 2 的 core compaction substrate：replacement-capable `ContextCompactor`、provider payload block、Responses replay、unsupported provider rejection、JSON-RPC `context_compactor` adapter、strict conformance 更新，以及 public Responses request rendering helper。
- 已完成 Phase 3 的 `noloong-openai` integration crate：token/storage、browser OAuth primitives、device-code flow、refresh/revoke `HttpAuthProvider`、ChatGPT Responses provider helper、OpenAI `/responses/compact` compactor 和 mock endpoint 测试。
- 已完成 Phase 4 的 product wiring、文档补全和 gated live ChatGPT subscription smoke tests；真实 browser/device-code 认证由调用方展示链接或 code 后等待用户完成。已分别通过 device-code 与 browser OAuth 生成本地 ChatGPT subscription token，并跑通 `gpt-5.4-mini` 的 Responses streaming 与 `/responses/compact` smoke。

## 架构决策

- 新增 crate 使用 `crates/noloong-openai`，而不是 `noloong-openai-auth`；auth 只是 OpenAI integration 的一部分，compact endpoint 也属于同一 provider integration 边界。
- `noloong-agent-core` 不出现 `chatgpt.com/backend-api/codex`、`/responses/compact`、`ChatGPT-Account-ID`、OAuth client id、keyring storage 等私有实现细节。
- `noloong-agent-core` 的 auth 和 compaction 扩展必须同时支持 Rust trait 与 stdio JSON-RPC extension，不能成为 Rust-only API。
- built-in `ResponsesApiProvider` 和 `ChatCompletionsProvider` 只消费通用 `HttpAuthProvider`，不关心 token 来源。
- OpenAI compact 不替换默认 compaction；它只是一个可注册的 `ContextCompactor` 实现。
- replacement compaction 使用 provider-neutral message representation；OpenAI raw item 通过通用 provider payload 承载，只有 Responses provider 负责 replay。
- 不硬编码模型名；ChatGPT subscription provider helper 只设置 base URL 和 auth。
- v1 不实现 Agent Identity auth、connector auth、Responses websocket，也不自动导入 Codex CLI keyring/auth store。

## 任务列表

### Phase 1：Core HTTP Auth Substrate

#### Task 1：定义 provider-neutral `HttpAuthProvider`

**描述：** 在 core 中新增 HTTP auth 抽象，让模型 provider 每次请求前获取 headers，并在认证失败时触发刷新。该抽象必须能被 Rust crate 和 JSON-RPC extension 共同实现。

**验收标准：**

- [x] 新增 `HttpAuthProvider` trait，支持 async `headers` 和 `refresh`。
- [x] request/response 类型只使用 serde-friendly plain data，不暴露 `reqwest::HeaderMap` 作为 public contract。
- [x] auth context 至少包含 provider id、HTTP method、URL、attempt index、metadata。
- [x] refresh reason 至少支持 `unauthorized` 和 `proactive`。
- [x] trait 不引用 OpenAI、ChatGPT、Codex、OAuth、keyring。

**验证：**

- [x] 新增 trait-level mock tests。
- [x] `cargo test -p noloong-agent-core`
- [x] `cargo clippy -p noloong-agent-core --all-targets --all-features -- -D warnings`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent-core/src/providers.rs`
- `crates/noloong-agent-core/src/provider_utils.rs`
- `crates/noloong-agent-core/src/error.rs`

**预计范围：** 中

#### Task 2：将 built-in providers 接入 `HttpAuthProvider`

**描述：** 让 `ResponsesApiProvider` 和 `ChatCompletionsProvider` 消费通用 auth provider，同时保留现有 API key/env 入口。

**验收标准：**

- [x] `ResponsesApiProviderConfig` 支持 `.auth_provider(...)`。
- [x] `ChatCompletionsProviderConfig` 支持 `.auth_provider(...)`。
- [x] 显式 `auth_provider` 优先于 `api_key` / `api_key_env`。
- [x] 未配置 `auth_provider` 时，现有 API key/env 行为不变。
- [x] `Debug`、错误文本和测试快照不泄漏 token。

**验证：**

- [x] header injection 测试。
- [x] auth provider 优先级测试。
- [x] API key/env 回归测试。
- [x] `cargo test -p noloong-agent-core chat_completions`
- [x] `cargo test -p noloong-agent-core responses`

**依赖：** Task 1

**可能涉及文件：**

- `crates/noloong-agent-core/src/chat_completions.rs`
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/chat_completions.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**预计范围：** 中

#### Task 3：实现结构化 HTTP status 与 401 retry

**描述：** 让 SSE 和 unary 请求能区分认证失败、可重试传输错误和普通 provider 错误，避免靠字符串解析错误。

**验收标准：**

- [x] 非 2xx 响应保留 status code 和有限 body preview。
- [x] 401 调用 `HttpAuthProvider::refresh` 后最多重试一次。
- [x] 非 401 的 4xx 不触发 auth refresh。
- [x] 已收到 SSE data 后不自动重放请求。
- [x] stream reconnect 仍只处理未收到 data 前的可重连失败。

**验证：**

- [x] 401 refresh success 测试。
- [x] repeated 401 fail 测试。
- [x] 400/403 不 refresh 测试。
- [x] SSE data delivered 后断流不重放测试。
- [x] `cargo test -p noloong-agent-core`

**依赖：** Task 1、Task 2

**可能涉及文件：**

- `crates/noloong-agent-core/src/sse.rs`
- `crates/noloong-agent-core/src/provider_utils.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**预计范围：** 中

#### Task 4：新增 JSON-RPC auth provider extension

**描述：** 将 `HttpAuthProvider` 暴露为标准 stdio JSON-RPC capability，使 TS/Python 等非 Rust 扩展也能提供动态 headers 和 refresh。

**验收标准：**

- [x] 新增 `ExtensionCapability::HttpAuthProvider { id }`。
- [x] 新增 `StdioHttpAuthProvider` adapter。
- [x] `auth/headers` 输入为 `{ authProviderId, context }`。
- [x] `auth/headers` 输出为 `{ headers: [{ name, value }], metadata }`。
- [x] `auth/refresh` 输入为 `{ authProviderId, context, reason }`。
- [x] `auth/refresh` 输出为 `{ retry, headers?, metadata }`；未返回 headers 时 host 重试前重新调用 `auth/headers`。
- [x] header name/value 使用统一校验；非法 header 使当前请求失败。
- [x] conformance 覆盖正常 headers、401 refresh success、refresh denied、malformed result、duplicate capability id。

**验证：**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [x] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [x] TypeScript/Python fixtures 增加 auth provider conformance case。

**依赖：** Task 1、Task 3

**可能涉及文件：**

- `crates/noloong-agent-core/src/types/extension.rs`
- `crates/noloong-agent-core/src/jsonrpc/adapters.rs`
- `crates/noloong-agent-core/docs/EXTENSIONS.md`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**预计范围：** 中

### Checkpoint：Core Auth

- [x] API key/env provider 行为保持通过。
- [x] Rust 和 JSON-RPC 两种 auth provider 均可接入 built-in providers。
- [x] 401 refresh/retry 有结构化测试。
- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent-core`

### Phase 2：Core Compaction Substrate

#### Task 5：引入 replacement-capable `ContextCompactor`

**描述：** 将 core compaction 从“只能返回 summary text”扩展为“可返回 summary 或 replacement history”，但保留现有 `CompactionSummarizer` 的兼容 adapter。

**验收标准：**

- [x] 新增 `ContextCompactor` 或等价 trait。
- [x] compaction output 支持 `Summary` 和 `Replacement`。
- [x] 现有 `CompactionSummarizer` 通过 adapter 继续可用。
- [x] 默认 runtime 仍使用当前 summary compaction。
- [x] `PersistentState` 和 `RequestOnly` 模式都能处理 replacement。

**验证：**

- [x] 现有 compaction 测试保持通过。
- [x] 新增 replacement compaction reducer/runtime 测试。
- [x] `cargo test -p noloong-agent-core --test compaction`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent-core/src/compaction.rs`
- `crates/noloong-agent-core/src/phase/standard.rs`
- `crates/noloong-agent-core/src/reducer.rs`
- `crates/noloong-agent-core/tests/compaction.rs`

**预计范围：** 中

#### Task 6：新增 provider-neutral payload block

**描述：** 为 provider-specific history replay 提供通用 payload 承载方式，使 OpenAI compact 返回的 raw Responses item 不污染 core 语义。

**验收标准：**

- [x] 新增通用 provider payload block，字段至少包含 `provider`、`kind`、`value`。
- [x] event store serde roundtrip 保留 raw JSON。
- [x] `ResponsesApiProvider` 能 replay `provider = "openai.responses"` 且 `kind = "response_item"` 的 payload。
- [x] Chat Completions、Anthropic 和其它不支持的 provider 遇到 payload 时返回清晰错误。
- [x] core 文档明确 payload 是 provider-owned data，不是通用 agent message 语义。

**验证：**

- [x] provider payload serde roundtrip 测试。
- [x] Responses replay 测试。
- [x] unsupported provider rejection 测试。
- [x] `cargo test -p noloong-agent-core responses`

**依赖：** Task 5

**可能涉及文件：**

- `crates/noloong-agent-core/src/types/messages.rs`
- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `crates/noloong-agent-core/tests/responses.rs`

**预计范围：** 中

#### Task 7：新增 JSON-RPC context compactor extension

**描述：** 让非 Rust 扩展也能实现 replacement-capable compaction，而不仅仅是现有 summary summarizer。

**验收标准：**

- [x] 新增 `ExtensionCapability::ContextCompactor { id }`。
- [x] 新增 `StdioContextCompactor` adapter。
- [x] `compaction/compact` 输入复用 core compaction request，并包含 compactor id。
- [x] `compaction/compact` 输出支持 summary 和 replacement 两种结果。
- [x] replacement messages 支持 provider payload block。
- [x] conformance 覆盖 summary、replacement、malformed result、duplicate capability id。

**验证：**

- [x] `cargo test -p noloong-agent-core --test jsonrpc_conformance`
- [x] TypeScript/Python fixtures 增加 context compactor case。
- [x] `cargo test -p noloong-agent-core --test extension_docs_contract`

**依赖：** Task 5、Task 6

**可能涉及文件：**

- `crates/noloong-agent-core/src/types/extension.rs`
- `crates/noloong-agent-core/src/jsonrpc/adapters.rs`
- `crates/noloong-agent-core/docs/EXTENSIONS.md`
- `crates/noloong-agent-core/tests/jsonrpc_conformance.rs`

**预计范围：** 中

#### Task 8：提取 Responses request rendering helper

**描述：** 将 `ResponsesApiProvider` 内部的 request rendering 提取为可复用但不发 HTTP 的 public helper，供 core 外的 OpenAI integration crate 构造 compact payload。

**验收标准：**

- [x] helper 只负责 Responses wire serialization，不知道 `/responses/compact`。
- [x] `ResponsesApiProvider` 自身使用同一 helper，避免重复渲染逻辑。
- [x] helper 能渲染 messages、tools、reasoning、text controls 和 provider payload。
- [x] public API 不暴露 reqwest client 或 endpoint URL。

**验证：**

- [x] 现有 Responses payload tests 迁移到 helper。
- [x] helper 单元测试覆盖 tool call、tool result、reasoning、provider payload。
- [x] `cargo test -p noloong-agent-core responses`

**依赖：** Task 6

**可能涉及文件：**

- `crates/noloong-agent-core/src/responses.rs`
- `crates/noloong-agent-core/tests/responses.rs`

**预计范围：** 中

### Checkpoint：Core Compaction

- [x] core 能表达 replacement compaction，但不包含 OpenAI compact endpoint 实现。
- [x] Rust 和 JSON-RPC 两种 context compactor 都可用。
- [x] provider payload 可被 Responses replay，其他 provider 清晰拒绝。
- [x] `cargo test -p noloong-agent-core`

### Phase 3：OpenAI Integration Crate

#### Task 9：新增 `noloong-openai` crate

**描述：** 新增独立 crate 承载 OpenAI/ChatGPT integration，包括 auth、provider helper 和 compact compactor。

**验收标准：**

- [x] workspace members 包含 `crates/noloong-openai`。
- [x] crate 暴露 `auth`、`provider`、`compact` 模块。
- [x] crate 依赖 `noloong-agent-core`，core 不依赖该 crate。
- [x] crate 使用 Rust 2024 edition 和 workspace lints。

**验证：**

- [x] `cargo test -p noloong-openai`
- [x] `cargo clippy -p noloong-openai --all-targets --all-features -- -D warnings`

**依赖：** Task 1、Task 5

**可能涉及文件：**

- `Cargo.toml`
- `crates/noloong-openai/Cargo.toml`
- `crates/noloong-openai/src/lib.rs`

**预计范围：** 小

#### Task 10：实现 ChatGPT token model 与 storage

**描述：** 建模 ChatGPT token 数据，解析 JWT claims，并实现安全 storage。

**验收标准：**

- [x] `ChatGptTokenData` 包含 `id_token`、`access_token`、`refresh_token`、`account_id`、`last_refresh`。
- [x] 解析 email、plan type、ChatGPT user id、account id、FedRAMP flag 和 JWT `exp`。
- [x] storage 支持 `File`、`Keyring`、`Auto`、`Ephemeral`。
- [x] Unix file storage 权限为 `0600`。
- [x] token 不出现在 `Debug` 输出。

**验证：**

- [x] JWT claim parsing 测试。
- [x] storage save/load/delete 测试。
- [x] Auto fallback fake keyring 测试。
- [x] `cargo test -p noloong-openai auth`

**依赖：** Task 9

**可能涉及文件：**

- `crates/noloong-openai/src/auth/token.rs`
- `crates/noloong-openai/src/auth/storage.rs`
- `crates/noloong-openai/tests/auth_token.rs`
- `crates/noloong-openai/tests/auth_storage.rs`

**预计范围：** 中

#### Task 11：实现 browser OAuth 与 device-code login

**描述：** 实现完整 ChatGPT 登录流程，包括 PKCE browser callback 和 headless device-code login。

**验收标准：**

- [x] 默认 issuer 为 `https://auth.openai.com`。
- [x] browser login 使用 PKCE、state 校验、本地 callback server。
- [x] callback port 默认 `1455`，冲突 fallback 到 `1457`。
- [x] authorize URL 包含 `openid profile email offline_access api.connectors.read api.connectors.invoke` scope。
- [x] device-code login 暴露 verification URL、user code、polling state。
- [x] token exchange 成功后保存 token data。

**验证：**

- [x] authorize URL 构造测试。
- [x] state mismatch 测试。
- [x] mock token exchange 测试。
- [x] device-code pending/success/timeout 测试。
- [x] `cargo test -p noloong-openai login`

**依赖：** Task 10

**可能涉及文件：**

- `crates/noloong-openai/src/auth/login/browser.rs`
- `crates/noloong-openai/src/auth/login/device.rs`
- `crates/noloong-openai/src/auth/pkce.rs`
- `crates/noloong-openai/tests/login.rs`

**预计范围：** 中

#### Task 12：实现 refresh、revoke 和 `HttpAuthProvider`

**描述：** 实现 ChatGPT token 刷新、logout revoke，并让 `ChatGptAuthManager` 实现 core `HttpAuthProvider`。

**验收标准：**

- [x] access token 过期或 `last_refresh` 超过 8 天时主动 refresh。
- [x] 401 后 refresh 并允许 provider 重试一次。
- [x] refresh 成功后更新 access token、refresh token 和 `last_refresh`。
- [x] refresh token 永久失效时返回需要重新登录的错误。
- [x] headers 包含 Bearer token、optional account id 和 FedRAMP flag。
- [x] logout 可选择调用 revoke endpoint。

**验证：**

- [x] proactive refresh 测试。
- [x] unauthorized refresh 测试。
- [x] permanent refresh failure 测试。
- [x] header injection 测试。
- [x] revoke mock 测试。

**依赖：** Task 10、Task 11

**可能涉及文件：**

- `crates/noloong-openai/src/auth/manager.rs`
- `crates/noloong-openai/src/auth/refresh.rs`
- `crates/noloong-openai/src/auth/http_auth.rs`
- `crates/noloong-openai/tests/auth_refresh.rs`

**预计范围：** 中

#### Task 13：提供 ChatGPT Responses provider helper

**描述：** 提供小型 helper，让调用方用 ChatGPT auth manager 组装 `ResponsesApiProviderConfig`。

**验收标准：**

- [x] 默认 base URL 为 `https://chatgpt.com/backend-api/codex`。
- [x] helper 接受调用方提供的 model。
- [x] helper 注入 `ChatGptAuthManager` 作为 `HttpAuthProvider`。
- [x] helper 不阻止调用方改用 JSON-RPC auth provider。
- [x] 不硬编码任何模型名。

**验证：**

- [x] config helper 单元测试。
- [x] request path 测试确认最终路径为 `/responses`。
- [x] `cargo test -p noloong-openai provider`

**依赖：** Task 2、Task 12

**可能涉及文件：**

- `crates/noloong-openai/src/provider.rs`
- `crates/noloong-openai/tests/provider.rs`

**预计范围：** 小

#### Task 14：实现 OpenAI `/responses/compact` compactor

**描述：** 在 `noloong-openai` 中实现 OpenAI compact endpoint 的 `ContextCompactor`，不把 endpoint 实现写入 core。

**验收标准：**

- [x] POST `{base_url}/responses/compact`。
- [x] payload 使用 core 的 Responses rendering helper。
- [x] payload 包含 model、input、可选 instructions、tools、parallel tool calls、reasoning、text controls。
- [x] 使用同一个 `HttpAuthProvider` 注入 ChatGPT headers。
- [x] 返回的 `output` 转为 replacement history，raw Responses item 使用 provider payload block。
- [x] 失败时不提交 compaction effect。
- [x] compactor 可由 Rust 注册，也可由 JSON-RPC extension 提供等价实现。

**验证：**

- [x] mock `/responses/compact` payload 测试。
- [x] replacement history 转换测试。
- [x] 401 refresh 后 compact 成功测试。
- [x] 4xx/5xx 不提交 effect 测试。
- [x] `cargo test -p noloong-openai compact`

**依赖：** Task 8、Task 12、Task 13

**可能涉及文件：**

- `crates/noloong-openai/src/compact.rs`
- `crates/noloong-openai/tests/compact.rs`

**预计范围：** 中

### Checkpoint：OpenAI Integration

- [x] ChatGPT auth manager 可作为 `HttpAuthProvider` 注入 core provider。
- [x] ChatGPT Responses helper 不硬编码模型名。
- [x] OpenAI compact 实现在 core 外。
- [x] `cargo test -p noloong-openai`
- [x] `cargo clippy -p noloong-openai --all-targets --all-features -- -D warnings`

### Phase 4：Product Wiring 与文档

#### Task 15：在 `noloong-agent` 增加可选装配

**描述：** 在 product runtime 层提供可选 OpenAI integration 装配入口，让使用者可以启用 ChatGPT auth provider 和 OpenAI compact compactor。

**验收标准：**

- [x] `noloong-agent` 可选择性依赖或 feature-gate `noloong-openai`。
- [x] 默认 agent 行为不自动启用 ChatGPT login 或 OpenAI compact。
- [x] builder/config 可显式注册 ChatGPT-backed Responses provider。
- [x] builder/config 可显式注册 OpenAI compact compactor。
- [x] JSON-RPC auth provider 和 JSON-RPC context compactor 能走同一注册路径。

**验证：**

- [x] builder wiring 单元测试。
- [x] 默认不开启 OpenAI integration 的回归测试。
- [x] `cargo test -p noloong-agent`

**依赖：** Task 13、Task 14

**可能涉及文件：**

- `crates/noloong-agent/Cargo.toml`
- `crates/noloong-agent/src/session.rs`
- `crates/noloong-agent/tests/session.rs`

**预计范围：** 中

#### Task 16：更新架构与扩展文档

**描述：** 说明 core、OpenAI integration crate、product wiring、第三方扩展之间的边界和使用方式。

**验收标准：**

- [x] `ARCHITECTURE.md` 说明 core 不拥有 OpenAI 私有协议。
- [x] `EXTENSIONS.md` 记录 `auth/headers`、`auth/refresh`、replacement compactor wire contract。
- [x] `noloong-openai` README 给出 browser login、device-code login、reuse stored token、Responses provider、compact compactor 示例。
- [x] TS/Python 示例展示最小 auth provider 和 context compactor。
- [x] 文档说明 live tests 验证 ChatGPT subscription，不能用 OpenRouter 替代。

**验证：**

- [x] `cargo test -p noloong-agent-core --test extension_docs_contract`
- [x] `rg -n "HttpAuthProvider|auth/headers|ContextCompactor|responses/compact|noloong-openai" crates`
- [x] 人工阅读确认扩展作者无需读 Rust 源码即可实现 auth/compaction 扩展。

**依赖：** Task 4、Task 7、Task 14、Task 15

**可能涉及文件：**

- `crates/noloong-agent-core/docs/ARCHITECTURE.md`
- `crates/noloong-agent-core/docs/EXTENSIONS.md`
- `crates/noloong-openai/README.md`
- `examples/extensions/typescript-conformance`
- `examples/extensions/python-conformance`

**预计范围：** 中

#### Task 17：新增 gated live tests

**描述：** 增加显式启用的真实 ChatGPT subscription smoke tests，验证登录状态、Responses streaming 和 compact endpoint。

**验收标准：**

- [x] live tests 默认跳过，必须通过环境变量显式启用。
- [x] live tests 使用本地 ChatGPT auth storage 或显式 test token。
- [x] live tests 不使用 OpenRouter。
- [x] 测试失败输出不包含 token。

**验证：**

- [x] 未设置 live env 时测试跳过。
- [x] 在具备真实 ChatGPT login state 的机器上运行 Responses streaming smoke。
- [x] 在具备真实 ChatGPT login state 的机器上运行 `/responses/compact` smoke。

**依赖：** Task 12、Task 13、Task 14

**可能涉及文件：**

- `crates/noloong-openai/tests/live_chatgpt.rs`
- `crates/noloong-agent-core/tests/responses_live.rs`

**预计范围：** 小

#### Task 18：全 workspace 验证

**描述：** 执行最终格式化、测试和 clippy，确保新增 core substrate、OpenAI integration 和 product wiring 一起通过。

**验收标准：**

- [x] `cargo fmt --check` 通过。
- [x] `cargo test --workspace` 通过。
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过。
- [x] 无 `#[allow(dead_code)]`。
- [x] 文档、public API 和 examples 命名一致。

**验证：**

- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

**依赖：** 所有实现任务

**可能涉及文件：**

- Workspace 内所有相关 crate

**预计范围：** 小

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---:|---|
| ChatGPT/Codex backend 属于私有协议，未来可能变化 | 高 | 私有实现只在 `noloong-openai`；core 只保留通用 trait 和 JSON-RPC contract。 |
| core substrate 被 OpenAI 需求污染 | 高 | core 类型命名保持 provider-neutral；OpenAI URL/header/endpoint 只允许出现在 `noloong-openai`。 |
| auth provider 变成 Rust-only | 高 | JSON-RPC `HttpAuthProvider` 是同一计划的一等任务，并有 TS/Python conformance。 |
| replacement compaction 破坏现有 summary compaction | 中 | `CompactionSummarizer` adapter 保持默认路径；replacement 是 opt-in。 |
| provider payload 被误当通用消息语义 | 中 | payload 明确标记 provider/kind；不支持的 provider 必须清晰拒绝。 |
| OAuth token 泄漏 | 高 | redaction、`0600` file storage、keyring 优先、错误文本审计和测试覆盖。 |
| live tests 不稳定 | 中 | 默认跳过；mock tests 覆盖协议行为；live tests 只做真实 provider smoke。 |

## 可并行工作

- Task 2 和 Task 4 可在 Task 1 的 public auth types 稳定后并行。
- Task 6 和 Task 7 可在 Task 5 的 compaction result contract 稳定后并行。
- Task 10 和 Task 11 可在 Task 9 后并行。
- Task 15 可在 Task 13、Task 14 public API 稳定后与 Task 16 并行。
- Task 17 依赖真实登录条件，可以在 mock coverage 完成后单独执行。

## 不做事项

- 不把 ChatGPT OAuth login 放进 `noloong-agent-core`。
- 不把 OpenAI `/responses/compact` endpoint 实现放进 `noloong-agent-core`。
- 不把 `HttpAuthProvider` 或 `ContextCompactor` 设计成 Rust-only。
- 不硬编码任何具体模型名。
- 不自动复用或迁移 Codex CLI 的 keyring/auth store。
- 不在 v1 实现 Agent Identity auth、connector auth 或 Responses websocket。
- 不让 OpenRouter live tests 代表 ChatGPT subscription auth 测试。

## 未决问题

- 无。默认按上述边界实施。
