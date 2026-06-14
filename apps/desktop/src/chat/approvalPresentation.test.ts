import { describe, expect, it } from "vitest";
import { createI18n } from "../i18n";
import type {
  ApprovalTimelineItem,
  ToolTimelineItem,
} from "../interaction/conversationState";
import { approvalDecisionViewModel, toolActivityViewModel } from "./approvalPresentation";

describe("approval presentation", () => {
  const i18n = createI18n("en");

  it("maps known local permissions to user-facing labels without collapsing distinct capabilities", () => {
    const view = approvalDecisionViewModel(
      approval({
        permissions: [
          { capability: "host.exec", description: "Run host commands." },
          { capability: "host.command", description: "Run shell commands." },
          { capability: "host.cwd", description: "Use the selected working directory." },
          { capability: "write", description: "Modify local project files." },
        ],
      }),
      i18n,
    );

    expect(view.permissions).toEqual([
      { id: "host.exec", label: "Can run a local command.", detail: null },
      { id: "host.command", label: "Can run a local command.", detail: null },
      { id: "host.cwd", label: "Uses the selected working folder.", detail: null },
      { id: "write", label: "Can change files in the project.", detail: null },
    ]);
  });

  it("keeps non-redundant details for known permissions", () => {
    const view = approvalDecisionViewModel(
      approval({
        permissions: [
          {
            capability: "host.exec",
            description: "Runs inside the active project shell.",
          },
        ],
      }),
      i18n,
    );

    expect(view.permissions).toEqual([
      {
        id: "host.exec",
        label: "Can run a local command.",
        detail: "Runs inside the active project shell.",
      },
    ]);
  });

  it("preserves authoritative descriptions for unknown permissions", () => {
    const view = approvalDecisionViewModel(
      approval({
        permissions: [
          {
            capability: "mcp.tool.call",
            description: "Call MCP tool search from the design server.",
          },
        ],
      }),
      i18n,
    );

    expect(view.permissions).toEqual([
      {
        id: "mcp.tool.call",
        label: "Call MCP tool search from the design server.",
        detail: null,
      },
    ]);
  });

  it("keeps a capability fallback when no description exists", () => {
    const view = approvalDecisionViewModel(
      approval({
        permissions: [{ capability: "plugin.camera.read" }],
      }),
      i18n,
    );

    expect(view.permissions).toEqual([
      {
        id: "plugin.camera.read",
        label: "Requests local access: plugin.camera.read.",
        detail: null,
      },
    ]);
  });

  it("keeps tool audit identifiers available for explicit details", () => {
    const view = toolActivityViewModel(
      {
        kind: "tool",
        toolCallId: "call-1",
        toolName: "host.exec.start",
        status: "running",
        updates: ["running pwd"],
        outputText: "",
        isError: false,
      } satisfies ToolTimelineItem,
      i18n,
    );

    expect(view).toMatchObject({
      title: "Local command",
      auditLabel: "host.exec.start",
      detail: "running pwd",
      auditDetail: "running pwd",
      statusLabel: "Running",
    });
  });
});

function approval(
  overrides: Partial<ApprovalTimelineItem> = {},
): ApprovalTimelineItem {
  return {
    kind: "approval",
    approvalId: "approval-1",
    toolCallId: "call-1",
    toolName: "host.exec.start",
    prompt: "Run command?",
    reason: "Needs approval.",
    command: "pwd",
    cwd: "/Users/m4n5ter/rust/noloong",
    permissions: [],
    status: "pending",
    ...overrides,
  };
}
