# Noloong-native 高保真视觉整合

Status: ready-for-human

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

在真实 Chat 功能已经可用后，对 Chat 画布做 Noloong-native 高保真视觉整合。目标是复刻参考视频的窗口节奏、暗色沉浸感、底部输入区手感、右侧浮动工具栏、流式尾迹和运行活动的低干扰层次，但保留 Noloong 自身品牌、图标和交互模型。该切片需要人类视觉确认。

## Acceptance criteria

- [ ] Chat 画布默认视觉不再像 Settings 表单，而是沉浸式对话工作区。
- [ ] 会话列表、transcript、输入区和右侧浮动工具栏在桌面尺寸下布局协调。
- [ ] 输入区手感接近参考视频：低干扰、多行、运行态清晰、附件 chip 不拥挤。
- [ ] 流式尾迹在真实 delta 下视觉顺滑，无明显闪烁或跳动。
- [ ] 思考展示、工具活动和审批卡片层级清楚，不抢占 transcript 主线。
- [ ] Settings 入口仍清晰，但不主导 Chat 画布。
- [ ] 视觉实现不引入假数据、假能力或外部产品品牌。
- [ ] 通过 Computer Use 录屏或截图进行人类视觉确认。

## Blocked by

- .scratch/noloong-app-chat-client/issues/03-composer-send-and-basic-streaming.md
- .scratch/noloong-app-chat-client/issues/04-reasoning-display-events-and-thought-ui.md
- .scratch/noloong-app-chat-client/issues/05-run-lifecycle-stop-and-error-states.md
- .scratch/noloong-app-chat-client/issues/06-tool-activity-and-approval-cards.md
- .scratch/noloong-app-chat-client/issues/07-attachment-input-to-real-media-blocks.md
- .scratch/noloong-app-chat-client/issues/08-session-metadata-title-workdir-profile-context.md
