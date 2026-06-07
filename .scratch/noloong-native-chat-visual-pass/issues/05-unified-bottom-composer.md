# 底部统一输入台

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

重做 Chat 输入区，让它成为参考视频风格的底部统一输入台。文本输入、附件、真实 profile/model/workdir 状态和 send/stop 控制应融入同一个低对比面板，并保持完整可见、整块可聚焦和状态可信。

## Acceptance criteria

- [ ] Composer 固定在 Chat 画布底部，不被 transcript 压缩或裁切。
- [ ] Composer 整块点击可以聚焦输入；输入文本始终清晰可见。
- [ ] Empty、can send、running、paused、failed 等状态下 send/stop 控件视觉和可用性正确。
- [ ] 附件按钮和附件 chip 与输入台视觉一致，且仍生成真实 media block。
- [ ] 输入区只显示真实 profile/model/workdir/能力状态，不出现假 token、假 memory、假工具数量或假上传状态。
- [ ] 覆盖 composer model/view adapter 测试，并用真实窗口 smoke 验证输入、发送和停止按钮。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md
- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md

## Implementation status

第一版已调低 composer 对比度、统一底部输入台外观，并保留整块点击聚焦。距离参考视频的控件密度、模型/状态组合和 send/stop 细节仍需继续打磨。
