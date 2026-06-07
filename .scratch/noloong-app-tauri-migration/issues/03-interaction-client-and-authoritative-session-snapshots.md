# Interaction client and authoritative session snapshots

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

在前端建立 typed interaction client，让主交互客户端通过 interaction HTTP/WebSocket 直接连接 embedded 或 external runtime。实现 initialize、session list、session create、session get 等基础能力，并在 Chat 画布中展示会话列表、当前会话和由会话快照恢复的稳定 transcript。

这一切片要明确：展示事件负责实时过程，会话快照负责权威收敛；前端不能读取 SQLite、raw event log 或 runtime 内部 registry。

## Acceptance criteria

- [x] 前端 interaction client 能调用 initialize 并显示 server/profile 状态。
- [x] 前端能列出 agent 会话。
- [x] 前端能创建 agent 会话。
- [x] 前端能读取当前会话快照并恢复稳定 transcript。
- [x] Chat 画布能选择当前会话。
- [x] 前端不通过 Tauri command 代理 interaction chat/session 协议。
- [x] 前端不读取本地数据库或 raw event log。
- [x] Interaction client 和会话快照应用逻辑有测试覆盖。

## Blocked by

- [02-bootstrap-and-generated-typescript-contracts.md](./02-bootstrap-and-generated-typescript-contracts.md)
