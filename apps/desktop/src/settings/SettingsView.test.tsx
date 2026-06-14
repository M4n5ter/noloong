// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppProfileConfigDocument } from "../generated/contracts";
import { createI18n } from "../i18n";
import { SettingsView } from "./SettingsView";
import {
  loadProfileConfig,
  restartInteraction,
  saveProfileConfig,
  validateProfileConfig,
} from "./api";

vi.mock("./api", () => ({
  completeProfileConfig: vi.fn(async () => ({ completions: [] })),
  loadProfileConfig: vi.fn(),
  restartInteraction: vi.fn(),
  saveProfileConfig: vi.fn(),
  validateProfileConfig: vi.fn(),
}));

describe("SettingsView", () => {
  afterEach(() => {
    cleanup();
    document.title = "";
    window.localStorage.clear();
  });

  beforeEach(() => {
    installTestLocalStorage();
    window.localStorage.clear();
    vi.mocked(loadProfileConfig).mockResolvedValue(profileDocument());
    vi.mocked(validateProfileConfig).mockImplementation(async (text) => ({
      valid: true,
      config: JSON.parse(text),
      canonicalText: text,
    }));
    vi.mocked(saveProfileConfig).mockResolvedValue(profileDocument());
    vi.mocked(restartInteraction).mockResolvedValue({
      interactionEndpoint: { wsUrl: "ws://127.0.0.1:7777/jsonrpc/ws" },
      interactionStatus: { status: "unavailable" },
    });
  });

  it("disables typed setting controls while save is pending", async () => {
    const user = userEvent.setup();
    let resolveSave: ((value: AppProfileConfigDocument) => void) | null = null;
    vi.mocked(saveProfileConfig).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    const model = await screen.findByLabelText("Model");
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Discard" })).not.toBeInTheDocument();
    await user.clear(model);
    await user.type(model, "gpt-5.5");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Discard" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(screen.getByRole("button", { name: "Saving" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Discard" })).toBeDisabled();
    expect(model).toBeDisabled();

    expect(resolveSave).not.toBeNull();
    const finishSave = resolveSave as unknown as (value: AppProfileConfigDocument) => void;
    finishSave(profileDocument());
    await waitFor(() => expect(model).not.toBeDisabled());
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Discard" })).not.toBeInTheDocument();
  });

  it("keeps pane content focused on the editable settings", async () => {
    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    expect(await screen.findByRole("heading", { name: "Provider" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Current environment")).not.toBeInTheDocument();

    const modelField = screen
      .getAllByLabelText("Model")
      .find((element): element is HTMLInputElement => element instanceof HTMLInputElement);
    if (!modelField) {
      throw new Error("Expected a model input field");
    }
    expect(modelField).toHaveValue("gpt-5.4");
  });

  it("uses the active settings pane as the window title", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    expect(await screen.findByRole("heading", { name: "Provider" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Provider"));

    await user.click(screen.getByRole("button", { name: "Storage" }));

    expect(await screen.findByRole("heading", { name: "Storage" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Storage"));
  });

  it("shows save actions as soon as focused JSON field text becomes valid", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Storage" }));

    const eventStore = await screen.findByRole("textbox", { name: "Event store" });
    await user.clear(eventStore);
    await user.click(eventStore);
    await user.paste('{ "type": "memory", "scope": "session" }');

    expect(eventStore).toHaveFocus();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Discard" })).toBeEnabled();
  });

  it("blocks saving the last valid JSON draft while the focused field is invalid", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Storage" }));

    const eventStore = await screen.findByRole("textbox", { name: "Event store" });
    await user.clear(eventStore);
    await user.click(eventStore);
    await user.paste('{ "type": "memory", "scope": "session" }');
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    await user.clear(eventStore);
    await user.paste("{");

    expect(eventStore).toHaveFocus();
    expect(screen.getByText("Fix the invalid JSON field before saving.")).toBeVisible();
    expect(screen.getByText(/SyntaxError/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    vi.mocked(saveProfileConfig).mockClear();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(saveProfileConfig).not.toHaveBeenCalled();
  });

  it("clears a plugin JSON parse error when the invalid item is deleted", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Context" }));
    await user.click(await screen.findByRole("button", { name: "Add plugin" }));

    const pluginEditor = await screen.findByRole("textbox", { name: "#1" });
    await user.clear(pluginEditor);
    await user.click(pluginEditor);
    await user.paste("{");

    expect(screen.getByText("Fix the invalid JSON field before saving.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Delete plugin" }));

    expect(screen.queryByText("Fix the invalid JSON field before saving.")).not.toBeInTheDocument();
    expect(screen.queryByText(/SyntaxError/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("keeps a plugin JSON parse error attached to its item when an earlier item is deleted", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Context" }));
    await user.click(await screen.findByRole("button", { name: "Add plugin" }));
    await user.click(screen.getByRole("button", { name: "Add plugin" }));

    const secondPluginEditor = await screen.findByRole("textbox", { name: "#2" });
    await user.clear(secondPluginEditor);
    await user.click(secondPluginEditor);
    await user.paste("{");

    expect(screen.getByText("Fix the invalid JSON field before saving.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    const deleteButtons = screen.getAllByRole("button", { name: "Delete plugin" });
    await user.click(deleteButtons[0]);

    await waitFor(() => expect(screen.getAllByRole("textbox")).toHaveLength(1));
    const remainingPluginEditor = screen.getAllByRole("textbox")[0];
    expect(remainingPluginEditor).toHaveValue("{");
    expect(screen.getByText("Fix the invalid JSON field before saving.")).toBeVisible();
    expect(screen.getByText(/SyntaxError/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("does not move a deleted plugin JSON parse error onto the next item", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Context" }));
    await user.click(await screen.findByRole("button", { name: "Add plugin" }));
    await user.click(screen.getByRole("button", { name: "Add plugin" }));

    const firstPluginEditor = await screen.findByRole("textbox", { name: "#1" });
    await user.clear(firstPluginEditor);
    await user.click(firstPluginEditor);
    await user.paste("{");

    expect(screen.getByText("Fix the invalid JSON field before saving.")).toBeVisible();
    await user.click(screen.getAllByRole("button", { name: "Delete plugin" })[0]);

    expect(screen.queryByText("Fix the invalid JSON field before saving.")).not.toBeInTheDocument();
    expect(screen.queryByText(/SyntaxError/)).not.toBeInTheDocument();
    const remainingPluginEditor = (await screen.findByRole("textbox", {
      name: "#1",
    })) as HTMLTextAreaElement;
    expect(() => JSON.parse(String(remainingPluginEditor.value))).not.toThrow();
    expect(remainingPluginEditor).not.toHaveValue("{");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("clears field-level JSON errors when discarding settings changes", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    await user.click(screen.getByRole("button", { name: "Storage" }));

    const eventStore = await screen.findByRole("textbox", { name: "Event store" });
    await user.clear(eventStore);
    await user.click(eventStore);
    await user.paste('{ "type": "memory", "scope": "session" }');
    await user.clear(eventStore);
    await user.paste("{");

    expect(screen.getByText(/SyntaxError/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Discard" }));

    expect(screen.queryByText("Fix the invalid JSON field before saving.")).not.toBeInTheDocument();
    expect(screen.queryByText(/SyntaxError/)).not.toBeInTheDocument();
    expect(eventStore).toHaveValue("{\n  \"type\": \"memory\"\n}");
  });

  it("restores the last viewed settings pane", async () => {
    const user = userEvent.setup();

    window.localStorage.setItem("noloong.settings.activeNode", "storage");
    const first = render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    expect(await screen.findByRole("heading", { name: "Storage" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Storage"));
    await user.click(screen.getByRole("button", { name: "Advanced JSONC" }));
    expect(window.localStorage.getItem("noloong.settings.activeNode")).toBe("jsonc");

    first.unmount();
    document.title = "";

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onRuntimeRestart={() => {}}
      />,
    );

    expect(await screen.findByRole("heading", { name: "Advanced JSONC" })).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Advanced JSONC"));
  });

  it("keeps the profile config path out of visible settings chrome", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{ runtimeControlEndpoint: { httpUrl: "http://127.0.0.1:7777" } }}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Provider" });
    expect(screen.queryByText("profile.jsonc")).not.toBeInTheDocument();

    const model = screen
      .getAllByLabelText("Model")
      .find((element): element is HTMLInputElement => element instanceof HTMLInputElement);
    if (!model) {
      throw new Error("Expected a model input field");
    }
    await user.clear(model);
    await user.type(model, "gpt-5.5");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Saved and applied")).toBeInTheDocument();
    expect(screen.queryByText(/profile\.jsonc/)).not.toBeInTheDocument();
  });
});

function installTestLocalStorage(): void {
  const entries = new Map<string, string>();
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => entries.get(key) ?? null,
      setItem: (key: string, value: string) => {
        entries.set(key, value);
      },
      removeItem: (key: string) => {
        entries.delete(key);
      },
      clear: () => {
        entries.clear();
      },
      key: (index: number) => Array.from(entries.keys())[index] ?? null,
      get length() {
        return entries.size;
      },
    } satisfies Storage,
  });
}

function profileDocument(): AppProfileConfigDocument {
  const config = {
    defaultProfileId: "default",
    profiles: [
      {
        profileId: "default",
        displayName: "Default",
        provider: {
          type: "chatgpt_responses" as const,
          model: "gpt-5.4",
          reasoning: {
            enabled: true,
            effort: "medium" as const,
            summary: "auto" as const,
          },
        },
      },
    ],
  };
  return {
    path: "/tmp/profile.jsonc",
    text: JSON.stringify(config, null, 2),
    exists: true,
    validation: {
      valid: true as const,
      config,
    },
  };
}
