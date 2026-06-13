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
  });

  beforeEach(() => {
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
        onBack={() => {}}
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

  it("presents the selected environment before the editable fields", async () => {
    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{}}
        onBack={() => {}}
        onRuntimeRestart={() => {}}
      />,
    );

    const summary = await screen.findByLabelText("Current environment");

    expect(summary).toHaveTextContent("Provider");
    expect(summary).toHaveTextContent("ChatGPT");
    expect(summary).toHaveTextContent("Model");
    expect(summary).toHaveTextContent("gpt-5.4");
    expect(summary).toHaveTextContent("Reasoning");
    expect(summary).toHaveTextContent("medium / auto");
    expect(summary).toHaveTextContent("Context");
    expect(summary).toHaveTextContent("No plugins");

    const modelField = screen
      .getAllByLabelText("Model")
      .find((element): element is HTMLInputElement => element instanceof HTMLInputElement);
    if (!modelField) {
      throw new Error("Expected a model input field");
    }
    expect(
      summary.compareDocumentPosition(modelField) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps the profile config path out of visible settings chrome", async () => {
    const user = userEvent.setup();

    render(
      <SettingsView
        i18n={createI18n("en")}
        launchOptions={{ runtimeControlEndpoint: { httpUrl: "http://127.0.0.1:7777" } }}
        onBack={() => {}}
        onRuntimeRestart={() => {}}
      />,
    );

    await screen.findByLabelText("Current environment");
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
