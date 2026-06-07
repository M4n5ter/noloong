# Settings entry with CodeMirror JSONC editing

Status: done

## Parent

[PRD：Noloong app Tauri 迁移](../PRD.md)

## What to build

在 Tauri/WebView app 中实现 Settings 配置入口。Settings 不是默认落点，但可以从 Chat 画布进入。配置入口支持 profile config 的读取、编辑、校验和保存；JSONC 编辑器使用 CodeMirror 6，支持 schema-aware completion。可视化表单与 JSONC 文本双向同步；当 JSONC 无效时阻止保存，并避免无效文本污染 typed draft。

## Acceptance criteria

- [x] Settings 可从 Chat 画布打开并返回。
- [x] Tauri command 能读取 profile config。
- [x] Tauri command 能保存 profile config。
- [x] Tauri command 能执行 Rust 侧 validate。
- [x] CodeMirror 6 用作 JSONC 编辑器。
- [x] JSONC 编辑支持基础 schema-aware completion。
- [x] 表单修改会同步 JSONC。
- [x] JSONC 修改为有效配置后会同步表单和 typed draft。
- [x] 无效 JSONC 显示错误并阻止保存。
- [x] Settings draft store 有测试覆盖。

## Blocked by

- [02-bootstrap-and-generated-typescript-contracts.md](./02-bootstrap-and-generated-typescript-contracts.md)
