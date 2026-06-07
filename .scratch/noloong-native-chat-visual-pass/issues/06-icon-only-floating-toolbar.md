# 右侧 icon-only 浮动工具栏

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

把右侧浮动工具栏改成更轻、更窄、更圆润的 icon-only 悬浮控件。它应保留 Chat、Tools、Settings 等入口的真实路由能力，但不遮挡 transcript 或 composer，也不再像侧边栏按钮堆。

## Acceptance criteria

- [ ] 工具栏只使用图标和 tooltip，不在按钮内显示文字。
- [ ] 工具栏宽度、圆角、背景透明度和 active 状态更接近参考视频的轻量悬浮感。
- [ ] hover、focus、active 和 disabled 状态柔和且清楚。
- [ ] 工具栏在宽窄窗口下都不遮挡正文、最新回复或 composer 控件。
- [ ] Settings 入口仍能进入配置入口，返回 Chat 后当前会话状态不丢失。
- [ ] 覆盖 toolbar route active state 和 Settings 切换行为测试。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md
- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md

## Implementation status

第一版已缩窄右侧浮动 toolbar 并降低背景/active 对比。tooltip、focus 和窄窗口遮挡矩阵仍待补齐。
