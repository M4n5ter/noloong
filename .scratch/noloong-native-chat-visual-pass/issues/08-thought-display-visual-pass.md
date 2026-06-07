# Thought 展示视觉重做

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

重做思考展示视觉，让运行中的 reasoning summary 低干扰可见，结束后自动折叠为 “Thought for N seconds” 风格摘要。存在且允许展示的 reasoning 原文可以展开查看，但 summary 始终优先。

## Acceptance criteria

- [ ] 运行中 thought 展示清楚但不抢占 assistant 正文主线。
- [ ] 有 reasoning summary 时优先展示 summary；reasoning raw 只在存在且允许展示时可展开。
- [ ] 思考结束后默认折叠为耗时摘要，视觉接近参考视频的低干扰 thought row。
- [ ] 展开/折叠状态不影响 transcript 自动贴底和 final assistant message。
- [ ] 覆盖 thought delta、summary priority、completion collapse 和展开状态测试。
- [ ] 使用真实或 fixture display events 做视觉 smoke。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md
- .scratch/noloong-native-chat-visual-pass/issues/07-high-fidelity-streaming-tail.md

## Implementation status

第一版已在 reducer 中实现 summary 优先、raw 保留、completed 后默认折叠和展开状态；视图层运行中显示低干扰 thought card，完成后折叠为耗时 pill。已有单测覆盖 summary priority、completion collapse 和展开状态。仍需真实 reasoning display smoke 截图/录屏验收。
