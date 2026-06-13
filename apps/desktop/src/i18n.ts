import type { AppInteractionStatus, Locale } from "./generated/contracts";

export type UiLocale = "en" | "zh";

export type I18nKey =
  | "app.brand"
  | "nav.chat"
  | "nav.settings"
  | "header.starting"
  | "header.failed"
  | "header.starterDraft"
  | "bootstrap.loadingTitle"
  | "bootstrap.loadingDetail"
  | "bootstrap.failedTitle"
  | "chat.openSettings"
  | "chat.sessionToolbar"
  | "chat.connectingTitle"
  | "chat.connectingDetail"
  | "chat.missingConfigTitle"
  | "chat.missingConfigDetail"
  | "chat.unavailableTitle"
  | "chat.unavailableDetail"
  | "chat.initializationFailedTitle"
  | "chat.failedTitle"
  | "sessions.title"
  | "sessions.create"
  | "sessions.loading"
  | "sessions.empty"
  | "sessionsPanel.title"
  | "sessionsPanel.subtitle"
  | "sessionsPanel.return"
  | "runtime.disconnected"
  | "runtime.interrupted"
  | "transcript.newSessionTitle"
  | "transcript.newSessionDetail"
  | "transcript.empty"
  | "message.sending"
  | "composer.write"
  | "composer.connecting"
  | "composer.attach"
  | "composer.send"
  | "composer.expand"
  | "composer.collapse"
  | "composer.removeAttachment"
  | "tool.running"
  | "tool.done"
  | "run.stop"
  | "reasoning.thinking"
  | "reasoning.thoughtFor"
  | "reasoning.hideRaw"
  | "reasoning.showRaw"
  | "reasoning.empty"
  | "approval.required"
  | "approval.actionTitle"
  | "approval.commandTitle"
  | "approval.pending"
  | "approval.approved"
  | "approval.denied"
  | "approval.expired"
  | "approval.tool"
  | "approval.command"
  | "approval.directory"
  | "approval.reason"
  | "approval.permissions"
  | "approval.allow"
  | "approval.deny"
  | "settings.loadingTitle"
  | "settings.loadingDetail"
  | "settings.failedTitle"
  | "settings.backToChat"
  | "settings.title"
  | "settings.save"
  | "settings.saved"
  | "settings.savedAndApplied"
  | "settings.savedApplyFailed"
  | "settings.savedExternal"
  | "settings.environmentTitle"
  | "settings.currentProfile"
  | "settings.discard"
  | "settings.saving"
  | "settings.profile"
  | "settings.profileId"
  | "settings.fixJsonc"
  | "settings.provider"
  | "settings.plugins"
  | "settings.jsonc"
  | "settings.validating"
  | "settings.unsaved"
  | "settings.savedState"
  | "settings.noProfile"
  | "settings.activeProfile"
  | "settings.name"
  | "settings.description"
  | "settings.model"
  | "settings.useDefaultProfile"
  | "settings.addProfile"
  | "settings.copyProfile"
  | "settings.deleteProfile"
  | "settings.providerId"
  | "settings.baseUrl"
  | "settings.apiKeyEnv"
  | "settings.stateMode"
  | "settings.reasoningEnabled"
  | "settings.reasoningEffort"
  | "settings.reasoningSummary"
  | "settings.storage"
  | "settings.eventStore"
  | "settings.compaction"
  | "settings.registryStore"
  | "settings.addPlugin"
  | "settings.deletePlugin"
  | "settings.noPlugins"
  | "settings.context"
  | "settings.advancedJsonc"
  | "settings.providerReasoningTitle";

const messages = {
  en: {
    "app.brand": "Noloong",
    "nav.chat": "Chat",
    "nav.settings": "Settings",
    "header.starting": "Starting runtime",
    "header.failed": "Bootstrap failed",
    "header.starterDraft": "starter draft",
    "bootstrap.loadingTitle": "Starting",
    "bootstrap.loadingDetail": "Reading the Rust-side launch state.",
    "bootstrap.failedTitle": "Bootstrap failed",
    "chat.openSettings": "Open settings",
    "chat.sessionToolbar": "Session controls",
    "chat.connectingTitle": "Connecting",
    "chat.connectingDetail": "Initializing the interaction client.",
    "chat.missingConfigTitle": "Profile configuration is missing",
    "chat.missingConfigDetail":
      "Create or save a profile configuration before starting a chat session.",
    "chat.unavailableTitle": "Runtime unavailable",
    "chat.unavailableDetail":
      "No interaction endpoint is active for this launch. Check the runtime or open settings.",
    "chat.initializationFailedTitle": "Interaction initialization failed",
    "chat.failedTitle": "Interaction failed",
    "sessions.title": "Sessions",
    "sessions.create": "Create session",
    "sessions.loading": "Loading sessions...",
    "sessions.empty": "No sessions yet. Send a message to create one.",
    "sessionsPanel.title": "Sessions",
    "sessionsPanel.subtitle": "Switch context or start a clean thread without leaving the chat.",
    "sessionsPanel.return": "Return",
    "runtime.disconnected": "Display stream disconnected",
    "runtime.interrupted": "Display stream was interrupted.",
    "transcript.newSessionTitle": "New session",
    "transcript.newSessionDetail": "Send a message to create a session.",
    "transcript.empty": "Start with the next thing you want Noloong to think through.",
    "message.sending": "sending",
    "composer.write": "Write a message...",
    "composer.connecting": "Connecting display stream...",
    "composer.attach": "Attach files",
    "composer.send": "Send message",
    "composer.expand": "Expand composer",
    "composer.collapse": "Collapse composer",
    "composer.removeAttachment": "Remove {name}",
    "tool.running": "Running",
    "tool.done": "Done",
    "run.stop": "Stop",
    "reasoning.thinking": "Thinking",
    "reasoning.thoughtFor": "Thought for {duration}",
    "reasoning.hideRaw": "Hide raw",
    "reasoning.showRaw": "Show raw",
    "reasoning.empty": "No visible reasoning yet.",
    "approval.required": "Approval required",
    "approval.actionTitle": "Review this action",
    "approval.commandTitle": "Run this command?",
    "approval.pending": "Needs your decision",
    "approval.approved": "Approved",
    "approval.denied": "Denied",
    "approval.expired": "Expired",
    "approval.tool": "Tool",
    "approval.command": "Command",
    "approval.directory": "Directory",
    "approval.reason": "Why",
    "approval.permissions": "Access",
    "approval.allow": "Allow",
    "approval.deny": "Deny",
    "settings.loadingTitle": "Loading settings",
    "settings.loadingDetail": "Reading profile configuration.",
    "settings.failedTitle": "Settings failed",
    "settings.backToChat": "Back to chat",
    "settings.title": "Settings",
    "settings.save": "Save",
    "settings.saved": "Saved {path}",
    "settings.savedAndApplied": "Saved and applied {path}",
    "settings.savedApplyFailed": "Saved {path}, but applying it failed: {error}",
    "settings.savedExternal": "Saved {path}. External runtime must be restarted outside the app.",
    "settings.environmentTitle": "Environment",
    "settings.currentProfile": "Current profile",
    "settings.discard": "Discard",
    "settings.saving": "Saving",
    "settings.profile": "Profile",
    "settings.profileId": "Profile ID",
    "settings.fixJsonc":
      "Fix JSONC errors to restore the typed profile form. The last valid draft is preserved.",
    "settings.provider": "Provider",
    "settings.plugins": "Plugins",
    "settings.jsonc": "JSONC",
    "settings.validating": "Validating...",
    "settings.unsaved": "Unsaved changes",
    "settings.savedState": "Saved",
    "settings.noProfile": "No profile exists.",
    "settings.activeProfile": "Active profile",
    "settings.name": "Name",
    "settings.description": "Description",
    "settings.model": "Model",
    "settings.useDefaultProfile": "Use as default profile",
    "settings.addProfile": "Add profile",
    "settings.copyProfile": "Copy profile",
    "settings.deleteProfile": "Delete profile",
    "settings.providerId": "Provider ID",
    "settings.baseUrl": "Base URL",
    "settings.apiKeyEnv": "API key env",
    "settings.stateMode": "State mode",
    "settings.reasoningEnabled": "Reasoning enabled",
    "settings.reasoningEffort": "Effort",
    "settings.reasoningSummary": "Summary",
    "settings.storage": "Storage",
    "settings.eventStore": "Event store",
    "settings.compaction": "Compaction",
    "settings.registryStore": "Registry store",
    "settings.addPlugin": "Add plugin",
    "settings.deletePlugin": "Delete plugin",
    "settings.noPlugins": "No plugins in this profile.",
    "settings.context": "Context",
    "settings.advancedJsonc": "Advanced JSONC",
    "settings.providerReasoningTitle": "Provider and reasoning move together",
  },
  zh: {
    "app.brand": "Noloong",
    "nav.chat": "聊天",
    "nav.settings": "设置",
    "header.starting": "正在启动运行时",
    "header.failed": "启动失败",
    "header.starterDraft": "新配置草稿",
    "bootstrap.loadingTitle": "正在启动",
    "bootstrap.loadingDetail": "正在读取 Rust 侧启动状态。",
    "bootstrap.failedTitle": "启动失败",
    "chat.openSettings": "打开设置",
    "chat.sessionToolbar": "会话控制",
    "chat.connectingTitle": "正在连接",
    "chat.connectingDetail": "正在初始化交互客户端。",
    "chat.missingConfigTitle": "缺少配置",
    "chat.missingConfigDetail": "请先创建或保存配置，然后再开始聊天会话。",
    "chat.unavailableTitle": "运行时不可用",
    "chat.unavailableDetail": "本次启动没有可用的交互端点。请检查运行时，或先打开设置。",
    "chat.initializationFailedTitle": "交互初始化失败",
    "chat.failedTitle": "交互失败",
    "sessions.title": "会话",
    "sessions.create": "创建会话",
    "sessions.loading": "正在加载会话...",
    "sessions.empty": "还没有会话。发送消息后会自动创建。",
    "sessionsPanel.title": "会话",
    "sessionsPanel.subtitle": "切换上下文，或在不离开聊天的情况下开始一个干净的新线程。",
    "sessionsPanel.return": "返回",
    "runtime.disconnected": "Display 流已断开",
    "runtime.interrupted": "Display 流已中断。",
    "transcript.newSessionTitle": "新会话",
    "transcript.newSessionDetail": "发送消息后会自动创建会话。",
    "transcript.empty": "从下一件想让 Noloong 思考的事开始。",
    "message.sending": "发送中",
    "composer.write": "输入消息...",
    "composer.connecting": "正在连接 Display 流...",
    "composer.attach": "添加附件",
    "composer.send": "发送消息",
    "composer.expand": "展开输入区",
    "composer.collapse": "收起输入区",
    "composer.removeAttachment": "移除 {name}",
    "tool.running": "运行中",
    "tool.done": "完成",
    "run.stop": "停止",
    "reasoning.thinking": "正在思考",
    "reasoning.thoughtFor": "思考了 {duration}",
    "reasoning.hideRaw": "隐藏原文",
    "reasoning.showRaw": "显示原文",
    "reasoning.empty": "暂时没有可见思考内容。",
    "approval.required": "需要审批",
    "approval.actionTitle": "确认这次操作",
    "approval.commandTitle": "运行这条命令？",
    "approval.pending": "需要你决定",
    "approval.approved": "已同意",
    "approval.denied": "已拒绝",
    "approval.expired": "已过期",
    "approval.tool": "工具",
    "approval.command": "命令",
    "approval.directory": "目录",
    "approval.reason": "原因",
    "approval.permissions": "权限",
    "approval.allow": "同意",
    "approval.deny": "拒绝",
    "settings.loadingTitle": "正在加载设置",
    "settings.loadingDetail": "正在读取配置。",
    "settings.failedTitle": "设置加载失败",
    "settings.backToChat": "返回聊天",
    "settings.title": "设置",
    "settings.save": "保存",
    "settings.saved": "已保存 {path}",
    "settings.savedAndApplied": "已保存并应用 {path}",
    "settings.savedApplyFailed": "已保存 {path}，但应用失败：{error}",
    "settings.savedExternal": "已保存 {path}。外部运行时需要在应用外重启。",
    "settings.environmentTitle": "环境",
    "settings.currentProfile": "当前配置档",
    "settings.discard": "放弃",
    "settings.saving": "保存中",
    "settings.profile": "配置档",
    "settings.profileId": "配置档 ID",
    "settings.fixJsonc": "请先修复 JSONC 错误，表单会保留最后一次有效配置。",
    "settings.provider": "提供商",
    "settings.plugins": "插件",
    "settings.jsonc": "JSONC",
    "settings.validating": "正在检查...",
    "settings.unsaved": "有未保存修改",
    "settings.savedState": "已保存",
    "settings.noProfile": "还没有配置档。",
    "settings.activeProfile": "当前配置档",
    "settings.name": "名称",
    "settings.description": "描述",
    "settings.model": "模型",
    "settings.useDefaultProfile": "设为默认配置档",
    "settings.addProfile": "新增配置档",
    "settings.copyProfile": "复制配置档",
    "settings.deleteProfile": "删除配置档",
    "settings.providerId": "提供商 ID",
    "settings.baseUrl": "Base URL",
    "settings.apiKeyEnv": "API key 环境变量",
    "settings.stateMode": "状态模式",
    "settings.reasoningEnabled": "启用推理",
    "settings.reasoningEffort": "推理强度",
    "settings.reasoningSummary": "推理摘要",
    "settings.storage": "存储",
    "settings.eventStore": "事件存储",
    "settings.compaction": "上下文压缩",
    "settings.registryStore": "Registry 存储",
    "settings.addPlugin": "新增插件",
    "settings.deletePlugin": "删除插件",
    "settings.noPlugins": "当前配置档没有插件。",
    "settings.context": "上下文",
    "settings.advancedJsonc": "高级 JSONC",
    "settings.providerReasoningTitle": "提供商与推理一起调整",
  },
} satisfies Record<UiLocale, Record<I18nKey, string>>;

export type AppI18n = ReturnType<typeof createI18n>;

export function resolveUiLocale(locale: Locale | null | undefined, language?: string): UiLocale {
  if (locale === "en" || locale === "zh") {
    return locale;
  }
  const detected = language ?? globalThis.navigator?.language ?? "";
  return detected.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function createI18n(locale: UiLocale) {
  const catalog = messages[locale];
  return {
    locale,
    t(key: I18nKey, vars?: Record<string, string | number>) {
      return interpolate(catalog[key], vars);
    },
    headerSubtitle(options: {
      status: "loading" | "failed" | "ready";
      appVersion?: string | null;
      profileConfigPath?: string | null;
    }) {
      if (options.status === "loading") {
        return catalog["header.starting"];
      }
      if (options.status === "failed") {
        return catalog["header.failed"];
      }
      return `${options.appVersion || "unknown"} · ${
        options.profileConfigPath || catalog["header.starterDraft"]
      }`;
    },
    disconnected(status: AppInteractionStatus | null | undefined) {
      if (status?.status === "unavailable") {
        return {
          title: catalog["chat.unavailableTitle"],
          detail: catalog["chat.unavailableDetail"],
        };
      }
      if (status?.status === "failed") {
        return {
          title: catalog["chat.initializationFailedTitle"],
          detail: status.error,
        };
      }
      return {
        title: catalog["chat.missingConfigTitle"],
        detail: catalog["chat.missingConfigDetail"],
      };
    },
    duration(elapsedMs: number | undefined) {
      if (elapsedMs == null) {
        return locale === "zh" ? "一小会" : "a moment";
      }
      if (elapsedMs < 1000) {
        return `${elapsedMs}ms`;
      }
      const seconds = Math.max(1, Math.round(elapsedMs / 1000));
      if (locale === "zh") {
        return `${seconds} 秒`;
      }
      return `${seconds} second${seconds === 1 ? "" : "s"}`;
    },
  };
}

function interpolate(text: string, vars: Record<string, string | number> | undefined): string {
  if (!vars) {
    return text;
  }
  return text.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, key: string) => {
    const value = vars[key];
    return value == null ? match : String(value);
  });
}
