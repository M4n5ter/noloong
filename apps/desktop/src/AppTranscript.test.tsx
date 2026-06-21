// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { resetDevInteractionRuntimeForTests } from "./devInteractionRuntime";
import {
  completedSessionWithText,
  emptySession,
  FakeInteractionRuntime,
} from "./test/fakeInteractionRuntime";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const tauriEvent = vi.hoisted(() => ({
  emitTo: vi.fn(),
  listen: vi.fn(),
}));

const tauriCore = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCore.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  emitTo: tauriEvent.emitTo,
  listen: tauriEvent.listen,
}));

describe("transcript accessibility", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
    tauriEvent.emitTo.mockReset();
    tauriEvent.listen.mockReset();
    tauriCore.invoke.mockReset();
    tauriCore.invoke.mockResolvedValue(undefined);
    tauriEvent.listen.mockResolvedValue(() => {});
    resetDevInteractionRuntimeForTests();
    let frameTime = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frameTime += 16;
      callback(frameTime);
      return 1;
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps completed assistant messages attributed without visible role labels", async () => {
    const runtime = new FakeInteractionRuntime(completedSessionWithText("final assistant response"));

    render(<App dependencies={dependenciesFor(runtime)} />);

    const message = await screen.findByRole("article", { name: "Assistant message" });
    expect(message).toHaveTextContent("final assistant response");
    expect(screen.queryByText("assistant")).not.toBeInTheDocument();
  });

  it("announces a local prompt as sending until the assistant starts responding", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await user.type(await screen.findByPlaceholderText("Write a message..."), "hello from user");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    expect(screen.getByRole("article", { name: "Sending your message" })).toHaveTextContent(
      "hello from user",
    );

    act(() => {
      runtime.emitAssistantDelta("assistant response");
    });

    const userMessage = await screen.findByRole("article", { name: "Your message" });
    expect(userMessage).toHaveTextContent("hello from user");
    expect(screen.queryByRole("article", { name: "Sending your message" })).not.toBeInTheDocument();
  });
});

function dependenciesFor(runtime: FakeInteractionRuntime, locale: "en" | "zh" = "en") {
  return {
    bootstrap: async () => runtime.bootstrap(locale),
    createInteractionClient: runtime.createClient,
    connectInteractionDisplayStream: runtime.connectDisplayStream,
  };
}
