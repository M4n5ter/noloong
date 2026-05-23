# 运行生命周期、停止运行与错误状态

Status: ready-for-agent

## Parent

.scratch/noloong-app-chat-client/PRD.md

## What to build

让 Chat 明确呈现当前 run 的生命周期：started、completed、failed、paused 和 aborted。输入区在运行中把发送按钮切换为停止按钮；停止运行调用 interaction 协议的 agent abort，只中止当前 run，不拒绝审批、不删除会话。runtime 或连接错误要在 Chat 上下文中可见。

## Acceptance criteria

- [ ] Chat 能展示 run started、completed、failed、paused、aborted 状态。
- [ ] 运行中输入区显示停止按钮，并禁用重复发送。
- [ ] 停止按钮调用 agent abort。
- [ ] 停止运行不等同于拒绝审批，也不删除或关闭 agent 会话。
- [ ] run failed 和连接断开有低干扰但明确的错误状态。
- [ ] stopped、failed、completed 后输入区恢复可发送状态。
- [ ] 有覆盖 run lifecycle reducer、abort request shape、paused 与 stopped 区分、connection error 的测试。

## Blocked by

- .scratch/noloong-app-chat-client/issues/03-composer-send-and-basic-streaming.md
