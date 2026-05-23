# Noloong app Chat client completion audit

Status: implementation-covered; visual Computer Use verification still unproven.

Date: 2026-05-24

## What is proven

- `noloong app` defaults to the Chat route in `AppViewModel::load`.
- Embedded mode starts a loopback interaction runtime from the root CLI and passes the endpoint into the macOS app bundle launch options.
- External runtime mode uses the same typed interaction client path.
- `noloong-app` depends on `noloong-config`, GPUI, gpui-component and transport crates; `cargo tree -p noloong-app --edges normal --depth 2` shows no direct `noloong-agent` or `noloong-agent-core` dependency.
- Chat state is driven by typed interaction DTOs and display notifications, not by a direct registry handle.
- Session list, current session selection, transcript recovery, first-message session creation, prompt submission, abort, approval resolve, workdir metadata and title metadata are covered by `crates/noloong-app/src/model/tests.rs`.
- Assistant delta/final replacement, streaming opacity ramp, thought summary priority, thought completion collapse, run lifecycle, tool aggregation, long output preview and approval cards are covered by `crates/noloong-app/src/chat/tests.rs`.
- Attachment draft handling converts local files into real media blocks with file URI, media kind, file name and MIME type. Drag/drop and picker both feed the same attachment path ingestion path in `view/chat.rs`.
- Display events in `noloong-agent` include run lifecycle, assistant delta/final, thought, tool started/updated/completed and approval events.
- `RunPaused` display projection now redacts internal `ToolApprovalContinuation` state to `{"type":"tool_approval"}` so GUI clients do not receive system prompts, model request bodies or tool manifests inside display data.
- README, `CONTEXT.md` and ADR-0001 describe `noloong app` as the primary interaction client, with Settings as a configuration entry and GUI communication through interaction protocol/display events.

## Verification commands that passed

```bash
cargo fmt --all --check
cargo test -p noloong-agent --test interaction_control
cargo test -p noloong-app
cargo test -p noloong
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

## Remaining verification gap

Computer Use can list the running bundle:

```text
Noloong — /Users/m4n5ter/Library/Application Support/Noloong/Noloong.app/ — dev.noloong.Noloong [running]
```

But `get_app_state` currently returns:

```text
Computer Use server error -10005: cgWindowNotFound
```

System Events also reports zero accessibility windows for the GPUI process:

```text
false, 0,
```

`CGWindowList` does show the onscreen Noloong window, so the app exists at the CoreGraphics layer. The same Computer Use failure is reproducible against Zed Preview, which is also GPUI-based. Current evidence therefore does not prove the PRD requirement “use Computer Use to verify default Chat, session list, composer, streaming bubble, floating toolbar and Settings switching.”

Do not mark the goal complete until one of these is true:

- Computer Use can inspect/click the GPUI window and the required GUI smoke is rerun.
- A human explicitly accepts manual visual verification as the substitute for Computer Use for this GPUI limitation.
- The app is changed so its GPUI window exposes the accessibility/window state Computer Use needs, and the GUI smoke passes.

## Strict code-quality review notes

- No production app file currently exceeds 1000 lines.
- `crates/noloong-app/src/model/tests.rs` is a large test module. It is test-only and predates the latest hardening commit, but it should be split into focused test modules if the chat/settings model grows further.
- The latest hardening change avoided adding another GUI-side special case by fixing display projection at the canonical interaction layer.
- The macOS bundle fix keeps launch option serialization local to `macos_bundle.rs`; it does not leak endpoint propagation into view/model code.
