# Telegram Agent Cockpit

`noloong-agent-telegram` is the first built-in interaction client for Noloong. It is not part of `noloong-agent-core`; it is an application-layer bridge that connects Telegram Bot API updates to the `noloong-agent` JSON-RPC interaction control plane.

The current design is a personal-first, long-polling-first Agent Cockpit. Telegram is the mobile control surface for sessions, profile selection, media input/output, approvals, background processes, queues, manifest proposals, subagents, and long-running run status. Group and topic routing are supported through the existing allowlist and mention gates, but the optimized path is still a private chat with one trusted operator.

## Architecture

The Telegram bridge always talks to the host through WebSocket JSON-RPC:

```text
Telegram Bot API
  -> noloong-agent-telegram
  -> /jsonrpc/ws
  -> InteractionControlHandler
  -> AgentSessionRegistry
  -> AgentRuntimeProfile
  -> noloong-agent-core
```

This is true even for `noloong telegram`, where the WebSocket server is bound to `127.0.0.1:0` and protected by a generated bearer token. The bridge does not receive provider credentials or construct model providers.

## Commands

OpenRouter free single-process smoke:

```sh
NOLOONG_PROFILE_CONFIG=examples/profile-configs/telegram-openrouter-free.json \
OPENROUTER_API_KEY=... \
TELEGRAM_BOT_TOKEN=... \
TELEGRAM_BOT_USERNAME=noloong_bot \
TELEGRAM_ALLOWED_USERS=123456789 \
TELEGRAM_LOCALE=zh \
noloong telegram
```

ChatGPT subscription single-process smoke:

```sh
noloong chatgpt login --flow browser
NOLOONG_PROFILE_CONFIG=examples/profile-configs/chatgpt-codex-subscription.json \
TELEGRAM_BOT_TOKEN=... \
TELEGRAM_BOT_USERNAME=noloong_bot \
TELEGRAM_ALLOWED_USERS=123456789 \
TELEGRAM_LOCALE=zh \
noloong telegram
```

The ChatGPT profile reads `~/.agents/noloong/chatgpt/token.json` by default. Use `NOLOONG_CHATGPT_TOKEN_FILE` or `noloong chatgpt login --token-file <path>` when a different token location is needed.

Split host:

```sh
NOLOONG_PROFILE_CONFIG=examples/profile-configs/telegram-openrouter-free.json \
OPENROUTER_API_KEY=... \
NOLOONG_INTERACTION_TOKEN=local-secret \
noloong serve interaction --bind 127.0.0.1:8787
```

Split Telegram bridge:

```sh
NOLOONG_INTERACTION_URL=ws://127.0.0.1:8787/jsonrpc/ws \
NOLOONG_INTERACTION_TOKEN=local-secret \
TELEGRAM_BOT_TOKEN=... \
TELEGRAM_BOT_USERNAME=noloong_bot \
TELEGRAM_ALLOWED_USERS=123456789 \
TELEGRAM_LOCALE=zh \
noloong telegram-bridge
```

## Profile Config

Telegram bridge config does not contain model/provider settings. The root binary loads runtime profiles from `NOLOONG_PROFILE_CONFIG` or `--profile-config`; Telegram runtime settings stay in CLI flags or environment variables so the same profile can be reused by terminal, WebSocket, Telegram, or third-party clients.

See `examples/profile-configs/telegram-openrouter-free.json` for a runnable example that uses `openrouter/free` and reads credentials from `OPENROUTER_API_KEY`.

The JSONC variant, `examples/profile-configs/telegram-openrouter-free.jsonc`, includes the companion Telegram bridge runtime knobs as comments. The effective bridge runtime shape uses camelCase fields:

```json
{
  "filePolicy": {
    "inlineMaxBytes": 262144,
    "maxDownloadBytes": 20971520,
    "downloadDir": ".noloong/telegram-files",
    "retentionSeconds": 604800,
    "unsupportedMediaFallback": {
      "audio": {
        "mode": "native_for_mime_types",
        "mimeTypes": ["audio/mpeg", "audio/wav", "audio/x-wav"]
      },
      "voice": { "mode": "unsupported" },
      "video": { "mode": "native" }
    }
  },
  "startupUpdatePolicy": "skip_pending_without_checkpoint"
}
```

This is only the file/startup policy subset of `TelegramBridgeConfig`; a complete bridge config also needs the bot token, interaction URL, access policy, network settings, and UX timings. For `noloong telegram` and `noloong telegram-bridge`, configure the same values with `TELEGRAM_FILE_INLINE_MAX_BYTES`, `TELEGRAM_FILE_MAX_DOWNLOAD_BYTES`, `TELEGRAM_FILE_DOWNLOAD_DIR`, `TELEGRAM_FILE_RETENTION_SECONDS`, `TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE`, and `TELEGRAM_STARTUP_UPDATE_POLICY`, or with the equivalent `--telegram-file-*`, `--telegram-unsupported-media-fallback-to-file`, and `--telegram-startup-update-policy` flags.

`unsupportedMediaFallback` is provider capability gating, not a blind MIME rewrite. Supported modes are:

- `native`: submit the Telegram attachment as its native media kind.
- `file`: force-submit it as `MediaKind::File`.
- `unsupported`: reject before agent submission with a localized user-visible notice.
- `native_for_mime_types`: submit as native media only when the MIME type is listed; otherwise reject before agent submission.
- `file_for_mime_types`: submit as `MediaKind::File` only when the MIME type is listed; otherwise reject before agent submission.

The embedded `noloong telegram` command derives a conservative default from the selected provider. For example, ChatGPT/Responses can accept some regular file MIME types through `input_file.file_data`, but it does not accept `audio/ogg` as a regular file. In that case Telegram reports the unsupported attachment instead of surfacing a provider 400 response.

Supported provider types:

- `chat_completions`
- `responses`
- `anthropic_messages`
- `chatgpt_responses`

`responses` and `chatgpt_responses` profiles expose `allowFileDataUrlInput` for callers that intentionally pass inline file data to the provider. Telegram keeps non-photo files path-first, and the built-in provider adapter materializes local `file://` inputs at request time. Enable this option when Telegram should submit regular files through those providers.

Supported registry stores:

- `memory`
- `sqlite`
- `postgres`
- `object_memory`
- `object_fs`

## Security Defaults

- An allowlist is required unless `--telegram-allow-all` is explicitly set.
- Group/supergroup messages require mentioning the bot by default. Set `TELEGRAM_BOT_USERNAME` or `--telegram-bot-username` when group gating is enabled; replies to bot messages are accepted when the replied message contains the configured bot username.
- Public interaction bind addresses require a bearer token.
- Bot tokens and API keys should come from environment variables, not JSON config.

Bridge environment variables:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_BOT_USERNAME`
- `TELEGRAM_ALLOWED_USERS`
- `TELEGRAM_ALLOWED_CHATS`
- `TELEGRAM_REQUIRE_MENTION_IN_GROUPS`
- `TELEGRAM_LOCALE`
- `TELEGRAM_FILE_INLINE_MAX_BYTES`
- `TELEGRAM_FILE_MAX_DOWNLOAD_BYTES`
- `TELEGRAM_FILE_DOWNLOAD_DIR`
- `TELEGRAM_FILE_RETENTION_SECONDS`
- `TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE`
- `TELEGRAM_STARTUP_UPDATE_POLICY`
- `TELEGRAM_OFFSET_CHECKPOINT`
- `NOLOONG_INTERACTION_URL`
- `NOLOONG_INTERACTION_TOKEN`

`TELEGRAM_LOCALE` controls Telegram-side UI strings such as inline approval buttons, approval status text, callback notifications, and tool status messages. It currently supports `en` and `zh`; if unset, the bridge detects locale from the process environment.

## Network

The bridge uses a direct `reqwest` Telegram Bot API adapter. This keeps long polling, retry, fake API tests, proxy, and fallback DNS behavior under our control.

Environment variables:

- `TELEGRAM_PROXY`
- `TELEGRAM_FALLBACK_IPS`
- `TELEGRAM_DISABLE_FALLBACK_IPS`
- `TELEGRAM_DISABLE_ENV_PROXY`

Network selection is automatic. Explicit `TELEGRAM_PROXY` wins first. If it is not set, ambient proxy environment variables such as `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY` are honored. If no proxy is active, the bridge uses normal system DNS by default so macOS TUN/fake-IP routing can decide how `api.telegram.org` is reached. Static fallback addresses are used only when `TELEGRAM_FALLBACK_IPS` or DoH endpoints are explicitly configured; they preserve the request host and TLS SNI through `resolve_to_addrs("api.telegram.org", ...)`. Set `TELEGRAM_DISABLE_ENV_PROXY=1` only when you need to ignore ambient proxy variables.

For Shadowrocket fake-IP setups, leave fallback IPs unset so the system resolver and TUN stack can select the fake-IP route. Use static fallback IPs only as a diagnostic override:

```sh
unset TELEGRAM_PROXY
export TELEGRAM_FALLBACK_IPS=149.154.167.220
```

## Cockpit Surface

Telegram commands are registered through the Bot API command menu on startup. The current cockpit surface includes:

- `/start` and `/help`: show the command surface.
- `/status`: render the active session descriptor, profile, status, message count, tools, pending approvals, and plugin count.
- `/new`, `/profiles`, `/sessions`: create and switch sessions, select profiles, and delete sessions with confirmation.
- `/continue`, `/abort`: resume or abort the active run; aborting a running session uses an inline confirmation button.
- `/queue`: list, append, clear, and mode-switch steering and follow-up queues.
- `/approvals`: list pending approval cards and resolve them with localized inline buttons.
- `/processes` and `/process <job_id>`: list background commands, read bounded output, wait, write stdin with confirmation, terminate with confirmation, and send long output as a document.
- `/manifest`: inspect manifest state, resolved system prompt, pending proposals, approve proposals, and apply approved patches with confirmation.
- `/subagent <role> [initial prompt]`: create a child session, subscribe Telegram display routing, and prompt it after subscription is active.
- `/settings`: reserved command-menu entry for bridge settings; it currently returns a localized not-implemented card.

## Media and Files

The bridge uses a hybrid file policy:

- Text and small Telegram photos are converted to inline `MediaBlock` image data with Telegram metadata preserved.
- Telegram documents, audio, voice, video, and oversized photos are downloaded to the configured `downloadDir` and passed to the agent as `file://` media URIs. Provider adapters or extensions can then decide whether to upload, transcribe, parse, inline, or reject the file for a specific model.
- When the active built-in provider cannot accept a Telegram audio, voice, or video attachment as native media, `noloong telegram` automatically falls back to `MediaKind::File` and sends a localized notice before submitting the prompt. `telegram-bridge` can request the same behavior with `TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE=audio,voice,video` or `all`.
- Assistant image, document, audio, voice, and video blocks are sent with native Telegram media APIs when local bytes or files are available.
- Provider-only media that cannot be uploaded is rendered as a readable fallback card.
- Long process output is truncated for chat readability and can be sent as a Telegram document.

`startupUpdatePolicy` defaults to `skip_pending_without_checkpoint`. Telegram polling offset is stored in the unified SQLite state database by bot token fingerprint, using `~/.agents/noloong/state.sqlite` or `NOLOONG_STATE_DATABASE_URL`. On first startup with no stored offset, the bridge consumes pending updates and starts from new messages to avoid replaying old user input after restart. `--telegram-offset-checkpoint` / `TELEGRAM_OFFSET_CHECKPOINT` remains available as an explicit file-store diagnostic path.

## Live Smoke SOP

Use a private chat with the allowlisted test user.

Minimal smoke:

- Start the bridge with either the OpenRouter free or ChatGPT subscription command above.
- Send `/status`.
- Send one text prompt and wait for the final reply.

Extended regression:

- Photo, document, voice, and video input.
- Native assistant media or document output.
- Approval card allow/deny.
- `/processes` and `/process <job_id>` after asking the agent to run a background command.
- `/manifest` proposal approve/apply path.
- `/subagent researcher <task>` routing.

Webhook, Mini App, payments, inline mode, business connections, channel administration, and topic auto-management are intentionally out of scope for this phase.
