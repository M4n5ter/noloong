import type {
  AppLaunchOptions,
  AppProfileConfigCompletionSet,
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  HostProfileConfig,
} from "./generated/contracts";

export function isTauriRuntime(): boolean {
  return typeof window === "undefined" || "__TAURI_INTERNALS__" in window;
}

export function devLaunchOptions(): AppLaunchOptions {
  return {
    appVersion: "web-dev",
    interactionEndpoint: null,
    interactionStatus: { status: "unavailable" },
    locale: null,
    profileConfigPath: "/tmp/noloong-web-dev-profile.jsonc",
    runtimeControlEndpoint: null,
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
    defaultProfileId: "desktop-dev",
    registryStore: { type: "memory" },
    profiles: [
      {
        profileId: "desktop-dev",
        displayName: "Desktop Dev",
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
