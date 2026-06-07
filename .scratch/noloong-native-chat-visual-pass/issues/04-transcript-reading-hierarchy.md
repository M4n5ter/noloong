# Transcript 阅读层级重构

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

重构 transcript 的阅读层级，让 assistant 最终回复成为自然正文流，用户消息保持紧凑右侧气泡，运行活动与正文分层呈现。目标是长文本阅读接近参考视频，而不是被强卡片和粗边框打断。

## Acceptance criteria

- [ ] Assistant 最终回复默认不再使用强边框气泡，而是以舒适行宽、行高和段落间距的正文流呈现。
- [ ] 用户消息保持右侧对齐、紧凑、低对比，能与 assistant 正文清楚区分。
- [ ] Transcript 滚动条低调但可用，长回复阅读不被右侧工具栏或 composer 遮挡。
- [ ] 近底自动贴底逻辑保留；用户上滚阅读历史时新事件不强制拉回底部。
- [ ] 中文和英文长文本都不溢出、不拥挤、不出现过长行。
- [ ] 覆盖 transcript view model 或 render adapter 的行为测试，验证用户消息、assistant 正文和滚动跟随状态。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/02-chat-canvas-and-title-bar.md

## Implementation status

第一版已把 assistant 消息从强边框卡片改为正文流，用户消息保持右侧低对比气泡。滚动跟随与长文本录屏检查仍待后续切片完成。
