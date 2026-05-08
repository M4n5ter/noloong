# Noloong Agent Core

`noloong-agent-core` is an event-sourced, providerless Rust agent kernel with a stateful agent UX layer.

## Layers

- Kernel: `AgentRuntime`, typed phase graph, `AgentEvent`, `AgentEffect`, reducer, and `EventStore`.
- Native extensions: Rust `ModelProvider`, `ToolProvider`, `ContextProvider`, `PhaseNode`, and `ToolCallHook`.
- Process extensions: newline-delimited JSON-RPC 2.0 over stdio.
- Runtime plugins: `noloong-agent` manifest/profile declarations that start stdio extensions with approval, env isolation, and capability allowlists.
- UX layer: `Agent` with persistent state, subscriptions, `prompt`, `continue_run`, `reset`, `abort`, `wait_for_idle`, steering, and follow-up queues.

Detailed architecture notes live in [`crates/noloong-agent-core/docs/ARCHITECTURE.md`](crates/noloong-agent-core/docs/ARCHITECTURE.md). Extension authoring details live in [`crates/noloong-agent-core/docs/EXTENSIONS.md`](crates/noloong-agent-core/docs/EXTENSIONS.md).

## Build Info Source Snapshot

The root `noloong` binary embeds a build-time source snapshot for immutable host inspection. This lets an agent see the Rust host, product layer, examples, schemas, and docs that were present when the binary was built, even when the original checkout is unavailable.

```bash
noloong build-info manifest
noloong build-info command
noloong build-info source list
noloong build-info source cat Cargo.toml
noloong build-info source extract --output-dir /tmp/noloong-source
noloong build-info source archive --output /tmp/noloong-source.tar.zst
```

The snapshot follows `.gitignore` and always excludes `.git/`. Treat `.gitignore` as the safety boundary before adding local credentials, databases, logs, or other private files to a checkout.

This feature is for understanding and auditing the immutable Rust host behind a binary. It is not the recommended self-improvement path to extract the embedded source, edit it, and rebuild a replacement binary. Noloong should evolve through plugins first: write or update plugin code, reload the extension layer, and keep the Rust host small and stable unless the core contract itself needs to change.

## Diagnostics

The `noloong` binary uses the `log` facade with `env_logger`. The default diagnostic filter is `info`, and `RUST_LOG` overrides it:

```bash
RUST_LOG=noloong=debug cargo run -p noloong -- build-info command
RUST_LOG=warn cargo run -p noloong -- build-info command
```

Diagnostics are written to stderr and do not mix into machine-readable stdout contracts such as profile schema output and build-info output. `noloong-extension-conformance` keeps report output on stdout and CLI errors on stderr so third-party extension test runners can consume it without enabling a logger backend.

Release builds also install `human-panic` for user-friendly crash reports. Set `RUST_BACKTRACE=1` when you need the traditional panic backtrace.

## Examples

```bash
cargo run -p noloong-agent-core --example native_kernel
cargo run -p noloong-agent-core --example stateful_agent
```

Built-in OpenAI-compatible Chat Completions provider:

```rust
use noloong_agent_core::{
    AgentRuntime, ChatCompletionsProvider, ChatCompletionsProviderConfig,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let provider = ChatCompletionsProvider::new(
        ChatCompletionsProviderConfig::new("openai-chat", "gpt-5.5-mini")
            .api_key_env("OPENAI_API_KEY")
            .max_completion_tokens(512),
    )?;

    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(1)
        .build()?;

    let report = runtime.run("Say hello from Chat Completions").await?;
    println!("messages: {}", report.state.messages.len());
    Ok(())
}
```

Provider-specific compatible APIs should be configured by the caller through `base_url`, `api_key_env`, headers, and `extra_body`; the core provider intentionally does not hardcode vendor/model presets. OpenAI Chat Completions uses `max_completion_tokens` for the generated-token upper bound, including visible output and reasoning tokens. Some compatible providers still require their legacy or provider-specific field names, so those overrides should stay in caller-owned `extra_body`.

Built-in OpenAI Responses API provider:

```rust
use noloong_agent_core::{
    AgentRuntime, ResponsesApiProvider, ResponsesApiProviderConfig,
    ResponsesReasoningConfig, ResponsesReasoningEffort,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let provider = ResponsesApiProvider::new(
        ResponsesApiProviderConfig::new("openai-responses", "gpt-5.5-mini")
            .api_key_env("OPENAI_API_KEY")
            .max_output_tokens(1024)
            .reasoning(
                ResponsesReasoningConfig::new()
                    .effort(ResponsesReasoningEffort::Low),
            ),
    )?;

    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(1)
        .build()?;

    let report = runtime.run("Think briefly, then say hello").await?;
    println!("messages: {}", report.state.messages.len());
    Ok(())
}
```

Responses-compatible routers stay caller-owned as well. For example, OpenRouter can be configured with `base_url("https://openrouter.ai/api/v1")`, `api_key_env("OPENROUTER_API_KEY")`, optional headers such as `X-Title`, and provider-specific request fields through `extra_body`; core does not provide an OpenRouter or model preset.

Built-in Anthropic Messages provider:

```rust
use noloong_agent_core::{
    AgentRuntime, AnthropicMessagesProvider, AnthropicMessagesProviderConfig,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let provider = AnthropicMessagesProvider::new(
        AnthropicMessagesProviderConfig::new("anthropic", "claude-sonnet-4-5")
            .api_key_env("ANTHROPIC_API_KEY")
            .max_tokens(2048)
            .enable_thinking(1024),
    )?;

    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(provider))
        .max_turns(1)
        .build()?;

    let report = runtime.run("Think briefly, then say hello").await?;
    println!("messages: {}", report.state.messages.len());
    Ok(())
}
```

Anthropic-compatible routers should also stay caller-owned config. For example, OpenRouter's Anthropic Messages endpoint can use `base_url("https://openrouter.ai/api")`, `api_key_env("OPENROUTER_API_KEY")`, `auth_scheme(AnthropicAuthScheme::Bearer)`, and `without_anthropic_version()` without adding an OpenRouter preset to core.

## ChatGPT Subscription

The root `noloong` binary can use a ChatGPT subscription through the ChatGPT Codex Responses backend. Login writes a local token file at `~/.agents/noloong/chatgpt/token.json` by default. Set `NOLOONG_CHATGPT_TOKEN_FILE` or pass `--token-file` to use another path.

```bash
cargo run -p noloong -- chatgpt login --flow browser
cargo run -p noloong -- chatgpt status
```

The profile example below uses token-file auth by default, uses `gpt-5.4-mini`, and enables Codex compact automatically:

```bash
cargo run -p noloong -- telegram --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

Set `"compaction": {"type": "none"}` in the profile to disable the ChatGPT Codex compact endpoint.

## Profile Config Schema

Root profile config has a checked-in JSON Schema at [`schemas/profile-config.schema.json`](schemas/profile-config.schema.json). Editors can reference it with a `$schema` field:

```json
{
  "$schema": "../../schemas/profile-config.schema.json",
  "profiles": [{
    "profileId": "default",
    "displayName": "Default",
    "provider": {"type": "responses", "model": "gpt-5.5-mini"}
  }]
}
```

Regenerate or check the artifact with the root CLI:

```bash
cargo run -p noloong -- profile-config schema --output schemas/profile-config.schema.json
cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json
```

Profile config loading also supports JSONC for comments and trailing commas; see [`examples/profile-configs/telegram-openrouter-free.jsonc`](examples/profile-configs/telegram-openrouter-free.jsonc). This applies only to root profile config files. JSON-RPC extension protocol messages, model provider payloads, and Telegram API payloads remain strict JSON. Noloong intentionally does not accept JSON5 syntax such as unquoted keys, single-quoted strings, or hexadecimal numbers, because that would widen the public config language beyond the editor-oriented JSONC use case.

## Extension Authoring

The deterministic conformance examples are the fastest way to learn the stdio JSON-RPC extension contract. They do not call a real model; they exist to validate the bridge surface and pass `noloong-extension-conformance --profile strict`.

TypeScript:

```bash
cd examples/extensions/typescript-conformance
npm install
npm run check
npm run conformance
```

Python:

```bash
python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py
cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- python3 examples/extensions/python-conformance/full_conformance_extension.py
```

The TS AI SDK stdio provider example lives in `examples/extensions/ai-sdk-provider`. It is a real provider integration example, not the strict conformance template:

```bash
cd examples/extensions/ai-sdk-provider
npm install
OPENAI_API_KEY=... OPENAI_MODEL=gpt-5.5-mini npm run start
```

The Rust side for launching that provider is:

```bash
cargo run -p noloong-agent-core --example stdio_ai_sdk
```

## Product Plugins

The core JSON-RPC extension bridge is also exposed as a safer product plugin layer in `noloong-agent`. A plugin declaration lives in a profile or session manifest, starts a stdio extension with direct `command + args`, maps only named host environment variables into the child process, and registers only `allowedCapabilities`.

The example profile loads the Python conformance extension as a plugin and allows only its echo tool:

```bash
cargo run -p noloong -- telegram --profile-config examples/profile-configs/plugin-stdio-example.json
```

Agents cannot silently install plugins. They can only propose `register_plugin`, `set_plugin_enabled`, or `remove_plugin` manifest patches through `agent.manifest.propose_patch`; a bridge or human then approves and applies the proposal. Plugin changes take effect on the next runtime build/run, not by hot reloading an already running runtime.

## Session Stores

The root `noloong` profile config has two separate persistence layers. `registryStore` is a host-level session snapshot store for `AgentManifest`, `AgentState`, steering/follow-up queues, profile ids, metadata, and session descriptors. Profile-level `eventStore` is the core run event log for `AgentEvent` replay, tool approval resume, permission audit ordering, and run-level diagnostics.

```json
{
  "registryStore": {"type": "sqlite", "databaseUrl": "sqlite:target/noloong-sessions.sqlite"},
  "profiles": [{
    "profileId": "default",
    "displayName": "Default",
    "provider": {"type": "responses", "model": "gpt-5.5-mini"},
    "eventStore": {"type": "sqlite", "databaseUrl": "sqlite:target/noloong-events.sqlite"}
  }]
}
```

`eventStore` defaults to `{"type":"memory"}`. Use a SQLite file URL, not `sqlite::memory:`, when a profile needs paused approval resume or event replay across process restarts. A persisted event store does not make interrupted `running` sessions continue automatically; they are still marked failed on restore.

## Thinking

Thinking is represented as structured data instead of plain text. `ContentBlock::Thinking` contains a `ThinkingBlock` with a kind, optional display text, optional raw provider payload, optional replay descriptor, and metadata. `ModelStreamEvent::ThinkingDelta` carries a `ThinkingDelta`, so providers can stream visible summaries while preserving JSON/object reasoning details for same-provider replay.

OpenAI-compatible Chat Completions does not define a single standard thinking field. The built-in provider extracts common compatible fields such as `reasoning`, `reasoning_content`, `reasoning_text`, and `reasoning_details`, while provider-specific request parameters stay in caller-owned config.

Anthropic Messages exposes extended thinking as stream events. The built-in provider keeps it off by default, enables it with `enable_thinking(budget_tokens)`, records `thinking_delta` as `ThinkingDelta`, preserves `signature_delta` in metadata/raw snapshots, and only replays prior thinking into assistant history when provider id and model match.

OpenAI Responses exposes reasoning as first-class response items and summary/text deltas. The built-in provider maps reasoning summary text to `ThinkingKind::Summary`, raw reasoning text to `ThinkingKind::Raw`, encrypted reasoning payloads to `ThinkingKind::Encrypted`, and replays prior reasoning only when provider id and model match.

## Media I/O

Messages can carry provider-neutral media blocks. The core stores media as references by default, with inline base64 available for small payloads when the caller already owns encoded data:

```rust
use noloong_agent_core::{
    AgentMessage, ContentBlock, MediaBlock, MediaKind,
};

let user_message = AgentMessage::user("user-1", "Describe this image");
let image = ContentBlock::Media {
    media: MediaBlock::uri(MediaKind::Image, "https://example.test/diagram.png"),
};
```

Tool outputs use the same `Vec<ContentBlock>` surface, so tools can return images, audio, video references, or files without a separate tool-specific media API:

```rust
use noloong_agent_core::{
    ContentBlock, MediaBlock, MediaKind, ToolOutput,
};
use serde_json::Value;

let output = ToolOutput {
    content: vec![ContentBlock::Media {
        media: MediaBlock::provider(MediaKind::File, "openai-chat", "file_123"),
    }],
    details: Value::Null,
    is_error: false,
    updates: Vec::new(),
};
```

The built-in Chat Completions provider maps image URI/inline media to `image_url`, inline WAV/MP3 audio to `input_audio`, video URI/inline media to `video_url`, and provider file references to `file_id`. Provider-hosted video references are passed as `file_id` only when `allow_provider_video_file_media(true)` is explicitly configured. It does not download media URIs or manage blob storage; a future `MediaStore` can be added without changing the message model.

The built-in Responses provider maps image URI/inline/provider media to `input_image` and file URI/provider media to `input_file`. Inline file data is opt-in through `allow_file_data_url_input(true)`. Audio, video, custom media kinds, system media, and assistant media replay fail fast in this provider v1.

The built-in Anthropic Messages provider maps image URI/inline media to `image` blocks and file URI/inline media to `document` blocks. Provider-hosted Anthropic file ids are opt-in through `allow_files_api_media(true)`, which also adds the Files API beta header. Audio, video, custom media kinds, and system media fail fast in this provider v1.

Provider mapping references:

- OpenAI Chat Completions API: <https://platform.openai.com/docs/api-reference/chat/create-chat-completion>
- OpenAI Responses API: <https://platform.openai.com/docs/api-reference/responses/create>
- OpenAI vision guide: <https://platform.openai.com/docs/guides/images-vision?api-mode=chat>
- OpenAI audio guide: <https://platform.openai.com/docs/guides/audio>
- Anthropic Messages examples: <https://docs.anthropic.com/en/api/messages-examples>
- Anthropic Files API: <https://docs.anthropic.com/en/docs/build-with-claude/files>

## Verification

The conformance source of truth is [`crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md`](crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md). Update that matrix whenever a core capability, invariant, or verification command changes.

The product-layer agent runtime lives in [`crates/noloong-agent`](crates/noloong-agent). Its architecture notes are in [`crates/noloong-agent/docs/ARCHITECTURE.md`](crates/noloong-agent/docs/ARCHITECTURE.md). The first product-layer execution primitive is a host-first background command lifecycle with `host.exec.start/read/wait/write/terminate/list`; `host.exec.start` uses a short foreground window before falling back to a background job handle.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p noloong-agent-core --examples
cargo test -p noloong-agent-core --test extension_language_examples
python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py
node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs
node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs
node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs
```

Extension authoring gate:

```bash
cd examples/extensions/typescript-conformance
npm install
npm run check
npm run conformance
```

Manual external gate:

```bash
cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture
cargo test -p noloong-agent-core --test anthropic_live openrouter_anthropic_messages -- --ignored --nocapture
cargo test -p noloong-agent-core --test responses_live -- --ignored --nocapture
```

The OpenRouter live test requires `OPENROUTER_API_KEY`. It routes `deepseek/deepseek-v4-flash` to the official DeepSeek provider with thinking enabled, uses the generic `openrouter/free` router for image input coverage, and uses `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` for audio and video input coverage because the free router currently reports no endpoints for input audio/video. Provider-specific request details are constructed in tests through generic `ChatCompletionsProviderConfig` and `extra_body`, not in core provider code. The manual gate uses larger live output budgets so thinking, visible text, tool-call streaming, and multimodal payload acceptance can be observed. It is intentionally excluded from default CI because it depends on external network access and provider availability.

The Anthropic-compatible live gate uses OpenRouter and requires `OPENROUTER_API_KEY`. The text gate defaults to `openrouter/free` and can be overridden with `NOLOONG_OPENROUTER_ANTHROPIC_LIVE_MODEL`; the tool gate runs only when `NOLOONG_OPENROUTER_ANTHROPIC_TOOL_MODEL` names a tool-capable model. Official Anthropic live tests are present only as explicit opt-in diagnostics and require `NOLOONG_RUN_OFFICIAL_ANTHROPIC_LIVE=1` plus a valid `ANTHROPIC_API_KEY`.

The Responses-compatible live gate also uses OpenRouter and requires only `OPENROUTER_API_KEY`. The text gate defaults to `openrouter/free` and can be overridden with `NOLOONG_OPENROUTER_RESPONSES_LIVE_MODEL`; tool and reasoning gates run only when `NOLOONG_OPENROUTER_RESPONSES_TOOL_MODEL` or `NOLOONG_OPENROUTER_RESPONSES_REASONING_MODEL` names a capable model.

GitHub Actions runs the default local gate on push and pull request. Live provider gates stay manual.
