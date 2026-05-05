# noloong-agent Conformance Matrix

This matrix tracks the application-layer interaction control plane. The core kernel conformance matrix remains in `crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md`.

## Control Plane

| Area | Public surface | Evidence |
|---|---|---|
| Wire and capabilities | `InteractionAuthorityCapability`, `InteractionUxCapabilities`, JSON-RPC errors | `cargo test -p noloong-agent --test interaction` |
| JSON-RPC substrate | Line-delimited request/response, parse errors, shutdown, notification writer | `cargo test -p noloong-agent --test interaction_jsonrpc` |
| Runtime profiles | `AgentRuntimeProfile`, `profile/list`, default profile selection | `cargo test -p noloong-agent --test interaction_registry --test interaction_control` |
| Session registry | `session/create`, `session/list`, `session/get`, `session/delete` | `cargo test -p noloong-agent --test interaction_registry --test interaction_control` |
| Subagents | `subagent/spawn`, parent metadata, initial prompt, capability gate | `cargo test -p noloong-agent --test interaction_registry --test interaction_control` |
| Agent run control | `agent/prompt`, `agent/continue`, `agent/abort`, `agent/wait_idle`, `agent/state` | `cargo test -p noloong-agent --test interaction_control` |
| Queues | `agent/steer`, `agent/follow_up`, `queue/list`, `queue/edit`, `queue/clear`, `queue/set_mode` | `cargo test -p noloong-agent --test interaction_control` |
| Raw events | `event/subscribe`, `event/unsubscribe`, `agent/event` notification | `cargo test -p noloong-agent --test interaction_control` |
| Display events | `display/subscribe`, `display/event`, streaming/final-only, bounded text | `cargo test -p noloong-agent --test interaction_control` |
| Approval | `approval/list`, `approval/resolve`, `approval/resume_timeouts` | `cargo test -p noloong-agent --test interaction_control` |
| Manifest | `manifest/get`, `manifest/proposals/list`, `manifest/proposals/approve`, `manifest/apply_approved` | `cargo test -p noloong-agent --test interaction_control` |
| Process control | `process/list`, `process/read`, `process/wait`, `process/write`, `process/terminate` | `cargo test -p noloong-agent --test interaction_control` |
| Bridge examples | TypeScript and Python stdio JSON-RPC clients | `node --check examples/interaction/typescript-bridge/bridge.mjs`; `python3 -m py_compile examples/interaction/python-bridge/bridge.py` |

## Required Gate

Run these before accepting a change that modifies interaction wire types, handler methods, session registry behavior, approval, manifest, process control, or bridge examples:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --check examples/interaction/typescript-bridge/bridge.mjs
python3 -m py_compile examples/interaction/python-bridge/bridge.py
```

## Update Rule

When adding, renaming, or deleting a public interaction method:

1. Update `crates/noloong-agent/docs/INTERACTION.md`.
2. Update this matrix.
3. Add or adjust a structural integration test in `crates/noloong-agent/tests/interaction_control.rs` or a narrower test file.
4. Avoid tests that only scan for isolated words; prefer protocol-level request/response or typed serde behavior.
