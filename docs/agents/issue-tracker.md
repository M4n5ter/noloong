# Issue Tracker：本地 Markdown

本仓库的 issue 和 PRD 使用本地 Markdown 文件管理，统一放在 `.scratch/` 下。

## 约定

- 每个功能或主题使用一个目录：`.scratch/<feature-slug>/`
- PRD 文件固定为：`.scratch/<feature-slug>/PRD.md`
- 实施 issue 放在：`.scratch/<feature-slug>/issues/<NN>-<slug>.md`，编号从 `01` 开始
- issue 的 triage 状态写在文件顶部附近的 `Status:` 行，状态词汇见 `docs/agents/triage-labels.md`
- 评论和讨论追加到文件底部的 `## Comments` 小节

## 当技能要求“发布到 issue tracker”

在 `.scratch/<feature-slug>/` 下创建新的 Markdown 文件；如果目录不存在，先创建目录。

## 当技能要求“读取相关 ticket”

读取用户给出的文件路径。通常用户会直接提供 `.scratch/...` 路径，或提供可定位到对应文件的 issue 编号。
