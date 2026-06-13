// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
    await user.clear(model);
    await user.type(model, "gpt-5.5");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(model).toBeDisabled();

    expect(resolveSave).not.toBeNull();
    const finishSave = resolveSave as unknown as (value: AppProfileConfigDocument) => void;
    finishSave(profileDocument());
    await waitFor(() => expect(model).not.toBeDisabled());
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
