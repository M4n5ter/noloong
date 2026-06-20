import type {
  ProfileCompactionConfig,
  ProfileEventStoreConfig,
  RegistryStoreConfig,
} from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { JsonObjectEditor } from "./JsonEditors";
import {
  selectedProfile,
  type SettingsDraftState,
  updateRegistryStore,
  updateSelectedProfileCompaction,
  updateSelectedProfileEventStore,
} from "./store";

export function StoragePane({
  draft,
  i18n,
  onJsonFieldErrorChange,
  updateDraft,
}: {
  draft: SettingsDraftState;
  i18n: AppI18n;
  onJsonFieldErrorChange: (key: string, error: string | null) => void;
  updateDraft: (update: (draft: SettingsDraftState) => SettingsDraftState) => void;
}) {
  const profile = selectedProfile(draft);
  if (!profile) {
    return null;
  }
  const eventStore = profile.eventStore ?? null;
  const currentCompaction = (profile.compaction ?? { type: "auto" }) satisfies ProfileCompactionConfig;
  const registryStore = draft.config?.registryStore ?? null;
  const providerSupportsOpenAiCompaction = profile.provider.type === "chatgpt_responses";
  return (
    <div className="lens-form">
      <label>
        <span>{i18n.t("settings.eventStore")}</span>
        <select
          onChange={(event) =>
            updateDraft((current) =>
              updateSelectedProfileEventStore(
                current,
                eventStoreForSelectValue(event.target.value as EventStoreSelectValue, eventStore),
              ),
            )
          }
          value={eventStoreSelectValue(eventStore)}
        >
          <option value="default">{i18n.t("settings.storageDefault")}</option>
          {eventStoreOptions().map((option) => (
            <option key={option} value={option}>
              {eventStoreOptionLabel(option, i18n)}
            </option>
          ))}
        </select>
      </label>
      {eventStore?.type === "sqlite" ? (
        <JsonObjectEditor
          errorKey="storage.eventStore"
          label={i18n.t("settings.eventStoreJson")}
          value={eventStore}
          fallback={eventStoreForType("sqlite", eventStore)}
          onChange={(value) =>
            updateDraft((current) => updateSelectedProfileEventStore(current, value ?? null))
          }
          onParseErrorChange={onJsonFieldErrorChange}
        />
      ) : null}
      <label>
        <span>{i18n.t("settings.compaction")}</span>
        <select
          onChange={(event) =>
            updateDraft((current) =>
              updateSelectedProfileCompaction(
                current,
                compactionForType(event.target.value as ProfileCompactionConfig["type"], currentCompaction),
              ),
            )
          }
          value={currentCompaction.type}
        >
          <option value="auto">{i18n.t("settings.compactionAuto")}</option>
          <option value="none">{i18n.t("settings.compactionNone")}</option>
          {providerSupportsOpenAiCompaction || currentCompaction.type === "openai_responses" ? (
            <option value="openai_responses">{i18n.t("settings.compactionOpenaiResponses")}</option>
          ) : null}
        </select>
      </label>
      {currentCompaction.type === "openai_responses" ? (
        <JsonObjectEditor
          errorKey="storage.compaction"
          label={i18n.t("settings.compactionJson")}
          value={currentCompaction}
          fallback={compactionForType("openai_responses", currentCompaction)}
          onChange={(value) =>
            updateDraft((current) =>
              updateSelectedProfileCompaction(current, value ?? compactionForType("auto", currentCompaction)),
            )
          }
          onParseErrorChange={onJsonFieldErrorChange}
        />
      ) : null}
      <label>
        <span>{i18n.t("settings.registryStore")}</span>
        <select
          onChange={(event) =>
            updateDraft((current) =>
              updateRegistryStore(
                current,
                registryStoreForSelectValue(event.target.value as RegistryStoreSelectValue, registryStore),
              ),
            )
          }
          value={registryStoreSelectValue(registryStore)}
        >
          <option value="default">{i18n.t("settings.storageDefault")}</option>
          {registryStoreOptions(registryStore).map((option) => (
            <option key={option} value={option}>
              {registryStoreOptionLabel(option, i18n)}
            </option>
          ))}
        </select>
      </label>
      {registryStore == null || registryStore.type === "memory" ? null : (
        <JsonObjectEditor
          errorKey="storage.registryStore"
          label={i18n.t("settings.registryStoreJson")}
          value={registryStore}
          fallback={registryStoreForType(registryStore.type, registryStore)}
          onChange={(value) => updateDraft((current) => updateRegistryStore(current, value ?? null))}
          onParseErrorChange={onJsonFieldErrorChange}
        />
      )}
    </div>
  );
}

type EventStoreSelectValue = "default" | ProfileEventStoreConfig["type"];
type RegistryStoreSelectValue = "default" | RegistryStoreConfig["type"];

function eventStoreSelectValue(value: ProfileEventStoreConfig | null): EventStoreSelectValue {
  return value?.type ?? "default";
}

function registryStoreSelectValue(value: RegistryStoreConfig | null): RegistryStoreSelectValue {
  return value?.type ?? "default";
}

function eventStoreOptions(): ProfileEventStoreConfig["type"][] {
  return ["memory", "sqlite"];
}

function registryStoreOptions(current: RegistryStoreConfig | null): RegistryStoreConfig["type"][] {
  const base: RegistryStoreConfig["type"][] = ["memory", "sqlite", "object_memory"];
  if (current?.type === "postgres" || current?.type === "object_fs") {
    return [...base, current.type];
  }
  return base;
}

function eventStoreOptionLabel(type: ProfileEventStoreConfig["type"], i18n: AppI18n): string {
  switch (type) {
    case "memory":
      return i18n.t("settings.storageMemory");
    case "sqlite":
      return i18n.t("settings.storageSqlite");
  }
}

function registryStoreOptionLabel(type: RegistryStoreConfig["type"], i18n: AppI18n): string {
  switch (type) {
    case "memory":
      return i18n.t("settings.storageMemory");
    case "sqlite":
      return i18n.t("settings.storageSqlite");
    case "postgres":
      return i18n.t("settings.storagePostgres");
    case "object_memory":
      return i18n.t("settings.storageObjectMemory");
    case "object_fs":
      return i18n.t("settings.storageObjectFs");
  }
}

function eventStoreForSelectValue(
  value: EventStoreSelectValue,
  current: ProfileEventStoreConfig | null,
): ProfileEventStoreConfig | null {
  return value === "default" ? null : eventStoreForType(value, current);
}

function eventStoreForType(
  type: ProfileEventStoreConfig["type"],
  current: ProfileEventStoreConfig | null,
): ProfileEventStoreConfig {
  if (current?.type === type) {
    return current;
  }
  switch (type) {
    case "memory":
      return { type };
    case "sqlite":
      return { type, databaseUrl: "sqlite:target/noloong-events.sqlite", migrateOnConnect: true };
  }
}

function compactionForType(
  type: ProfileCompactionConfig["type"],
  current: ProfileCompactionConfig,
): ProfileCompactionConfig {
  if (current.type === type) {
    return current;
  }
  switch (type) {
    case "auto":
    case "none":
      return { type };
    case "openai_responses":
      return { type };
  }
}

function registryStoreForType(
  type: RegistryStoreConfig["type"],
  current: RegistryStoreConfig | null,
): RegistryStoreConfig {
  if (current?.type === type) {
    return current;
  }
  switch (type) {
    case "memory":
      return { type };
    case "sqlite":
      return { type, databaseUrl: "sqlite:target/noloong-registry.sqlite" };
    case "postgres":
      return { type, databaseUrl: "postgres://localhost/noloong" };
    case "object_memory":
      return { type };
    case "object_fs":
      return { type, root: "target/noloong-objects" };
  }
}

function registryStoreForSelectValue(
  value: RegistryStoreSelectValue,
  current: RegistryStoreConfig | null,
): RegistryStoreConfig | null {
  return value === "default" ? null : registryStoreForType(value, current);
}
