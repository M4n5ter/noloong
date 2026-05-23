# PRD：Noloong app 主交互客户端

Status: ready-for-agent

## Problem Statement

用户希望 `noloong app` 成为 Noloong 的主交互客户端，而不是停留在 profile 配置工具。现有桌面 GUI 已有 Settings 和局部视觉系统，但 Chat 仍是占位，无法创建或继续 agent 会话，也无法展示真实运行过程、流式回复、思考展示、工具活动、审批请求和附件输入。

Telegram 和微信已经证明 interaction runtime 可以作为多客户端共享的会话后端，但这些客户端受平台能力限制，不适合作为主要的人机交互入口。桌面 app 需要提供一个更完整、更沉浸、更符合参考视频手感的 Chat 画布，同时保持 Noloong-native：复刻窗口节奏、输入区手感和流式尾迹，但不复制外部产品品牌、文案或协议模型。

## Solution

`noloong app` 默认打开 Chat 画布，并默认启动内嵌 interaction runtime。GUI 即使在 embedded mode 下也通过 loopback JSON-RPC interaction 协议通信，不直接持有 registry。Chat 只消费展示事件来渲染 transcript、流式回复、思考展示、运行活动、审批状态和完成消息；当展示事件语义不足时，扩展展示事件，而不是让 GUI 订阅原始事件或维护第二套事实来源。

Chat 画布包含可折叠会话列表、当前会话 transcript、底部输入区和右侧浮动工具栏。没有当前会话时显示空态和新建入口；发送第一条消息会创建 agent 会话并提交 prompt。Settings 仍存在，但作为配置入口通过工具栏或快捷键进入，不再是 app 的默认落点。

体验上，assistant 文本使用真实 delta 做流式尾迹：新片段短暂以较低透明度进入，再稳定为正常亮度。思考展示优先展示 reasoning summary；如果有可展示 reasoning 原文，可以展开查看；思考结束后折叠为类似 “Thought for 2 seconds” 的低干扰摘要。工具活动、审批请求和运行状态作为 transcript 中的 inline activity 呈现，默认折叠但可展开。

## User Stories

1. As a Noloong 用户, I want 打开 `noloong app` 后默认进入 Chat, so that 我可以直接开始使用 agent 会话而不是先面对配置表单。
2. As a Noloong 用户, I want app 在本机默认启动可用 runtime, so that 我不需要先手动启动外部服务才能聊天。
3. As a Noloong 高级用户, I want 可以连接外部 interaction runtime, so that 我可以在远程或共享 runtime 上继续工作。
4. As a Noloong 用户, I want 没有会话时看到清晰的空态和新建入口, so that 我知道下一步怎么开始。
5. As a Noloong 用户, I want 发送第一条普通消息时自动创建 agent 会话, so that Chat 的使用路径自然且低摩擦。
6. As a Noloong 用户, I want 看到可折叠的会话列表, so that 我可以在多个 agent 会话之间切换而不牺牲当前 Chat 画布空间。
7. As a Noloong 用户, I want 切换会话时其它正在运行的会话继续执行, so that 我可以并行观察多个任务。
8. As a Noloong 用户, I want 会话列表显示每个会话的运行状态, so that 我能知道哪个会话正在运行、暂停、失败或完成。
9. As a Noloong 用户, I want 会话标题默认来自第一条用户消息, so that 新会话能自然命名。
10. As a Noloong 用户, I want 可以手动重命名会话标题, so that 长期会话更容易识别。
11. As a Noloong 用户, I want 会话标题持久化到 agent 会话 metadata, so that 重新打开 app 后标题不会丢失。
12. As a Noloong 用户, I want Chat 标题栏显示当前会话名称、profile/model 和会话工作目录, so that 我知道当前 agent 在什么上下文中运行。
13. As a Noloong 用户, I want 会话工作目录是真实运行上下文, so that 相对路径和工具执行结果可预期。
14. As a Noloong 用户, I want 能为新会话选择或切换工作目录, so that 不同任务可以在不同项目目录中执行。
15. As a Noloong 用户, I want 工作目录切换不影响已经运行中的 run, so that 正在执行的任务不会被偷偷改变上下文。
16. As a Noloong 用户, I want 历史 transcript 从 agent 会话状态恢复, so that 重启 app 后仍能看到稳定对话记录。
17. As a Noloong 用户, I want 运行中的流式回复由展示事件继续补充, so that 当前 run 的视觉状态和 runtime 保持一致。
18. As a Noloong 用户, I want assistant 回复有流式尾迹, so that 输出看起来顺滑且能感知模型仍在生成。
19. As a Noloong 用户, I want 流式尾迹基于真实 delta 而不是假动画, so that 视觉效果不伪造模型输出。
20. As a Noloong 用户, I want final assistant message 到达后稳定替换 streaming bubble, so that transcript 不出现重复回复。
21. As a Noloong 用户, I want 思考中看到低干扰的思考展示, so that 我知道 agent 正在推理而不是卡住。
22. As a Noloong 用户, I want 有 reasoning summary 时优先看 summary, so that 我能快速理解 agent 的思考方向。
23. As a Noloong 用户, I want 可展示 reasoning 原文时能按需展开, so that 我可以调试或审查更细的推理过程。
24. As a Noloong 用户, I want 思考结束后折叠成耗时摘要, so that transcript 保持干净。
25. As a Noloong 用户, I want 工具执行以 inline activity 展示, so that 我能看到 agent 正在做什么而不被日志淹没。
26. As a Noloong 用户, I want 工具输出默认折叠, so that 长输出不会破坏阅读节奏。
27. As a Noloong 用户, I want 能展开工具输出, so that 我可以检查命令结果或错误细节。
28. As a Noloong 用户, I want 超长工具输出用虚拟滚动或文件链接呈现, so that GUI 不会因为大输出卡顿。
29. As a Noloong 用户, I want 审批请求以内联卡片出现, so that 我可以在当前 transcript 中理解审批上下文。
30. As a Noloong 用户, I want 能在审批卡片中同意或拒绝, so that 我不需要切到其它控制面处理工具权限。
31. As a Noloong 用户, I want 审批态清楚显示 run 已暂停, so that 我知道 agent 在等待我的决定。
32. As a Noloong 用户, I want 输入区的停止按钮中止当前 run, so that 我可以终止失控或不需要的任务。
33. As a Noloong 用户, I want 停止运行不等同于拒绝审批或删除会话, so that 操作语义明确。
34. As a Noloong 用户, I want 输入区支持多行输入, so that 我可以发送较长的任务说明。
35. As a Noloong 用户, I want Enter 发送、Shift+Enter 换行, so that 键盘交互符合主流 Chat 体验。
36. As a Noloong 用户, I want 输入区只显示真实可用的 profile/model/workdir 状态, so that 我不会被假控件误导。
37. As a Noloong 用户, I want 通过文件选择添加附件, so that 我可以把本地文件发给 agent。
38. As a Noloong 用户, I want 通过拖拽文件添加附件, so that 文件输入更符合桌面使用习惯。
39. As a Noloong 用户, I want 附件显示为紧凑 chip, so that 我能确认即将发送哪些文件。
40. As a Noloong 用户, I want 附件实际落到消息 media block, so that runtime 和 provider 能真实消费这些文件。
41. As a Noloong 用户, I want 不支持的附件能力被隐藏或禁用, so that UI 不显示无法兑现的承诺。
42. As a Noloong 用户, I want 右侧浮动工具栏保留 Chat、运行工具、Settings 等入口, so that 主画布保持简洁。
43. As a Noloong 用户, I want Settings 仍可随时打开, so that 我能调整 profile、provider、MCP、skills 和运行策略。
44. As a Noloong 用户, I want 从 Settings 返回 Chat 时当前会话状态不丢失, so that 配置入口不会打断工作流。
45. As a Noloong 用户, I want 缺少配置时在 Chat 中得到低干扰引导, so that 我可以进入 Settings 创建配置而不是遇到硬错误。
46. As a Noloong 用户, I want 运行失败以内联错误呈现, so that 我能在任务上下文中理解失败原因。
47. As a Noloong 用户, I want 网络或 runtime 断开时看到明确状态, so that 我知道是连接问题而不是模型无响应。
48. As a Noloong 用户, I want Chat 视觉贴近参考视频的暗色沉浸体验, so that 桌面端是一个舒适的长期工作入口。
49. As a Noloong 用户, I want 视觉复刻保留 Noloong 品牌, so that app 不像外部产品克隆。
50. As a Noloong 开发者, I want GUI 只通过 interaction 协议通信, so that embedded 和 external runtime 使用同一套客户端路径。
51. As a Noloong 开发者, I want GUI 不直接持有 registry, so that app 不形成特殊内部协议。
52. As a Noloong 开发者, I want Chat 只消费展示事件, so that Telegram、微信和桌面端能共享展示语义。
53. As a Noloong 开发者, I want 展示事件补齐 reasoning 语义, so that GUI 不需要窥探原始 provider event。
54. As a Noloong 开发者, I want Chat 状态机可单独测试, so that UI 复杂度不会扩散到视图代码中。
55. As a Noloong 开发者, I want interaction client 是轻量边界, so that `noloong-app` 不重新依赖完整 agent runtime。
56. As a Noloong 开发者, I want 流式尾迹动画与数据合并解耦, so that 可以稳定测试 reducer 而不依赖渲染帧。
57. As a Noloong 开发者, I want 附件构建逻辑独立测试, so that file picker 和 drag/drop 不会产生不同 message shape。
58. As a Noloong 开发者, I want approval action 复用 interaction 协议, so that GUI 与 Telegram 的审批语义一致。
59. As a Noloong 开发者, I want Chat 手动 smoke 能使用真实 profile, so that 桌面端上线前能验证真实 provider 路径。
60. As a Noloong 维护者, I want PRD 和 ADR 记录主交互客户端边界, so that 后续 agent 不会把 app 退化回配置工具。

## Implementation Decisions

- `noloong app` 默认落点改为 Chat 画布；Settings 是辅助配置入口。
- app 默认使用 embedded interaction runtime，但 embedded mode 也走 loopback JSON-RPC interaction 协议。
- 外部 runtime connection 保留为高级模式，使用同一 typed interaction client。
- GUI 不直接持有 registry，不调用 runtime 内部对象。
- GUI 只消费展示事件；展示事件缺失语义时扩展展示事件。
- 展示事件新增 reasoning/thought 语义，覆盖思考开始、summary/raw delta、完成和耗时信息。
- 现有 core 的 thinking stream 是事实来源；interaction projector 负责把它投影到展示事件。
- Chat 状态需要一个深模块：`ChatSessionStore` 或等价模型，封装 session list、current session、transcript recovery、live display event application 和 run state。
- interaction 通信需要一个深模块：typed `InteractionClient`，封装 initialize、profile list、session create/list/get、agent prompt/abort、display subscribe、approval resolve。
- DisplayEvent reducer 需要是深模块，输入为历史 agent state 与 live 展示事件，输出为 UI 可渲染 transcript view model。
- Streaming tail 需要是独立视觉状态模块，负责 delta batching、segment age、opacity ramp 和 final stabilization，不负责协议或 session 状态。
- Tool activity reducer 需要把 tool started/updated/completed 聚合为可折叠 activity row。
- Approval reducer 需要把 approval requested/resolved 语义映射为 transcript inline card 与操作状态。
- Composer model 需要封装 draft text、附件列表、send/stop 状态、keyboard behavior 和 disabled reason。
- 附件输入使用现有 media block 结构：本地文件通过 URI media source 表达，并设置 kind、name、mime type。
- 不支持的附件能力不显示；不能发送的附件不进入 draft。
- 会话标题是 agent 会话 metadata。默认标题可由第一条用户消息本地生成，后续用户可手动重命名。
- 会话工作目录是真实运行上下文。新会话默认使用 app cwd 或 profile 默认 cwd；切换工作目录不影响已经运行中的 run。
- 输入区 stop 按钮调用 agent abort；它只停止当前 run，不拒绝审批、不删除会话。
- 流式文本使用真实 assistant delta；新 segment 以 120-180ms opacity ramp 进入稳定状态。
- very small delta 可以按 16-32ms 批处理，减少视觉抖动。
- final assistant message 到达后替换对应 streaming bubble，避免重复内容。
- 思考展示优先使用 reasoning summary；原文只在存在且允许展示时可展开。
- 思考结束后折叠为耗时摘要，类似 “Thought for 2 seconds”。
- 工具活动和审批请求属于运行活动，不作为普通 assistant 消息混入 transcript。
- 长工具输出使用折叠、虚拟滚动或文件链接，不能撑爆 Chat 画布。
- Chat 视觉方向遵守 Noloong-native 高保真复刻：复刻布局节奏、输入区手感和流式尾迹，不复制外部品牌或协议模型。
- 配置入口继续承载 profile、provider、MCP、skills、storage、runtime 等设置，不进入 Chat 的主视觉层。
- 现有 ADR-0001 是本 PRD 的架构约束。

## Testing Decisions

- 测试应覆盖外部行为和协议契约，不测试视图内部布局实现细节。
- DisplayEvent serde 和 projector 需要单测：text delta、thinking delta、reasoning summary、run completed、run failed、run aborted。
- DisplayEvent reducer 需要单测：历史 transcript 恢复、streaming delta 合并、final replacement、run failed、run paused、tool activity 聚合、approval card 状态。
- InteractionClient 需要协议测试：request/response DTO round trip、display subscription notification decode、abort/prompt/approval resolve 请求 shape。
- ChatSessionStore 需要状态机测试：无会话、新建会话、切换会话、运行中切换、session list 更新、metadata title 更新。
- Composer model 需要单测：Enter send、Shift+Enter newline、empty send disabled、stop button state、first send creates session。
- 附件输入需要单测：file path 到 media block、mime/name 推断、unsupported file handling、file picker 与 drag/drop 生成一致 message shape。
- Streaming tail 需要可控时钟测试：segment 入场 opacity、批处理窗口、final stabilization。
- Tool activity 需要测试长输出 folding 和 completed/error 状态。
- Approval flow 需要测试 approve/reject、paused run、审批后继续、停止运行与拒绝审批语义区分。
- App launch 需要测试：默认 Chat route、missing config guidance、embedded runtime options、external runtime options。
- GUI smoke 需要用真实 profile 跑一次：新建会话、发送文本、流式回复、stop、approval、工具活动和附件。
- 回归测试应包括 app crate、interaction protocol、agent interaction tests、root CLI app command。
- 视觉 smoke 应使用 Computer Use 验证 Chat 默认打开、浮动工具栏、输入区、streaming bubble 和 Settings 切换。

## Out of Scope

- 不做 pixel-perfect 外部产品克隆。
- 不复制外部产品品牌资产、文案或私有交互模型。
- 不让 GUI 订阅 raw event。
- 不让 GUI 直接持有 registry。
- 不实现 GUI 专属 session/transcript 数据源。
- 不做假工具数量、假记忆、假令牌统计或假能力展示。
- 不实现语音输入、录音、截图捕获、剪贴板图片自动捕获。
- 不实现完整 IDE project/language-server 栈。
- 不重做 Telegram 或微信客户端；它们只受益于展示事件语义补齐。
- 不在本 PRD 中改写 Settings 的完整配置体验。
- 不保留历史兼容状态；当前项目没有兼容性包袱。

## Further Notes

- 本 PRD 基于 `CONTEXT.md` 中的领域词汇和 ADR-0001。
- 参考视频的重点是窗口节奏、输入区手感和流式输出的细腻程度，不是 UI 文案或品牌复制。
- 需要避免让 `noloong-app` 因 Chat 接入重新依赖完整 agent runtime；runtime 由 root CLI embedded mode 或外部服务提供。
- 如果实现中发现展示事件不足，应优先补 DisplayEvent 协议，而不是在 GUI 内临时解析 raw event。
- 如果后续希望把 Chat 做成远程 interaction client，当前 loopback JSON-RPC 边界应天然支持。
