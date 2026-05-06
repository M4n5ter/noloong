# Telegram Interaction Client

`noloong-agent-telegram` is the first built-in interaction client for Noloong. It is not part of `noloong-agent-core`; it is an application-layer bridge that connects Telegram Bot API updates to the `noloong-agent` JSON-RPC interaction control plane.

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

Single-process mode:

```sh
NOLOONG_PROFILE_CONFIG=examples/profile-configs/telegram-openrouter-free.json \
OPENROUTER_API_KEY=... \
TELEGRAM_BOT_TOKEN=... \
TELEGRAM_BOT_USERNAME=noloong_bot \
TELEGRAM_ALLOWED_USERS=123456789 \
TELEGRAM_LOCALE=zh \
noloong telegram
```

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

Telegram config does not contain model/provider settings. The root binary loads profiles from `NOLOONG_PROFILE_CONFIG` or `--profile-config`.

See `examples/profile-configs/telegram-openrouter-free.json` for a runnable example that uses `openrouter/free` and reads credentials from `OPENROUTER_API_KEY`.

Supported provider types:

- `chat_completions`
- `responses`
- `anthropic_messages`
- `chatgpt_responses`

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

Network selection is automatic. Explicit `TELEGRAM_PROXY` wins first. If it is not set, ambient proxy environment variables such as `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY` are honored. If no proxy is active, fallback addresses are injected through `resolve_to_addrs("api.telegram.org", ...)` while preserving the request host and TLS SNI. Set `TELEGRAM_DISABLE_ENV_PROXY=1` to force the fallback/direct path even when proxy env vars are present.

For Shadowrocket fake-IP setups, prefer the fallback/direct path when the HTTP proxy returns TLS handshake errors for Telegram:

```sh
unset TELEGRAM_PROXY
export TELEGRAM_DISABLE_ENV_PROXY=1
export TELEGRAM_FALLBACK_IPS=149.154.167.220
```

## V1 Scope

V1 supports text input, text batching, display event delivery, streaming edits, final replies, tool status messages, and inline approval buttons. It does not support Telegram webhook, media input/output, file upload, per-chat profile mapping, or topic auto-management.
