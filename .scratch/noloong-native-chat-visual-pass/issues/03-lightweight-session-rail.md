# 轻量会话导航 rail

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

把当前偏重的会话列表改造成更轻、更窄、可折叠的会话导航 rail。它仍然展示真实 agent 会话、当前会话和运行状态，但不再压迫 transcript，也不再像 Settings sidebar。

## Acceptance criteria

- [ ] 会话导航默认占用更少横向空间，并在桌面尺寸下不抢占 transcript 主视觉。
- [ ] 当前会话、运行中、暂停、失败、完成等状态以低干扰方式呈现。
- [ ] 会话切换行为保持不变，切换不会暂停其它正在运行的 agent 会话。
- [ ] 折叠或紧凑模式不会丢失可访问的会话状态信息。
- [ ] 窄窗口下会话导航不遮挡 composer、toolbar 或 transcript。
- [ ] 覆盖会话 rail 状态映射测试或等价模型测试。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md

## Implementation status

第一版已改为默认隐藏 session rail，并在 title bar 增加 icon-only toggle；展开时 rail 用宽度 + 垂直 reveal 动画推开正文流。仍需录屏抽帧确认动画质感。
