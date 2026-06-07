# 使用 Tauri/WebView 实现主交互客户端

Noloong 的主交互客户端从 GPUI 迁移到 Tauri/WebView：Rust 继续承载本地 runtime、配置和 interaction 协议边界，用户界面改由成熟的 Web 技术实现。我们放弃继续修补 GPUI 实现，因为输入区、焦点、滚动、编辑器和流式回复这些核心体验在 GPUI 上持续产生框架摩擦；Tauri/WebView 能更稳定地支持 Chat 画布、JSONC 编辑器、动画和自动化测试，同时仍保留本地优先的 Rust 部署模型。
