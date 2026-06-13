import {
  Braces,
  ChevronLeft,
  Copy,
  Database,
  Plug,
  Plus,
  RotateCcw,
  Save,
  Server,
  Settings2,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentPluginDeclaration,
  AppLaunchOptions,
  AppProfileConfigCompletionSet,
  AppProfileConfigDocument,
  AppRuntimeRestartResult,
  HostProfileConfig,
  ProfileCompactionConfig,
  ProfileEventStoreConfig,
  RegistryStoreConfig,
} from "../generated/contracts";
import type { AppI18n } from "../i18n";
import {
  completeProfileConfig,
  loadProfileConfig,
  restartInteraction,
  saveProfileConfig,
  validateProfileConfig,
} from "./api";
import { JsonListEditor, JsonObjectEditor } from "./JsonEditors";
import { JsoncEditor } from "./JsoncEditor";
import { environmentSummaryItems } from "./environmentSummary";
import {
  addProfile,
  applyJsoncTextPending,
  applyJsoncValidation,
  applySavedDocument,
  canSaveSettings,
  copySelectedProfile,
  deleteSelectedPlugin,
  deleteSelectedProfile,
  discardSettingsChanges,
  providerModel,
  renameSelectedProfile,
  selectedProfile,
  selectProfile,
  setSaving,
  settingsDraftFromDocument,
  type SettingsDraftState,
  updateRegistryStore,
  updateSelectedProfile,
  updateSelectedProfileStorage,
  upsertSelectedPlugin,
} from "./store";

type SettingsNode =
  | "profile"
  | "provider"
  | "storage"
  | "plugins"
  | "jsonc";

export function SettingsView({
  i18n,
  launchOptions,
  onBack,
  onRuntimeRestart,
}: {
  i18n: AppI18n;
  launchOptions: AppLaunchOptions;
  onBack: () => void;
  onRuntimeRestart: (result: AppRuntimeRestartResult) => void;
}) {
  const [state, setState] = useState<SettingsViewState>({ status: "loading" });
  const [activeNode, setActiveNode] = useState<SettingsNode>("provider");
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
            ? { ...current, draft: applyJsoncValidation(current.draft, text, validation) }
            : current,
        );
      });
    },
    [updateDraft],
  );

  const saveCurrentText = useCallback(async () => {
    if (state.status !== "ready" || !canSaveSettings(state.draft)) {
      return;
    }
    setState({ ...state, draft: setSaving(state.draft, true), notice: null });
    let document: AppProfileConfigDocument;
    try {
      document = await saveProfileConfig(state.draft.text);
      setState((current) =>
        current.status === "ready"
          ? {
              ...current,
              draft: applySavedDocument(current.draft, document),
              notice: launchOptions.runtimeControlEndpoint
                ? i18n.t("settings.saved")
                : i18n.t("settings.savedExternal"),
            }
          : current,
      );
    } catch (error) {
      setState((current) =>
        current.status === "ready"
          ? { ...current, draft: setSaving(current.draft, false), notice: String(error) }
          : current,
      );
      return;
    }

    if (!launchOptions.runtimeControlEndpoint) {
      return;
    }

    try {
      const restart = await restartInteraction();
      onRuntimeRestart(restart);
      setState((current) =>
        current.status === "ready"
          ? { ...current, notice: i18n.t("settings.savedAndApplied") }
          : current,
      );
    } catch (error) {
      setState((current) =>
        current.status === "ready"
          ? {
              ...current,
              notice: i18n.t("settings.savedApplyFailed", {
                error: String(error),
              }),
            }
          : current,
      );
    }
  }, [i18n, launchOptions.runtimeControlEndpoint, onRuntimeRestart, state]);

  const completeJsonc = useCallback((text: string, byteOffset: number) => {
    return completeProfileConfig(text, byteOffset);
  }, []);

  if (state.status === "loading") {
    return <SettingsStatus title={i18n.t("settings.loadingTitle")} detail={i18n.t("settings.loadingDetail")} />;
  }
  if (state.status === "failed") {
    return <SettingsStatus title={i18n.t("settings.failedTitle")} detail={state.error} />;
  }

  const draft = state.draft;
  const config = draft.config;
  const profile = selectedProfile(draft);
  const panelNode = config ? activeNode : "jsonc";
  const environmentSummary = profile ? environmentSummaryItems(profile, i18n) : [];
  const showSaveActions = draft.dirty || draft.saving;

  return (
    <section className="settings-workbench" data-render-surface="environment">
      <aside className="settings-workbench-sidebar">
        <header className="settings-workbench-heading">
          <button
            aria-label={i18n.t("settings.backToChat")}
            className="settings-back-button"
            onClick={onBack}
            type="button"
          >
            <ChevronLeft size={16} />
          </button>
          <div>
            <h1 data-render-heading>{i18n.t("settings.environmentTitle")}</h1>
            <p>{profile?.displayName ?? i18n.t("settings.currentProfile")}</p>
          </div>
        </header>
        <nav aria-label={i18n.t("settings.environmentTitle")} className="settings-pane-nav">
          {settingsNodes(i18n, config != null).map((node) => (
            <button
              aria-label={node.label}
              className={panelNode === node.id ? "settings-pane-button active" : "settings-pane-button"}
              disabled={!config && node.id !== "jsonc"}
              key={node.id}
              onClick={() => {
                setActiveNode(node.id);
              }}
              type="button"
            >
              <node.icon size={18} />
              <span>
                <strong>{node.label}</strong>
                <small>{settingsNodeSubtitle(node.id, i18n, profile ?? undefined)}</small>
              </span>
            </button>
          ))}
        </nav>
      </aside>
      <section className="settings-workbench-detail">
        <div className="lens-header">
          <div>
            <h2>{settingsNodeLabel(panelNode, i18n)}</h2>
          </div>
          {showSaveActions ? (
            <div className="lens-actions">
              <button
                className="text-button subtle icon-text"
                disabled={!draft.dirty || draft.saving}
                onClick={() => updateDraft(discardSettingsChanges)}
                type="button"
              >
                <RotateCcw size={15} />
                <span>{i18n.t("settings.discard")}</span>
              </button>
              <button
                className="text-button primary icon-text"
                disabled={!canSaveSettings(draft)}
                onClick={() => void saveCurrentText()}
                type="button"
              >
                <Save size={15} />
                <span>{draft.saving ? i18n.t("settings.saving") : i18n.t("settings.save")}</span>
              </button>
            </div>
          ) : null}
        </div>
        {environmentSummary.length > 0 ? (
          <section className="environment-summary" aria-label={i18n.t("settings.environmentSummaryLabel")}>
            {environmentSummary.map((item) => (
              <div className="environment-summary-item" key={item.label}>
                <span>{item.label}</span>
                <strong>{item.value}</strong>
              </div>
            ))}
          </section>
        ) : null}
        <div className="settings-feedback" aria-live="polite">
          {state.notice ? <p className="settings-notice">{state.notice}</p> : null}
          {draft.error ? <p className="settings-error">{draft.error}</p> : null}
        </div>
        {!config ? (
          <JsoncPane
            completeJsonc={completeJsonc}
            draft={draft}
            i18n={i18n}
            onChange={handleJsoncChange}
          />
        ) : (
          <fieldset className="settings-pane-fieldset" disabled={draft.saving}>
            <SettingsNodePanel
              activeNode={panelNode}
              config={config}
              draft={draft}
              i18n={i18n}
              onJsoncChange={handleJsoncChange}
              completeJsonc={completeJsonc}
              updateDraft={updateDraft}
            />
          </fieldset>
        )}
      </section>
    </section>
  );
}

type SettingsViewState =
  | { status: "loading" }
  | { status: "failed"; error: string }
  | { status: "ready"; draft: SettingsDraftState; notice: string | null };

function SettingsNodePanel({
  activeNode,
  config,
  draft,
  i18n,
  onJsoncChange,
  completeJsonc,
  updateDraft,
}: {
  activeNode: SettingsNode;
  config: HostProfileConfig;
  draft: SettingsDraftState;
  i18n: AppI18n;
  onJsoncChange: (text: string) => void;
  completeJsonc: (text: string, offset: number) => Promise<AppProfileConfigCompletionSet>;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  if (activeNode === "jsonc") {
    return <JsoncPane completeJsonc={completeJsonc} draft={draft} i18n={i18n} onChange={onJsoncChange} />;
  }
  if (!profile) {
    return <p className="muted">{i18n.t("settings.noProfile")}</p>;
  }
  switch (activeNode) {
    case "profile":
      return (
        <ProfilePane
          config={config}
          draft={draft}
          i18n={i18n}
          updateDraft={updateDraft}
        />
      );
    case "provider":
      return <ProviderPane draft={draft} i18n={i18n} updateDraft={updateDraft} />;
    case "storage":
      return <StoragePane draft={draft} i18n={i18n} updateDraft={updateDraft} />;
    case "plugins":
      return <PluginsPane draft={draft} i18n={i18n} updateDraft={updateDraft} />;
  }
}

function ProfilePane({
  config,
  draft,
  i18n,
  updateDraft,
}: {
  config: HostProfileConfig;
  draft: SettingsDraftState;
  i18n: AppI18n;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  if (!profile) {
    return null;
  }
  return (
    <div className="lens-form">
      <label>
        <span>{i18n.t("settings.activeProfile")}</span>
        <select
          onChange={(event) => updateDraft((current) => selectProfile(current, event.target.value))}
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
        <span>{i18n.t("settings.profileId")}</span>
        <input
          onChange={(event) => updateDraft((current) => renameSelectedProfile(current, event.target.value))}
          value={profile.profileId}
        />
      </label>
      <label>
        <span>{i18n.t("settings.name")}</span>
        <input
          onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { displayName: event.target.value }))}
          value={profile.displayName}
        />
      </label>
      <label>
        <span>{i18n.t("settings.description")}</span>
        <textarea
          onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { description: event.target.value }))}
          rows={3}
          value={profile.description ?? ""}
        />
      </label>
      <div className="inline-actions">
        <button className="text-button icon-text" onClick={() => updateDraft(addProfile)} type="button">
          <Plus size={15} />
          <span>{i18n.t("settings.addProfile")}</span>
        </button>
        <button className="text-button icon-text" onClick={() => updateDraft(copySelectedProfile)} type="button">
          <Copy size={15} />
          <span>{i18n.t("settings.copyProfile")}</span>
        </button>
        <button
          className="text-button danger icon-text"
          disabled={config.profiles.length <= 1}
          onClick={() => updateDraft(deleteSelectedProfile)}
          type="button"
        >
          <Trash2 size={15} />
          <span>{i18n.t("settings.deleteProfile")}</span>
        </button>
      </div>
      <label className="check-row">
        <input
          checked={config.defaultProfileId === profile.profileId}
          disabled={config.defaultProfileId === profile.profileId}
          onChange={() => updateDraft((current) => updateSelectedProfile(current, { makeDefault: true }))}
          type="checkbox"
        />
        <span>{i18n.t("settings.useDefaultProfile")}</span>
      </label>
    </div>
  );
}

function ProviderPane({
  draft,
  i18n,
  updateDraft,
}: {
  draft: SettingsDraftState;
  i18n: AppI18n;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  if (!profile) {
    return null;
  }
  const provider = profile.provider;
  const reasoning = "reasoning" in provider ? provider.reasoning : null;
  const reasoningEnabled = reasoning && "enabled" in reasoning ? reasoning.enabled ?? true : true;
  const hasReasoningEnabled = reasoning != null && "enabled" in reasoning;
  const reasoningSummary = reasoning && "summary" in reasoning ? reasoning.summary ?? "" : "";
  const hasReasoningSummary = reasoning != null && "summary" in reasoning;
  return (
    <div className="lens-form">
      <label>
        <span>{i18n.t("settings.provider")}</span>
        <select
          onChange={(event) =>
            updateDraft((current) =>
              updateSelectedProfile(current, { providerType: event.target.value as typeof provider.type }),
            )
          }
          value={provider.type}
        >
          <option value="chatgpt_responses">ChatGPT Responses</option>
          <option value="responses">Responses</option>
          <option value="chat_completions">Chat Completions</option>
          <option value="anthropic_messages">Anthropic Messages</option>
        </select>
      </label>
      <label>
        <span>{i18n.t("settings.model")}</span>
        <input
          onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { model: event.target.value }))}
          value={provider.model}
        />
      </label>
      <label>
        <span>{i18n.t("settings.providerId")}</span>
        <input
          onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { providerId: event.target.value }))}
          value={provider.providerId ?? ""}
        />
      </label>
      {"baseUrl" in provider ? (
        <label>
          <span>{i18n.t("settings.baseUrl")}</span>
          <input
            onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { baseUrl: event.target.value }))}
            value={provider.baseUrl ?? ""}
          />
        </label>
      ) : null}
      {"apiKeyEnv" in provider ? (
        <label>
          <span>{i18n.t("settings.apiKeyEnv")}</span>
          <input
            onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { apiKeyEnv: event.target.value }))}
            value={provider.apiKeyEnv ?? ""}
          />
        </label>
      ) : null}
      {"stateMode" in provider ? (
        <label>
          <span>{i18n.t("settings.stateMode")}</span>
          <select
            onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { stateMode: event.target.value as "stateful" | "stateless" }))}
            value={provider.stateMode ?? "stateful"}
          >
            <option value="stateful">stateful</option>
            <option value="stateless">stateless</option>
          </select>
        </label>
      ) : null}
      {reasoning !== undefined ? (
        <div className="subsection">
          <h3>{i18n.t("settings.providerReasoningTitle")}</h3>
          {hasReasoningEnabled ? (
            <label className="check-row">
              <input
                checked={reasoningEnabled}
                onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { reasoningEnabled: event.target.checked }))}
                type="checkbox"
              />
              <span>{i18n.t("settings.reasoningEnabled")}</span>
            </label>
          ) : null}
          <label>
            <span>{i18n.t("settings.reasoningEffort")}</span>
            <select
              onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { reasoningEffort: event.target.value }))}
              value={String(reasoning?.effort ?? "")}
            >
              <option value="">default</option>
              {reasoningEffortOptions(provider.type).map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </label>
          {hasReasoningSummary ? (
            <label>
              <span>{i18n.t("settings.reasoningSummary")}</span>
              <select
                onChange={(event) => updateDraft((current) => updateSelectedProfile(current, { reasoningSummary: event.target.value }))}
                value={String(reasoningSummary)}
              >
                <option value="">default</option>
                <option value="auto">auto</option>
                <option value="concise">concise</option>
                <option value="detailed">detailed</option>
                <option value="none">none</option>
              </select>
            </label>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function reasoningEffortOptions(providerType: HostProfileConfig["profiles"][number]["provider"]["type"]): string[] {
  if (providerType === "anthropic_messages") {
    return ["low", "medium", "high", "xhigh", "max"];
  }
  if (providerType === "chat_completions") {
    return ["low", "medium", "high", "xhigh"];
  }
  return ["minimal", "low", "medium", "high", "xhigh"];
}

function StoragePane({
  draft,
  i18n,
  updateDraft,
}: {
  draft: SettingsDraftState;
  i18n: AppI18n;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  if (!profile) {
    return null;
  }
  const currentCompaction = (profile.compaction ?? { type: "auto" }) satisfies ProfileCompactionConfig;
  return (
    <div className="lens-form">
      <JsonObjectEditor
        label={i18n.t("settings.eventStore")}
        value={profile.eventStore ?? null}
        fallback={{ type: "memory" } satisfies ProfileEventStoreConfig}
        onChange={(value) =>
          updateDraft((current) =>
            updateSelectedProfileStorage(current, value, currentCompaction),
          )
        }
      />
      <JsonObjectEditor
        label={i18n.t("settings.compaction")}
        value={currentCompaction}
        fallback={{ type: "auto" } satisfies ProfileCompactionConfig}
        onChange={(value) =>
          updateDraft((current) =>
            updateSelectedProfileStorage(
              current,
              profile.eventStore ?? null,
              value ?? currentCompaction,
            ),
          )
        }
      />
      <JsonObjectEditor
        label={i18n.t("settings.registryStore")}
        value={draft.config?.registryStore ?? null}
        fallback={{ type: "memory" } satisfies RegistryStoreConfig}
        onChange={(value) => updateDraft((current) => updateRegistryStore(current, value))}
      />
    </div>
  );
}

function PluginsPane({
  draft,
  i18n,
  updateDraft,
}: {
  draft: SettingsDraftState;
  i18n: AppI18n;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  const plugins = profile?.plugins ?? [];
  return (
    <JsonListEditor
      addLabel={i18n.t("settings.addPlugin")}
      deleteLabel={i18n.t("settings.deletePlugin")}
      emptyLabel={i18n.t("settings.noPlugins")}
      fallback={defaultPlugin()}
      items={plugins}
      onDelete={(index) => updateDraft((current) => deleteSelectedPlugin(current, index))}
      onUpsert={(item, index) => updateDraft((current) => upsertSelectedPlugin(current, item, index))}
    />
  );
}

function JsoncPane({
  completeJsonc,
  draft,
  i18n,
  onChange,
}: {
  completeJsonc: (text: string, offset: number) => Promise<AppProfileConfigCompletionSet>;
  draft: SettingsDraftState;
  i18n: AppI18n;
  onChange: (text: string) => void;
}) {
  return (
    <div className="jsonc-pane">
      <p>{draft.validating ? i18n.t("settings.validating") : draft.dirty ? i18n.t("settings.unsaved") : i18n.t("settings.savedState")}</p>
      <JsoncEditor complete={completeJsonc} onChange={onChange} readOnly={draft.saving} value={draft.text} />
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

function settingsNodes(i18n: AppI18n, includeTypedPanes: boolean) {
  return [
    ...(includeTypedPanes
      ? [
          { id: "profile" as const, label: i18n.t("settings.profile"), icon: Settings2 },
          { id: "provider" as const, label: i18n.t("settings.provider"), icon: Server },
          { id: "storage" as const, label: i18n.t("settings.storage"), icon: Database },
          { id: "plugins" as const, label: i18n.t("settings.context"), icon: Plug },
        ]
      : []),
    { id: "jsonc" as const, label: i18n.t("settings.advancedJsonc"), icon: Braces },
  ];
}

function settingsNodeLabel(activeNode: SettingsNode, i18n: AppI18n) {
  const node = settingsNodes(i18n, true).find((item) => item.id === activeNode);
  if (node) {
    return node.label;
  }
  switch (activeNode) {
    case "profile":
      return i18n.t("settings.profile");
    case "jsonc":
      return i18n.t("settings.jsonc");
    case "provider":
      return i18n.t("settings.provider");
    case "storage":
      return i18n.t("settings.storage");
    case "plugins":
      return i18n.t("settings.plugins");
  }
}

function settingsNodeSubtitle(
  activeNode: SettingsNode,
  i18n: AppI18n,
  profile: HostProfileConfig["profiles"][number] | undefined,
): string {
  switch (activeNode) {
    case "profile":
      return profile?.profileId ?? i18n.t("settings.currentProfile");
    case "provider":
      return profile ? providerModel(profile.provider) : i18n.t("settings.fixJsonc");
    case "storage":
      return i18n.t("settings.eventStore");
    case "plugins":
      return i18n.t("settings.noPlugins");
    case "jsonc":
      return i18n.t("settings.savedState");
  }
}

function defaultPlugin(): AgentPluginDeclaration {
  return {
    pluginId: "new-plugin",
    displayName: "New Plugin",
    enabled: true,
    onLoadFailure: "disable_for_run",
    components: [{ roots: [] }],
  };
}
