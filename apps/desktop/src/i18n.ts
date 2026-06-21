import type { AppInteractionStatus, Locale } from "./generated/contracts";

export type UiLocale = "en" | "zh";

export type I18nKey =
  | "bootstrap.loadingTitle"
  | "bootstrap.loadingDetail"
  | "bootstrap.failedTitle"
  | "chat.openSettings"
  | "chat.prepareEnvironment"
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
  | "sessionsPanel.close"
  | "runtime.disconnected"
  | "runtime.interrupted"
  | "transcript.newSessionTitle"
  | "sessionContext.environment"
  | "sessionContext.running"
  | "sessionContext.paused"
  | "sessionContext.failed"
  | "sessionContext.aborted"
  | "message.sending"
  | "message.userLabel"
  | "message.assistantLabel"
  | "message.pendingUserLabel"
  | "composer.write"
  | "composer.waitingForApproval"
  | "composer.connecting"
  | "composer.attach"
  | "composer.send"
  | "composer.expand"
  | "composer.collapse"
  | "composer.editingDraft"
  | "composer.removeAttachment"
  | "tool.running"
  | "tool.done"
  | "tool.failed"
  | "tool.genericTitle"
  | "tool.previewTitle"
  | "tool.commandTitle"
  | "run.stop"
  | "reasoning.thinking"
  | "reasoning.thoughtFor"
  | "reasoning.hideDetails"
  | "reasoning.showDetails"
  | "reasoning.empty"
  | "approval.required"
  | "approval.actionTitle"
  | "approval.commandTitle"
  | "approval.commandSummary"
  | "approval.projectWriteSummary"
  | "approval.actionSummary"
  | "approval.pending"
  | "approval.approved"
  | "approval.denied"
  | "approval.expired"
  | "approval.command"
  | "approval.workingFolder"
  | "approval.affectedFiles"
  | "approval.reason"
  | "approval.permissions"
  | "approval.runCommand"
  | "approval.continue"
  | "approval.cancel"
  | "approval.permission.hostExec"
  | "approval.permission.hostCwd"
  | "approval.permission.write"
  | "approval.permission.generic"
  | "approval.permission.genericWithCapability"
  | "settings.loadingTitle"
  | "settings.loadingDetail"
  | "settings.failedTitle"
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
  | "settings.fixJsonField"
  | "settings.provider"
  | "settings.plugins"
  | "settings.jsonc"
  | "settings.validating"
  | "settings.unsaved"
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
  | "settings.eventStoreJson"
  | "settings.compaction"
  | "settings.compactionJson"
  | "settings.registryStore"
  | "settings.registryStoreJson"
  | "settings.storageDefault"
  | "settings.storageMemory"
  | "settings.storageSqlite"
  | "settings.storagePostgres"
  | "settings.storageObjectMemory"
  | "settings.storageObjectFs"
  | "settings.compactionAuto"
  | "settings.compactionNone"
  | "settings.compactionOpenaiResponses"
  | "settings.addPlugin"
  | "settings.deletePlugin"
  | "settings.noPlugins"
  | "settings.context"
  | "settings.advanced"
  | "settings.advancedJsonc"
  | "settings.providerReasoningTitle";

const messages = {
  en: {
    "bootstrap.loadingTitle": "Starting",
    "bootstrap.loadingDetail": "Reading the Rust-side launch state.",
    "bootstrap.failedTitle": "Bootstrap failed",
    "chat.openSettings": "Open settings",
    "chat.prepareEnvironment": "Set up environment",
    "chat.sessionToolbar": "Session controls",
    "chat.connectingTitle": "Connecting",
    "chat.connectingDetail": "Initializing the interaction client.",
    "chat.missingConfigTitle": "Profile configuration is missing",
    "chat.missingConfigDetail":
      "Create or save a profile configuration before starting a chat session.",
    "chat.unavailableTitle": "Choose an environment",
    "chat.unavailableDetail": "Set up a profile before starting a conversation.",
    "chat.initializationFailedTitle": "Interaction initialization failed",
    "chat.failedTitle": "Interaction failed",
    "sessions.title": "Sessions",
    "sessions.create": "Create session",
    "sessions.loading": "Loading sessions...",
    "sessions.empty": "No sessions yet. Send a message to create one.",
    "sessionsPanel.title": "Sessions",
    "sessionsPanel.close": "Close sessions",
    "runtime.disconnected": "Display stream disconnected",
    "runtime.interrupted": "Display stream was interrupted.",
    "transcript.newSessionTitle": "Start with a question.",
    "sessionContext.environment": "{profile} environment",
    "sessionContext.running": "{profile} is thinking",
    "sessionContext.paused": "{profile} needs a decision",
    "sessionContext.failed": "{profile} stopped with an error",
    "sessionContext.aborted": "{profile} stopped",
    "message.sending": "sending",
    "message.userLabel": "Your message",
    "message.assistantLabel": "Assistant message",
    "message.pendingUserLabel": "Sending your message",
    "composer.write": "Write a message...",
    "composer.waitingForApproval": "Waiting for your approval...",
    "composer.connecting": "Connecting display stream...",
    "composer.attach": "Attach files",
    "composer.send": "Send message",
    "composer.expand": "Expand composer",
    "composer.collapse": "Collapse composer",
    "composer.editingDraft": "Editing draft",
    "composer.removeAttachment": "Remove {name}",
    "tool.running": "Running",
    "tool.done": "Done",
    "tool.failed": "Failed",
    "tool.genericTitle": "Using a local tool",
    "tool.previewTitle": "Inspecting preview",
    "tool.commandTitle": "Local command",
    "run.stop": "Stop Run",
    "reasoning.thinking": "Thinking",
    "reasoning.thoughtFor": "Thought for {duration}",
    "reasoning.hideDetails": "Hide details",
    "reasoning.showDetails": "Show details",
    "reasoning.empty": "No visible reasoning yet.",
    "approval.required": "Approval required",
    "approval.actionTitle": "Review this action",
    "approval.commandTitle": "Run a local command?",
    "approval.commandSummary": "Noloong wants to run this command in your project.",
    "approval.projectWriteSummary": "This action can change files in your project.",
    "approval.actionSummary": "Noloong needs your permission before continuing.",
    "approval.pending": "Waiting for your decision",
    "approval.approved": "Approved",
    "approval.denied": "Canceled",
    "approval.expired": "Expired",
    "approval.command": "Command",
    "approval.workingFolder": "Working folder",
    "approval.affectedFiles": "Affected files",
    "approval.reason": "Why",
    "approval.permissions": "Access",
    "approval.runCommand": "Run Local Command",
    "approval.continue": "Continue",
    "approval.cancel": "Cancel",
    "approval.permission.hostExec": "Can run a local command.",
    "approval.permission.hostCwd": "Uses the selected working folder.",
    "approval.permission.write": "Can change files in the project.",
    "approval.permission.generic": "Requests local access for this action.",
    "approval.permission.genericWithCapability": "Requests local access: {capability}.",
    "settings.loadingTitle": "Loading settings",
    "settings.loadingDetail": "Reading profile configuration.",
    "settings.failedTitle": "Settings failed",
    "settings.save": "Save Changes",
    "settings.saved": "Saved",
    "settings.savedAndApplied": "Saved and applied",
    "settings.savedApplyFailed": "Saved, but applying it failed: {error}",
    "settings.savedExternal": "Saved. Restart the external runtime to use the changes.",
    "settings.environmentTitle": "Environment",
    "settings.currentProfile": "Current profile",
    "settings.discard": "Discard Changes",
    "settings.saving": "Saving Changes",
    "settings.profile": "Profile",
    "settings.profileId": "Profile ID",
    "settings.fixJsonc":
      "Fix JSONC errors to restore the typed profile form. The last valid draft is preserved.",
    "settings.fixJsonField": "Fix the invalid JSON field before saving.",
    "settings.provider": "Provider",
    "settings.plugins": "Plugins",
    "settings.jsonc": "JSONC",
    "settings.validating": "Validating...",
    "settings.unsaved": "Unsaved changes",
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
    "settings.eventStoreJson": "Event store JSON",
    "settings.compaction": "Compaction",
    "settings.compactionJson": "Compaction JSON",
    "settings.registryStore": "Registry store",
    "settings.registryStoreJson": "Registry store JSON",
    "settings.storageDefault": "Default state database",
    "settings.storageMemory": "Memory",
    "settings.storageSqlite": "SQLite",
    "settings.storagePostgres": "Postgres",
    "settings.storageObjectMemory": "Object memory",
    "settings.storageObjectFs": "Object filesystem",
    "settings.compactionAuto": "Automatic",
    "settings.compactionNone": "Disabled",
    "settings.compactionOpenaiResponses": "OpenAI Responses",
    "settings.addPlugin": "Add plugin",
    "settings.deletePlugin": "Delete plugin",
    "settings.noPlugins": "No plugins in this profile.",
    "settings.context": "Context",
    "settings.advanced": "Advanced",
    "settings.advancedJsonc": "Advanced JSONC",
    "settings.providerReasoningTitle": "Reasoning",
  },
  zh: {
    "bootstrap.loadingTitle": "正在启动",
    "bootstrap.loadingDetail": "正在读取 Rust 侧启动状态。",
    "bootstrap.failedTitle": "启动失败",
    "chat.openSettings": "打开设置",
    "chat.prepareEnvironment": "设置环境",
    "chat.sessionToolbar": "会话控制",
    "chat.connectingTitle": "正在连接",
    "chat.connectingDetail": "正在初始化交互客户端。",
    "chat.missingConfigTitle": "缺少配置",
    "chat.missingConfigDetail": "请先创建或保存配置，然后再开始聊天会话。",
    "chat.unavailableTitle": "选择环境",
    "chat.unavailableDetail": "开始对话前，请先设置一个配置档。",
    "chat.initializationFailedTitle": "交互初始化失败",
    "chat.failedTitle": "交互失败",
    "sessions.title": "会话",
    "sessions.create": "创建会话",
    "sessions.loading": "正在加载会话...",
    "sessions.empty": "还没有会话。发送消息后会自动创建。",
    "sessionsPanel.title": "会话",
    "sessionsPanel.close": "关闭会话面板",
    "runtime.disconnected": "Display 流已断开",
    "runtime.interrupted": "Display 流已中断。",
    "transcript.newSessionTitle": "从一个问题开始。",
    "sessionContext.environment": "{profile} 环境",
    "sessionContext.running": "{profile} 正在思考",
    "sessionContext.paused": "{profile} 需要你决定",
    "sessionContext.failed": "{profile} 已因错误停止",
    "sessionContext.aborted": "{profile} 已停止",
    "message.sending": "发送中",
    "message.userLabel": "你的消息",
    "message.assistantLabel": "助手消息",
    "message.pendingUserLabel": "正在发送你的消息",
    "composer.write": "输入消息...",
    "composer.waitingForApproval": "等待你审批...",
    "composer.connecting": "正在连接 Display 流...",
    "composer.attach": "添加附件",
    "composer.send": "发送消息",
    "composer.expand": "展开输入区",
    "composer.collapse": "收起输入区",
    "composer.editingDraft": "正在编辑草稿",
    "composer.removeAttachment": "移除 {name}",
    "tool.running": "运行中",
    "tool.done": "完成",
    "tool.failed": "失败",
    "tool.genericTitle": "正在使用本地工具",
    "tool.previewTitle": "正在检查预览",
    "tool.commandTitle": "本地命令",
    "run.stop": "停止运行",
    "reasoning.thinking": "正在思考",
    "reasoning.thoughtFor": "思考了 {duration}",
    "reasoning.hideDetails": "隐藏详情",
    "reasoning.showDetails": "显示详情",
    "reasoning.empty": "暂时没有可见思考内容。",
    "approval.required": "需要审批",
    "approval.actionTitle": "确认这次操作",
    "approval.commandTitle": "运行本地命令？",
    "approval.commandSummary": "Noloong 想在你的项目里运行这条命令。",
    "approval.projectWriteSummary": "这次操作可以修改你的项目文件。",
    "approval.actionSummary": "Noloong 需要你的许可后才能继续。",
    "approval.pending": "等待你决定",
    "approval.approved": "已同意",
    "approval.denied": "已取消",
    "approval.expired": "已过期",
    "approval.command": "命令",
    "approval.workingFolder": "工作目录",
    "approval.affectedFiles": "受影响文件",
    "approval.reason": "原因",
    "approval.permissions": "权限",
    "approval.runCommand": "运行本地命令",
    "approval.continue": "继续",
    "approval.cancel": "取消",
    "approval.permission.hostExec": "可以运行本地命令。",
    "approval.permission.hostCwd": "会使用选定的工作目录。",
    "approval.permission.write": "可以修改项目文件。",
    "approval.permission.generic": "请求这次操作所需的本地访问权限。",
    "approval.permission.genericWithCapability": "请求本地访问权限：{capability}。",
    "settings.loadingTitle": "正在加载设置",
    "settings.loadingDetail": "正在读取配置。",
    "settings.failedTitle": "设置加载失败",
    "settings.save": "保存更改",
    "settings.saved": "已保存",
    "settings.savedAndApplied": "已保存并应用",
    "settings.savedApplyFailed": "已保存，但应用失败：{error}",
    "settings.savedExternal": "已保存。请重启外部运行时以使用更改。",
    "settings.environmentTitle": "环境",
    "settings.currentProfile": "当前配置档",
    "settings.discard": "放弃更改",
    "settings.saving": "正在保存更改",
    "settings.profile": "配置档",
    "settings.profileId": "配置档 ID",
    "settings.fixJsonc": "请先修复 JSONC 错误，表单会保留最后一次有效配置。",
    "settings.fixJsonField": "请先修复无效的 JSON 字段，然后再保存。",
    "settings.provider": "提供商",
    "settings.plugins": "插件",
    "settings.jsonc": "JSONC",
    "settings.validating": "正在检查...",
    "settings.unsaved": "有未保存修改",
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
    "settings.eventStoreJson": "事件存储 JSON",
    "settings.compaction": "上下文压缩",
    "settings.compactionJson": "上下文压缩 JSON",
    "settings.registryStore": "Registry 存储",
    "settings.registryStoreJson": "Registry 存储 JSON",
    "settings.storageDefault": "默认状态数据库",
    "settings.storageMemory": "内存",
    "settings.storageSqlite": "SQLite",
    "settings.storagePostgres": "Postgres",
    "settings.storageObjectMemory": "对象内存",
    "settings.storageObjectFs": "对象文件系统",
    "settings.compactionAuto": "自动",
    "settings.compactionNone": "关闭",
    "settings.compactionOpenaiResponses": "OpenAI Responses",
    "settings.addPlugin": "新增插件",
    "settings.deletePlugin": "删除插件",
    "settings.noPlugins": "当前配置档没有插件。",
    "settings.context": "上下文",
    "settings.advanced": "高级",
    "settings.advancedJsonc": "高级 JSONC",
    "settings.providerReasoningTitle": "推理",
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
