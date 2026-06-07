# 最终高保真视觉验收

Status: ready-for-human

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

对完成后的 Noloong-native Chat 视觉改进做最终人类验收。使用真实 `noloong app`、真实 profile、Computer Use 截图或录屏、ffmpeg 抽帧和参考视频关键帧进行对比，确认 Chat 画布是否已经从“功能验证版”达到可长期使用的主交互客户端质感。

## Acceptance criteria

- [ ] 使用真实 `noloong app` 和真实 profile 完成一次 Chat smoke：打开、发送消息、观察流式回复、thought、工具栏、composer 和 Settings 切换。
- [ ] 使用 Computer Use 截图或录屏证明 composer 完整可用、transcript 贴底、toolbar 不遮挡正文。
- [ ] 使用 ffmpeg 从真实流式输出录屏中抽帧，检查尾迹是否平滑且无跳动。
- [ ] 对照参考视频关键帧写出视觉差距清单，并标出已接受的差异和仍需后续处理的差异。
- [ ] 人类确认是否达到本 PRD 的 Noloong-native 高保真目标。
- [ ] 验收记录保存到本主题审计材料中。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/01-visual-baseline-and-review-loop.md
- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md
- .scratch/noloong-native-chat-visual-pass/issues/03-lightweight-session-rail.md
- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md
- .scratch/noloong-native-chat-visual-pass/issues/05-unified-bottom-composer.md
- .scratch/noloong-native-chat-visual-pass/issues/06-icon-only-floating-toolbar.md
- .scratch/noloong-native-chat-visual-pass/issues/07-high-fidelity-streaming-tail.md
- .scratch/noloong-native-chat-visual-pass/issues/08-thought-display-visual-pass.md
- .scratch/noloong-native-chat-visual-pass/issues/09-run-activity-tool-approval-visual-pass.md
- .scratch/noloong-native-chat-visual-pass/issues/10-chat-empty-error-and-config-guidance.md
- .scratch/noloong-native-chat-visual-pass/issues/11-responsive-and-i18n-visual-matrix.md
