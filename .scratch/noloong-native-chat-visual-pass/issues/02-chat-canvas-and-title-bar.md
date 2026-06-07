# Chat 画布框架与 title bar 重塑

Status: in-progress

## Parent

.scratch/noloong-native-chat-visual-pass/PRD.md

## What to build

重塑 Chat 画布的整体框架和 title bar 信息层级，让 `noloong app` 默认界面从“功能验证版/设置页衍生页”转向沉浸式主交互客户端。该切片应保留现有真实 Chat 链路，只调整视觉架构、颜色层级、窗口节奏和标题区域。

## Acceptance criteria

- [ ] Chat 默认界面不再呈现 Settings 表单风格，主视觉落点是当前会话和 transcript。
- [ ] Title bar 中心显示当前会话标题，副标题低对比展示真实 profile/model/工作目录等上下文。
- [ ] 主画布颜色、边框、圆角、阴影和留白集中到可复用的 Chat 视觉 token 中。
- [ ] Chat 页面外层保持固定视口，后续 transcript 和 composer 能在此框架内稳定布局。
- [ ] Settings 路由仍可正常进入和返回，不丢失当前会话状态。
- [ ] 使用 Computer Use 或截图记录改造后的默认 Chat 画布。

## Blocked by

- .scratch/noloong-native-chat-visual-pass/issues/01-visual-baseline-and-review-loop.md

## Implementation status

第一版已移除 Chat 页 Settings 操作按钮，居中标题/副标题保留真实会话上下文，并引入 Chat 视觉 token。仍需继续调 composer、streaming 和 thought/tool 细节。
