# PRD：Noloong-native Chat 高保真视觉改进

Status: ready-for-agent

## Problem Statement

`noloong app` 的 Chat 功能链路已经可用：它能通过 embedded interaction runtime 创建和继续 agent 会话，发送消息，消费展示事件，展示流式回复、运行状态和输入区。但当前视觉仍明显偏“功能验证版”：会话列表、气泡、边框、输入区和右侧工具栏都带有较强的设置页/控制台感，与参考视频中沉浸、克制、轻量、流式输出细腻的 Noloong-native 目标差距较大。

用户希望 Noloong 的主交互客户端不只是“能用”，而是成为一个可以长期停留、舒服阅读、清楚观察 agent 运行过程的桌面工作界面。当前 UI 主要问题是视觉层级过重、布局不够像沉浸式 Chat 画布、assistant 正文过度卡片化、输入区没有参考视频里的高级手感、流式尾迹还不够细腻，且 session list 和工具栏仍像功能面板而不是自然融入画布的导航与控制。

## Solution

在不改变现有 interaction 协议边界、不引入假数据、不复制外部品牌的前提下，对 `noloong app` 的 Chat 画布做一次专门的高保真视觉改进。目标是保留已经稳定的真实运行链路，重做 Chat 视觉层：窗口/title bar 信息层级、沉浸式 transcript、轻量会话导航、底部输入区、右侧浮动工具栏、思考展示、运行活动卡片和流式尾迹。

最终体验应接近参考视频的“窗口节奏、暗色沉浸感、底部输入区手感、正文流式输出和轻量工具栏”，但仍使用 Noloong 自己的品牌、图标、文案、interaction 协议和展示事件模型。Chat 画布应该看起来像主交互客户端，而不是 Settings 的另一个表单页。

## User Stories

1. As a Noloong 用户, I want Chat 画布视觉接近参考视频的沉浸式对话工作区, so that 我愿意把 `noloong app` 当作主要工作入口长期打开。
2. As a Noloong 用户, I want Chat 默认界面不再像 Settings 表单, so that 我能清楚区分主交互客户端和配置入口。
3. As a Noloong 用户, I want 当前会话的标题栏信息克制居中, so that 我能看到当前任务、profile/model 和工作目录而不被工具按钮干扰。
4. As a Noloong 用户, I want macOS title bar 的高度、留白和按钮位置自然, so that app 看起来像成熟桌面应用。
5. As a Noloong 用户, I want 会话列表不占用过多横向空间, so that transcript 和输入区成为视觉主体。
6. As a Noloong 用户, I want 会话列表可以折叠为轻量 rail 或小型面板, so that 多会话能力存在但不压迫主画布。
7. As a Noloong 用户, I want 当前会话状态在会话列表中低干扰显示, so that 我能知道 run 状态而不被彩色标签抢视线。
8. As a Noloong 用户, I want 用户消息保持紧凑、右侧对齐、低对比气泡, so that 我能快速区分自己输入和 assistant 输出。
9. As a Noloong 用户, I want assistant 最终回复以正文流呈现而不是强卡片气泡, so that 长文本阅读更自然。
10. As a Noloong 用户, I want assistant 正文有舒适行宽、行高和段落间距, so that 长输出不累眼。
11. As a Noloong 用户, I want assistant 正文不被粗边框包围, so that transcript 更像自然文档流。
12. As a Noloong 用户, I want 只有运行活动、审批和工具输出使用必要的 inline activity 容器, so that 功能状态和正文层级分明。
13. As a Noloong 用户, I want 流式回复的新文字有短暂柔和尾迹, so that 我能感知模型正在生成而不是硬跳文本。
14. As a Noloong 用户, I want 流式尾迹不做逐字弹跳, so that 输出感觉高级、平稳、不廉价。
15. As a Noloong 用户, I want 流式尾迹跟随真实 delta 节奏, so that UI 不伪造或重排模型内容。
16. As a Noloong 用户, I want 快速连续 delta 被轻量批处理, so that 文本不会高频闪烁。
17. As a Noloong 用户, I want final assistant message 到达后无感稳定, so that streaming bubble 不突然跳位或重复。
18. As a Noloong 用户, I want transcript 自动贴底只在我已经接近底部时发生, so that 我阅读历史时不会被新 token 强制拉走。
19. As a Noloong 用户, I want transcript 滚动条低调但可见, so that 长输出可控又不破坏视觉。
20. As a Noloong 用户, I want 思考展示在运行中清楚但低干扰, so that 我知道 agent 正在推理。
21. As a Noloong 用户, I want 有 reasoning summary 时优先展示 summary, so that 我能快速理解思考方向。
22. As a Noloong 用户, I want 思考结束后自动折叠为 “Thought for N seconds” 风格摘要, so that transcript 保持干净。
23. As a Noloong 用户, I want 可展示 reasoning 原文时能够展开, so that 我可以按需审查细节。
24. As a Noloong 用户, I want 工具活动默认以小型 inline row 展示, so that 我能看到 agent 在工作但不被日志淹没。
25. As a Noloong 用户, I want 工具活动完成后折叠为摘要, so that 历史 transcript 不充满过程噪音。
26. As a Noloong 用户, I want 工具错误有足够可见性, so that 我能知道 run 为什么失败。
27. As a Noloong 用户, I want 审批卡片视觉明确但不粗糙, so that 我可以放心同意或拒绝工具请求。
28. As a Noloong 用户, I want 审批卡片展示最少必要信息, so that 不暴露内部 JSON 或模型请求细节。
29. As a Noloong 用户, I want 输入区像参考视频一样是底部统一输入台, so that 文本输入、附件、模型状态和运行控制属于同一个自然区域。
30. As a Noloong 用户, I want 输入区完整可见且不会被底部裁切, so that 我能稳定输入和发送。
31. As a Noloong 用户, I want 输入区整块都能聚焦文本输入, so that 不需要精确点击一个小区域。
32. As a Noloong 用户, I want 输入文本始终清晰可见, so that 我能确认即将发送的内容。
33. As a Noloong 用户, I want 发送按钮只在可发送时亮起, so that UI 状态可信。
34. As a Noloong 用户, I want 运行中发送按钮切换为停止按钮, so that 我可以直觉中止当前 run。
35. As a Noloong 用户, I want 停止按钮视觉上表达危险但不过度刺眼, so that 它明确但不破坏整体风格。
36. As a Noloong 用户, I want 输入区中的 profile/model/workdir 状态来自真实运行上下文, so that 没有假工具数量、假 memory 或假 token。
37. As a Noloong 用户, I want 附件按钮和附件 chip 与输入区视觉一致, so that 文件输入不显得像临时补丁。
38. As a Noloong 用户, I want 右侧浮动工具栏更轻、更窄、更圆润, so that 它像悬浮控件而不是侧边栏。
39. As a Noloong 用户, I want 右侧浮动工具栏只使用图标, so that 视觉更加简洁。
40. As a Noloong 用户, I want 工具栏 hover/focus/active 状态柔和, so that 交互反馈高级且不刺眼。
41. As a Noloong 用户, I want Settings 入口仍然清楚, so that 我可以回到配置入口调整 profile、MCP、skills 和 provider。
42. As a Noloong 用户, I want Settings 入口不主导 Chat 视觉, so that 主交互客户端仍以对话为中心。
43. As a Noloong 用户, I want Chat 中的空态简洁自然, so that 没有会话时也不是设置页或错误页。
44. As a Noloong 用户, I want 缺少配置时在 Chat 画布内得到低干扰引导, so that 我知道去配置入口补齐设置。
45. As a Noloong 用户, I want 连接失败或 runtime 不可用时看到自然错误状态, so that 我知道问题在后端连接而不是 UI 卡住。
46. As a Noloong 用户, I want app 暗色主题颜色更高级, so that 不像简单深蓝/灰色堆叠。
47. As a Noloong 用户, I want 边框、背景和阴影层次更克制, so that 信息层级通过空间和透明度表达。
48. As a Noloong 用户, I want 关键字体大小和字重贴近参考视频, so that 阅读节奏和窗口气质一致。
49. As a Noloong 用户, I want 中文输出在行高、字距和段落间距上舒适, so that 长中文回复不会拥挤。
50. As a Noloong 用户, I want 英文 locale 下同样保持视觉质量, so that i18n 不破坏布局。
51. As a Noloong 用户, I want 在窄窗口下布局仍可用, so that 调整窗口大小不会让输入区或工具栏遮挡内容。
52. As a Noloong 用户, I want 在大窗口下 transcript 不无限拉宽, so that 长行不会难读。
53. As a Noloong 用户, I want 浮动工具栏不会遮挡消息正文, so that 右侧空间始终可读。
54. As a Noloong 用户, I want assistant 输出贴底时不会被 composer 遮挡, so that 最新回复完整可见。
55. As a Noloong 用户, I want 新用户消息发送后立即可见, so that 我有明确提交反馈。
56. As a Noloong 用户, I want 最终回复完成后布局不突然跳动, so that 视觉稳定。
57. As a Noloong 用户, I want 可以通过录屏对比参考视频检查流式输出, so that 改进有明确视觉验收依据。
58. As a Noloong 设计维护者, I want Chat 视觉 token 集中管理, so that 后续颜色、圆角、阴影和间距可以统一调整。
59. As a Noloong 设计维护者, I want Chat 组件边界清楚, so that 之后继续微调不会把 view 文件变成巨型条件堆。
60. As a Noloong 开发者, I want 保留现有 DisplayEvent 和 typed interaction client 边界, so that 高保真视觉不会破坏多客户端架构。
61. As a Noloong 开发者, I want streaming animation 独立于 reducer 逻辑, so that 动效可以迭代而不影响协议和状态测试。
62. As a Noloong 开发者, I want transcript 布局和 composer 布局有专门模块, so that 布局问题能局部修复。
63. As a Noloong 开发者, I want 视觉 smoke 能通过 Computer Use 或截图录屏复核, so that 高保真目标不是主观口头判断。
64. As a Noloong 开发者, I want 关键帧对比留在本地审计记录中, so that 后续 agent 能知道为什么这些视觉细节重要。
65. As a Noloong 维护者, I want 此轮只做视觉与交互质感, so that 不把 Chat runtime、Settings、MCP 和 provider 配置改造混在一起。

## Implementation Decisions

- 本 PRD 是对已完成主交互客户端功能链路的视觉改进，不重写 interaction runtime。
- 保留 `noloong app` 作为主交互客户端，Settings 仍是配置入口。
- GUI 继续只通过 interaction 协议和展示事件工作；不直接持有 registry，不订阅 raw event。
- 不新增假数据。输入区、工具栏、状态、模型、工作目录和运行活动必须来自真实 app state、profile 或 DisplayEvent。
- 当前 Chat 功能状态模型保留；视觉层通过新的渲染模块消费现有 view model。
- 提取 Chat 视觉 token 模块，集中定义颜色、透明度、圆角、边框、阴影、行高、composer 高度、工具栏尺寸和 animation timing。
- 提取 Chat canvas layout 模块，负责 title bar 下方主区域、会话导航、transcript、composer 和浮动工具栏的空间关系。
- 提取 transcript rendering 模块，区分 stable transcript、live assistant stream、thought summary、tool activity 和 approval cards。
- 提取 streaming text renderer 或等价深模块，封装 delta segment aging、opacity ramp、batching 和 final stabilization。
- 提取 composer surface 模块，封装输入区整体布局、focus hit area、附件 chip、send/stop 控件和状态栏。
- 提取 floating toolbar 模块，封装 Chat/Tools/Settings 等入口的 icon-only 视觉、active 状态和 hover/focus 反馈。
- 提取 session rail 模块，替代当前偏重的 session card 列表；默认应更窄，可折叠，不压迫 transcript。
- Title bar 信息层级需要贴近参考视频：中心显示当前会话标题，副标题显示消息数、profile/model 或工作目录等真实信息。
- 会话列表不应像 Settings sidebar；它是 Chat 画布中的轻量导航。
- Assistant 正文默认不使用强边框气泡；用户消息可以保留紧凑右侧气泡。
- Tool activity、approval 和 error 使用低干扰 inline activity 容器，而不是普通 assistant bubble。
- Thought UI 在运行中可展开，完成后默认折叠为耗时摘要。
- 有 reasoning summary 时始终优先展示 summary；reasoning raw 只在存在且允许展示时可展开。
- 流式尾迹使用真实 DisplayEvent delta；可对微小 delta 做 16-32ms 视觉批处理。
- 新文本片段以短时间低透明进入并稳定，禁止逐字弹跳和伪造字符。
- Transcript 自动贴底逻辑保留近底跟随策略；用户主动上滚阅读时不强制跳到底。
- Composer 固定为 chat workspace 底部 non-shrinking footer；外层页面不滚动，transcript 是唯一主滚动区域。
- Composer 整块点击应聚焦 input；send button 的状态必须与 `can_send`/run status 一致。
- Composer 的附件、workdir、模型状态和运行控制应融入同一低对比面板，不做多个割裂按钮堆叠。
- 右侧工具栏更窄、圆角更大、透明度更轻；按钮用图标，不使用文字。
- 工具栏必须避开 transcript 正文和 composer，不能遮挡最新内容。
- 保留中英文 i18n；但 UI 同一时刻只显示当前 locale 的文案。
- 参考视频仅作为体验目标，不作为品牌、图标或文案来源。
- 如果视觉实现需要新增 icon，应使用 Noloong 自有资产或通用符号，不复制参考产品图标。
- 本轮不扩大 `noloong-app` 对 runtime crate 的依赖；它仍保持 app/client 边界。
- 本轮应避免把单个 view 文件继续做大；新增视觉模块应是深模块、可局部测试。
- 视觉验收需要保留关键帧截图、当前 app 截图和对比结论，放入本主题的审计记录。

## Testing Decisions

- 测试重点是外部行为、状态映射和布局约束，不测试具体像素值。
- Streaming text renderer 需要用可控时钟测试：segment 创建、opacity ramp、batching、final stabilization。
- Transcript model/render adapter 需要测试：assistant 正文 vs user bubble、thought completed collapse、tool activity collapse、approval card 状态。
- Composer model/view adapter 需要测试：empty disabled、text visible、send/stop state、attachment chip 状态、focus hit area 可通过集成 smoke 验证。
- Session rail model 需要测试：active session、running/paused/failed/completed 状态、折叠/展开数据不丢失。
- Floating toolbar model 需要测试：route active state、disabled placeholder、Settings 切换不丢失当前会话。
- Layout 约束需要通过 GPUI app smoke 或 screenshot 验证：composer 不裁切、toolbar 不遮挡、transcript 贴底、窄窗口不崩。
- i18n 检查需要覆盖 zh/en 两种 locale：Chat 标题、空态、composer placeholder、run status、thought summary、toolbar tooltip。
- 视觉 smoke 需要使用真实 `noloong app`、真实 profile 和 Computer Use 截图。
- 流式输出 smoke 应录屏或截取连续帧，用 ffmpeg 抽帧对比是否有尾迹、跳动或遮挡。
- 回归命令至少包括 app tests、root CLI tests、app clippy 和格式检查。
- 如果视觉改动触碰 DisplayEvent 语义，需要补 interaction protocol/projector 测试。
- 如果只改视觉 token、layout 和 renderer，不需要新增 provider 或 runtime 测试。

## Out of Scope

- 不重写 interaction runtime。
- 不改变 `noloong app` 的 embedded/external runtime 架构。
- 不让 GUI 直接持有 registry。
- 不订阅 raw event。
- 不实现新的 provider、MCP 或 skills 配置能力。
- 不重做 Settings 信息架构。
- 不做 Telegram 或微信客户端视觉改造。
- 不做 pixel-perfect 外部产品克隆。
- 不复制参考视频里的品牌资产、图标、私有文案或产品身份。
- 不显示假工具数量、假 token、假 memory、假模型能力或假上传状态。
- 不把 JSONC editor、profile 设置或其它配置页混进 Chat 视觉主线。
- 不在本 PRD 中实现语音输入、截图、屏幕录制入口或剪贴板图片捕获。

## Further Notes

- 参考视频路径：`/Users/m4n5ter/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/wxid_vu2ffheq52ea21_fa4a/temp/RWTemp/2026-05/05918b4efaee81172e0868e6a0daedd3/123562583becf6174d04731106679fb2.mp4`。
- 当前抽帧显示的关键体验：大圆角沉浸窗口、居中任务标题、低对比副标题、正文无强气泡、底部整合输入台、右侧轻量竖向工具栏、thought 完成后折叠、流式正文尾部渐入。
- 当前 app 已解决功能链路、composer 可见性、用户消息 optimistic append、transcript tail follow 和真实 Computer Use smoke；本 PRD 不回退这些成果。
- 现有 `.scratch/noloong-app-chat-client/issues/09-noloong-native-high-fidelity-visual-integration.md` 已指出需要人类视觉确认。本 PRD 将该方向扩展为可交给 agent 执行的完整产品要求。
- 无兼容性负担：如果现有 Chat 视觉结构妨碍高保真目标，可以直接替换，不需要保留旧布局。
