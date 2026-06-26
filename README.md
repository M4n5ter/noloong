# Noloong

English | [简体中文](README_ZH.md)

**Noloong is an early-stage, radically extensible agent runtime for building AI
systems that can replace providers, tools, phases, hooks, context, compaction,
auth, interaction bridges, and product behavior without turning the core into a
fragile black box.**

Noloong is still young. Expect fast-moving internals, incomplete polish, and
occasional breaking changes while the project searches for the right product
shape. The direction is deliberate: keep a small event-sourced Rust kernel
reliable, move evolution into typed extension boundaries, and make it realistic
for an AI agent to iterate on its own surrounding architecture while the core
runtime remains auditable and recoverable.

## Why Noloong Exists

Most agent systems collapse too many concerns into one opaque loop: model calls,
tool execution, approvals, UI state, persistence, provider quirks, and product
policy all blur together. Noloong takes the opposite path.

It treats the agent runtime as a set of replaceable contracts. A model provider
can come from Rust, TypeScript, Python, or another process. A tool, phase node,
phase hook, tool hook, context provider, compactor, or auth provider can be
swapped through the same runtime semantics. Product behavior evolves through
manifests and plugins instead of hardcoded branching.

The goal is not to make another chatbot. The goal is to make an agent substrate
that can keep changing itself at the edges while preserving the properties that
matter: event replay, structured failures, approval audit trails, typed state
transitions, and a core that does not need to understand every future component
in advance.

## What Is Inside

- **A providerless Rust kernel** with event-sourced execution, typed phases,
  reducers, effect validation, replayable state, and structured run failure
  handling.
- **A replaceable phase graph** where providers, tools, context providers, phase
  nodes, phase hooks, tool hooks, compactors, and auth providers share one
  runtime contract.
- **A cross-language extension bridge** over stdio JSON-RPC, with TypeScript and
  Python conformance examples and a public conformance runner.
- **A product runtime** with sessions, manifests, approvals, persistence,
  follow-up queues, steering, background jobs, subagents, and local execution
  tools.
- **A macOS desktop app** built with Tauri and React, focused on reading,
  editing, approvals, and configuration.
- **Provider integrations** for OpenAI-compatible Chat Completions, OpenAI
  Responses, ChatGPT subscription auth, and Anthropic Messages.
- **Provider-neutral content models** for thinking, reasoning, media, tool calls,
  provider replay payloads, and bounded tool output.
- **Messaging bridges** for Telegram and Weixin iLink experiments.
- **Profile configuration schema** for editor-friendly, checked configuration.

## Product Principles

Noloong is being shaped around a few hard preferences:

- **Runtime parts should be replaceable:** model calls, tools, context, phases,
  hooks, compaction, auth, interaction bridges, and product policy should not
  require rewriting the kernel.
- **Extensions should be language-agnostic:** Rust-native traits and process
  extensions must share the same semantics, so a component can start as Python
  or TypeScript and later move to Rust only when that is actually useful.
- **Self-evolution belongs at the boundary:** an AI agent should be able to
  modify plugins, manifests, prompts, bridge code, and tools without
  destabilizing the event-sourced kernel.
- **Failures should be structured:** malformed extension output, provider
  errors, tool denials, approval pauses, aborts, and run failures should become
  auditable state, not mysterious process corruption.
- **Providerless core:** provider-specific behavior belongs behind explicit
  configuration and adapters.
- **Human approval is part of the runtime:** risky actions should be explained,
  paused, resumed, and replayed as first-class events.
- **Quiet desktop experience:** the app should help people read, decide, and
  continue work without dashboard noise.

## Quick Start

### Prerequisites

- Rust toolchain with edition 2024 support
- Bun
- macOS for the packaged desktop app

### Run the desktop app

```bash
bun install
bun run app:bundle
cargo run -p noloong -- app \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

For frontend-only desktop development:

```bash
bun run desktop:dev
bun run desktop:build
bun run desktop:typecheck
```

For the full Tauri shell:

```bash
bun run app:dev
bun run app:bundle
```

### ChatGPT subscription flow

```bash
cargo run -p noloong -- chatgpt login --flow browser
cargo run -p noloong -- chatgpt status
```

Then use the subscription profile:

```bash
cargo run -p noloong -- app \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

### Generate or check the profile schema

```bash
cargo run -p noloong -- profile-config schema --output schemas/profile-config.schema.json
cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json
```

## Developer Paths

### Agent core examples

```bash
cargo run -p noloong-agent-core --example native_kernel
cargo run -p noloong-agent-core --example stateful_agent
```

### Extension conformance

```bash
python3 -m py_compile \
  examples/extensions/python-conformance/noloong_jsonrpc.py \
  examples/extensions/python-conformance/full_conformance_extension.py

cargo run -p noloong-agent-core --bin noloong-extension-conformance -- \
  --profile strict \
  -- python3 examples/extensions/python-conformance/full_conformance_extension.py
```

### Messaging bridges

Telegram and Weixin bridges are active experiments. They are useful for learning
how the runtime behaves outside the desktop app, but their UX is intentionally
different from the desktop surface.

```bash
cargo run -p noloong -- telegram \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json

cargo run -p noloong -- weixin login
cargo run -p noloong -- weixin run \
  --profile-config examples/profile-configs/weixin-chatgpt-subscription.json \
  --weixin-account-id <account-id> \
  --weixin-allowed-users <user-id>
```

## Architecture Notes

- Core architecture: [`crates/noloong-agent-core/docs/ARCHITECTURE.md`](crates/noloong-agent-core/docs/ARCHITECTURE.md)
- Extension authoring: [`crates/noloong-agent-core/docs/EXTENSIONS.md`](crates/noloong-agent-core/docs/EXTENSIONS.md)
- Product runtime architecture: [`crates/noloong-agent/docs/ARCHITECTURE.md`](crates/noloong-agent/docs/ARCHITECTURE.md)
- Extension conformance matrix: [`crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md`](crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md)
- Weixin bridge notes: [`crates/noloong-agent-weixin/docs/WEIXIN.md`](crates/noloong-agent-weixin/docs/WEIXIN.md)
- Desktop design language: [`DESIGN.md`](DESIGN.md)

## Current Status

Noloong is pre-release software. The repository is currently optimized for fast
iteration rather than broad compatibility.

What is relatively concrete today:

- Rust workspace layout and typed runtime contracts
- Desktop app launch path and dev scripts
- Profile config schema generation
- Stdio extension conformance tests
- SQLite-backed state paths
- ChatGPT subscription auth experiments
- Manifest-driven product runtime evolution
- Structured provider-neutral thinking and media blocks
- Background command lifecycle and subagent tools

What may still change aggressively:

- Desktop UX and visual language
- Profile schema details
- Plugin manifest ergonomics
- Messaging bridge command surfaces
- Provider convenience fields
- Persistence defaults

## Contributing

Issues, experiments, and design critique are welcome, especially around:

- cross-language extension authoring
- runtime component replacement
- self-evolving agent workflows
- safer local tool execution
- better approval UX
- provider adapter boundaries
- desktop interaction quality
- practical examples that expose missing contracts

For now, prefer small, focused changes with clear verification. Noloong is not
trying to preserve historical quirks; if an old shape no longer serves the
product, it should be replaced with the cleaner one.

## License

Noloong is open source under your choice of either:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)
