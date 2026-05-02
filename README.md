# Noloong Agent Core

`noloong-agent-core` is an event-sourced, providerless Rust agent kernel with a stateful agent UX layer.

## Layers

- Kernel: `AgentRuntime`, typed phase graph, `AgentEvent`, `AgentEffect`, reducer, and `EventStore`.
- Native extensions: Rust `ModelProvider`, `ToolProvider`, `ContextProvider`, `PhaseNode`, and `ToolCallHook`.
- Process extensions: newline-delimited JSON-RPC 2.0 over stdio.
- UX layer: `Agent` with persistent state, subscriptions, `prompt`, `continue_run`, `reset`, `abort`, `wait_for_idle`, steering, and follow-up queues.

## Examples

```bash
cargo run -p noloong-agent-core --example native_kernel
cargo run -p noloong-agent-core --example stateful_agent
```

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

## Verification

The conformance source of truth is [`plans/CONFORMANCE_MATRIX.md`](plans/CONFORMANCE_MATRIX.md). Update that matrix whenever a core capability, invariant, or verification command changes.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
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

The OpenRouter live test requires `OPENROUTER_API_KEY` and routes `deepseek/deepseek-v4-flash` to the official DeepSeek provider with reasoning enabled. It is intentionally excluded from default CI because it depends on external network access and provider availability.

GitHub Actions runs the default local gate on push and pull request. The live OpenRouter gate stays manual.
