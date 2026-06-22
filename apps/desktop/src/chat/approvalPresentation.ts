import type { AppToolPermissionRequirement } from "../generated/contracts";
import type {
  ApprovalTimelineItem,
  ToolTimelineItem,
} from "../interaction/conversationState";
import type { AppI18n } from "../i18n";

export type ApprovalDecisionViewModel = {
  title: string;
  summary: string;
  reason: string;
  command: string | null;
  cwd: string | null;
  targetPaths: string[];
  permissions: ApprovalPermissionViewModel[];
  cancelNote: string;
  confirmLabel: string;
  confirmTone: "normal" | "caution";
};

type ApprovalRisk = "normal" | "project-write";

export type ApprovalPermissionViewModel = {
  id: string;
  label: string;
  detail: string | null;
};

export type ToolActivityViewModel = {
  title: string;
  auditLabel: string | null;
  detail: string;
  auditDetail: string;
  statusLabel: string;
};

export function approvalDecisionViewModel(
  approval: ApprovalTimelineItem,
  i18n: AppI18n,
): ApprovalDecisionViewModel {
  const prompt = approval.prompt.trim();
  const risk = approvalRisk(approval.permissions);
  return {
    title: approval.command ? i18n.t("approval.commandTitle") : i18n.t("approval.actionTitle"),
    summary: approvalSummary(approval.command != null, risk, i18n),
    reason: approval.reason || (approval.command ? "" : prompt),
    command: approval.command,
    cwd: approval.cwd,
    targetPaths: uniqueNonEmptyStrings(approval.targetPaths),
    permissions: permissionViewModels(approval.permissions, i18n),
    cancelNote: i18n.t("approval.cancelNote"),
    confirmLabel: approval.command ? i18n.t("approval.runCommand") : i18n.t("approval.continue"),
    confirmTone: risk === "project-write" ? "caution" : "normal",
  };
}

function approvalRisk(permissions: AppToolPermissionRequirement[]): ApprovalRisk {
  return permissions.some((permission) => isProjectWritePermission(permission.capability))
    ? "project-write"
    : "normal";
}

function approvalSummary(hasCommand: boolean, risk: ApprovalRisk, i18n: AppI18n): string {
  if (risk === "project-write") {
    return i18n.t("approval.projectWriteSummary");
  }
  return hasCommand ? i18n.t("approval.commandSummary") : i18n.t("approval.actionSummary");
}

export function toolActivityViewModel(
  tool: ToolTimelineItem,
  i18n: AppI18n,
): ToolActivityViewModel {
  const statusLabel = tool.isError
    ? i18n.t("tool.failed")
    : tool.status === "running"
      ? i18n.t("tool.running")
      : i18n.t("tool.done");

  return {
    title: toolTitle(tool.toolName, i18n),
    auditLabel: tool.toolName || null,
    detail: tool.outputText || tool.updates.at(-1) || "",
    auditDetail: tool.updates.length > 0 ? tool.updates.join("\n") : "",
    statusLabel,
  };
}

function permissionViewModels(
  permissions: AppToolPermissionRequirement[],
  i18n: AppI18n,
): ApprovalPermissionViewModel[] {
  const views: ApprovalPermissionViewModel[] = [];
  const seenIds = new Set<string>();
  for (const permission of permissions) {
    const view = permissionViewModel(permission, i18n);
    if (!seenIds.has(view.id)) {
      seenIds.add(view.id);
      views.push(view);
    }
  }
  return views;
}

function permissionViewModel(
  permission: AppToolPermissionRequirement,
  i18n: AppI18n,
): ApprovalPermissionViewModel {
  const id = permission.capability.trim() || permission.description?.trim() || "permission";
  const description = permission.description?.trim() ?? "";
  if (isCommandPermission(permission.capability)) {
    return {
      id,
      label: i18n.t("approval.permission.hostExec"),
      detail: nonRedundantDetail(description, ["Run host commands.", "Run shell commands."]),
    };
  }
  if (isWorkingDirectoryPermission(permission.capability)) {
    return {
      id,
      label: i18n.t("approval.permission.hostCwd"),
      detail: nonRedundantDetail(description, ["Use the selected working directory."]),
    };
  }
  if (isProjectWritePermission(permission.capability)) {
    return {
      id,
      label: i18n.t("approval.permission.write"),
      detail: nonRedundantDetail(description, [
        "Modify local project files.",
        "Modify local project files",
      ]),
    };
  }

  if (description) {
    return { id, label: description, detail: null };
  }

  return {
    id,
    label: permission.capability.trim()
      ? i18n.t("approval.permission.genericWithCapability", { capability: permission.capability })
      : i18n.t("approval.permission.generic"),
    detail: null,
  };
}

function toolTitle(toolName: string, i18n: AppI18n): string {
  const normalized = toolName.toLowerCase();
  if (normalized.startsWith("desktop.preview.")) {
    return i18n.t("tool.previewTitle");
  }
  if (normalized.startsWith("host.exec") || normalized.startsWith("host.command")) {
    return i18n.t("tool.commandTitle");
  }
  return i18n.t("tool.genericTitle");
}

function isCommandPermission(capability: string): boolean {
  const normalized = capability.toLowerCase();
  return normalized === "host.exec" || normalized === "host.command";
}

function isWorkingDirectoryPermission(capability: string): boolean {
  return capability.toLowerCase() === "host.cwd";
}

function isProjectWritePermission(capability: string): boolean {
  const normalized = capability.toLowerCase();
  return (
    normalized === "write" ||
    normalized === "host.write" ||
    normalized === "file.write" ||
    normalized === "host.file.write"
  );
}

function nonRedundantDetail(description: string, redundantDescriptions: string[]): string | null {
  if (!description) {
    return null;
  }
  const normalized = normalizeDescription(description);
  return redundantDescriptions.some((redundant) => normalizeDescription(redundant) === normalized)
    ? null
    : description;
}

function normalizeDescription(description: string): string {
  return description.trim().replace(/[.。]+$/, "").toLowerCase();
}

function uniqueNonEmptyStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const strings: string[] = [];
  for (const value of values) {
    const trimmed = value.trim();
    if (trimmed.length > 0 && !seen.has(trimmed)) {
      seen.add(trimmed);
      strings.push(trimmed);
    }
  }
  return strings;
}
