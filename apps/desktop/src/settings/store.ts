import type {
  AgentPluginDeclaration,
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  BuiltInProviderConfig,
  HostProfileConfig,
  ProfileCompactionConfig,
  ProfileEventStoreConfig,
  RegistryStoreConfig,
  RuntimeProfileConfig,
} from "../generated/contracts";

export type SettingsDraftState = {
  path: string;
  text: string;
  lastSavedText: string;
  exists: boolean;
  config: HostProfileConfig | null;
  lastSavedConfig: HostProfileConfig | null;
  selectedProfileId: string | null;
  error: string | null;
  dirty: boolean;
  validating: boolean;
  saving: boolean;
};

export type ProfileFormPatch = {
  displayName?: string;
  description?: string;
  model?: string;
  providerType?: BuiltInProviderConfig["type"];
  providerId?: string;
  baseUrl?: string;
  apiKeyEnv?: string;
  stateMode?: "stateful" | "stateless";
  reasoningEnabled?: boolean;
  reasoningEffort?: string;
  reasoningSummary?: string;
  makeDefault?: boolean;
};

export function settingsDraftFromDocument(document: AppProfileConfigDocument): SettingsDraftState {
  const config = document.validation.config ?? null;
  return {
    path: document.path,
    text: document.text,
    lastSavedText: document.text,
    exists: document.exists ?? false,
    config,
    lastSavedConfig: config,
    selectedProfileId: selectedProfileId(config),
    error: document.validation.error ?? null,
    dirty: false,
    validating: false,
    saving: false,
  };
}

export function applyJsoncTextPending(
  state: SettingsDraftState,
  text: string,
): SettingsDraftState {
  return {
    ...state,
    text,
    dirty: text !== state.lastSavedText,
    validating: true,
  };
}

export function applyJsoncValidation(
  state: SettingsDraftState,
  text: string,
  validation: AppProfileConfigValidationResult,
): SettingsDraftState {
  const config = validation.config ?? null;
  return {
    ...state,
    text,
    config,
    selectedProfileId: keepSelectedProfile(config, state.selectedProfileId),
    error: validation.error ?? null,
    dirty: text !== state.lastSavedText,
    validating: false,
  };
}

export function applySavedDocument(
  state: SettingsDraftState,
  document: AppProfileConfigDocument,
): SettingsDraftState {
  return {
    ...settingsDraftFromDocument(document),
    selectedProfileId: keepSelectedProfile(document.validation.config ?? null, state.selectedProfileId),
  };
}

export function setSaving(state: SettingsDraftState, saving: boolean): SettingsDraftState {
  return { ...state, saving };
}

export function selectProfile(
  state: SettingsDraftState,
  profileId: string,
): SettingsDraftState {
  if (!state.config?.profiles.some((profile) => profile.profileId === profileId)) {
    return state;
  }
  return { ...state, selectedProfileId: profileId };
}

export function updateSelectedProfile(
  state: SettingsDraftState,
  patch: ProfileFormPatch,
): SettingsDraftState {
  if (!state.config || !state.selectedProfileId) {
    return state;
  }
  const config: HostProfileConfig = {
    ...state.config,
    profiles: state.config.profiles.map((profile) =>
      profile.profileId === state.selectedProfileId ? patchProfile(profile, patch) : profile,
    ),
  };
  if (patch.makeDefault) {
    config.defaultProfileId = state.selectedProfileId;
  }
  const text = canonicalProfileConfigText(config);
  return {
    ...state,
    text,
    config,
    error: null,
    dirty: text !== state.lastSavedText,
    validating: false,
  };
}

export function discardSettingsChanges(state: SettingsDraftState): SettingsDraftState {
  return {
    ...state,
    text: state.lastSavedText,
    config: state.lastSavedConfig,
    selectedProfileId: keepSelectedProfile(state.lastSavedConfig, state.selectedProfileId),
    error: null,
    dirty: false,
    validating: false,
    saving: false,
  };
}

export function renameSelectedProfile(
  state: SettingsDraftState,
  nextProfileId: string,
): SettingsDraftState {
  if (!state.config || !state.selectedProfileId) {
    return state;
  }
  const profileId = nextProfileId.trim();
  const currentProfileId = state.selectedProfileId;
  if (!profileId || profileId === currentProfileId) {
    return state;
  }
  if (state.config.profiles.some((profile) => profile.profileId === profileId)) {
    return state;
  }
  return replaceConfig(
    state,
    {
      ...state.config,
      defaultProfileId:
        state.config.defaultProfileId === currentProfileId
          ? profileId
          : state.config.defaultProfileId,
      profiles: state.config.profiles.map((profile) =>
        profile.profileId === currentProfileId ? { ...profile, profileId } : profile,
      ),
    },
    profileId,
  );
}

export function addProfile(state: SettingsDraftState): SettingsDraftState {
  if (!state.config) {
    return state;
  }
  const profile = defaultProfile(uniqueId("profile", state.config.profiles.map((item) => item.profileId)));
  return replaceConfig(state, {
    ...state.config,
    profiles: [...state.config.profiles, profile],
    defaultProfileId: state.config.defaultProfileId ?? profile.profileId,
  }, profile.profileId);
}

export function copySelectedProfile(state: SettingsDraftState): SettingsDraftState {
  const profile = selectedProfile(state);
  if (!state.config || !profile) {
    return state;
  }
  const profileId = uniqueId(`${profile.profileId}-copy`, state.config.profiles.map((item) => item.profileId));
  return replaceConfig(
    state,
    {
      ...state.config,
      profiles: [
        ...state.config.profiles,
        {
          ...clone(profile),
          profileId,
          displayName: `${profile.displayName} Copy`,
        },
      ],
    },
    profileId,
  );
}

export function deleteSelectedProfile(state: SettingsDraftState): SettingsDraftState {
  if (!state.config || !state.selectedProfileId || state.config.profiles.length <= 1) {
    return state;
  }
  const profiles = state.config.profiles.filter(
    (profile) => profile.profileId !== state.selectedProfileId,
  );
  const defaultProfileId =
    state.config.defaultProfileId === state.selectedProfileId
      ? profiles[0]?.profileId
      : state.config.defaultProfileId;
  return replaceConfig(state, { ...state.config, profiles, defaultProfileId }, defaultProfileId);
}

export function updateRegistryStore(
  state: SettingsDraftState,
  registryStore: RegistryStoreConfig | null,
): SettingsDraftState {
  if (!state.config) {
    return state;
  }
  return replaceConfig(state, { ...state.config, registryStore });
}

export function updateSelectedProfileEventStore(
  state: SettingsDraftState,
  eventStore: ProfileEventStoreConfig | null,
): SettingsDraftState {
  return updateSelectedProfileObject(state, (profile) => ({ ...profile, eventStore }));
}

export function updateSelectedProfileCompaction(
  state: SettingsDraftState,
  compaction: ProfileCompactionConfig,
): SettingsDraftState {
  return updateSelectedProfileObject(state, (profile) => ({ ...profile, compaction }));
}

export function upsertSelectedPlugin(
  state: SettingsDraftState,
  plugin: AgentPluginDeclaration,
  index: number | null,
): SettingsDraftState {
  return updateSelectedProfileObject(state, (profile) => {
    const plugins = [...(profile.plugins ?? [])];
    if (index === null) {
      plugins.push(plugin);
    } else {
      plugins[index] = plugin;
    }
    return { ...profile, plugins };
  });
}

export function deleteSelectedPlugin(state: SettingsDraftState, index: number): SettingsDraftState {
  return updateSelectedProfileObject(state, (profile) => ({
    ...profile,
    plugins: (profile.plugins ?? []).filter((_, itemIndex) => itemIndex !== index),
  }));
}

export function selectedProfile(state: SettingsDraftState): RuntimeProfileConfig | null {
  if (!state.config || !state.selectedProfileId) {
    return null;
  }
  return (
    state.config.profiles.find((profile) => profile.profileId === state.selectedProfileId) ?? null
  );
}

export function providerModel(provider: BuiltInProviderConfig): string {
  return provider.model;
}

export function canSaveSettings(state: SettingsDraftState): boolean {
  return state.dirty && Boolean(state.config) && !state.error && !state.validating && !state.saving;
}

function patchProfile(
  profile: RuntimeProfileConfig,
  patch: ProfileFormPatch,
): RuntimeProfileConfig {
  const provider = patchProvider(profile.provider, patch);
  const compaction = normalizeCompactionForProvider(profile.compaction ?? { type: "auto" }, provider);
  return {
    ...profile,
    displayName: patch.displayName ?? profile.displayName,
    description: patch.description ?? profile.description,
    provider,
    compaction,
  };
}

function normalizeCompactionForProvider(
  compaction: ProfileCompactionConfig,
  provider: BuiltInProviderConfig,
): ProfileCompactionConfig {
  if (provider.type === "chatgpt_responses" || compaction.type !== "openai_responses") {
    return compaction;
  }
  return { type: "auto" };
}

function patchProvider(
  provider: BuiltInProviderConfig,
  patch: ProfileFormPatch,
): BuiltInProviderConfig {
  const next =
    patch.providerType && patch.providerType !== provider.type
      ? defaultProvider(patch.providerType)
      : { ...provider };
  if (patch.model !== undefined) {
    next.model = patch.model;
  }
  if ("providerId" in patch) {
    next.providerId = emptyToNull(patch.providerId);
  }
  if ("baseUrl" in patch && "baseUrl" in next) {
    next.baseUrl = emptyToNull(patch.baseUrl);
  }
  if ("apiKeyEnv" in patch && "apiKeyEnv" in next) {
    next.apiKeyEnv = emptyToNull(patch.apiKeyEnv);
  }
  if ("stateMode" in patch && "stateMode" in next) {
    next.stateMode = patch.stateMode;
  }
  patchReasoning(next, patch);
  return next;
}

function patchReasoning(provider: BuiltInProviderConfig, patch: ProfileFormPatch): void {
  if (provider.type === "responses" || provider.type === "chatgpt_responses") {
    const reasoning = { ...(provider.reasoning ?? {}) };
    if ("reasoningEnabled" in patch) {
      reasoning.enabled = patch.reasoningEnabled;
    }
    if ("reasoningEffort" in patch) {
      reasoning.effort = emptyToNull(patch.reasoningEffort) as typeof reasoning.effort;
    }
    if ("reasoningSummary" in patch) {
      reasoning.summary = emptyToNull(patch.reasoningSummary) as typeof reasoning.summary;
    }
    provider.reasoning = reasoning;
    return;
  }
  if (provider.type === "chat_completions") {
    const reasoning = { ...(provider.reasoning ?? {}) };
    if ("reasoningEnabled" in patch) {
      reasoning.enabled = patch.reasoningEnabled;
    }
    if ("reasoningEffort" in patch) {
      reasoning.effort = emptyToNull(patch.reasoningEffort) as typeof reasoning.effort;
    }
    provider.reasoning = reasoning;
    return;
  }
  if (provider.type === "anthropic_messages" && "reasoningEffort" in patch) {
    provider.reasoning = {
      ...(provider.reasoning ?? {}),
      effort: emptyToNull(patch.reasoningEffort) as NonNullable<
        typeof provider.reasoning
      >["effort"],
    };
  }
}

function selectedProfileId(config: HostProfileConfig | null): string | null {
  if (!config) {
    return null;
  }
  return keepSelectedProfile(config, config.defaultProfileId ?? null);
}

function keepSelectedProfile(
  config: HostProfileConfig | null,
  currentProfileId: string | null,
): string | null {
  if (!config || config.profiles.length === 0) {
    return null;
  }
  if (currentProfileId && config.profiles.some((profile) => profile.profileId === currentProfileId)) {
    return currentProfileId;
  }
  if (
    config.defaultProfileId &&
    config.profiles.some((profile) => profile.profileId === config.defaultProfileId)
  ) {
    return config.defaultProfileId;
  }
  return config.profiles[0]?.profileId ?? null;
}

function canonicalProfileConfigText(config: HostProfileConfig): string {
  return JSON.stringify(config, null, 2);
}

function replaceConfig(
  state: SettingsDraftState,
  config: HostProfileConfig,
  selectedProfileId = state.selectedProfileId,
): SettingsDraftState {
  const text = canonicalProfileConfigText(config);
  return {
    ...state,
    text,
    config,
    selectedProfileId: keepSelectedProfile(config, selectedProfileId),
    error: null,
    dirty: text !== state.lastSavedText,
    validating: false,
  };
}

function updateSelectedProfileObject(
  state: SettingsDraftState,
  update: (profile: RuntimeProfileConfig) => RuntimeProfileConfig,
): SettingsDraftState {
  if (!state.config || !state.selectedProfileId) {
    return state;
  }
  return replaceConfig(state, {
    ...state.config,
    profiles: state.config.profiles.map((profile) =>
      profile.profileId === state.selectedProfileId ? update(profile) : profile,
    ),
  });
}

function defaultProfile(profileId: string): RuntimeProfileConfig {
  return {
    profileId,
    displayName: titleFromId(profileId),
    provider: defaultProvider("chatgpt_responses"),
    compaction: { type: "auto" },
    plugins: [],
    manifestPatches: [{ op: "set_locale", locale: "zh" }],
    metadata: {},
  };
}

function defaultProvider(type: BuiltInProviderConfig["type"]): BuiltInProviderConfig {
  switch (type) {
    case "responses":
      return { type, model: "gpt-5.4-mini", stateMode: "stateful", allowFileDataUrlInput: true };
    case "chat_completions":
      return { type, model: "gpt-5.4-mini" };
    case "anthropic_messages":
      return { type, model: "claude-test" };
    case "chatgpt_responses":
      return {
        type,
        model: "gpt-5.4-mini",
        auth: { type: "token_file" },
        stateMode: "stateful",
        allowFileDataUrlInput: true,
        reasoning: { enabled: true, effort: "medium", summary: "auto" },
      };
  }
}

function uniqueId(base: string, existing: string[]): string {
  const normalized = base
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "") || "profile";
  if (!existing.includes(normalized)) {
    return normalized;
  }
  for (let index = 2; ; index += 1) {
    const candidate = `${normalized}-${index}`;
    if (!existing.includes(candidate)) {
      return candidate;
    }
  }
}

function titleFromId(id: string): string {
  return id
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

function emptyToNull(value: string | undefined): string | null | undefined {
  if (value === undefined) {
    return undefined;
  }
  return value.trim() ? value : null;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}
