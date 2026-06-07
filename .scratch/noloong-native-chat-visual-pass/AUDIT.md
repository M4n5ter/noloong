# Noloong-native Chat visual audit

Status: in-progress

Date: 2026-05-24

## Reference material

Source video:

```text
/Users/m4n5ter/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_vu2ffheq52ea21_fa4a/temp/RWTemp/2026-05/05918b4efaee81172e0868e6a0daedd3/123562583becf6174d04731106679fb2.mp4
```

Extracted frames:

- `artifacts/reference-video/contact-sheet.jpg`
- `artifacts/reference-video/frame-02-title-thought-composer.jpg`
- `artifacts/reference-video/frame-09-stream-scroll.jpg`
- `artifacts/reference-video/frame-18-long-stream-tail.jpg`

The reference emphasizes a dark gray immersive window, centered task title, low-contrast subtitle, text-first assistant output, a unified bottom composer, a light icon-only right toolbar, collapsed thought summary, and soft streaming text tail.

## Current baseline

Captured from the current `noloong app` before this visual pass:

- `artifacts/current-baseline/current-chat-baseline.png`

Command used:

```bash
cargo run -p noloong -- app --locale zh \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

Baseline differences:

- Chat still reads as a functional validation screen rather than an immersive Chat canvas.
- Assistant output is enclosed in strong bordered cards; the reference uses text-first document flow for assistant prose.
- The session list is wide and card-heavy, closer to a settings/sidebar panel than a lightweight session rail.
- Chat title bar still shows settings-oriented action buttons on the right, which distracts from the centered current session.
- The color system is cold blue/green and high-contrast compared with the reference's softer dark gray hierarchy.
- Composer is functionally correct but still feels like a boxed form surface rather than an integrated bottom input deck.
- The floating toolbar is visually heavy and still competes with the transcript.

## First visual pass

Captured after the first Chat canvas/transcript/composer/rail pass:

- `artifacts/current-progress/chat-after-first-pass.png`
- `artifacts/current-progress/chat-rail-open.png`
- `artifacts/current-progress/chat-rail-hidden.png`

Implemented changes:

- Chat title bar no longer shows Settings validate/save actions.
- Assistant messages render as text-first reading flow instead of strong bordered cards.
- User messages remain compact, low-contrast right bubbles.
- Composer uses a softer unified bottom deck and keeps the full click-to-focus affordance.
- Floating toolbar is narrower and less visually dominant.
- Session rail is hidden by default; the title bar has a small icon button that toggles it. Opening the rail animates width and a vertical reveal, pushing the transcript aside instead of overlaying it.

Remaining deltas:

- The session rail animation needs a screen-recorded frame check before it can be called high fidelity.
- The title-bar rail icon is functional but should be reviewed for visual clarity against the app icon set.
- Composer still needs a closer pass against the reference controls, including model/workdir/status grouping and send/stop affordance.
- Streaming tail and thought/tool activity visuals remain separate follow-up slices.

## Verification process

For each visual slice:

1. Launch the app with the real ChatGPT subscription profile.
2. Use Computer Use to confirm the app window and visible state.
3. Capture a window screenshot with `screencapture`.
4. For streaming changes, record a short screen movie and extract frames with `ffmpeg`.
5. Compare the result against the reference frame set above and update this audit with concrete deltas.

Functional correctness is not the same as visual fidelity. A slice is not visually complete merely because Chat can send and receive messages.

## Streaming and submit diagnosis

Date: 2026-05-24

User-observed symptoms:

- A long assistant reply waited for roughly 10-20 seconds and then appeared all at once.
- A second submitted message did not produce a reply for a long time.

Findings:

- The previous real run persisted thousands of `model_stream_event`/display-related rows in SQLite, including many text deltas before `run_completed`. That means provider/core streaming was still active; the failure was in the app-side display/reveal path, not in the model provider.
- The Chat reducer previously replaced the streaming assistant bubble with the full final assistant message as soon as `AssistantMessageFinal` arrived. If final arrived while the visual stream still had unrevealed text, the UI could jump from partial/no visible text to the complete message.
- Large deltas were rendered as a single segment. If the provider or display bridge batched text, the app showed the whole batch in one frame instead of visually revealing it over several frames.
- The app launcher reused an already-running bundle instance. When `noloong app` wrote fresh embedded interaction launch options, `open` could activate the old process instead of starting a process that reads the new endpoint/token. This explains duplicate Dock instances with different locales and a GUI that appears alive but does not submit into the current SQLite event store.
- In the latest local smoke attempt, Computer Use and `screencapture` could see the process/window through CGWindow APIs but could not capture or operate the window (`cgWindowNotFound` / black screenshot). This blocks fully automated visual smoke on this machine until that tooling issue is resolved or a human sends the prompt.

Implemented fixes:

- Streaming text now splits large deltas into small visual segments, batches very fast deltas, and reveals hidden segments across animation frames.
- The app schedules animation frames while streaming text still has hidden or fading segments.
- `AssistantMessageFinal` now confirms the assistant message id and appends any missing suffix to the existing stream instead of replacing the bubble with static text.
- The Chat submit loop keeps consuming display notifications after the prompt RPC result until a terminal display event or a short drain window.
- WebSocket connect has an explicit timeout so a broken interaction endpoint cannot silently hang forever.
- `noloong app` now terminates an existing generated bundle instance before writing new launch options and reopening, avoiding stale endpoint/locale reuse.

Remaining validation:

- Needs a fresh human-assisted or working Computer Use smoke: launch current app, send a real Chat prompt, confirm SQLite receives a new session/run, then record or inspect whether text appears incrementally.

## Display stream verification

Date: 2026-05-25

Probe:

```text
cargo run -p noloong-app --example display_probe -- ws://127.0.0.1:8797/jsonrpc/ws '请写一个约800字的中文寓言故事，分段输出。'
```

Result:

```text
run_started=1 thought_delta=74 assistant_delta=757 assistant_bytes=2564 assistant_final=1 assistant_final_bytes=2564 run_completed=1 first_delta_ms=Some(4694) prompt_done=true final_status=Completed
```

Conclusion:

- The interaction `display/event` stream is healthy: it emits thought deltas, assistant deltas, assistant final, and run completed.
- The blank/stale GUI symptoms were app-side projection issues, not provider/core streaming failures.
- The probe was deleted after verification; the result above is retained as audit evidence.

Implemented fixes after verification:

- The app WebSocket notification buffer was increased so normal display bursts are less likely to lag the UI consumer.
- If the GPUI app does lag a broadcast receiver, the submit loop now stops consuming display notifications and falls back to the authoritative prompt/session descriptor instead of busy-looping on `Lagged`.
- Running descriptor refreshes preserve optimistic local `app-user-*` messages until the final descriptor contains the real persisted user message.

Live smoke:

- Launched the current app with `examples/profile-configs/chatgpt-codex-subscription.json`.
- Sent `Reply exactly: live smoke ok`.
- Verified in the Noloong window that the user bubble stayed visible immediately and the assistant reply `live smoke ok` appeared.

Current remaining deltas:

- Streaming is functionally verified, but final high-fidelity acceptance still needs a fresh recorded run compared against the reference frames.
- Tool/approval activity visuals have been tightened with shared Chat semantic tokens; they still need a real tool/approval visual smoke before final acceptance.
