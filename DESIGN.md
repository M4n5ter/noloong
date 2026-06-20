---
version: alpha
name: Noloong Warm Intelligence
description: >
  Noloong desktop 的温和智能设计语言。
colors:
  primary: "#20221b"
  primary-deep: "#171914"
  primary-dark: "#11130f"
  secondary: "#f3efe4"
  secondary-muted: "rgba(243, 239, 228, 0.74)"
  secondary-subtle: "rgba(243, 239, 228, 0.50)"
  secondary-faint: "rgba(243, 239, 228, 0.30)"
  tertiary: "#a8c4dc"
  tertiary-warm: "#e5d4b3"
  neutral: "rgba(23, 25, 20, 0.82)"
  neutral-soft: "rgba(23, 25, 20, 0.42)"
  border: "rgba(243, 239, 228, 0.13)"
  border-subtle: "rgba(243, 239, 228, 0.075)"
  focus: "rgba(206, 225, 239, 0.72)"
  success: "#a4d17d"
  warning: "#e5d4b3"
  error: "#d89373"
typography:
  display:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 32px
    fontWeight: 590
    lineHeight: 38px
    letterSpacing: 0px
  title-lg:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 22px
    fontWeight: 590
    lineHeight: 26px
    letterSpacing: 0px
  title-md:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 17px
    fontWeight: 620
    lineHeight: 22px
    letterSpacing: 0px
  body-lg:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 17px
    fontWeight: 400
    lineHeight: 22px
    letterSpacing: 0px
  body-md:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 16px
    fontWeight: 400
    lineHeight: 26px
    letterSpacing: 0px
  body-sm:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 14px
    fontWeight: 400
    lineHeight: 20px
    letterSpacing: 0px
  label-md:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 13px
    fontWeight: 500
    lineHeight: 16px
    letterSpacing: 0px
  label-sm:
    fontFamily: '"Inter Tight Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    fontSize: 12px
    fontWeight: 500
    lineHeight: 16px
    letterSpacing: 0px
rounded:
  sm: 16px
  md: 22px
  lg: 26px
  xl: 28px
  full: 9999px
spacing:
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 32px
  xxl: 56px
  transcript-width: 760px
  app-max-width: 1040px
  composer-width: 760px
components:
  app-shell:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.secondary}"
    typography: "{typography.body-md}"
    rounded: "{rounded.xl}"
    padding: 24px
  surface:
    backgroundColor: "{colors.primary-deep}"
    textColor: "{colors.secondary-muted}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 16px
  surface-dark:
    backgroundColor: "{colors.primary-dark}"
    textColor: "{colors.secondary-muted}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 16px
  divider:
    backgroundColor: "{colors.border}"
    width: 1px
    height: 1px
  divider-subtle:
    backgroundColor: "{colors.border-subtle}"
    width: 1px
    height: 1px
  metadata:
    backgroundColor: transparent
    textColor: "{colors.secondary-subtle}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: 4px
  faint-icon:
    backgroundColor: transparent
    textColor: "{colors.secondary-faint}"
    typography: "{typography.label-sm}"
    rounded: "{rounded.full}"
    padding: 4px
    size: 28px
  focus-indicator:
    backgroundColor: "{colors.focus}"
    textColor: "{colors.primary-dark}"
    rounded: "{rounded.full}"
    size: 4px
  status-success:
    backgroundColor: "{colors.success}"
    textColor: "{colors.primary-dark}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
  status-warning:
    backgroundColor: "{colors.warning}"
    textColor: "{colors.primary-dark}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
  approval-chip:
    backgroundColor: "{colors.tertiary-warm}"
    textColor: "{colors.primary-dark}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
  button-secondary:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.secondary-muted}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
    height: 36px
  button-primary:
    backgroundColor: "rgba(58, 84, 107, 0.72)"
    textColor: "{colors.secondary}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
    height: 36px
  button-danger:
    backgroundColor: "{colors.error}"
    textColor: "{colors.primary-dark}"
    typography: "{typography.label-md}"
    rounded: "{rounded.full}"
    padding: 8px
    height: 36px
  input:
    backgroundColor: "{colors.neutral-soft}"
    textColor: "{colors.secondary}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.sm}"
    padding: 12px
    height: 36px
  composer-capsule:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.secondary-muted}"
    typography: "{typography.body-lg}"
    rounded: "{rounded.full}"
    padding: 8px
    height: 68px
  composer-expanded:
    backgroundColor: "rgba(37, 40, 31, 0.78)"
    textColor: "{colors.secondary}"
    typography: "{typography.body-lg}"
    rounded: "{rounded.lg}"
    padding: 18px
    height: 224px
---

# Noloong Warm Intelligence

## Overview

Noloong desktop 的界面应该像一个安静、可信、可停留的本地 agent 工作空间。用户来到这里是为了阅读、思考、转向、审批和继续工作，不是为了浏览品牌页、管理后台或 IDE 面板。

本设计语言的核心判断是：通过减少可见机器感，让 agent 显得亲近、清楚、可信。界面应该先呈现当前内容和下一步动作，再呈现导航、状态和配置。

Apple Human Interface Guidelines 是更高优先级。只要本文与 macOS 的窗口行为、Settings、菜单、键盘可达性、可访问性、系统颜色、焦点、破坏性确认冲突，就按 HIG 修改本文或实现。macOS 默认文本不低于 13pt，最小文本不低于 10pt；默认控制目标按 28x28pt 设计，最小不低于 20x20pt。

项目没有历史兼容负担。任何不再服务当前产品状态的旧概念、旧文案、旧布局和旧文档都应该删除，不要为了历史惯性保留。

## Colors

色彩是温度和状态，不是装饰。主色 `primary` 是深橄榄黑，承载整个空间；`secondary` 是暖白，用于正文和主要信息；`tertiary` 是低饱和灰蓝，用于链接、焦点邻近提示和少量选中边缘；`tertiary-warm` 是安静琥珀，用于审批、warning 和温和注意。

- **Primary (`#20221b`)：** 主背景，避免纯黑和 slate dashboard 感。
- **Primary deep / dark：** 底部胶囊、编辑器、浮层和暗部材料。
- **Secondary：** 暖白文字。通过 `secondary-muted`、`secondary-subtle`、`secondary-faint` 建立信息层级。
- **Tertiary：** 克制的冷色，只用于链接、焦点相关边缘和少量高价值强调。
- **Tertiary warm：** 审批、警告、风险提示和需要温度的状态。
- **Error / warning / success：** 必须配合文字或形状，不单靠颜色表达状态。

禁止蓝紫科技渐变、装饰性 glow、光斑、渐变球、大面积 primary button 填色，以及同一个颜色在不同 pane 中表达不同含义。

## Typography

排版应该像被编辑过的软件，而不是组件旁边自动生成的说明。默认使用系统无衬线栈，保持清晰和 macOS 熟悉感。

- **Display：** 只用于空态、当前会话问题或非常少的 editorial 时刻。
- **Title lg / md：** 用于当前 pane、当前任务和紧凑 section 标题。
- **Body lg：** 用于 composer 输入。它应该显得可触达，不像后台表单。
- **Body md：** 用于 assistant 正文和长阅读。
- **Body sm：** 用于 helper text 和次级说明。
- **Label md / sm：** 用于控制、metadata 和轻量 title context。

不要让整个界面停留在 14px label。那会立刻变成配置后台。`letterSpacing` 保持 0，不用负字距模仿营销页。

## Layout

布局从注意力开始，不从组件开始。

优先级是：当前 transcript、当前回答或当前问题；当前输入和下一步动作；对人有意义的状态；导航和上下文；Settings 和低频工具。桌面空间要用来提升阅读，而不是展示更多 chrome。

Chat 工作面是阅读面。assistant 输出是正文流，不是一堆卡片；user message 可以轻量区分，但不做大气泡；空态可以问一句简短问题，但不能变成产品 hero。顶部 copy 必须轻，不能用大标题和副标题口号挤占工作区。

Composer 是阅读面的可见输入焦点。默认是稳定横向胶囊，单行 text field 语义，prompt 变长时不继续撑高。长文本、换行或溢出时，显示小的展开控制，在胶囊上方打开附着的 multiline editor；胶囊本身保持高度不变，展开 editor 内部滚动，并只在确实有更多内容时显示轻微上下渐隐。

Title bar 是 macOS 窗口结构，不是状态栏。不要展示品牌、logo、口号、running / idle、profile path、provider、runtime id、config path 或 debug state。

Settings 是 macOS Settings 窗口，可以视觉重塑，但行为必须熟悉。通过 App menu 和 Command-Comma 打开，重新打开时恢复上次 pane。Save、Discard、validation 和 restart feedback 靠近 pane 标题或具体字段，不固定在窗口底部。JSONC 和 raw JSON editor 是专家界面，不是默认设置体验。

## Elevation & Depth

层级主要靠色调、透明度、间距和排版，不靠重阴影。边框只让材料边缘可感知，不应该把每个区域都框成卡片。

底部 composer capsule 可以有最明显的浮动感，因为它是输入焦点。临时浮层、展开 editor、approval 可以使用柔和阴影。正文、tool activity、reasoning state 不应使用 dashboard card 式强阴影。

Reduce Transparency 下，透明材料必须退化为不透明的深色表面；Reduce Motion 下，状态仍要清楚，只移除非必要过渡。

## Shapes

形状要柔和但不幼稚。胶囊使用 `rounded.full`，普通字段使用 `rounded.sm`，浮层和编辑器使用 `rounded.md` / `rounded.lg`。

圆角不能代替层级设计。不要把所有内容都做成圆角卡片，不要在同一视图里混用过多圆角体系，也不要用方形输入框破坏底部材料的连续性。

## Components

按钮必须用明确动作和对象。好的文案是 `Save Changes`、`Discard Changes`、`Stop Run`、`Delete Plugin`；差的文案是 `OK`、`Confirm`、`Submit`，以及没有上下文的 `Approve` / `Deny`。每个局部上下文最多一个视觉 primary；破坏性动作不是 primary，必须有取消路径。

字段是安静材料面。label 稳定且短，helper text 只在解释约束时出现，validation 靠近字段，focus 必须有可见 ring。multiline text view 超过目标高度时内部滚动。

Reasoning 不是日志面板。思考中只显示一个稳定、低噪音状态；不出现两条等价 thinking row；完成后收束为短 inline 状态；详情默认折叠，用户主动展开才显示。

Tool activity 是嵌在阅读流里的审计线索。默认视图是一句人类语言、低对比、位置稳定；展开视图再展示 command、cwd、file scope、raw id、stdout 和结构化审计内容。正在思考期间触发的 tool activity 是 reasoning 的下属活动，不是第二条同级 thinking 状态。失败的 tool activity 必须用明确的失败标签和更高对比的错误 affordance，不能显示成完成。不要把 tool events 渲染成 dashboard、终端模拟器或重卡片堆。

Human approval 是安全关键路径。它必须说明将要发生什么、在哪里发生、为什么需要确认，并用人类语言说明风险。破坏性或高风险动作必须提供 Cancel。Escape 和 Command-Period 取消当前前景 approval。按钮、label、错误和权限文案必须走 i18n。主文案不暴露 raw tool id；技术 id 可以放进展开的审计详情。

Icon-only 控件必须有可访问名称，并提供 macOS hover help。关键动作不能只存在于自定义浮动控件；发送、停止、聚焦 composer、清空和审批相关动作需要合适的键盘、菜单或系统路径。

## Do's and Don'ts

- Do 让当前内容或当前问题成为第一视觉对象。
- Do 先用透明度、间距和排版建立层级，再考虑边框。
- Do 保持 composer 胶囊稳定，长文本向上展开附着 editor。
- Do 让 Save、Discard、validation、approval 和 error 靠近上下文。
- Do 把 JSON 当成专家模式，而不是默认设置体验。
- Do 用真实 desktop、窄窗口、长 prompt、流式输出、approval、Settings、Reduce Transparency 和 Reduce Motion 验证界面。
- Don't 做 SaaS dashboard、IDE 面板布局或 landing-page hero。
- Don't 把 running / idle 做成持久 chrome。
- Don't 把 title bar 做成 debug/status strip。
- Don't 出现两条等价 thinking 状态。
- Don't 单靠颜色表达状态。
- Don't 加装饰性 glow、蓝紫渐变、光斑或视觉噪音。
- Don't 因为测试通过就认为界面质量合格；UI 改动必须有渲染证据。
