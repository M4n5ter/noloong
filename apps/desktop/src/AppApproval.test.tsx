// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { resetDevInteractionRuntimeForTests } from "./devInteractionRuntime";
import { emptySession, FakeInteractionRuntime } from "./test/fakeInteractionRuntime";

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

describe("approval decisions", () => {
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

  it("keeps default browser preview approvals and sessions panel wired to the dev runtime", async () => {
    const user = userEvent.setup();

    render(<App />);

    await user.type(await screen.findByPlaceholderText("Write a message..."), "approval please");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    const approval = await screen.findByRole(
      "article",
      { name: "Run a local command?" },
      { timeout: 3000 },
    );
    expect(within(approval).getByRole("heading", { name: "Run a local command?" })).toBeVisible();
    expect(approval).toHaveAccessibleDescription(
      "Noloong wants to run this command in your project.",
    );
    expect(within(approval).getByRole("button", { name: "Run Local Command" })).toBeVisible();
    expect(within(approval).getByRole("button", { name: "Cancel" })).toBeVisible();
    expect(within(approval).queryByText("desktop.preview.change")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Sessions" }));
    const sessionsPanel = await screen.findByRole("dialog", { name: "Sessions" });
    expect(within(sessionsPanel).getByRole("button", { name: "Create session" })).toBeVisible();
    expect(within(sessionsPanel).getByText("Desktop Dev needs a decision")).toBeVisible();
  });

  it("keeps approval cancellation on the decision card while a run is paused", async () => {
    const user = userEvent.setup();

    render(<App />);

    await user.type(await screen.findByPlaceholderText("Write a message..."), "approval please");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    const approval = await screen.findByRole(
      "article",
      { name: "Run a local command?" },
      { timeout: 3000 },
    );
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    await user.click(within(approval).getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(within(approval).getByText("Canceled")).toBeVisible());
    expect(within(approval).queryByRole("button", { name: "Run Local Command" })).not.toBeInTheDocument();
    expect(within(approval).queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    expect(
      await screen.findByText("Approval denied in the dev preview. The flow stopped cleanly."),
    ).toBeVisible();
  });

  it("keeps stop available for a restored paused run without an approval card", async () => {
    const runtime = new FakeInteractionRuntime({
      ...emptySession(),
      status: "paused",
    });
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    const stop = await screen.findByRole("button", { name: "Stop" });
    expect(screen.queryByRole("article", { name: "Run a local command?" })).not.toBeInTheDocument();

    await user.click(stop);

    expect(runtime.abortRequests).toEqual([expect.objectContaining({ sessionId: "session-1" })]);
  });

  it("resolves command approvals from a localized decision card", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    emitApprovalRequest(runtime, "approval-1");

    const card = await screen.findByRole("article", { name: "Run a local command?" });
    expect(within(card).getByRole("heading", { name: "Run a local command?" })).toBeVisible();
    expect(within(card).getByText("pwd && ls -la")).toBeVisible();
    expect(within(card).getByText("Can run a local command.")).toBeVisible();
    expect(within(card).getByText("Runs inside the active project shell.")).toBeVisible();
    expect(within(card).getByText("Uses the selected working folder.")).toBeVisible();
    expect(within(card).getByText("Call MCP tool search from the design server.")).toBeVisible();
    expect(within(card).queryByText("host.exec.start")).not.toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Run Local Command" }));

    await waitFor(() =>
      expect(runtime.approvalResolveRequests).toEqual([
        expect.objectContaining({
          approvalId: "approval-1",
          decision: expect.objectContaining({ outcome: "allow" }),
          sessionId: "session-1",
        }),
      ]),
    );
  });

  it("localizes approval decision controls in Chinese", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime, "zh")} />);

    await composerReadyForInput("输入消息...");
    emitApprovalRequest(runtime, "approval-1");

    const card = await screen.findByRole("article", { name: "运行本地命令？" });
    expect(within(card).getByRole("heading", { name: "运行本地命令？" })).toBeVisible();
    expect(within(card).getByText("等待你决定")).toBeVisible();
    expect(within(card).getByRole("button", { name: "运行本地命令" })).toBeVisible();
    expect(within(card).getByRole("button", { name: "取消" })).toBeVisible();
  });

  it("cancels a pending approval with Escape without aborting the run", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    emitApprovalRequest(runtime, "approval-escape");

    await screen.findByRole("article", { name: "Run a local command?" });
    await user.keyboard("{Escape}");

    await waitFor(() =>
      expect(runtime.approvalResolveRequests).toEqual([
        expect.objectContaining({
          approvalId: "approval-escape",
          decision: expect.objectContaining({ outcome: "deny" }),
          sessionId: "session-1",
        }),
      ]),
    );
    expect(runtime.abortRequests).toHaveLength(0);
  });

  it("cancels a pending approval with Command-Period", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    emitApprovalRequest(runtime, "approval-command-period");

    await screen.findByRole("article", { name: "Run a local command?" });
    await user.keyboard("{Meta>}.{/Meta}");

    await waitFor(() =>
      expect(runtime.approvalResolveRequests).toEqual([
        expect.objectContaining({
          approvalId: "approval-command-period",
          decision: expect.objectContaining({ outcome: "deny" }),
          sessionId: "session-1",
        }),
      ]),
    );
    expect(runtime.abortRequests).toHaveLength(0);
  });

  it("lets the sessions dialog consume Escape before a background approval", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    emitApprovalRequest(runtime, "approval-with-dialog");
    await screen.findByRole("article", { name: "Run a local command?" });

    await user.click(screen.getByRole("button", { name: "Sessions" }));
    expect(screen.getByRole("dialog", { name: "Sessions" })).toBeVisible();

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog", { name: "Sessions" })).not.toBeInTheDocument();
    expect(runtime.approvalResolveRequests).toHaveLength(0);

    await user.keyboard("{Escape}");

    await waitFor(() =>
      expect(runtime.approvalResolveRequests).toEqual([
        expect.objectContaining({
          approvalId: "approval-with-dialog",
          decision: expect.objectContaining({ outcome: "deny" }),
          sessionId: "session-1",
        }),
      ]),
    );
  });
});

function dependenciesFor(runtime: FakeInteractionRuntime, locale: "en" | "zh" = "en") {
  return {
    bootstrap: async () => runtime.bootstrap(locale),
    createInteractionClient: runtime.createClient,
    connectInteractionDisplayStream: runtime.connectDisplayStream,
  };
}

async function composerReadyForInput(placeholder = "Write a message..."): Promise<void> {
  await screen.findByPlaceholderText(placeholder);
}

function emitApprovalRequest(runtime: FakeInteractionRuntime, approvalId: string): void {
  act(() => {
    runtime.emitDisplayEvent({
      type: "approval_requested",
      approval: {
        approvalId,
        toolCall: { id: `call-${approvalId}`, name: "host.exec.start" },
        request: {
          prompt: "Run command?",
          reason: "Needs human approval.",
          metadata: {
            command: "pwd && ls -la",
            cwd: "/Users/m4n5ter/rust/noloong",
          },
        },
        permissions: [
          {
            capability: "host.exec",
            description: "Runs inside the active project shell.",
          },
          {
            capability: "host.cwd",
            description: "Use the selected working directory.",
          },
          {
            capability: "mcp.tool.call",
            description: "Call MCP tool search from the design server.",
          },
        ],
      },
    });
  });
}
