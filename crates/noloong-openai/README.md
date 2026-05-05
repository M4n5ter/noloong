# noloong-openai

`noloong-openai` 是 Noloong 的 OpenAI / ChatGPT integration crate。它不属于 `noloong-agent-core`，也不改变 core 的 provider-neutral 设计边界。

## 边界

- ChatGPT OAuth、device-code login、token refresh/revoke、Codex backend URL 和 `/responses/compact` 都放在本 crate。
- `noloong-agent-core` 只暴露 `HttpAuthProvider`、Responses request rendering helper、`ContextCompactor` 和 JSON-RPC wire contract。
- `noloong-agent` 只在启用 `openai` feature 时提供薄的 product wiring helper；默认不会自动启用 ChatGPT login 或 OpenAI compact。
- 非 Rust 扩展可以通过 JSON-RPC 实现同等的 `http_auth_provider` 和 `context_compactor`，不需要链接本 crate。

实现细节按 OpenAI Codex 当前实现对齐：OAuth client id、PKCE S256、browser callback ports、device-code endpoints、token refresh/revoke endpoints、ChatGPT account/FedRAMP headers，以及 compact path `responses/compact`。

## Browser Login

调用方负责把 `authorization_url` 展示给用户；library 只绑定本地 callback server 并等待回调。
Browser flow 使用与官方 Codex 对齐的 `http://localhost:{port}/auth/callback` redirect URI。

```rust
use noloong_openai::auth::{
    BrowserLoginServer, ChatGptFileTokenStorage, ChatGptLoginConfig, complete_browser_login,
};

let config = ChatGptLoginConfig::new();
let server = BrowserLoginServer::bind(config.clone()).await?;
println!("Open this URL: {}", server.session().authorization_url);

let session = server.session().clone();
let callback = server.wait_for_callback().await?;
let storage = ChatGptFileTokenStorage::new("/path/to/chatgpt-token.json");
let token = complete_browser_login(
    &reqwest::Client::new(),
    &config,
    &session,
    callback,
    &storage,
)
.await?;
```

本 crate 也提供 browser flow example：

```bash
cargo run -p noloong-openai --example chatgpt_browser_login -- /path/to/chatgpt-token.json
```

## Device-Code Login

适合 headless 或远程环境。调用方展示 verification URL 和 user code，然后轮询直到用户完成认证或超时。

本 crate 也提供一个 example，便于手动生成 live test token file：

```bash
cargo run -p noloong-openai --example chatgpt_device_login -- /path/to/chatgpt-token.json
```

```rust
use noloong_openai::auth::{
    ChatGptFileTokenStorage, ChatGptLoginConfig, complete_device_authorization,
    request_device_authorization,
};

let client = reqwest::Client::new();
let config = ChatGptLoginConfig::new();
let device = request_device_authorization(&client, &config).await?;

println!("Open: {}", device.verification_url);
println!("Code: {}", device.user_code);

let storage = ChatGptFileTokenStorage::new("/path/to/chatgpt-token.json");
let token = complete_device_authorization(&client, &config, device, &storage).await?;
```

## Reuse Stored Tokens

`ChatGptAuthManager` 实现了 core 的 `HttpAuthProvider`。它会在 access token 过期或本地刷新时间超过阈值时主动 refresh，并在 401 后让 built-in HTTP providers 重试一次。

```rust
use noloong_openai::auth::{ChatGptAuthManager, ChatGptTokenStorage};
use std::sync::Arc;

let storage = Arc::new(ChatGptTokenStorage::file("/path/to/chatgpt-token.json"));
let auth = Arc::new(ChatGptAuthManager::new(storage));
let headers = auth.auth_headers().await?;
```

## ChatGPT Responses Provider

Provider helper 不硬编码模型；调用方显式传入 provider id、model 和 auth provider。

```rust
use noloong_openai::provider::chatgpt_responses_provider;
use std::sync::Arc;

let provider = chatgpt_responses_provider(
    "chatgpt-responses",
    "<model-id>",
    auth.clone(),
)?;
```

在 `noloong-agent` product crate 中，启用 `openai` feature 后可以使用薄的 builder helper：

```rust
use noloong_agent::AgentSession;

let runtime = AgentSession::builder()
    .build()
    .runtime_builder()
    .with_chatgpt_responses_provider("chatgpt-responses", "<model-id>", auth.clone())?
    .build()?;
```

## Responses Compact

`OpenAiResponsesCompactor` 实现 core 的 `ContextCompactor`，调用 ChatGPT Codex backend 的 `responses/compact`，并把返回的 raw Responses item 保存为 provider payload replacement history。

```rust
use noloong_agent_core::ContextCompactionConfig;
use noloong_openai::compact::{OpenAiResponsesCompactor, OpenAiResponsesCompactorConfig};
use std::sync::Arc;

let compactor = OpenAiResponsesCompactor::new(
    OpenAiResponsesCompactorConfig::new("openai-compact", "<model-id>")
        .auth_provider(auth.clone()),
)?;

let runtime = AgentSession::builder()
    .build()
    .runtime_builder()
    .with_context_compactor(
        ContextCompactionConfig::new(128_000),
        Arc::new(compactor),
    )
    .with_chatgpt_responses_provider("chatgpt-responses", "<model-id>", auth.clone())?
    .build()?;
```

`noloong-agent` 的 `openai` feature 也提供了 `with_openai_responses_compactor(...)` helper。该 helper 只是装配入口；是否启用 compact、使用哪个模型、何时登录，都由调用方显式决定。

## Live Tests

真实 ChatGPT subscription 测试不能用 OpenRouter 代替。默认情况下 live tests 被 `#[ignore]` 和环境变量双重 gate。

```text
NOLOONG_OPENAI_LIVE_CHATGPT=1 \
NOLOONG_CHATGPT_LIVE_MODEL=<model-id> \
NOLOONG_CHATGPT_TOKEN_FILE=/path/to/chatgpt-token.json \
cargo test -p noloong-openai --test live_chatgpt -- --ignored
```

也可以不使用 token file，改用显式 token 环境变量：

```text
NOLOONG_CHATGPT_ID_TOKEN=...
NOLOONG_CHATGPT_ACCESS_TOKEN=...
NOLOONG_CHATGPT_REFRESH_TOKEN=...
NOLOONG_CHATGPT_ACCOUNT_ID=...
```
