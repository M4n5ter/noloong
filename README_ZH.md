# Noloong

[English](README.md) | 简体中文

**Noloong 是一个仍处于早期阶段、以极致扩展性为核心的 agent runtime。
它的目标是让 provider、tool、phase、hook、context、compaction、auth、
interaction bridge 和产品行为都可以被替换，而不是把所有能力塞进一个脆弱黑盒。**

项目还很年轻。内部接口会快速变化，桌面体验仍在打磨，破坏性改动也可能发生。
当前方向很明确：保留一个小而可靠的 event-sourced Rust kernel，
把演进放到类型化扩展边界上，让 AI Agent 可以持续迭代自身外围架构，
同时让核心运行保持可审计、可恢复。

Noloong 不是想再做一个普通聊天机器人。它想成为一个 agent 基底：
外围组件可以不断替换、重写、实验，核心仍然保留 replay、结构化失败、
审批审计和类型化状态迁移。

## 为什么做 Noloong

很多 agent 系统把太多东西混在一个黑盒循环里：模型调用、工具执行、审批、
UI 状态、持久化、provider 兼容细节和产品策略互相缠绕。
Noloong 选择反过来做。

它把 agent runtime 拆成一组可替换 contract。模型 provider 可以来自 Rust、
TypeScript、Python 或其它进程；tool、phase node、phase hook、tool hook、
context provider、compactor、auth provider 都通过同一套 runtime 语义接入；
产品行为通过 manifest 和 plugin 演进，而不是靠硬编码分支堆叠。

## 当前包含什么

- **Providerless Rust kernel：** event-sourced 执行、typed phases、reducers、
  effect validation、可 replay 的状态和结构化 run failure。
- **可替换 phase graph：** providers、tools、context providers、phase nodes、
  phase hooks、tool hooks、compactors 和 auth providers 共享同一套 runtime
  contract。
- **跨语言 extension bridge：** stdio JSON-RPC，带 TypeScript/Python
  conformance examples 和公开 conformance runner。
- **产品层 runtime：** sessions、manifests、approvals、persistence、follow-up
  queues、steering、background jobs、subagents 和本地执行工具。
- **macOS 桌面应用：** 基于 Tauri 和 React，重点是阅读、输入、审批和配置。
- **Provider 集成：** OpenAI-compatible Chat Completions、OpenAI Responses、
  ChatGPT subscription auth、Anthropic Messages。
- **Provider-neutral 内容模型：** thinking、reasoning、media、tool calls、
  provider replay payloads 和 bounded tool output 都有统一表达。
- **消息桥实验：** Telegram 和 Weixin iLink。
- **Profile config schema：** 方便编辑器提示和配置校验。

## 产品原则

- **Runtime 部件应该可替换：** model calls、tools、context、phases、hooks、
  compaction、auth、interaction bridges 和产品策略都不应该要求重写 kernel。
- **Extension 应该语言无关：** Rust-native traits 和进程扩展共享同一套语义，
  组件可以先用 Python/TypeScript 实现，只有真的有收益时再迁移到 Rust。
- **自演化发生在边界上：** AI Agent 应该能修改插件、manifest、prompt、
  bridge code 和 tools，而不破坏 event-sourced kernel。
- **失败应该结构化：** malformed extension output、provider error、tool denial、
  approval pause、abort 和 run failure 都应该变成可审计状态，
  而不是神秘的进程损坏。
- **Providerless core：** provider 差异应该留在显式配置和 adapter 后面。
- **人类审批是 runtime 的一部分：** 风险动作应该被解释、暂停、恢复，
  并作为一等事件 replay。
- **安静的桌面体验：** 界面服务于阅读、判断和继续工作，而不是展示 dashboard 噪音。

## 快速开始

### 前置条件

- 支持 edition 2024 的 Rust toolchain
- Bun
- 如果要打包桌面 app，需要 macOS

### 运行桌面 app

```bash
bun install
bun run app:bundle
cargo run -p noloong -- app \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

前端开发：

```bash
bun run desktop:dev
bun run desktop:build
bun run desktop:typecheck
```

完整 Tauri shell：

```bash
bun run app:dev
bun run app:bundle
```

### ChatGPT subscription

```bash
cargo run -p noloong -- chatgpt login --flow browser
cargo run -p noloong -- chatgpt status

cargo run -p noloong -- app \
  --profile-config examples/profile-configs/chatgpt-codex-subscription.json
```

### Profile schema

```bash
cargo run -p noloong -- profile-config schema --output schemas/profile-config.schema.json
cargo run -p noloong -- profile-config schema --check schemas/profile-config.schema.json
```

## 开发入口

- Core architecture: [`crates/noloong-agent-core/docs/ARCHITECTURE.md`](crates/noloong-agent-core/docs/ARCHITECTURE.md)
- Extension authoring: [`crates/noloong-agent-core/docs/EXTENSIONS.md`](crates/noloong-agent-core/docs/EXTENSIONS.md)
- Product runtime architecture: [`crates/noloong-agent/docs/ARCHITECTURE.md`](crates/noloong-agent/docs/ARCHITECTURE.md)
- Extension conformance matrix: [`crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md`](crates/noloong-agent-core/docs/CONFORMANCE_MATRIX.md)
- Weixin bridge notes: [`crates/noloong-agent-weixin/docs/WEIXIN.md`](crates/noloong-agent-weixin/docs/WEIXIN.md)
- Desktop design language: [`DESIGN.md`](DESIGN.md)

## 当前状态

Noloong 还没有正式发布。仓库目前更重视快速迭代和正确方向，不承诺历史兼容。

相对稳定的部分：

- Rust workspace 结构和 typed runtime contracts
- 桌面 app 启动路径和开发脚本
- Profile config schema 生成
- stdio extension conformance tests
- SQLite-backed state paths
- ChatGPT subscription auth 实验
- manifest-driven 产品 runtime 演进
- provider-neutral thinking 和 media blocks
- background command lifecycle 和 subagent tools

还可能大幅变化的部分：

- 桌面 UX 和视觉语言
- Profile schema 细节
- Plugin manifest 体验
- 消息桥命令界面
- Provider convenience fields
- 持久化默认值

## 参与贡献

欢迎围绕这些方向提 issue、实验和设计 critique：

- 跨语言 extension authoring
- runtime component replacement
- self-evolving agent workflows
- 更安全的本地工具执行
- 更清楚的人类审批体验
- provider adapter 边界
- 桌面交互品质
- 能暴露缺失 contract 的真实例子

当前阶段更适合小而聚焦、验证清楚的改动。Noloong 不打算为了历史惯性保留旧形状；如果一个旧概念不再服务产品，就应该换成更干净的设计。

## License

Noloong 以双协议开源，你可以任选其一使用：

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)
