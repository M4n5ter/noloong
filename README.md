# Noloong Agent Core

`noloong-agent-core` is an event-sourced, providerless Rust agent kernel with a stateful agent UX layer.

## Layers

- Kernel: `AgentRuntime`, typed phase graph, `AgentEvent`, `AgentEffect`, reducer, and `EventStore`.
- Native extensions: Rust `ModelProvider`, `ToolProvider`, `ContextProvider`, `PhaseNode`, and `ToolCallHook`.
- Process extensions: newline-delimited JSON-RPC 2.0 over stdio.
- UX layer: `Agent` with persistent state, subscriptions, `prompt`, `continue_run`, `reset`, `abort`, `wait_for_idle`, steering, and follow-up queues.

Detailed architecture notes live in [`crates/noloong-agent-core/docs/ARCHITECTURE.md`](crates/noloong-agent-core/docs/ARCHITECTURE.md).

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
        ChatCompletionsProviderConfig::new("openai-chat", "gpt-4.1-mini")
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

The TS AI SDK stdio provider example lives in `examples/extensions/ai-sdk-provider`:

```bash
cd examples/extensions/ai-sdk-provider
npm install
OPENAI_API_KEY=... npm run start
```

The Rust side for launching that provider is:

```bash
cargo run -p noloong-agent-core --example stdio_ai_sdk
```

## Thinking

Thinking is represented as structured data instead of plain text. `ContentBlock::Thinking` contains a `ThinkingBlock` with a kind, optional display text, optional raw provider payload, optional replay descriptor, and metadata. `ModelStreamEvent::ThinkingDelta` carries a `ThinkingDelta`, so providers can stream visible summaries while preserving JSON/object reasoning details for same-provider replay.

OpenAI-compatible Chat Completions does not define a single standard thinking field. The built-in provider extracts common compatible fields such as `reasoning`, `reasoning_content`, `reasoning_text`, and `reasoning_details`, while provider-specific request parameters stay in caller-owned config.

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

Provider mapping references:

- OpenAI Chat Completions API: <https://platform.openai.com/docs/api-reference/chat/create-chat-completion>
- OpenAI vision guide: <https://platform.openai.com/docs/guides/images-vision?api-mode=chat>
- OpenAI audio guide: <https://platform.openai.com/docs/guides/audio>
- Anthropic Messages examples: <https://docs.anthropic.com/en/api/messages-examples>
- Anthropic Files API: <https://docs.anthropic.com/en/docs/build-with-claude/files>

## Verification

The conformance source of truth is [`plans/CONFORMANCE_MATRIX.md`](plans/CONFORMANCE_MATRIX.md). Update that matrix whenever a core capability, invariant, or verification command changes.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p noloong-agent-core --examples
node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs
node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs
node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs
```

Manual external gate:

```bash
cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture
```

The OpenRouter live test requires `OPENROUTER_API_KEY`. It routes `deepseek/deepseek-v4-flash` to the official DeepSeek provider with thinking enabled, uses the generic `openrouter/free` router for image input coverage, and uses `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` for audio and video input coverage because the free router currently reports no endpoints for input audio/video. Provider-specific request details are constructed in tests through generic `ChatCompletionsProviderConfig` and `extra_body`, not in core provider code. The manual gate uses larger live output budgets so thinking, visible text, tool-call streaming, and multimodal payload acceptance can be observed. It is intentionally excluded from default CI because it depends on external network access and provider availability.

GitHub Actions runs the default local gate on push and pull request. The live OpenRouter gate stays manual.
