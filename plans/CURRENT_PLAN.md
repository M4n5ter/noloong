# 实施计划：Build Info 与构建时源码快照

> 状态：已完成实现、本地验证、workspace 测试和 clippy。目标是在 `noloong` Rust 二进制中嵌入构建时仓库源码快照和构建 provenance，并提供 `noloong build-info ...` 子命令，让 Agent 在自我迭代时能够审阅不可变 Rust host 的源码和可复现构建命令。

## 概览

当前 Agent 可以通过工具操作宿主环境，但运行中的 Rust 二进制本身是不可变层；如果运行环境没有原始 git checkout，Agent 无法确认自己正在调用的 host/core/interaction 代码。下一步在 root `noloong` crate 增加构建时 source snapshot：`build.rs` 遵循 `.gitignore` 收集仓库文件，显式排除 `.git/`，生成 `source.tar.zst` 和 `build-info.json`，运行时通过 CLI 输出 manifest、构建命令、源码列表、单文件内容、归档和解包目录。

## 架构决策

- 使用二进制自包含方案：源码快照以 `tar.zst` 嵌入 root `noloong` binary，不依赖运行时 checkout、网络或外部 artifact。
- `.gitignore` 是源码快照的安全边界；先补齐本地 secret、数据库、日志、IDE/cache/temp 排除规则，再让 `build.rs` 遵循 ignore 规则遍历仓库。
- 快照范围是被 `.gitignore` 允许的仓库快照，包含 `.github/`、`.gitignore`、docs、examples、schemas、Cargo manifests 和 Rust 源码；显式排除 `.git/` 与 build output。
- 构建命令输出 normalized reproducible recipe，不承诺还原用户 shell 中的原始命令；manifest 同时记录 target/profile/features、rustc/cargo version、git 状态和 source hash。
- CLI 入口采用 `noloong build-info`，下挂 `manifest`、`command`、`source list`、`source cat`、`source extract`、`source archive`。
- v1 不嵌入 crates.io 依赖源码、不自动重建新二进制、不替换当前进程，只提供可审计、可解包、可复现的信息。
- 嵌入源码只推荐用于理解当前二进制背后的不可变 Rust host 内容；不推荐把内置源码解包后直接修改并重新编译作为常规自我改进路径。
- 真正的自我改进应优先通过编写、更新和热插拔插件完成，让不可变核心保持稳定，演进能力落在可替换扩展层。

## 任务列表

### 阶段 1：源码快照边界与构建依赖

#### 任务 1：收紧 `.gitignore` 作为快照安全边界

**描述：** 完善 `.gitignore`，确保构建时仓库快照不会嵌入本地 secret、临时数据库、日志、编辑器缓存和 OS 噪声文件。保留 `.github/`、`.gitignore`、docs、examples、schemas 等可审阅上下文。

**验收标准：**
- [x] `.gitignore` 继续排除 `target/`、`node_modules/`、Python cache 和 `.zed/`。
- [x] `.gitignore` 新增排除 `.env*`、`.envrc`、`*.sqlite*`、`*.db`、`*.pem`、`*.key`、`*.log`、`.DS_Store`、`.idea/`、`.vscode/`、`tmp/`、`temp/`。
- [x] `git ls-files -co --exclude-standard` 不显示本地 secret/token/database/log 类文件。
- [x] `.github/workflows/ci.yml` 仍会进入快照候选文件。

**验证：**
- [x] `git ls-files -co --exclude-standard | rg '(^|/)(\\.env|.*\\.sqlite|.*\\.db|.*\\.pem|.*\\.key|.*\\.log)$'` 无输出。
- [x] `git ls-files -co --exclude-standard | rg '^\\.github/workflows/ci\\.yml$'` 有输出。

**依赖：** 无

**预计涉及文件：**
- `.gitignore`

**预计范围：** XS

#### 任务 2：添加 build-time archive 依赖

**描述：** 在 workspace 中加入源码遍历、归档、压缩和 hash 相关依赖。`ignore` 只用于 `build.rs`；`tar`、`zstd` 同时用于 build-time 打包和 runtime list/cat/extract/archive。

**验收标准：**
- [x] workspace dependencies 增加 `ignore = "0.4"`、`tar = "0.4"`、`zstd = "0.13"`。
- [x] root package `build-dependencies` 包含 `ignore`、`tar`、`zstd`、`sha2`、`serde_json`。
- [x] root runtime dependencies 包含 `tar`、`zstd`、`sha2`。
- [x] 不给 library crates 增加无关默认依赖。

**验证：**
- [x] `cargo check -p noloong`

**依赖：** 无

**预计涉及文件：**
- `Cargo.toml`
- `Cargo.lock`

**预计范围：** S

### 检查点：快照边界基础

- [x] `git status --short` 只显示本计划相关文件。
- [x] `cargo check -p noloong`

### 阶段 2：构建时快照生成

#### 任务 3：实现 root `build.rs` 源码快照生成器

**描述：** 新增 root `build.rs`，在构建时从 workspace root 遍历 `.gitignore` 允许的文件，生成稳定顺序的 `source.tar.zst` 和 `build-info.json` 到 `OUT_DIR`。快照必须可复现、可审计，并显式拒绝 `.git/`。

**验收标准：**
- [x] `build.rs` 使用 `ignore::WalkBuilder`，开启 git ignore 规则，关闭 hidden-file 过滤，显式跳过 `.git/`。
- [x] 文件列表按仓库相对路径稳定排序。
- [x] 每个普通文件写入 tar 时使用相对路径；不跟随 symlink 写入仓库外内容。
- [x] `source.tar.zst` 使用 zstd 压缩，`build-info.json` 记录 archive sha256、compressed/uncompressed size、file count 和每个文件的 path/size/sha256。
- [x] `cargo:rerun-if-changed` 覆盖 `.gitignore` 和进入快照的文件；git metadata 变化通过 `.git/HEAD` 与对应 ref 尽量触发重建。
- [x] 构建脚本不读取环境变量中的 secret，不把绝对 workspace path 写入 manifest。

**验证：**
- [x] `cargo clean -p noloong`
- [x] `cargo build -p noloong`
- [x] 在 `target` 对应 `OUT_DIR` 中能找到生成的 `source.tar.zst` 与 `build-info.json`。

**依赖：** 任务 1、任务 2

**预计涉及文件：**
- `build.rs`
- `Cargo.toml`

**预计范围：** M

#### 任务 4：定义 build info manifest v1

**描述：** 在构建脚本侧生成稳定 JSON manifest，运行时直接嵌入。manifest v1 是 Agent 与外部审计工具消费的 contract，字段名使用 camelCase，schemaVersion 固定为 `1`。

**验收标准：**
- [x] manifest 顶层包含 `schemaVersion`、`package`、`workspace`、`git`、`rust`、`cargo`、`build`、`sourceArchive`、`files`。
- [x] `build.command` 输出 normalized recipe，例如 `cargo build -p noloong --bin noloong`，并按 profile/target/features 增补参数。
- [x] `git` 包含 `commit`、`dirty`、`hasUntracked`、`status`，git 不可用时字段为 `null` 或 `unknown`，命令仍可用。
- [x] `files` 只记录相对路径、byte size 和 sha256，不记录本机绝对路径。
- [x] manifest pretty JSON 末尾有换行，便于 CLI 直接输出。

**验证：**
- [x] `serde_json::from_str(include_str!(...))` 可解析 manifest。
- [x] 单元测试断言 manifest `schemaVersion == 1`。

**依赖：** 任务 3

**预计涉及文件：**
- `build.rs`
- `src/build_info.rs`

**预计范围：** S

### 检查点：构建产物可嵌入

- [x] `cargo build -p noloong`
- [x] `cargo test -p noloong build_info`

### 阶段 3：运行时 build-info 模块与 CLI

#### 任务 5：新增 runtime `build_info` 模块

**描述：** 新增 `src/build_info.rs`，通过 `include_str!` 和 `include_bytes!` 嵌入 `build-info.json` 与 `source.tar.zst`。模块提供 manifest 输出、build command 输出、source list/cat/extract/archive 的纯函数，CLI 只负责参数解析和 stdout/filesystem I/O。

**验收标准：**
- [x] `manifest_json()` 返回构建时 manifest JSON。
- [x] `build_command()` 返回 manifest 中的 normalized build command。
- [x] `source_paths()` 从嵌入 archive 读取并返回排序路径。
- [x] `source_file(path)` 安全读取单个文件内容；拒绝绝对路径、空路径、`..` 和目录路径。
- [x] `write_archive(path)` 原样写出嵌入的 `source.tar.zst`。
- [x] `extract_source(output_dir, force)` 安全解包，不允许 tar path traversal；默认拒绝覆盖非空目录。

**验证：**
- [x] 单元测试：manifest JSON 可解析。
- [x] 单元测试：`source_paths()` 包含 `Cargo.toml`、`.github/workflows/ci.yml`、`crates/noloong-agent-core/src/lib.rs`。
- [x] 单元测试：`source_paths()` 不包含 `.git/`、`target/`、`.env`、sqlite/log/key 类路径。
- [x] 单元测试：`source_file("Cargo.toml")` 返回包含 `[workspace]` 的文本。
- [x] 单元测试：`source_file("../Cargo.toml")` 和绝对路径返回错误。

**依赖：** 任务 4

**预计涉及文件：**
- `src/build_info.rs`
- `src/main.rs`

**预计范围：** M

#### 任务 6：接入 `noloong build-info` CLI

**描述：** 在 root `src/main.rs` 接入 `build-info` 子命令，保持现有 clap 风格。命令只做自省输出，不启动 Agent runtime，也不读取 profile config。

**验收标准：**
- [x] `noloong build-info manifest` 输出 pretty JSON。
- [x] `noloong build-info command` 输出 normalized build command 单行文本。
- [x] `noloong build-info source list` 输出每行一个相对路径。
- [x] `noloong build-info source cat <path>` 输出指定文件内容。
- [x] `noloong build-info source extract --output-dir <dir> [--force]` 解包快照。
- [x] `noloong build-info source archive --output <path>` 写出嵌入归档。
- [x] 错误归入 `CliError`，信息可诊断，不 panic。

**验证：**
- [x] CLI parse 单元测试覆盖所有新增子命令。
- [x] `cargo run -p noloong -- build-info manifest`
- [x] `cargo run -p noloong -- build-info command`
- [x] `cargo run -p noloong -- build-info source list`
- [x] `cargo run -p noloong -- build-info source cat Cargo.toml`

**依赖：** 任务 5

**预计涉及文件：**
- `src/main.rs`
- `src/build_info.rs`

**预计范围：** M

### 检查点：CLI 自省闭环

- [x] `cargo fmt --all --check`
- [x] `cargo test -p noloong build_info`
- [x] `cargo run -p noloong -- build-info source cat Cargo.toml`

### 阶段 4：安全边界、文档与 CI

#### 任务 7：强化解包与路径安全测试

**描述：** 针对 `source cat`、`source extract`、`source archive` 的路径处理补充负例，保证 Agent 即使传入恶意路径也不能写出 output dir 外或读取非法路径。

**验收标准：**
- [x] `source cat` 拒绝绝对路径、`..`、空路径、目录路径和不存在路径。
- [x] `extract` 拒绝非空目录，除非传 `--force`。
- [x] `extract` 对 archive entry 做 path traversal 防护。
- [x] `archive --output` 可创建父目录，但不会接受目录作为文件输出路径。

**验证：**
- [x] `cargo test -p noloong build_info`
- [x] `cargo test -p noloong cli_build_info`

**依赖：** 任务 5、任务 6

**预计涉及文件：**
- `src/build_info.rs`
- `src/main.rs`
- `src/test_support.rs`

**预计范围：** S

#### 任务 8：更新 README 与架构文档

**描述：** 文档说明 build-time source snapshot 的用途、命令示例、安全边界和限制。重点说清楚它是“不可变 Rust host 自省”，不是热更新机制。

**验收标准：**
- [x] README 增加 `build-info` 使用示例。
- [x] `crates/noloong-agent/docs/ARCHITECTURE.md` 增加不可变 host 自省说明。
- [x] 文档明确 `.gitignore` 是快照边界，添加 secret 文件前必须先确认 ignore 规则。
- [x] 文档明确 v1 不嵌入 crates.io 依赖源码、不自动重建或替换当前 binary。
- [x] 文档明确不推荐解包内置源码后修改并重新编译作为常规自我改进方式。
- [x] 文档明确自我改进的推荐路径是编写或更新插件，而不是修改不可变 Rust host。

**验证：**
- [x] `rg -n "build-info|source snapshot|sourceArchive" README.md crates/noloong-agent/docs/ARCHITECTURE.md`

**依赖：** 任务 6

**预计涉及文件：**
- `README.md`
- `crates/noloong-agent/docs/ARCHITECTURE.md`

**预计范围：** S

#### 任务 9：接入 CI 与完整验证

**描述：** 在 CI 中增加最小 build-info smoke checks，确保嵌入源码快照在 Linux CI 上可生成、可解析、可读取。

**验收标准：**
- [x] CI 增加 `cargo run -p noloong -- build-info manifest`。
- [x] CI 增加 `cargo run -p noloong -- build-info source cat Cargo.toml`。
- [x] CI 不运行会写入仓库目录的 extract/archive 命令。
- [x] workspace 原有 schema check、clippy、test 保持通过。

**验证：**
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json`
- [x] `cargo run -p noloong -- build-info manifest`
- [x] `cargo run -p noloong -- build-info source cat Cargo.toml`

**依赖：** 任务 6、任务 8

**预计涉及文件：**
- `.github/workflows/ci.yml`

**预计范围：** XS

### 检查点：完成

- [x] 所有新增 CLI 命令可用。
- [x] 嵌入源码快照不包含 `.git/`、`target/`、secret、数据库和日志类文件。
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [x] `cargo test --workspace` 通过。
- [x] 文档能指导 Agent 或开发者从二进制提取源码并构建派生版本。

## 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| `.gitignore` 漏掉 secret 类文件 | 高 | 先收紧 `.gitignore`，并在测试中断言快照不包含常见 secret/database/log 路径。 |
| 二进制体积明显增大 | 中 | 使用 `tar.zst` 压缩；v1 接受体积换自包含能力，后续可加 feature gate 或外部 artifact 模式。 |
| `build.rs` 触发重建过频繁 | 中 | 只对进入快照的文件输出 `rerun-if-changed`，git metadata 只监听 `.git/HEAD` 和当前 ref。 |
| archive 解包路径穿越 | 高 | runtime 解包前 normalize entry path，拒绝绝对路径、`..` 和非普通文件 entry。 |
| normalized command 被误认为原始命令 | 中 | manifest 字段和文档明确它是 reproducible recipe，不是 shell history。 |

## 并行化建议

- 任务 1 和任务 2 可以并行。
- 任务 3 和任务 4 必须顺序执行。
- 任务 5 和任务 6 建议同一人连续完成，避免 CLI 与模块 API 反复调整。
- 任务 8 可在任务 6 稳定后与任务 7 并行。
- 任务 9 最后执行，避免 CI 先引用未稳定命令。

## Open Questions

- 无阻塞问题。默认按二进制自包含、仓库快照、`build-info` CLI、normalized build recipe 执行。
