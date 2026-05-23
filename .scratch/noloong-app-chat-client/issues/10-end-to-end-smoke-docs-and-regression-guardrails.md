# 端到端 smoke、文档与回归护栏

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

为 Noloong app 主交互客户端补齐端到端验证、文档和回归护栏。使用真实 profile 跑 GUI smoke，覆盖新建会话、发送文本、流式回复、停止运行、审批、工具活动、附件和 Settings 切换。更新文档，明确 app 是主交互客户端，Settings 是配置入口，GUI 只通过 interaction 协议和展示事件工作。

## Acceptance criteria

- [ ] 使用真实 profile 完成一次 GUI Chat smoke。
- [ ] 使用 Computer Use 验证默认 Chat、会话列表、输入区、流式回复、浮动工具栏和 Settings 切换。
- [ ] smoke 覆盖停止运行、工具活动、审批卡片和附件输入。
- [ ] README 或相关文档说明 `noloong app` 主交互客户端行为。
- [ ] 文档明确 embedded runtime、external runtime、interaction 协议和展示事件边界。
- [ ] 文档不把 app 描述为单纯 profile 配置工具。
- [ ] 回归命令覆盖 app crate、interaction protocol、agent interaction tests 和 root CLI app command。
- [ ] PRD、CONTEXT 和 ADR 与实现保持一致。

## Blocked by

- .scratch/noloong-app-chat-client/issues/09-noloong-native-high-fidelity-visual-integration.md
