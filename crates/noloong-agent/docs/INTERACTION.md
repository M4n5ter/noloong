# noloong-agent Interaction Protocol

`noloong-agent` exposes a language-neutral control plane for external bridges such as terminal UIs, Telegram adapters, WeChat/iLink adapters, web UIs, or orchestration processes. The bridge is a JSON-RPC 2.0 client. The Rust host owns runtime profiles, provider credentials, tools, and approval policy.

V1 transport is line-delimited stdio:

- stdin: one JSON-RPC request per line.
- stdout: one JSON-RPC response or notification per line.
- stderr: logs only, never protocol data.

All params and results use `camelCase`. Sensitive methods require authority capabilities granted during `initialize`.

## Initialize

The client starts with `initialize` and requests authority plus UX capabilities. The server intersects those requests with host policy.

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"name":"telegram-bridge","requestedAuthority":["agent.run","agent.queue","approval.resolve"],"requestedUx":{"displayEvents":true,"streamText":true,"editMessage":true,"markdown":true,"maxMessageBytes":8192}}}
```

Result:

```json
{"jsonrpc":"2.0","id":1,"result":{"server":{"name":"noloong-agent","protocolVersion":"2026-05-05"},"grant":{"authority":["agent.run"],"ux":{"displayEvents":true,"streamText":true,"editMessage":true,"markdown":true,"maxMessageBytes":4096}},"profiles":[{"profileId":"default","displayName":"Default","defaultManifestPatches":[],"metadata":{}}]}}
```

Authority capabilities:

| Capability | Allows |
|---|---|
| `agent.run` | `agent/prompt`, `agent/continue`, `agent/abort`, `agent/wait_idle` |
| `agent.queue` | `agent/steer`, `agent/follow_up`, `queue/edit`, `queue/clear`, `queue/set_mode` |
| `approval.resolve` | `approval/resolve`, `approval/resume_timeouts` |
| `manifest.apply` | `manifest/proposals/approve`, `manifest/apply_approved` |
| `process.control` | `process/wait`, `process/write`, `process/terminate` |
| `subagent.spawn` | `subagent/spawn` |
| `session.delete` | `session/delete` |

Read-only methods such as `profile/list`, `session/list`, `session/get`, `agent/state`, `queue/list`, `approval/list`, `manifest/get`, `manifest/proposals/list`, `process/list`, and `process/read` do not require a sensitive authority capability.

## Sessions and Profiles

Profiles are registered by the Rust host. A bridge can select a `profileId` but cannot send provider credentials.

Session descriptors may come from a persisted registry store without a live runtime loaded in memory. `session/get` and `session/list` are read-only descriptor operations: they can return SQLite/PostgreSQL/OpenDAL-backed snapshots without constructing a provider, tools, or background process runtime. Run and mutation methods restore the live session lazily from the snapshot using the currently registered `AgentRuntimeProfile` with the same `profileId`.

If a persisted snapshot was `running` when the previous process stopped, the registry reports it as `failed` and writes that interrupted status back to the store. Persisted `paused` sessions remain paused so approval and human workflows can be resumed by the host.

```json
{"jsonrpc":"2.0","id":2,"method":"profile/list","params":{}}
```

Create a root session:

```json
{"jsonrpc":"2.0","id":3,"method":"session/create","params":{"sessionId":"root","profileId":"default","metadata":{"channel":"telegram"}}}
```

Get/list/delete sessions:

```json
{"jsonrpc":"2.0","id":4,"method":"session/get","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":5,"method":"session/list","params":{"parentSessionId":"root","profileId":"default","status":"idle"}}
{"jsonrpc":"2.0","id":6,"method":"session/delete","params":{"sessionId":"root","forceAbort":true}}
```

Spawn a subagent session:

```json
{"jsonrpc":"2.0","id":7,"method":"subagent/spawn","params":{"parentSessionId":"root","role":"researcher","metadata":{"topic":"storage"},"initialPrompt":{"id":"subagent-task-1","role":"user","content":[{"type":"text","text":"Investigate the storage layer."}],"metadata":{}}}}
```

## Agent Runs and Queues

Prompt with text:

```json
{"jsonrpc":"2.0","id":8,"method":"agent/prompt","params":{"sessionId":"root","input":{"type":"text","text":"Summarize the current task."}}}
```

Prompt with a full message:

```json
{"jsonrpc":"2.0","id":9,"method":"agent/prompt","params":{"sessionId":"root","input":{"type":"message","message":{"id":"user-1","role":"user","content":[{"type":"text","text":"Continue."}],"metadata":{}}}}}
```

Other run controls:

```json
{"jsonrpc":"2.0","id":10,"method":"agent/continue","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":11,"method":"agent/abort","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":12,"method":"agent/wait_idle","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":13,"method":"agent/state","params":{"sessionId":"root"}}
```

Steering and follow-up queue messages use explicit intent:

```json
{"jsonrpc":"2.0","id":14,"method":"agent/steer","params":{"sessionId":"root","message":{"id":"background-1","role":"user","content":[{"type":"text","text":"Background command completed."}],"metadata":{}},"intent":"observation"}}
{"jsonrpc":"2.0","id":15,"method":"agent/follow_up","params":{"sessionId":"root","message":{"id":"next-user-1","role":"user","content":[{"type":"text","text":"Use that result next."}],"metadata":{}}}}
```

Queue methods:

```json
{"jsonrpc":"2.0","id":16,"method":"queue/list","params":{"sessionId":"root","queue":"steering"}}
{"jsonrpc":"2.0","id":17,"method":"queue/edit","params":{"sessionId":"root","queue":"follow_up","messages":[{"message":{"id":"queued-1","role":"user","content":[{"type":"text","text":"Next turn input."}],"metadata":{}},"intent":"user_input"}]}}
{"jsonrpc":"2.0","id":18,"method":"queue/set_mode","params":{"sessionId":"root","queue":"steering","mode":"one_at_a_time"}}
{"jsonrpc":"2.0","id":19,"method":"queue/clear","params":{"sessionId":"root","queue":"follow_up"}}
```

## Raw and Display Events

Raw events are core `AgentEvent` values with `sessionId` and `subscriptionId`:

```json
{"jsonrpc":"2.0","id":20,"method":"event/subscribe","params":{"sessionId":"root"}}
```

Notification:

```json
{"jsonrpc":"2.0","method":"agent/event","params":{"sessionId":"root","subscriptionId":"subscription-1","event":{"sequence":1,"runId":"run-1","turnId":null,"phase":null,"kind":{"type":"run_started"}}}}
```

Display events are UI projections intended for bridges that do not want to render raw event logs:

```json
{"jsonrpc":"2.0","id":21,"method":"display/subscribe","params":{"sessionId":"root","ux":{"displayEvents":true,"streamText":true,"editMessage":true,"maxMessageBytes":4096}}}
```

Notification:

```json
{"jsonrpc":"2.0","method":"display/event","params":{"sessionId":"root","subscriptionId":"subscription-2","event":{"type":"assistant_message_delta","displayMessageId":"run-1:assistant","text":"hello"}}}
```

Telegram-like bridges should request `streamText + editMessage` and update one external message by `displayMessageId`. WeChat/iLink-like bridges should request `displayEvents` with `streamText = false`, then render only `assistant_message_final`.

Unsubscribe:

```json
{"jsonrpc":"2.0","id":22,"method":"event/unsubscribe","params":{"subscriptionId":"subscription-1"}}
```

## Approval

List pending approvals:

```json
{"jsonrpc":"2.0","id":23,"method":"approval/list","params":{"sessionId":"root"}}
```

Resolve one approval:

```json
{"jsonrpc":"2.0","id":24,"method":"approval/resolve","params":{"sessionId":"root","approvalId":"approval-run-1-1-host-exec-start-test-0","decision":{"outcome":"allow","reason":"approved by user","approver":"telegram:user:123","metadata":{}}}}
```

Resume timed-out approvals:

```json
{"jsonrpc":"2.0","id":25,"method":"approval/resume_timeouts","params":{"sessionId":"root"}}
```

`allow` decisions that match built-in cache metadata are recorded through `AgentSession::record_tool_approval_resolution`.

## Manifest

```json
{"jsonrpc":"2.0","id":26,"method":"manifest/get","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":27,"method":"manifest/proposals/list","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":28,"method":"manifest/proposals/approve","params":{"sessionId":"root","proposalId":"manifest-proposal-1"}}
{"jsonrpc":"2.0","id":29,"method":"manifest/apply_approved","params":{"sessionId":"root"}}
```

`manifest/apply_approved` drains approved proposals and applies supported manifest patches. Reserved phase profile patches remain rejected.

## Process Control

Background jobs are started by agent tools. Bridges can display and control them through the process methods:

```json
{"jsonrpc":"2.0","id":30,"method":"process/list","params":{"sessionId":"root"}}
{"jsonrpc":"2.0","id":31,"method":"process/read","params":{"sessionId":"root","jobId":"host-job-1","afterSeq":0,"maxBytes":8192,"waitMs":500}}
{"jsonrpc":"2.0","id":32,"method":"process/wait","params":{"sessionId":"root","jobId":"host-job-1","timeoutMs":1000}}
{"jsonrpc":"2.0","id":33,"method":"process/write","params":{"sessionId":"root","jobId":"host-job-1","text":"input\n"}}
{"jsonrpc":"2.0","id":34,"method":"process/terminate","params":{"sessionId":"root","jobId":"host-job-1"}}
```

`process/read` preserves the existing cursor, dropped-prefix, truncation, and bounded spool semantics. It does not copy complete process output into control-plane notifications.

## Errors

Stable error codes:

| Code | Meaning |
|---:|---|
| `-32601` | Unknown method |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `-32070` | Missing authority capability |
| `-32071` | Session or agent is busy |
| `-32072` | Session, profile, subscription, approval, or job not found |

Unauthorized errors include the required capability:

```json
{"jsonrpc":"2.0","id":6,"error":{"code":-32070,"message":"method session/delete requires session.delete","data":{"method":"session/delete","requiredCapability":"session.delete"}}}
```
