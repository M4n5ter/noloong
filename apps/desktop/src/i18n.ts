import type { AppInteractionStatus, Locale } from "./generated/contracts";
import type { RunStatus } from "./interaction/conversationState";

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
  | "runtime.ready"
  | "runtime.connecting"
  | "runtime.disconnected"
  | "runtime.interrupted"
  | "transcript.newSessionTitle"
  | "transcript.newSessionDetail"
  | "transcript.empty"
  | "message.sending"
  | "composer.write"
  | "composer.connecting"
  | "composer.running"
  | "composer.shortcut"
  | "run.refreshing"
  | "run.stop"
  | "run.idle"
  | "run.running"
  | "run.completed"
  | "run.failed"
  | "run.paused"
  | "run.aborted"
  | "activity.aria"
  | "reasoning.thinking"
  | "reasoning.thoughtFor"
  | "reasoning.hideRaw"
  | "reasoning.showRaw"
  | "reasoning.empty"
  | "approval.required"
  | "approval.allow"
  | "approval.deny"
  | "settings.loadingTitle"
  | "settings.loadingDetail"
  | "settings.failedTitle"
  | "settings.backToChat"
  | "settings.title"
  | "settings.validate"
  | "settings.save"
  | "settings.valid"
  | "settings.invalid"
  | "settings.saved"
  | "settings.profile"
  | "settings.fixJsonc"
  | "settings.provider"
  | "settings.plugins"
  | "settings.manifestPatches"
  | "settings.jsonc"
  | "settings.validating"
  | "settings.unsaved"
  | "settings.savedState"
  | "settings.format"
  | "settings.noProfile"
  | "settings.activeProfile"
  | "settings.name"
  | "settings.description"
  | "settings.model"
  | "settings.useDefaultProfile";

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
    "chat.connectingTitle": "Connecting",
    "chat.connectingDetail": "Initializing the interaction client.",
    "chat.missingConfigTitle": "Profile configuration is missing",
    "chat.missingConfigDetail":
      "Create or save a profile configuration before starting a chat session.",
    "chat.unavailableTitle": "Interaction runtime is unavailable",
    "chat.unavailableDetail":
      "No interaction endpoint is active for this launch. Check the runtime or open settings.",
    "chat.initializationFailedTitle": "Interaction initialization failed",
    "chat.failedTitle": "Interaction failed",
    "sessions.title": "Sessions",
    "sessions.create": "Create session",
    "sessions.loading": "Loading sessions...",
    "sessions.empty": "No sessions yet. Send a message to create one.",
    "runtime.ready": "Display stream ready",
    "runtime.connecting": "Connecting display stream",
    "runtime.disconnected": "Display stream disconnected",
    "runtime.interrupted": "Display stream was interrupted.",
    "transcript.newSessionTitle": "New session",
    "transcript.newSessionDetail": "Send a message to create a session.",
    "transcript.empty": "No messages yet.",
    "message.sending": "sending",
    "composer.write": "Write a message...",
    "composer.connecting": "Connecting display stream...",
    "composer.running": "Running",
    "composer.shortcut": "Cmd+Enter to send",
    "run.refreshing": "Refreshing",
    "run.stop": "Stop",
    "run.idle": "Idle",
    "run.running": "Running",
    "run.completed": "Completed",
    "run.failed": "Failed",
    "run.paused": "Paused",
    "run.aborted": "Aborted",
    "activity.aria": "Run activity",
    "reasoning.thinking": "Thinking",
    "reasoning.thoughtFor": "Thought for {duration}",
    "reasoning.hideRaw": "Hide raw",
    "reasoning.showRaw": "Show raw",
    "reasoning.empty": "No visible reasoning yet.",
    "approval.required": "Approval required",
    "approval.allow": "Allow",
    "approval.deny": "Deny",
    "settings.loadingTitle": "Loading settings",
    "settings.loadingDetail": "Reading profile configuration.",
    "settings.failedTitle": "Settings failed",
    "settings.backToChat": "Back to chat",
    "settings.title": "Settings",
    "settings.validate": "Validate",
    "settings.save": "Save",
    "settings.valid": "Configuration is valid.",
    "settings.invalid": "Configuration is invalid.",
    "settings.saved": "Saved {path}",
    "settings.profile": "Profile",
    "settings.fixJsonc":
      "Fix JSONC errors to restore the typed profile form. The last valid draft is preserved.",
    "settings.provider": "Provider",
    "settings.plugins": "Plugins",
    "settings.manifestPatches": "Manifest patches",
    "settings.jsonc": "JSONC",
    "settings.validating": "Validating...",
    "settings.unsaved": "Unsaved changes",
    "settings.savedState": "Saved",
    "settings.format": "Format",
    "settings.noProfile": "No profile exists.",
    "settings.activeProfile": "Active profile",
    "settings.name": "Name",
    "settings.description": "Description",
    "settings.model": "Model",
    "settings.useDefaultProfile": "Use as default profile",
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
    "chat.connectingTitle": "正在连接",
    "chat.connectingDetail": "正在初始化交互客户端。",
    "chat.missingConfigTitle": "缺少配置",
    "chat.missingConfigDetail": "请先创建或保存配置，然后再开始聊天会话。",
    "chat.unavailableTitle": "交互运行时不可用",
    "chat.unavailableDetail": "本次启动没有可用的交互端点。请检查运行时，或先打开设置。",
    "chat.initializationFailedTitle": "交互初始化失败",
    "chat.failedTitle": "交互失败",
    "sessions.title": "会话",
    "sessions.create": "创建会话",
    "sessions.loading": "正在加载会话...",
    "sessions.empty": "还没有会话。发送消息后会自动创建。",
    "runtime.ready": "Display 流已连接",
    "runtime.connecting": "正在连接 Display 流",
    "runtime.disconnected": "Display 流已断开",
    "runtime.interrupted": "Display 流已中断。",
    "transcript.newSessionTitle": "新会话",
    "transcript.newSessionDetail": "发送消息后会自动创建会话。",
    "transcript.empty": "还没有消息。",
    "message.sending": "发送中",
    "composer.write": "输入消息...",
    "composer.connecting": "正在连接 Display 流...",
    "composer.running": "正在运行",
    "composer.shortcut": "Cmd+Enter 发送",
    "run.refreshing": "正在刷新",
    "run.stop": "停止",
    "run.idle": "空闲",
    "run.running": "运行中",
    "run.completed": "已完成",
    "run.failed": "失败",
    "run.paused": "已暂停",
    "run.aborted": "已中止",
    "activity.aria": "运行活动",
    "reasoning.thinking": "正在思考",
    "reasoning.thoughtFor": "思考了 {duration}",
    "reasoning.hideRaw": "隐藏原文",
    "reasoning.showRaw": "显示原文",
    "reasoning.empty": "暂时没有可见思考内容。",
    "approval.required": "需要审批",
    "approval.allow": "同意",
    "approval.deny": "拒绝",
    "settings.loadingTitle": "正在加载设置",
    "settings.loadingDetail": "正在读取配置。",
    "settings.failedTitle": "设置加载失败",
    "settings.backToChat": "返回聊天",
    "settings.title": "设置",
    "settings.validate": "检查",
    "settings.save": "保存",
    "settings.valid": "配置有效。",
    "settings.invalid": "配置无效。",
    "settings.saved": "已保存 {path}",
    "settings.profile": "配置档",
    "settings.fixJsonc": "请先修复 JSONC 错误，表单会保留最后一次有效配置。",
    "settings.provider": "提供商",
    "settings.plugins": "插件",
    "settings.manifestPatches": "Manifest 补丁",
    "settings.jsonc": "JSONC",
    "settings.validating": "正在检查...",
    "settings.unsaved": "有未保存修改",
    "settings.savedState": "已保存",
    "settings.format": "格式化",
    "settings.noProfile": "还没有配置档。",
    "settings.activeProfile": "当前配置档",
    "settings.name": "名称",
    "settings.description": "描述",
    "settings.model": "模型",
    "settings.useDefaultProfile": "设为默认配置档",
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
    runStatus(status: RunStatus, detail?: string | null) {
      const key = `run.${status}` as I18nKey;
      const label = catalog[key] ?? status;
      return detail ? `${label} · ${detail}` : label;
    },
    streamStatus(status: "connecting" | "ready" | "failed", error: string | null) {
      if (status === "ready") {
        return catalog["runtime.ready"];
      }
      if (status === "failed") {
        return error || catalog["runtime.disconnected"];
      }
      return error || catalog["runtime.connecting"];
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
