# 实施计划：统一诊断日志到 `log` + `env_logger`

> 状态：已完成实现、本地验证、workspace 测试和 clippy。诊断类输出已迁移到 `log`，root CLI 使用 `env_logger` 与 `human-panic`，机器可读 stdout、CLI 错误和交互式提示保持原有输出语义。

## 概览

当前仓库里仍有一批 `eprintln!` 被用于运行时诊断、启动状态、重试提示和 live test skip 信息。下一步将这些诊断输出统一迁移到 `log` facade，并由二进制入口使用 `env_logger` 初始化。默认日志级别为 `info`，允许用户通过 `RUST_LOG` 覆盖。同时引入 `human-panic`，让 release panic 给出更适合用户提交问题的崩溃报告。

本计划只迁移“诊断日志”。命令承诺的 stdout、机器可读输出、交互式登录提示、示例程序演示输出继续保持普通 `println!` / `eprintln!`，避免日志污染 CLI contract。

## 架构决策

- workspace 统一依赖：新增 `log = "0.4"`、`env_logger = "0.11"`、`human-panic = "2.0"`。
- `log` 是库与二进制共享的诊断 facade；`env_logger` 只在二进制或测试初始化处使用。
- root `noloong` CLI 在入口最早位置初始化 logger 与 `human-panic`；`noloong-extension-conformance` 保持轻量 CLI，不把 logger backend 或 panic reporter 下沉进 core library 依赖。
- logger 使用 `env_logger::Env::default().default_filter_or("info")`，`RUST_LOG` 仍然具有最高优先级。
- logger 初始化使用 `try_init` 或同等幂等封装，避免测试或嵌入场景重复初始化 panic。
- 日志级别约定：
  - `info!`：正常启动、监听地址、网络模式、预期的 live test skip。
  - `warn!`：可恢复的重试，例如 Telegram polling retry。
  - `error!`：真正的运行时诊断错误。
- 不迁移以下输出：root CLI fatal error、conformance CLI error/usage、`build-info`、profile schema、conformance JSON/text report、ChatGPT browser/device login 提示、examples 演示输出。

## 任务列表

### 阶段 1：依赖与初始化基础

#### 任务 1：添加 workspace 日志与 panic 依赖

**描述：** 在 workspace 统一管理 `log`、`env_logger`、`human-panic` 版本，并只把依赖加到实际需要的 crate。root `noloong` 需要三者；测试中写日志的 crate 使用 dev-dependencies；`noloong-agent-core` 不为 conformance binary 下沉 logger backend 或 panic reporter。

**验收标准：**
- [x] workspace dependencies 包含 `log = "0.4"`、`env_logger = "0.11"`、`human-panic = "2.0"`。
- [x] root `noloong` crate 可使用 `log`、`env_logger`、`human-panic`。
- [x] `noloong-agent-core` 仅在 dev-dependencies 中使用 `log`、`env_logger`，不把 `env_logger` 或 `human-panic` 作为 library runtime dependency。
- [x] 仅测试需要日志的 crate 使用 dev-dependencies，不给无关库 crate 增加 runtime logging backend。

**验证：**
- [x] `cargo metadata --no-deps`
- [x] `cargo check -p noloong`
- [x] `cargo check -p noloong-agent-core --bins`

**依赖：** 无

**预计涉及文件：**
- `Cargo.toml`
- `crates/noloong-agent-core/Cargo.toml`
- `crates/noloong-agent/Cargo.toml`
- `crates/noloong-openai/Cargo.toml`
- `Cargo.lock`

**预计范围：** S

#### 任务 2：实现二进制入口初始化

**描述：** 为 root CLI 增加早期初始化逻辑。初始化顺序为先注册 `human-panic`，再初始化 logger，然后进入原有 CLI 解析与运行流程。conformance CLI 不初始化 logger backend，避免污染 core library dependency graph。

**验收标准：**
- [x] root `src/main.rs` 的 `main` 开头调用 `human_panic::setup_panic!();`。
- [x] `noloong-extension-conformance` binary 不依赖 `env_logger` 或 `human-panic`。
- [x] root CLI 初始化 `env_logger`，默认 filter 为 `info`。
- [x] `RUST_LOG=debug`、`RUST_LOG=warn` 等环境变量能覆盖默认 filter。
- [x] 重复初始化不会 panic。

**验证：**
- [x] `cargo run -p noloong -- build-info command`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile bad -- echo`
- [x] `RUST_LOG=warn cargo run -p noloong -- build-info command`

**依赖：** 任务 1

**预计涉及文件：**
- `src/main.rs`
- `crates/noloong-agent-core/src/bin/noloong-extension-conformance.rs`

**预计范围：** S

### 检查点：初始化可用

- [x] `cargo fmt --all --check`
- [x] `cargo check -p noloong`
- [x] `cargo check -p noloong-agent-core --bins`

### 阶段 2：迁移运行时诊断输出

#### 任务 3：迁移 root CLI 诊断输出

**描述：** 将 root CLI 中真正属于诊断信息的 `eprintln!` 替换为 `log` 宏。保持 stdout 命令输出不变，尤其是 schema、build-info manifest、source cat/list、ChatGPT 登录提示等用户可见 contract。

**验收标准：**
- [x] 顶层 `run_cli` error 保持直接 stderr，避免被 `RUST_LOG` 隐藏。
- [x] interaction server listening 使用 `log::info!`。
- [x] Telegram bridge initialized 使用 `log::info!`。
- [x] Telegram network mode 使用 `log::info!`。
- [x] Telegram polling retry 使用 `log::warn!`。
- [x] `build-info`、profile schema、ChatGPT login/logout/status 输出不改成日志。

**验证：**
- [x] `cargo run -p noloong -- build-info command` stdout 仍只包含 build command。
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `RUST_LOG=warn cargo run -p noloong -- build-info command` 不输出 info 日志。

**依赖：** 任务 2

**预计涉及文件：**
- `src/main.rs`

**预计范围：** S

#### 任务 4：迁移 conformance CLI 诊断输出

**描述：** 将 extension conformance runner 的报告输出和用户可见错误边界保持清晰：错误与 usage 仍直写 stderr，conformance report 仍写 stdout，避免破坏第三方扩展作者依赖的 JSON/text 输出，也避免 `RUST_LOG` 隐藏 CLI 错误。

**验收标准：**
- [x] runtime error 使用直接 stderr，确保用户必见。
- [x] CLI parse error 和 usage 使用直接 stderr，确保用户必见。
- [x] `--json` 输出仍是纯 JSON，不混入日志。
- [x] text report 仍写 stdout。

**验证：**
- [x] `cargo test -p noloong-agent-core --test extension_conformance_cli`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --json -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs` stdout 可被 `serde_json` 解析。
- [x] invalid profile 测试仍能在 stderr 中看到可诊断错误。

**依赖：** 任务 2

**预计涉及文件：**
- `crates/noloong-agent-core/src/bin/noloong-extension-conformance.rs`
- `crates/noloong-agent-core/tests/extension_conformance_cli.rs`

**预计范围：** S

### 检查点：运行时日志迁移完成

- [x] `rg -n "eprintln!" src crates --glob '*.rs'` 只剩用户输出、示例输出或交互提示。
- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong-agent-core --test extension_conformance_cli`

### 阶段 3：迁移测试诊断输出

#### 任务 5：为测试增加幂等 logger 初始化

**描述：** 为会打印 skip 信息的测试增加最小 logger 初始化 helper。测试 logger 使用 `env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).is_test(true).try_init()`，保证 `cargo test` 输出可控且不会因重复初始化失败。

**验收标准：**
- [x] `noloong-agent-core` live test support 有可复用测试 logger 初始化。
- [x] `noloong-agent` PostgreSQL registry store live test 初始化 logger。
- [x] `noloong-openai` ChatGPT live test 初始化 logger。
- [x] helper 不引入全局可变复杂状态，不因并发测试 panic。

**验证：**
- [x] `cargo test -p noloong-agent-core --tests`
- [x] `cargo test -p noloong-agent --tests`
- [x] `cargo test -p noloong-openai --tests`

**依赖：** 任务 1

**预计涉及文件：**
- `crates/noloong-agent-core/tests/support/mod.rs`
- `crates/noloong-agent/tests/interaction_registry_store_postgres.rs`
- `crates/noloong-openai/tests/live_chatgpt.rs`

**预计范围：** M

#### 任务 6：迁移 live test skip 诊断

**描述：** 将测试中的 expected skip 信息从 `eprintln!` 改为 `log::info!`。这些 skip 属于预期环境缺失，不应使用 `warn!` 或 `error!`。

**验收标准：**
- [x] `OPENROUTER_API_KEY`、ChatGPT live env、PostgreSQL env 等缺失提示使用 `log::info!`。
- [x] TypeScript example dependency 缺失提示使用 `log::info!`。
- [x] 测试逻辑和 skip 条件不改变。
- [x] 不使用关键词断言新增低价值测试。

**验证：**
- [x] `cargo test -p noloong-agent-core --tests`
- [x] `cargo test -p noloong-agent --tests`
- [x] `cargo test -p noloong-openai --tests`

**依赖：** 任务 5

**预计涉及文件：**
- `crates/noloong-agent-core/tests/support/mod.rs`
- `crates/noloong-agent-core/tests/extension_language_examples.rs`
- `crates/noloong-agent-core/tests/anthropic_live.rs`
- `crates/noloong-agent-core/tests/responses_live.rs`
- `crates/noloong-agent/tests/interaction_registry_store_postgres.rs`
- `crates/noloong-openai/tests/live_chatgpt.rs`

**预计范围：** M

### 检查点：测试日志迁移完成

- [x] `rg -n "eprintln!" src crates --glob '*.rs'` 输出已人工分类。
- [x] `cargo test --workspace`

### 阶段 4：文档、清理与完整验证

#### 任务 7：补充 diagnostics 文档

**描述：** 在 README 或现有 CLI 文档中补充诊断日志行为：默认 `info`、`RUST_LOG` 覆盖、release panic 报告和 `RUST_BACKTRACE=1` 的关系。文档要明确日志不会写入机器可读 stdout。

**验收标准：**
- [x] 文档说明默认日志级别是 `info`。
- [x] 文档说明可用 `RUST_LOG=noloong=debug` 或 `RUST_LOG=warn` 控制输出。
- [x] 文档说明 release panic 会由 `human-panic` 生成用户友好的 crash report。
- [x] 文档说明 `RUST_BACKTRACE=1` 可查看传统 backtrace。
- [x] 文档说明 JSON/schema/build-info 等 stdout contract 不混入日志。

**验证：**
- [x] `rg -n "RUST_LOG|human-panic|RUST_BACKTRACE|diagnostic" README.md crates`

**依赖：** 任务 2、任务 3、任务 4

**预计涉及文件：**
- `README.md`

**预计范围：** XS

#### 任务 8：最终清理与 clippy

**描述：** 检查剩余 print 宏和 lint 状态，确保迁移没有把用户输出误改为日志，也没有留下未使用依赖或 clippy warning。

**验收标准：**
- [x] `rg -n "eprintln!|println!|print!" src crates --glob '*.rs'` 的剩余项均有明确用户输出、机器输出或 example 输出用途。
- [x] 无 `#[allow(dead_code)]` 或新增 lint 规避。
- [x] workspace clippy 无 warning。
- [x] workspace tests 通过。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets --all-features`
- [x] `cargo test --workspace`
- [x] `cargo run -p noloong -- build-info command`
- [x] `cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --json -- node crates/noloong-agent-core/tests/fixtures/jsonrpc-conformance-extension.mjs`

**依赖：** 任务 1-7

**预计涉及文件：**
- `Cargo.toml`
- `Cargo.lock`
- `src/main.rs`
- `crates/noloong-agent-core/src/bin/noloong-extension-conformance.rs`
- live test files
- `README.md`

**预计范围：** S

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---:|---|
| 日志污染 JSON/stdout contract | 高 | 只把诊断写入 `log`/stderr，保留 report/schema/build-info stdout，并增加 CLI smoke 验证 |
| 测试重复初始化 logger | 中 | 统一使用 `try_init`，测试 logger 使用 `is_test(true)` |
| 把交互提示误改为日志 | 中 | ChatGPT login/device/browser prompts 和 examples 明确保留普通输出 |
| `human-panic` 改变 debug panic 体验 | 低 | 文档说明 `RUST_BACKTRACE=1` 可查看传统 backtrace；仅在二进制入口注册 |

## 完成标准

- [x] 所有诊断类输出已迁移到 `log`。
- [x] `env_logger` 默认 `info`，且 `RUST_LOG` 可覆盖。
- [x] root CLI 已接入 `human-panic`；conformance CLI 保持轻量 stderr/ stdout contract。
- [x] 用户输出与机器可读 stdout 未被日志污染。
- [x] `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features`、`cargo test --workspace` 全部通过。
