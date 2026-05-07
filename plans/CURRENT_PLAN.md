# 实施计划：ChatGPT/Codex 订阅一等公民支持

> 状态：已完成实现、本地验证、browser/device flow 登录验证、真实 ChatGPT/Codex `gpt-5.4-mini` live smoke 和 Telegram 端到端 smoke。

## 概览

当前 `noloong-openai` 已经具备 ChatGPT OAuth、browser/device login、token storage、auth refresh、ChatGPT Codex Responses provider 和 `responses/compact` compactor；但 root `noloong` 的 profile config 仍要求手写 `env_headers`，用户体验不够好。本轮要把它提升为可直接使用的产品能力：root CLI 能登录并写入 token file，profile config 默认读取 `~/.agents/noloong/chatgpt/token.json`，启用 ChatGPT Responses 时默认启用 Codex compact，并提供可复制运行的示例配置与用法。

## 架构决策

- `noloong-agent-core` 保持不变；ChatGPT 订阅是 `noloong` binary 与 `noloong-openai` 的装配能力。
- token file 默认路径为 `~/.agents/noloong/chatgpt/token.json`，允许通过 CLI、profile config 和 `NOLOONG_CHATGPT_TOKEN_FILE` 覆盖。
- `chatgpt_responses` 的 `auth` 改为可选；未配置时默认使用 token file auth，不再要求用户手写 header/env。
- 保留 `env_headers` auth 作为高级逃生口，便于第三方 auth provider 或临时调试，但默认路径不使用它。
- root CLI 新增 `noloong chatgpt login/status/logout`；browser flow 默认打印 URL 并等待本地 callback，不主动打开浏览器。
- 启用 `chatgpt_responses` 时，`compaction: auto` 默认启用 Codex `responses/compact`；用户可以显式配置 `compaction: none` 关闭。
- compact 使用同一个 `ChatGptAuthManager`，避免 provider 和 compactor 各自刷新 token 造成行为不一致。
- 示例配置不写入 secret，不写入机器私有 token 路径；默认路径与环境变量承担本地差异。

## 任务列表

### 阶段 1：配置与 auth 基础

#### 任务 1：扩展 ChatGPT auth 配置模型

**描述：** 扩展 root profile config 中的 `chatgpt_responses` provider，使 `auth` 可选，并新增 token-file auth 配置。默认路径解析规则需要稳定、可测试、无 secret 泄漏。

**验收标准：**
- [x] `{"type":"chatgpt_responses","model":"gpt-5.4-mini"}` 可以被解析并默认使用 token file auth。
- [x] 默认 token file 路径解析为 `~/.agents/noloong/chatgpt/token.json`。
- [x] 支持 profile 显式 token file path、token file env override 和 `NOLOONG_CHATGPT_TOKEN_FILE`。
- [x] 现有 `env_headers` 配置仍然可用，且只作为显式配置路径。

**验证：**
- [x] 测试通过：`cargo test -p noloong config::`
- [x] 测试通过：`cargo test -p noloong`

**依赖：** 无

**预计涉及文件：**
- `src/config.rs`
- `src/host.rs`

**预计范围：** M

#### 任务 2：用 ChatGptAuthManager 构造 provider auth

**描述：** 在 root host 装配层把 token-file auth 转成 `ChatGptTokenStorage::file` + `ChatGptAuthManager`，再传给 `chatgpt_responses_provider`。缺少 token 时给出可执行的登录提示。

**验收标准：**
- [x] `chatgpt_responses` 默认 auth 路径构造的是 `ChatGptAuthManager`，而不是硬编码 header。
- [x] 缺失 token 或 token 文件不可读时，错误信息提示运行 `noloong chatgpt login --flow browser`。
- [x] token 和 Authorization header 不会出现在 `Debug`、error 或 CLI 输出中。
- [x] 仍支持 401 后由 `ChatGptAuthManager` refresh 并让 provider retry。

**验证：**
- [x] 测试通过：`cargo test -p noloong-openai --test auth_manager`
- [x] 测试通过：`cargo test -p noloong-openai --test provider`
- [x] 测试通过：`cargo test -p noloong`

**依赖：** 任务 1

**预计涉及文件：**
- `src/host.rs`
- `crates/noloong-openai/src/auth/manager.rs`

**预计范围：** M

### 检查点：认证基础

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-openai`
- [x] `cargo test -p noloong`
- [x] Profile config 可以在没有任何 secret 的情况下构造 ChatGPT provider，运行时按 token file 取凭据。

### 阶段 2：root CLI 登录体验

#### 任务 3：新增 `noloong chatgpt login`

**描述：** 在 root CLI 增加 ChatGPT 登录入口。默认 browser flow 绑定本地 callback server，打印 authorization URL，等待用户在真实浏览器完成验证，然后交换 token 并写入 token file。device flow 作为 fallback，同样写入 token file。

**验收标准：**
- [x] 支持 `noloong chatgpt login --flow browser`，默认 flow 也是 `browser`。
- [x] browser flow 打印 authorization URL 和 token file path，并等待 callback 完成。
- [x] 支持 `noloong chatgpt login --flow device`，打印 verification URL 和 user code。
- [x] 支持 `--token-file <path>` 覆盖写入路径。
- [x] token file 使用现有 `ChatGptFileTokenStorage` 写入，Unix 下权限保持 `0600`。

**验证：**
- [x] 测试通过：`cargo test -p noloong chatgpt`
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt login --flow browser`
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt login --flow device`

**依赖：** 任务 1

**预计涉及文件：**
- `src/main.rs`
- `src/chatgpt.rs`
- `src/config.rs`

**预计范围：** M

#### 任务 4：新增 `status/logout` 与错误体验

**描述：** 补齐 token file 的生命周期操作。`status` 只显示是否已登录、账号摘要和 token 文件路径，不输出 token；`logout` 删除 token file。provider 构造失败、token 缺失、refresh 失败时统一给出下一步命令。

**验收标准：**
- [x] 支持 `noloong chatgpt status`，不会输出 access token、refresh token 或 id token。
- [x] 支持 `noloong chatgpt logout`，重复执行保持幂等。
- [x] 交互入口如 `noloong telegram` 使用 ChatGPT profile 但缺 token 时，错误信息能直接指向 login 命令。
- [x] `src/main.rs` 不继续膨胀；ChatGPT CLI 逻辑拆入专门模块，`mod.rs` 不放业务逻辑。

**验证：**
- [x] 测试通过：`cargo test -p noloong chatgpt`
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt status`
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt logout`

**依赖：** 任务 3

**预计涉及文件：**
- `src/main.rs`
- `src/chatgpt.rs`
- `src/config.rs`

**预计范围：** S

### 检查点：CLI 登录体验

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong`
- [x] `cargo test -p noloong-openai`
- [x] 真实 browser login 可写入 `~/.agents/noloong/chatgpt/token.json`。
- [x] 真实 device login fallback 可写入同一 token file。

### 阶段 3：Codex compact 默认接入

#### 任务 5：新增 root profile compaction 配置

**描述：** 给 `RuntimeProfileConfig` 增加可选 `compaction` 字段，支持 `auto`、`none` 和显式 `openai_responses` 配置。`auto` 在普通 provider 上保持关闭，在 `chatgpt_responses` 上默认启用 Codex compact。

**验收标准：**
- [x] 未配置 `compaction` 时等价于 `auto`。
- [x] `chatgpt_responses + auto` 自动注册 `OpenAiResponsesCompactor`。
- [x] `compaction: {"type":"none"}` 明确关闭 compact。
- [x] 显式 compact 配置可以覆盖 model、threshold、reserve tokens、keep recent tokens 和 mode。
- [x] compact 共享 provider 的 `ChatGptAuthManager`，不创建第二套 token storage。

**验证：**
- [x] 测试通过：`cargo test -p noloong compaction`
- [x] 测试通过：`cargo test -p noloong-openai --test compact`
- [x] 测试通过：`cargo test -p noloong-agent-core --test compaction`

**依赖：** 任务 2

**预计涉及文件：**
- `src/config.rs`
- `src/host.rs`
- `crates/noloong-openai/src/compact.rs`

**预计范围：** M

#### 任务 6：把 compactor 接入 runtime builder

**描述：** 当前 `RuntimeProfile::build_runtime` 只注册 model provider，需要扩展为根据 profile compaction 配置向 `AgentRuntimeBuilder` 注册 context compactor。保留 core 现有 phase hook 与 compaction phase，不新增 ChatGPT-specific phase。

**验收标准：**
- [x] `RuntimeProfile` 可以携带 model provider 与可选 compactor registration。
- [x] 启用 compact 后，runtime builder 注册 context compaction phase。
- [x] `auto` compact 的默认 context window 使用 Codex 级别的大上下文阈值，避免过早压缩。
- [x] compact output 使用 existing replacement history 行为，不把 summary-only 当默认。

**验证：**
- [x] 测试通过：`cargo test -p noloong`
- [x] 集成测试：profile build 后 runtime 中存在 context compaction。
- [x] 集成测试：`compaction: none` 时 runtime 中不存在 context compaction。

**依赖：** 任务 5

**预计涉及文件：**
- `src/host.rs`
- `src/config.rs`

**预计范围：** M

### 检查点：Compact 接入

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong`
- [x] `cargo test -p noloong-agent-core --test compaction`
- [x] `cargo test -p noloong-openai --test compact`
- [x] 真实 ChatGPT compact smoke 可通过 `NOLOONG_OPENAI_LIVE_CHATGPT=1` 手动执行。

### 阶段 4：示例、文档与端到端验证

#### 任务 7：新增 ChatGPT/Codex profile 示例

**描述：** 新增一个无 secret 的示例配置，展示 `chatgpt_responses` 默认 token-file auth 与默认 compact 行为。该配置应能直接用于 `noloong telegram` 或未来其它 interaction clients。

**验收标准：**
- [x] 新增 `examples/profile-configs/chatgpt-codex-subscription.json`。
- [x] 示例默认模型使用 `gpt-5.4-mini`。
- [x] 示例不包含 token、Authorization header 或用户本地绝对路径。
- [x] 示例通过 host registry build test。

**验证：**
- [x] 测试通过：`cargo test -p noloong example_chatgpt_codex_subscription_profile_builds_registry`
- [x] 手动 smoke：`cargo run -p noloong -- telegram --profile-config examples/profile-configs/chatgpt-codex-subscription.json`

**依赖：** 任务 1、任务 5

**预计涉及文件：**
- `examples/profile-configs/chatgpt-codex-subscription.json`
- `src/host.rs`

**预计范围：** S

#### 任务 8：更新文档和使用说明

**描述：** 把登录、token file、profile 示例、Telegram 用法、compact 默认行为和禁用方式写入文档。重点是让用户不需要理解 OAuth header 细节就能跑起来。

**验收标准：**
- [x] `crates/noloong-openai/README.md` 说明 root CLI login，而不仅是 crate examples。
- [x] `crates/noloong-agent-telegram/docs/TELEGRAM.md` 增加 ChatGPT subscription profile 用法。
- [x] root README 或 interaction 文档给出最短路径命令。
- [x] 文档明确 `NOLOONG_CHATGPT_TOKEN_FILE` 与 `~/.agents/noloong/chatgpt/token.json` 的关系。
- [x] 文档明确 `compaction: none` 可关闭 Codex compact。

**验证：**
- [x] Markdown 示例使用现有命令名。
- [x] 检查文档与示例：不能包含真实 token、真实 Authorization header、旧的 ChatGPT 默认模型或要求用户手写 ChatGPT header 的路径。

**依赖：** 任务 3、任务 5、任务 7

**预计涉及文件：**
- `README.md`
- `crates/noloong-openai/README.md`
- `crates/noloong-agent-telegram/docs/TELEGRAM.md`
- `crates/noloong-agent/docs/INTERACTION.md`

**预计范围：** S

#### 任务 9：真实订阅路径 smoke

**描述：** 在实现完成后，用真实浏览器完成登录，再用 root profile 和 Telegram interaction client 做一次最小端到端验证。真实外部动作只在用户明确许可后执行；默认先给出命令和等待用户在浏览器侧完成认证。

**验收标准：**
- [x] browser login 可以拿到 token 并写入默认 token file。
- [x] `chatgpt status` 可以识别登录状态且不输出 secret。
- [x] `noloong telegram` 使用 ChatGPT profile 能收到用户消息并返回模型回复。
- [x] compact live smoke 在长上下文测试中可手动执行，失败时错误能定位到 auth、endpoint 或模型权限。

**验证：**
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt login --flow browser`
- [x] 手动 smoke：`cargo run -p noloong -- chatgpt status`
- [x] 手动 smoke：`cargo run -p noloong -- telegram --profile-config examples/profile-configs/chatgpt-codex-subscription.json`
- [x] 可选 live test：`NOLOONG_OPENAI_LIVE_CHATGPT=1 cargo test -p noloong-openai --test live_chatgpt -- --ignored`

**依赖：** 任务 7、任务 8

**预计涉及文件：**
- 无，除非 smoke 暴露 bug。

**预计范围：** S

### 检查点：完成

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-openai`
- [x] `cargo test -p noloong-agent-core --test compaction`
- [x] `cargo test -p noloong`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `git diff --check`
- [x] examples/docs/tests 中没有泄漏凭据。
- [x] 已准备好进入 review。

## 目标示例配置

```json
{
  "defaultProfileId": "chatgpt-codex",
  "registryStore": {
    "type": "memory"
  },
  "profiles": [
    {
      "profileId": "chatgpt-codex",
      "displayName": "ChatGPT Codex",
      "provider": {
        "type": "chatgpt_responses",
        "model": "gpt-5.4-mini"
      },
      "compaction": {
        "type": "auto"
      },
      "manifestPatches": [
        {
          "op": "set_locale",
          "locale": "zh"
        },
        {
          "op": "update_file_edit_tool_policy",
          "policy": "auto_by_model"
        }
      ],
      "metadata": {
        "channel": "telegram",
        "example": true
      }
    }
  ]
}
```

## 目标用法

```bash
cargo run -p noloong -- chatgpt login --flow browser
cargo run -p noloong -- chatgpt status
cargo run -p noloong -- telegram --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

## 风险与缓解

| 风险 | 影响 | 缓解 |
|------|--------|------------|
| ChatGPT OAuth/browser flow 与上游 Codex 行为漂移 | 高 | 复用 `noloong-openai` 中已按 Codex 对齐的 login 模块；保留 device flow fallback；live smoke 使用真实浏览器确认。 |
| token file 泄漏或被日志打印 | 高 | 所有 CLI/status/error 输出只显示路径和账号摘要；storage 写入 `0600`；测试覆盖 `Debug`/status 不输出 token。 |
| 默认 compact 在不合适的 provider 上启用 | 中 | `auto` 只在 `chatgpt_responses` 上开启；其它 provider 默认关闭；显式 `none` 可禁用。 |
| provider 与 compactor 分别刷新 token 导致竞态 | 中 | 在 host 装配中共享同一个 `Arc<ChatGptAuthManager>`。 |
| root `main.rs` 继续膨胀 | 中 | 新增 `src/chatgpt.rs` 承载 CLI 子命令逻辑；`main.rs` 只保留路由。 |
| 真实订阅模型名不可用 | 中 | 默认文档使用 `gpt-5.4-mini`；真实 smoke 固定验证该模型，错误信息保留 provider/model context。 |

## 可并行点

- 任务 1、任务 3 不能完全并行，因为 CLI login 依赖 token path resolver。
- 任务 5、任务 7 可以在任务 1 完成后并行推进。
- 任务 8 可在接口名稳定后独立推进。
- 任务 9 必须最后执行，因为它依赖完整登录、provider、compact 和示例配置。

## 待确认问题

- 无。当前决策采用：root CLI 登录、默认 token file 路径 `~/.agents/noloong/chatgpt/token.json`、所有 interaction clients 可复用、ChatGPT profile 默认开启 Codex compact。

## 已执行验证

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-openai`
- [x] `cargo test -p noloong-openai --test login`
- [x] `cargo test -p noloong-agent-core --test compaction`
- [x] `cargo test -p noloong`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo run -p noloong -- chatgpt status --token-file target/tmp/noloong-chatgpt-test/token.json`
- [x] `cargo run -p noloong -- chatgpt logout --token-file target/tmp/noloong-chatgpt-test/token.json`
- [x] `cargo run -p noloong -- chatgpt login --flow browser`
- [x] `cargo run -p noloong -- chatgpt login --flow device`
- [x] `cargo run -p noloong -- chatgpt status`
- [x] `NOLOONG_OPENAI_LIVE_CHATGPT=1 NOLOONG_CHATGPT_LIVE_MODEL=gpt-5.4-mini NOLOONG_CHATGPT_TOKEN_FILE=/Users/m4n5ter/.agents/noloong/chatgpt/token.json cargo test -p noloong-openai --test live_chatgpt -- --ignored --nocapture`
- [x] `TELEGRAM_BOT_TOKEN=<test-bot-token> TELEGRAM_ALLOWED_USERS=<telegram-user-id> TELEGRAM_LOCALE=zh TELEGRAM_DISABLE_ENV_PROXY=1 TELEGRAM_FALLBACK_IPS=149.154.167.220 cargo run -p noloong -- telegram --profile-config examples/profile-configs/chatgpt-codex-subscription.json`
- [x] `git diff --check`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" crates src`

## 未完成外部验证

- 无。
