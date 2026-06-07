# 响应式与 i18n 视觉矩阵

Status: ready-for-agent

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

对 Chat 高保真视觉做整体响应式和 i18n 矩阵验证。覆盖 zh/en、窄/宽窗口、长文本/短文本、运行中/完成态、带附件/不带附件、工具活动/审批状态，修正文案溢出、布局遮挡和视觉层级问题。

## Acceptance criteria

- [ ] zh locale 下 Chat 主要页面不出现裸英文 UI label；en locale 下不出现裸中文 UI label。
- [ ] 窄窗口下会话 rail、transcript、toolbar 和 composer 不互相遮挡。
- [ ] 宽窗口下 transcript 保持合理最大行宽，不无限拉长。
- [ ] 长中文和英文回复都不溢出、不被 toolbar/composer 遮挡。
- [ ] 运行中、完成、暂停、失败、审批、附件等状态在 zh/en 下视觉和文案都成立。
- [ ] 通过截图或 Computer Use smoke 记录关键 viewport 与 locale 组合。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/03-lightweight-session-rail.md
- .scratch/noloong-native-chat-visual-pass/issues/04-transcript-reading-hierarchy.md
- .scratch/noloong-native-chat-visual-pass/issues/05-unified-bottom-composer.md
- .scratch/noloong-native-chat-visual-pass/issues/06-icon-only-floating-toolbar.md
- .scratch/noloong-native-chat-visual-pass/issues/08-thought-display-visual-pass.md
- .scratch/noloong-native-chat-visual-pass/issues/09-run-activity-tool-approval-visual-pass.md
- .scratch/noloong-native-chat-visual-pass/issues/10-chat-empty-error-and-config-guidance.md
