# Noloong app Chat client completion audit

Status: implementation-covered; Computer Use visual smoke passed for the latest chat hardening.

Date: 2026-05-24

## What is proven

- `noloong app` defaults to the Chat route in `AppViewModel::load`.
- Embedded mode starts a loopback interaction runtime from the root CLI and passes the endpoint into the macOS app bundle launch options.
- The macOS bundle uses the real `Noloong` binary as `CFBundleExecutable`; launch options are read by Rust in the bundle child instead of a shell wrapper, so Launch Services, the bundle id and the GPUI process use the same executable identity.
- External runtime mode uses the same typed interaction client path.
- `noloong-app` depends on `noloong-config`, GPUI, gpui-component and transport crates; `cargo tree -p noloong-app --edges normal --depth 2` shows no direct `noloong-agent` or `noloong-agent-core` dependency.
- Chat state is driven by typed interaction DTOs and display notifications, not by a direct registry handle.
- Session list, current session selection, transcript recovery, first-message session creation, prompt submission, abort, approval resolve, workdir metadata and title metadata are covered by `crates/noloong-app/src/model/tests.rs`.
- Assistant delta/final replacement, streaming opacity ramp, thought summary priority, thought completion collapse, run lifecycle, tool aggregation, long output preview and approval cards are covered by `crates/noloong-app/src/chat/tests.rs`.
- Attachment draft handling converts local files into real media blocks with file URI, media kind, file name and MIME type. Drag/drop and picker both feed the same attachment path ingestion path in `view/chat.rs`.
- Display events in `noloong-agent` include run lifecycle, assistant delta/final, thought, tool started/updated/completed and approval events.
- `RunPaused` display projection now redacts internal `ToolApprovalContinuation` state to `{"type":"tool_approval"}` so GUI clients do not receive system prompts, model request bodies or tool manifests inside display data.
- SQLite registry store startup is resilient to a partially-created registry schema in the shared state database. With `migrate_on_connect=true`, a partial registry table set is treated as an internal dev schema reset: registry tables are dropped and recreated, while unrelated state tables such as `stored_agent_events` are preserved.
- Chat composer layout is fixed to a non-shrinking footer inside the chat workspace. The chat page no longer uses the outer settings-page scrollbar; only the transcript scrolls.
- Composer hit area covers the full composer panel, typed text is visible in the field, and local user messages are appended optimistically before the final session descriptor arrives.
- Transcript tail-following is driven by a tracked `ScrollHandle`; when the user is already near the bottom, new user/assistant/display events keep the transcript pinned to the latest content.
- README, `CONTEXT.md` and ADR-0001 describe `noloong app` as the primary interaction client, with Settings as a configuration entry and GUI communication through interaction protocol/display events.

## Verification commands that passed

```bash
cargo fmt --all --check
cargo test -p noloong-agent --test interaction_control
cargo test -p noloong-agent --features registry-store-sqlite --test interaction_registry_store_sqlite
cargo test -p noloong-app
cargo test -p noloong
cargo clippy -p noloong-agent --features registry-store-sqlite --all-targets -- -D warnings
cargo clippy -p noloong-agent -p noloong-agent-telegram -p noloong-app -p noloong --all-targets -- -D warnings
```

## Live smoke evidence

Embedded app launch produced a real child process with the bundle app receiving:

```text
--interaction-ws-url ws://127.0.0.1:60316/jsonrpc/ws
--interaction-token <redacted>
```

The embedded HTTP interaction endpoint accepted `initialize`, `session/create`, `agent/prompt`, and `session/get` using `examples/profile-configs/chatgpt-codex-subscription.json`.

Observed final session state:

```json
{
  "status": "completed",
  "messages": 2,
  "text": "只回复这一行：noloong app http smoke ok noloong app http smoke ok"
}
```

This proves the app-owned embedded runtime is live and can run the real ChatGPT subscription profile through the public interaction protocol.

## Latest Computer Use visual smoke

After the bundle identity and native window fixes, Computer Use can inspect the running app:

```text
App=/Users/m4n5ter/Library/Application Support/Noloong/Noloong.app/
bundleID dev.noloong.Noloong
Window: "Noloong", App: Noloong
```

The live app was launched with:

```bash
cargo run -p noloong -- app --locale zh \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

Observed in the real GPUI window:

- Chat opens against the embedded interaction runtime and shows the active session list.
- Composer is fully visible: input row, status/workdir row and send button are all inside the composer panel.
- The full composer panel can focus the input; typed numeric content is visible.
- Sending `1111111111` appends the user bubble immediately before the final assistant descriptor arrives.
- The assistant reply streams/settles at the bottom, and the transcript remains tail-followed without needing a manual outer-page scroll.

## Strict code-quality review notes

- No production app file currently exceeds 1000 lines.
- `crates/noloong-app/src/model/tests.rs` is a large test module. It is test-only and predates the latest hardening commit, but it should be split into focused test modules if the chat/settings model grows further.
- The latest hardening change avoided adding another GUI-side special case by fixing display projection at the canonical interaction layer.
- The macOS bundle fix keeps launch option serialization local to `macos_bundle.rs`; it does not leak endpoint propagation into view/model code.
- The SQLite registry fix lives in the store layer, where schema ownership already exists, and avoids scattering state-reset handling into app/CLI startup paths.
