# 视觉验收基线与录屏对比流程

Status: done

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

建立 Noloong-native Chat 高保真改进的可重复视觉验收基线。该切片要把参考视频抽帧、当前 app 截图、真实窗口 smoke、录屏抽帧和对比记录串成一个固定流程，让后续每个视觉切片都能用同一套证据判断是否更接近目标。

## Acceptance criteria

- [ ] 参考视频关键帧被抽取并记录到本主题审计材料中，覆盖 title bar、transcript、composer、右侧工具栏和流式输出状态。
- [ ] 当前 `noloong app` 的基线截图或录屏被记录，明确指出与参考视频的主要差距。
- [ ] 有一条可重复的手动/半自动 smoke 流程，说明如何启动 app、发送真实消息、录屏并用 ffmpeg 抽帧检查流式输出。
- [ ] 审计记录明确区分“功能正确性”和“视觉高保真度”，避免把能运行误判为视觉达标。
- [ ] 不引入运行时行为变更；该切片只建立验收流程和参考材料。

## Blocked by

None - can start immediately

## Implementation status

已建立参考视频关键帧、当前基线截图和审计流程，见 `../AUDIT.md` 与 `../artifacts/`。
