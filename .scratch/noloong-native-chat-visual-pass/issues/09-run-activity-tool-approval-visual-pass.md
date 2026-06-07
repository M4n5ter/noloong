# 运行活动、工具与审批卡片视觉重做

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

重做运行活动、工具执行和审批请求的 inline activity 视觉层级。它们应作为 transcript 中的低干扰上下文出现，默认折叠或摘要化；错误和审批需要足够清楚，但不能退化为裸 JSON、控制台日志或粗糙卡片。

## Acceptance criteria

- [ ] 工具活动默认以小型 inline row 或 compact card 展示，完成后折叠为摘要。
- [ ] 工具错误、失败 run 和 aborted run 有明确但克制的视觉状态。
- [ ] 审批卡片展示最少必要信息，并提供同意/拒绝操作，不暴露内部 JSON 或模型请求细节。
- [ ] 停止运行和拒绝审批在视觉与行为上保持不同语义。
- [ ] 长工具输出不会撑爆 transcript，可折叠、预览或链接到外部文件。
- [ ] 覆盖 tool activity aggregation、approval requested/resolved、run paused/failed/aborted 的测试。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md
- .scratch/noloong-native-chat-visual-pass/issues/05-unified-bottom-composer.md

## Implementation status

第一版 reducer 已聚合 tool started/updated/completed、长输出预览、approval requested/resolved 和 run paused/failed/aborted 状态；视图层现在使用共享 Chat semantic tokens 区分运行、完成、错误、审批待处理、审批同意和审批拒绝，不再散落硬编码颜色。仍需真实工具调用/审批 smoke 截图确认视觉密度和错误态。
