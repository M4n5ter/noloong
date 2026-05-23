# 附件输入到真实 media block

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

让输入区支持文件选择和拖拽文件，并把附件构造成真实消息 media block 发送给 agent 会话。附件 UI 只展示已经进入 draft 的真实文件，不做假缩略图、假上传状态或无法兑现的能力宣传。不支持的附件能力应隐藏或禁用。

## Acceptance criteria

- [ ] 用户可以通过文件选择添加附件。
- [ ] 用户可以通过拖拽本地文件添加附件。
- [ ] 附件显示为紧凑 chip，包含文件名和可移除操作。
- [ ] 发送时附件转换为真实 media block，包含 kind、URI source、name 和 mime type。
- [ ] 文本和附件可以作为同一用户消息发送。
- [ ] 不支持或无法读取的文件不会进入 draft，并给出低干扰错误提示。
- [ ] 文件选择和拖拽生成一致的 message shape。
- [ ] 有覆盖 file path 到 media block、mime/name 推断、移除附件、unsupported file 的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/03-composer-send-and-basic-streaming.md
