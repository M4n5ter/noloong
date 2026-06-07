# PRD：Noloong app Tauri 迁移

Status: ready-for-agent

## Problem Statement

用户希望 `noloong app` 成为 Noloong 的主交互客户端，但当前 GPUI 实现已经在核心体验上持续暴露框架摩擦：输入区焦点、文本输入、自动滚动、流式回复、JSONC 编辑器、窗口标题栏和高保真动画都很难稳定打磨。经过较长时间调试，GPUI 版本仍难以达到主交互客户端应有的成熟度和人体工程学体验。

Noloong 已经通过领域文档确认：桌面 app 是主交互客户端，默认进入 Chat 画布，并通过 interaction 协议与 agent 会话通信。现在的问题不是是否要做 GUI，而是应当停止在 GPUI 上继续堆补丁，换成更成熟、更易测试、更适合复杂桌面交互的 Tauri/WebView 架构。

## Solution

将 `noloong app` 的主交互客户端实现从 GPUI 迁移到 Tauri/WebView。Rust 继续负责本地优先能力、内嵌 interaction runtime、配置读写、app bootstrap 和 native shell；用户界面改用 React、TypeScript、Vite、CodeMirror 6 和 Web 技术实现。前端通过 interaction HTTP/WebSocket 直接消费 agent 会话、展示事件和会话快照；Tauri commands 只负责 bootstrap、配置文件、schema、文件选择和其它 native 能力。

迁移不保留 GPUI 作为 fallback。项目没有兼容性负担，因此旧 GPUI view/runtime、自制 macOS bundle helper 和 GPUI 专属 assets 可以直接清理。`noloong app` 仍是用户入口，app identity 使用真实域名对应的 bundle identifier，窗口采用 Tauri 自绘 title bar 以支持 Noloong-native 高保真体验。

第一阶段目标是让 Tauri 主交互客户端替代 GPUI 入口并稳定跑通核心路径：打开 app、读取 bootstrap、进入 Chat 画布、连接 embedded 或 external interaction runtime、显示会话状态、通过展示事件呈现实时过程、通过会话快照完成权威收敛，并提供可编辑配置入口。视频级高保真动画、完整附件体验和复杂工具卡片作为后续阶段。

## User Stories

1. As a Noloong 用户, I want `noloong app` 打开 Tauri 主交互客户端, so that 我不再受 GPUI 输入、滚动和编辑器问题影响。
2. As a Noloong 用户, I want app 默认进入 Chat 画布, so that 我可以直接创建或继续 agent 会话。
3. As a Noloong 用户, I want app 默认启动本地内嵌 runtime, so that 我不用手动启动外部服务。
4. As a Noloong 高级用户, I want app 能连接外部 interaction runtime, so that 我可以使用远程或共享 agent 会话。
5. As a Noloong 用户, I want 桌面 app 继续使用 `noloong app` 启动, so that CLI 入口保持简单。
6. As a Noloong 用户, I want app 在 macOS 中显示为正式 Noloong 应用, so that 系统权限、Dock、Computer Use 和窗口识别都稳定。
7. As a Noloong 用户, I want app 使用 `com.noloong.desktop` 身份, so that 系统级身份与真实域名一致且不与 `.app` bundle 目录后缀混淆。
8. As a Noloong 用户, I want 看到 Noloong 自有 icon 和 product name, so that 应用身份清晰。
9. As a Noloong 用户, I want title bar 是 Noloong-native 自绘体验, so that 会话标题、profile、工作目录和操作按钮可以自然融入窗口。
10. As a Noloong 用户, I want 自绘 title bar 保留系统窗口行为, so that 拖动、全屏和窗口控制符合 macOS 习惯。
11. As a Noloong 用户, I want Chat 画布继续遵守 Noloong-native 高保真方向, so that 桌面端像成熟主交互客户端而不是普通网页。
12. As a Noloong 用户, I want app 不复制外部产品品牌, so that Noloong 有自己的身份和文案。
13. As a Noloong 用户, I want 输入区整块可靠聚焦, so that 我不需要精确点击小区域才能输入。
14. As a Noloong 用户, I want 输入内容立即可见且可换行, so that 长任务描述不会被截断。
15. As a Noloong 用户, I want 发送消息后用户消息立即出现在 transcript, so that 我知道提交成功。
16. As a Noloong 用户, I want assistant 回复按真实 delta 流式出现, so that 我能观察模型生成过程。
17. As a Noloong 用户, I want 新内容到达时自动跟随到底部, so that 我能持续看到最新输出。
18. As a Noloong 用户, I want 主动上滚阅读时不被强制拉到底部, so that 我能检查历史内容。
19. As a Noloong 用户, I want run 完成后 UI 可靠收敛到最终 transcript, so that 不会停留在部分流式输出。
20. As a Noloong 用户, I want 运行过程由展示事件驱动, so that GUI 不伪造 provider 输出。
21. As a Noloong 用户, I want 稳定 transcript 由会话快照恢复, so that 重启或切换会话后历史状态可靠。
22. As a Noloong 用户, I want 思考展示优先显示 reasoning summary, so that 我能快速理解 agent 的思考方向。
23. As a Noloong 用户, I want 存在 reasoning 原文时可以展开查看, so that 我能按需审查细节。
24. As a Noloong 用户, I want 思考结束后折叠为耗时摘要, so that transcript 保持干净。
25. As a Noloong 用户, I want tool 和 approval 作为运行活动呈现, so that 我能理解 agent 正在执行什么。
26. As a Noloong 用户, I want 审批请求可以在桌面 app 内处理, so that 我不用切换到 Telegram 或其它控制面。
27. As a Noloong 用户, I want 停止按钮中止当前 run, so that 我可以打断不需要的执行。
28. As a Noloong 用户, I want 停止运行不等于拒绝审批或删除会话, so that 操作语义明确。
29. As a Noloong 用户, I want 会话列表能够创建、选择和观察 agent 会话, so that 多个任务可以并行存在。
30. As a Noloong 用户, I want 当前会话标题来自真实 agent 会话 metadata, so that app 不维护第二套标题来源。
31. As a Noloong 用户, I want 会话工作目录是真实运行上下文, so that 工具执行和相对路径可预期。
32. As a Noloong 用户, I want Settings 仍作为配置入口存在, so that 我能编辑 profile、provider、MCP、skills、storage 和 runtime 设置。
33. As a Noloong 用户, I want 配置入口不再是 app 默认落点, so that 主体验以 agent 会话为中心。
34. As a Noloong 用户, I want JSONC 编辑使用成熟代码编辑器, so that 光标、滚动、补全和快捷键稳定。
35. As a Noloong 用户, I want JSONC 编辑支持 schema-aware completion, so that 配置复杂项时不需要记住全部字段。
36. As a Noloong 用户, I want Profile 表单和 JSONC 编辑能双向同步, so that 我可以选择最舒服的配置方式。
37. As a Noloong 用户, I want 无效 JSONC 阻止保存并给出清晰错误, so that 配置不会被坏状态污染。
38. As a Noloong 用户, I want 保存配置时仍由 Rust parser 和 validator 判定, so that 配置事实来源一致。
39. As a Noloong 用户, I want UI 文案支持 zh/en, so that 运行时只显示当前语言。
40. As a Noloong 用户, I want 缺少配置时在 Chat 中得到清晰引导, so that 我知道如何进入配置入口补齐设置。
41. As a Noloong 用户, I want 连接失败时看到明确状态, so that 我能区分 runtime 问题和模型问题。
42. As a Noloong 用户, I want dev 模式能通过 Bun 启动前端和 Tauri, so that 前端调试和热更新高效。
43. As a Noloong 用户, I want release 构建由 Tauri bundler 生成正式应用, so that 不再依赖自制 app bundle helper。
44. As a Noloong 开发者, I want interaction DTO 是正式协议类型, so that server、desktop app 和测试 fake 不再维护重复 shape。
45. As a Noloong 开发者, I want Rust DTO 自动生成 TypeScript 类型, so that 前端不会手写漂移的协议接口。
46. As a Noloong 开发者, I want profile config 类型也生成 TypeScript 类型, so that Settings 前端能获得类型提示。
47. As a Noloong 开发者, I want 前端直接使用 interaction HTTP/WebSocket, so that chat/display 不被 Tauri command 代理层重新定义。
48. As a Noloong 开发者, I want Tauri commands 只处理 native shell 和配置文件能力, so that 协议边界清晰。
49. As a Noloong 开发者, I want 前端状态使用小型领域 store, so that Chat 状态机可以独立测试。
50. As a Noloong 开发者, I want 不引入 Redux 或大状态框架作为第一阶段依赖, so that 状态模型保持直接。
51. As a Noloong 开发者, I want 不把 Lobe UI 作为主设计系统, so that Noloong-native 视觉不被 AntD/Lobe 风格牵引。
52. As a Noloong 开发者, I want 可以按组件级别评估第三方 UI, so that 不排除局部复用高价值组件。
53. As a Noloong 开发者, I want CodeMirror 负责 JSONC 编辑器能力, so that 不再手写编辑器行为。
54. As a Noloong 开发者, I want Playwright 能验证流式 UI 行为, so that 不再依赖人工盯屏判断是否实时输出。
55. As a Noloong 开发者, I want fake interaction server 支撑前端测试, so that 不需要真实模型也能稳定复现 display stream。
56. As a Noloong 开发者, I want 真实 ChatGPT smoke 只作为手动验收, so that 自动测试不会依赖网络和订阅状态。
57. As a Noloong 开发者, I want 删除 GPUI 依赖和宏优化配置, so that dev 编译不再为废弃 UI 栈付成本。
58. As a Noloong 开发者, I want 删除 GPUI view/runtime 代码, so that 旧实现不会继续污染抽象边界。
59. As a Noloong 开发者, I want 保留非 UI 的 provider timeout 修复, so that 真实 ChatGPT Responses 长任务不会被短超时中断。
60. As a Noloong 维护者, I want ADR 说明为什么迁移到 Tauri/WebView, so that 未来不会误以为这是审美偏好而非工程决策。

## Implementation Decisions

- 主交互客户端从 GPUI 迁移到 Tauri/WebView；GPUI 不作为 fallback 保留。
- `noloong app` 仍是用户入口，仍进入同一个主二进制的 app 模式。
- Tauri shell 归属于现有 app host；Web 前端作为独立 Bun/Vite/React 工作区存在。
- 包管理使用 Bun，而不是 pnpm、npm 或 Yarn。
- 前端栈使用 React、TypeScript、Vite 和 CodeMirror 6。
- 不采用 Lobe UI 作为主设计系统；只保留按组件级别局部评估的可能。
- 第一阶段先使用 CSS transition、Web Animations API 和少量自有组件；只有在复杂动画确实需要时再引入专门 motion 库。
- app identity 使用 product name `Noloong` 和 bundle identifier `com.noloong.desktop`。
- 应用图标沿用当前已经被 macOS 正常识别的 Noloong icon 资产，不继续手写不准确的 SVG logo。
- Tauri 使用自绘 title bar；保留系统窗口行为和 macOS 交通灯。
- 旧自制 macOS app bundle helper 删除；开发和发布都交给 Tauri 工具链。
- `noloong app` 开发期必须可用；同时提供 Bun/Tauri dev 命令用于前端热更新。
- 内嵌 runtime 和 external runtime 都通过 interaction 协议通信。
- 前端直接使用 interaction HTTP/WebSocket，不通过 Tauri command 代理 chat/display。
- Tauri commands 负责 bootstrap、profile config 读写、schema、文件选择、目录选择和后续 native-only 能力。
- 展示事件是实时过程的数据源；会话快照是稳定 transcript、重启恢复、切换会话和运行完成收敛的数据源。
- 前端不能读取 SQLite、raw event log 或 runtime 内部 registry。
- run completed、failed、paused、WebSocket reconnect、app 启动和切换当前会话后，都可以通过 interaction 协议读取会话快照。
- 第一阶段只做必要的 DisplayEvent 可靠性补强，不重设计整个事件模型。
- 前端所需 interaction DTO 上移到正式 interaction protocol 模块，不留在 app 私有 UI 层。
- 使用 Rust 类型生成 TypeScript 类型；interaction DTO 和 profile config wire 类型都要生成。
- `serde_json::Value` 和动态 metadata 允许在生成类型中保留动态 JSON 形态。
- Profile config 的事实来源仍是 Rust parser、schema 和 validator；前端只做轻量即时反馈和编辑体验。
- JSONC 编辑器使用 CodeMirror 6，并接入本地 schema-aware completion。
- Settings 的可视化表单和 JSONC 编辑必须双向同步；无效 JSONC 时表单只读或受控禁用，避免双源污染。
- React 状态第一阶段使用小型领域 store 和 reducer/state machine，不引入 Redux 或 Zustand。
- 需要独立的 interaction client 模块，封装 JSON-RPC HTTP、display WebSocket、初始化、会话、prompt、abort 和 approval。
- 需要独立的 Chat session store，封装会话列表、当前会话、live projection、会话快照收敛和运行状态。
- 需要独立的 Settings draft store，封装配置 draft、JSONC 文本、校验、dirty 和保存状态。
- 需要独立的 app shell state，封装 route、会话栏展开、toast、theme 和 locale。
- 第一阶段验收不追求视频级高保真全部细节；先打牢功能正确性、协议边界、输入区、流式显示和收敛。
- 视频级 streaming tail、复杂工具卡片、完整附件输入、完整动画系统和高级主题作为后续切片。
- `noloong-openai` 中 ChatGPT Responses request timeout 修复保留，它不属于 GPUI 迁移废弃范围。
- 当前 GPUI 调试期间产生的 view/runtime/toast/titlebar/composer/scroll 代码可以清理，不作为迁移兼容层。
- 旧 GPUI 中有价值的纯协议、配置和 transcript 语义可以重新提取，但不逐文件翻译。

## Testing Decisions

- 测试只验证外部行为和协议契约，不测试 React 组件内部实现细节。
- Rust 测试覆盖 Tauri bootstrap command、profile config load/save/validate/schema、interaction DTO serde、TS bindings 生成是否最新。
- interaction protocol 测试覆盖 DisplayEvent、SessionDescriptor、PromptRequest、ApprovalResolve、Initialize 和 WebSocket notification decode。
- 前端 reducer 测试覆盖展示事件应用顺序、会话快照收敛、streaming final replacement、run status、thought display、tool activity 和 approval card。
- Chat session store 测试覆盖无会话、创建会话、切换会话、运行中切换、WebSocket reconnect、run completed 后快照收敛。
- Settings draft store 测试覆盖表单修改同步 JSONC、JSONC 修改同步 typed draft、无效 JSONC 阻止保存、格式化和保存。
- JSONC completion 测试覆盖 key completion、enum completion、tagged enum variant snippet 和已存在 key 过滤。
- Composer 测试覆盖输入可见性、多行、发送 disabled/enabled、发送后 optimistic user message、stop button 调用 abort。
- Playwright 使用 fake interaction server 覆盖核心 UI 行为：发送消息立即出现、分批 delta 逐步显示、自动贴底、手动上滚暂停贴底、完成后状态收敛、JSONC invalid 阻止保存。
- Playwright 或等价测试不依赖真实 ChatGPT、Telegram 或微信。
- 真实 ChatGPT profile smoke 作为手动验收：新建会话、发送文本、观察流式输出、等待完成、确认 transcript 收敛。
- macOS app smoke 验证 Tauri app identity、icon、title bar、Computer Use 可见性和窗口截图。
- 构建验证覆盖 Rust format/check/test、前端 typecheck/build/test 和 Tauri app 启动。
- 迁移完成后验证 workspace 不再包含 GPUI 依赖和 GPUI macro dev profile 优化。

## Out of Scope

- 不保留 GPUI app 作为 fallback。
- 不继续修补 GPUI 输入、滚动、titlebar 或 stream UI。
- 不复制外部产品品牌、私有文案或协议模型。
- 不在第一阶段实现视频级全部微动效。
- 不在第一阶段实现完整附件输入。
- 不在第一阶段实现完整 tool output 高级卡片。
- 不在第一阶段实现完整 LSP 或 Zed project stack。
- 不把 Lobe UI、Ant Design 或其它组件库设为主设计系统。
- 不让前端读取 SQLite 或 raw event log。
- 不让前端直接持有 registry。
- 不用 Tauri command 重新定义 chat/display 协议。
- 不自动化真实 Telegram 或微信 smoke。
- 不保留历史 macOS bundle helper。
- 不为旧 GPUI 状态或旧本地数据库做兼容迁移。

## Further Notes

- 本 PRD 遵守 `CONTEXT.md` 中的领域语言：主交互客户端、Chat 画布、展示事件、会话快照、interaction 协议、流式回复、思考展示、运行活动、输入区。
- ADR-0001 确认 `noloong app` 是主交互客户端；ADR-0002 确认主交互客户端迁移到 Tauri/WebView。
- `CONTEXT.md` 已补充“会话快照”术语，用来精确表达稳定 transcript 与实时展示事件之间的关系。
- 迁移不是把 GPUI 文件翻译成 React 文件，而是保留协议和配置语义，重建主交互客户端的 UI 层。
- 后续应使用 `to-issues` 将本 PRD 拆成多个 tracer-bullet vertical slices；在用户明确指令前不创建 issue。
