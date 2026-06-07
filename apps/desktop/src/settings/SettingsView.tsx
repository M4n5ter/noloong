import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AppProfileConfigCompletionSet,
  AppProfileConfigDocument,
  AppProfileConfigValidationResult,
  HostProfileConfig,
} from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { JsoncEditor } from "./JsoncEditor";
import {
  applyJsoncTextPending,
  applyJsoncValidation,
  applySavedDocument,
  canSaveSettings,
  providerModel,
  selectedProfile,
  selectProfile,
  setSaving,
  settingsDraftFromDocument,
  type SettingsDraftState,
  updateSelectedProfile,
} from "./store";

export function SettingsView({ i18n, onBack }: { i18n: AppI18n; onBack: () => void }) {
  const [state, setState] = useState<SettingsViewState>({ status: "loading" });
  const validationRevisionRef = useRef(0);

  useEffect(() => {
    let active = true;
    loadProfileConfig()
      .then((document) => {
        if (active) {
          setState({ status: "ready", draft: settingsDraftFromDocument(document), notice: null });
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setState({ status: "failed", error: String(error) });
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const updateDraft = useCallback((update: (draft: SettingsDraftState) => SettingsDraftState) => {
    setState((current) =>
      current.status === "ready" ? { ...current, draft: update(current.draft) } : current,
    );
  }, []);

  const validateCurrentText = useCallback(async () => {
    if (state.status !== "ready") {
      return;
    }
    const validation = await validateProfileConfig(state.draft.text);
    setState((current) =>
      current.status === "ready"
        ? {
            ...current,
            draft: applyJsoncValidation(current.draft, current.draft.text, validation),
            notice: validation.valid ? i18n.t("settings.valid") : i18n.t("settings.invalid"),
          }
        : current,
    );
  }, [i18n, state]);

  const saveCurrentText = useCallback(async () => {
    if (state.status !== "ready" || !canSaveSettings(state.draft)) {
      return;
    }
    setState({ ...state, draft: setSaving(state.draft, true), notice: null });
    try {
      const document = await saveProfileConfig(state.draft.text);
      setState((current) =>
        current.status === "ready"
          ? {
              ...current,
              draft: applySavedDocument(current.draft, document),
              notice: i18n.t("settings.saved", { path: document.path }),
            }
          : current,
      );
    } catch (error) {
      setState((current) =>
        current.status === "ready"
          ? {
              ...current,
              draft: setSaving(current.draft, false),
              notice: String(error),
            }
          : current,
      );
    }
  }, [i18n, state]);

  const handleJsoncChange = useCallback(
    (text: string) => {
      const revision = validationRevisionRef.current + 1;
      validationRevisionRef.current = revision;
      updateDraft((draft) => applyJsoncTextPending(draft, text));
      void validateProfileConfig(text).then((validation) => {
        if (validationRevisionRef.current !== revision) {
          return;
        }
        setState((current) =>
          current.status === "ready"
            ? {
                ...current,
                draft: applyJsoncValidation(current.draft, text, validation),
              }
            : current,
        );
      });
    },
    [updateDraft],
  );

  const completeJsonc = useCallback((text: string, byteOffset: number) => {
    return completeProfileConfig(text, byteOffset);
  }, []);

  if (state.status === "loading") {
    return (
      <SettingsStatus
        title={i18n.t("settings.loadingTitle")}
        detail={i18n.t("settings.loadingDetail")}
      />
    );
  }
  if (state.status === "failed") {
    return <SettingsStatus title={i18n.t("settings.failedTitle")} detail={state.error} />;
  }

  const draft = state.draft;
  const profile = selectedProfile(draft);
  const formDisabled = !draft.config;

  return (
    <section className="settings-shell">
      <div className="settings-header">
        <div>
          <button className="text-button subtle" onClick={onBack} type="button">
            {i18n.t("settings.backToChat")}
          </button>
          <h1>{i18n.t("settings.title")}</h1>
          <p>{draft.path}</p>
        </div>
        <div className="settings-actions">
          <button className="text-button" onClick={() => void validateCurrentText()} type="button">
            {i18n.t("settings.validate")}
          </button>
          <button
            className="text-button primary"
            disabled={!canSaveSettings(draft)}
            onClick={() => void saveCurrentText()}
            type="button"
          >
            {i18n.t("settings.save")}
          </button>
        </div>
      </div>
      {state.notice ? <p className="settings-notice">{state.notice}</p> : null}
      {draft.error ? <p className="settings-error">{draft.error}</p> : null}
      <div className="settings-grid">
        <section className="settings-panel">
          <h2>{i18n.t("settings.profile")}</h2>
          {draft.config ? (
            <ProfileForm
              config={draft.config}
              disabled={formDisabled}
              i18n={i18n}
              selectedProfileId={draft.selectedProfileId}
              onPatch={(patch) => updateDraft((current) => updateSelectedProfile(current, patch))}
              onSelect={(profileId) => updateDraft((current) => selectProfile(current, profileId))}
            />
          ) : (
            <p className="muted">{i18n.t("settings.fixJsonc")}</p>
          )}
          {profile ? (
            <dl className="settings-summary">
              <div>
                <dt>{i18n.t("settings.provider")}</dt>
                <dd>{profile.provider.type}</dd>
              </div>
              <div>
                <dt>{i18n.t("settings.plugins")}</dt>
                <dd>{profile.plugins?.length ?? 0}</dd>
              </div>
              <div>
                <dt>{i18n.t("settings.manifestPatches")}</dt>
                <dd>{profile.manifestPatches?.length ?? 0}</dd>
              </div>
            </dl>
          ) : null}
        </section>
        <section className="settings-panel editor-panel">
          <div className="editor-header">
            <div>
              <h2>{i18n.t("settings.jsonc")}</h2>
              <p>
                {draft.validating
                  ? i18n.t("settings.validating")
                  : draft.dirty
                    ? i18n.t("settings.unsaved")
                    : i18n.t("settings.savedState")}
              </p>
            </div>
            <button
              className="text-button"
              disabled={!draft.config}
              onClick={() =>
                updateDraft((current) =>
                  current.config
                    ? applyJsoncValidation(current, JSON.stringify(current.config, null, 2), {
                        valid: true,
                        config: current.config,
                        canonicalText: JSON.stringify(current.config, null, 2),
                      })
                    : current,
                )
              }
              type="button"
            >
              {i18n.t("settings.format")}
            </button>
          </div>
          <JsoncEditor
            complete={completeJsonc}
            onChange={handleJsoncChange}
            readOnly={draft.saving}
            value={draft.text}
          />
        </section>
      </div>
    </section>
  );
}

type SettingsViewState =
  | { status: "loading" }
  | { status: "failed"; error: string }
  | { status: "ready"; draft: SettingsDraftState; notice: string | null };

function ProfileForm({
  config,
  disabled,
  i18n,
  selectedProfileId,
  onPatch,
  onSelect,
}: {
  config: HostProfileConfig;
  disabled: boolean;
  i18n: AppI18n;
  selectedProfileId: string | null;
  onPatch: (patch: Parameters<typeof updateSelectedProfile>[1]) => void;
  onSelect: (profileId: string) => void;
}) {
  const profile =
    config.profiles.find((item) => item.profileId === selectedProfileId) ?? config.profiles[0];
  if (!profile) {
    return <p className="muted">{i18n.t("settings.noProfile")}</p>;
  }
  const isDefault = config.defaultProfileId === profile.profileId;

  return (
    <div className="settings-form">
      <label>
        <span>{i18n.t("settings.activeProfile")}</span>
        <select
          disabled={disabled}
          onChange={(event) => onSelect(event.target.value)}
          value={profile.profileId}
        >
          {config.profiles.map((item) => (
            <option key={item.profileId} value={item.profileId}>
              {item.displayName}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{i18n.t("settings.name")}</span>
        <input
          disabled={disabled}
          onChange={(event) => onPatch({ displayName: event.target.value })}
          value={profile.displayName}
        />
      </label>
      <label>
        <span>{i18n.t("settings.description")}</span>
        <textarea
          disabled={disabled}
          onChange={(event) => onPatch({ description: event.target.value })}
          rows={3}
          value={profile.description ?? ""}
        />
      </label>
      <label>
        <span>{i18n.t("settings.model")}</span>
        <input
          disabled={disabled}
          onChange={(event) => onPatch({ model: event.target.value })}
          value={providerModel(profile.provider)}
        />
      </label>
      <label className="check-row">
        <input
          checked={isDefault}
          disabled={disabled || isDefault}
          onChange={() => onPatch({ makeDefault: true })}
          type="checkbox"
        />
        <span>{i18n.t("settings.useDefaultProfile")}</span>
      </label>
    </div>
  );
}

function SettingsStatus({ title, detail }: { title: string; detail: string }) {
  return (
    <section className="centered-status">
      <h1>{title}</h1>
      <p>{detail}</p>
    </section>
  );
}

function loadProfileConfig(): Promise<AppProfileConfigDocument> {
  return invoke("app_profile_config_load");
}

function validateProfileConfig(text: string): Promise<AppProfileConfigValidationResult> {
  return invoke("app_profile_config_validate", { request: { text } });
}

function saveProfileConfig(text: string): Promise<AppProfileConfigDocument> {
  return invoke("app_profile_config_save", { request: { text } });
}

function completeProfileConfig(
  text: string,
  offset: number,
): Promise<AppProfileConfigCompletionSet> {
  return invoke("app_profile_config_completions", { request: { text, offset } });
}
