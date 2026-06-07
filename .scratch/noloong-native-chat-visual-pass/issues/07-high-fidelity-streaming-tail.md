# 流式尾迹高保真渲染

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

实现真实 delta 驱动的高保真流式尾迹。新到达文本片段应以短暂低透明度进入并平滑稳定，快速连续 delta 可以轻量批处理；final assistant message 到达后应无感稳定为最终正文。

## Acceptance criteria

- [ ] 流式尾迹只基于真实 DisplayEvent delta，不伪造或重排模型内容。
- [ ] 新文本片段有短暂柔和渐入，不出现逐字弹跳、闪烁或明显跳位。
- [ ] 快速连续 delta 被轻量批处理，减少高频重绘带来的视觉抖动。
- [ ] Final assistant message 替换 streaming state 时不重复、不跳位、不突然改变排版。
- [ ] Streaming renderer 与 transcript reducer 解耦，能用可控时钟单测 segment aging、opacity ramp、batching 和 final stabilization。
- [ ] 使用录屏或连续抽帧记录真实流式输出效果。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md

## Implementation status

第一轮已实现 delta 分段、快速 delta 批处理、透明度渐入、streaming animation frame 调度，以及 final message 不再直接替换 streaming bubble。2026-05-25 已用独立 display probe 验证 `display/event` 会真实发送 thought/assistant delta、assistant final 和 run completed；也完成一次真实 App live smoke，确认本地用户气泡与最终回复可见。仍需最终录屏抽帧确认视觉节奏是否达到高保真目标。
