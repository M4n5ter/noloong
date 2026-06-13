import type {
  AppInteractionEndpoint,
  AppInteractionStatus,
  AppLaunchOptions,
  AppProfileConfigCompletionSet,
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  HostProfileConfig,
} from "./generated/contracts";

export const DEV_INTERACTION_SERVER_NAME = "noloong-dev-preview";
export const DEV_INTERACTION_PROTOCOL_VERSION = "dev";
export const DEV_PROFILE_DISPLAY_NAME = "Desktop Dev";
export const DEV_PROFILE_ID = "desktop-dev";

export function isTauriRuntime(): boolean {
  return typeof window === "undefined" || "__TAURI_INTERNALS__" in window;
}

export function devLaunchOptions(): AppLaunchOptions {
  return {
    appVersion: "web-dev",
    interactionEndpoint: devInteractionEndpoint(),
    interactionStatus: devInteractionStatus(),
    locale: null,
    profileConfigPath: "/tmp/noloong-web-dev-profile.jsonc",
    runtimeControlEndpoint: null,
  };
}

export function devInteractionEndpoint(): AppInteractionEndpoint {
  return {
    wsUrl: "ws://127.0.0.1:7777/noloong-dev/ws",
  };
}

export function devInteractionStatus(): AppInteractionStatus {
  return {
    status: "ready",
    serverName: DEV_INTERACTION_SERVER_NAME,
    protocolVersion: DEV_INTERACTION_PROTOCOL_VERSION,
    profiles: [{ profileId: DEV_PROFILE_ID, displayName: DEV_PROFILE_DISPLAY_NAME }],
  };
}

export function devProfileConfigDocument(text = devProfileConfigText()): AppProfileConfigDocument {
  return {
    exists: true,
    path: "/tmp/noloong-web-dev-profile.jsonc",
    text,
    validation: devValidateProfileConfig(text),
  };
}

export function devValidateProfileConfig(text: string): AppProfileConfigValidationResult {
  try {
    const config = JSON.parse(text) as HostProfileConfig;
    return {
      valid: true,
      config,
      canonicalText: JSON.stringify(config, null, 2),
    };
  } catch (error) {
    return {
      valid: false,
      error: String(error),
    };
  }
}

export function devProfileConfigCompletions(): AppProfileConfigCompletionSet {
  return {
    completions: [],
    replaceStart: 0,
  };
}

function devProfileConfigText(): string {
  return JSON.stringify(devProfileConfig(), null, 2);
}

function devProfileConfig(): HostProfileConfig {
  return {
    defaultProfileId: DEV_PROFILE_ID,
    registryStore: { type: "memory" },
    profiles: [
      {
        profileId: DEV_PROFILE_ID,
        displayName: DEV_PROFILE_DISPLAY_NAME,
        description: "Browser-only preview profile for visual QA.",
        provider: {
          type: "chatgpt_responses",
          model: "gpt-5.4-mini",
          allowFileDataUrlInput: true,
        },
        compaction: { type: "auto" },
        eventStore: { type: "memory" },
        plugins: [],
        manifestPatches: [{ op: "set_locale", locale: "zh" }],
        metadata: {},
      },
    ],
  };
}
