import type {
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  BuiltInProviderConfig,
  HostProfileConfig,
  RuntimeProfileConfig,
} from "../generated/contracts";

export type SettingsDraftState = {
  path: string;
  text: string;
  lastSavedText: string;
  exists: boolean;
  config: HostProfileConfig | null;
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
  return {
    ...profile,
    displayName: patch.displayName ?? profile.displayName,
    description: patch.description ?? profile.description,
    provider:
      patch.model === undefined
        ? profile.provider
        : {
            ...profile.provider,
            model: patch.model,
          },
  };
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
