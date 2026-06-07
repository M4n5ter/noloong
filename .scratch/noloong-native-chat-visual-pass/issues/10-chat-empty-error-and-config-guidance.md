# Chat 空态、错误态与缺配置引导

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

把无会话、缺配置、连接失败和 runtime 不可用等状态纳入新的 Chat 视觉系统。它们应出现在沉浸式 Chat 画布中，以低干扰方式引导用户继续，而不是像设置页、错误页或占位 demo。

## Acceptance criteria

- [ ] 无会话状态提供清晰的新建会话入口，视觉属于 Chat 画布而不是 Settings 页面。
- [ ] 缺少配置时在 Chat 中引导进入配置入口，不直接把 app 默认体验变成配置表单。
- [ ] 连接失败或 runtime 不可用时显示明确错误和可恢复路径。
- [ ] 空态、错误态和正常 transcript 间切换不丢失当前 route/session 状态。
- [ ] zh/en 两种 locale 的空态和错误文案完整且不混用语言。
- [ ] 覆盖 missing config、no session、connecting、connection failed 等 chat empty state 测试。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md
- .scratch/noloong-native-chat-visual-pass/issues/05-unified-bottom-composer.md
- .scratch/noloong-native-chat-visual-pass/issues/06-icon-only-floating-toolbar.md

## Implementation status

第一版 Chat empty state 已覆盖 missing config、connecting、connection failed 和 no session，并在 Chat 画布内提供低干扰行动入口；运行连接错误会显示在 composer 状态区域直到下一次 run started。已有 model 测试覆盖 missing config/no session/interaction 初始化状态。仍需 zh/en 与窄窗口视觉矩阵截图确认文案和布局。
