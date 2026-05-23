# Chat 默认入口与 embedded interaction 连接

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

让 `noloong app` 默认进入 Chat 画布，并在 embedded mode 下启动本地 loopback interaction runtime。GUI 必须通过 typed interaction client 初始化 runtime、读取 profile/capability 信息，并在没有会话时展示 Chat 空态和配置引导。不要让 GUI 直接持有 registry，也不要把 Settings 作为默认落点。

## Acceptance criteria

- [ ] `noloong app` 默认打开 Chat 画布，Settings 只能通过工具栏或快捷键进入。
- [ ] embedded mode 启动本地 loopback interaction server，并由 GUI 通过 JSON-RPC interaction 协议完成 initialize。
- [ ] external runtime mode 使用同一 typed interaction client 路径。
- [ ] 没有可用会话时，Chat 显示空态、新建会话入口和缺配置引导。
- [ ] GUI crate 不因为 Chat 接入直接依赖完整 agent runtime 或 registry。
- [ ] 有覆盖 app launch、embedded options、external options、initialize 成功/失败的测试。

## Blocked by

None - can start immediately
