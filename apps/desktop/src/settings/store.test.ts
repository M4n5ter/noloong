import { describe, expect, it } from "vitest";
import type { HostProfileConfig } from "../generated/contracts";
import {
  applyJsoncValidation,
  applySavedDocument,
  canSaveSettings,
  discardSettingsChanges,
  renameSelectedProfile,
  selectedProfile,
  settingsDraftFromDocument,
  updateSelectedProfile,
} from "./store";

describe("settings draft store", () => {
  it("creates a clean draft from a loaded profile config document", () => {
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(sampleConfig()),
      exists: true,
      validation: {
        valid: true,
        config: sampleConfig(),
        canonicalText: JSON.stringify(sampleConfig(), null, 2),
      },
    });

    expect(draft.dirty).toBe(false);
    expect(draft.selectedProfileId).toBe("chatgpt-responses");
    expect(selectedProfile(draft)?.displayName).toBe("ChatGPT Responses");
  });

  it("syncs profile form edits into canonical JSONC text", () => {
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(sampleConfig(), null, 2),
      exists: true,
      validation: {
        valid: true,
        config: sampleConfig(),
      },
    });

    const edited = updateSelectedProfile(draft, {
      displayName: "Edited",
      model: "gpt-5.5",
    });

    expect(edited.dirty).toBe(true);
    expect(selectedProfile(edited)?.displayName).toBe("Edited");
    expect(edited.text).toContain('"displayName": "Edited"');
    expect(edited.text).toContain('"model": "gpt-5.5"');
  });

  it("syncs valid JSONC edits into the typed draft", () => {
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(sampleConfig(), null, 2),
      exists: true,
      validation: {
        valid: true,
        config: sampleConfig(),
      },
    });
    const config = sampleConfig();
    config.profiles[0].displayName = "From JSONC";
    const text = JSON.stringify(config, null, 2);

    const edited = applyJsoncValidation(draft, text, {
      valid: true,
      config,
      canonicalText: text,
    });

    expect(edited.error).toBeNull();
    expect(selectedProfile(edited)?.displayName).toBe("From JSONC");
  });

  it("keeps invalid JSONC out of the typed draft and blocks save", () => {
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(sampleConfig(), null, 2),
      exists: true,
      validation: {
        valid: true,
        config: sampleConfig(),
      },
    });

    const invalid = applyJsoncValidation(draft, "{", {
      valid: false,
      error: "expected object",
    });

    expect(invalid.config).toBeNull();
    expect(invalid.error).toBe("expected object");
    expect(canSaveSettings(invalid)).toBe(false);
  });

  it("marks a saved document as clean", () => {
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(sampleConfig(), null, 2),
      exists: false,
      validation: {
        valid: true,
        config: sampleConfig(),
      },
    });
    const edited = updateSelectedProfile(draft, { displayName: "Saved" });

    const saved = applySavedDocument(edited, {
      path: "/tmp/profile.jsonc",
      text: edited.text,
      exists: true,
      validation: {
        valid: true,
        config: edited.config!,
      },
    });

    expect(saved.exists).toBe(true);
    expect(saved.dirty).toBe(false);
  });

  it("renames the selected profile and keeps default profile references coherent", () => {
    const config = sampleConfig();
    config.profiles.push({
      ...structuredClone(config.profiles[0]),
      profileId: "secondary",
      displayName: "Secondary",
    });
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: JSON.stringify(config, null, 2),
      exists: true,
      validation: {
        valid: true,
        config,
      },
    });

    const renamed = renameSelectedProfile(draft, "daily-driver");

    expect(renamed.selectedProfileId).toBe("daily-driver");
    expect(renamed.config?.defaultProfileId).toBe("daily-driver");
    expect(selectedProfile(renamed)?.profileId).toBe("daily-driver");

    const duplicate = renameSelectedProfile(renamed, "secondary");

    expect(duplicate.selectedProfileId).toBe("daily-driver");
    expect(duplicate.config?.profiles.map((profile) => profile.profileId)).toEqual([
      "daily-driver",
      "secondary",
    ]);
  });

  it("discards to the last saved typed config even when the saved text is JSONC", () => {
    const config = sampleConfig();
    const savedText = [
      "{",
      "  // JSONC comments must remain discard-safe.",
      '  "defaultProfileId": "chatgpt-responses",',
      '  "profiles": [],',
      "}",
    ].join("\n");
    const draft = settingsDraftFromDocument({
      path: "/tmp/profile.jsonc",
      text: savedText,
      exists: true,
      validation: {
        valid: true,
        config,
      },
    });
    const edited = updateSelectedProfile(draft, { displayName: "Unsaved" });

    const discarded = discardSettingsChanges(edited);

    expect(discarded.text).toBe(savedText);
    expect(discarded.config).toBe(config);
    expect(selectedProfile(discarded)?.displayName).toBe("ChatGPT Responses");
    expect(discarded.dirty).toBe(false);
  });
});

function sampleConfig(): HostProfileConfig {
  return {
    defaultProfileId: "chatgpt-responses",
    profiles: [
      {
        profileId: "chatgpt-responses",
        displayName: "ChatGPT Responses",
        description: "ChatGPT subscription through the Responses backend.",
        provider: {
          type: "chatgpt_responses",
          model: "gpt-5.4-mini",
          allowFileDataUrlInput: true,
        },
        compaction: {
          type: "auto",
        },
        plugins: [],
        manifestPatches: [
          {
            op: "set_locale",
            locale: "zh",
          },
        ],
        metadata: {},
      },
    ],
  };
}
