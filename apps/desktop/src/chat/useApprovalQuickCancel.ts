import { useEffect } from "react";
import type { AppToolPermissionOutcome } from "../generated/contracts";
import {
  pendingApprovalIdFromConversation,
  type ConversationState,
} from "../interaction/conversationState";

type ApprovalQuickCancelOptions = {
  conversation: ConversationState | null;
  disabled: boolean;
  resolvingApprovalIds: ReadonlySet<string>;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
};

export function useApprovalQuickCancel({
  conversation,
  disabled,
  resolvingApprovalIds,
  onResolveApproval,
}: ApprovalQuickCancelOptions): void {
  useEffect(() => {
    if (disabled || !conversation) {
      return;
    }

    const pendingApprovalId = pendingApprovalIdFromConversation(conversation);
    if (!pendingApprovalId) {
      return;
    }
    const approvalId = pendingApprovalId;

    function handleApprovalQuickCancel(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape" && !(event.metaKey && event.key === ".")) {
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      if (resolvingApprovalIds.has(approvalId)) {
        return;
      }
      void onResolveApproval(approvalId, "deny");
    }

    window.addEventListener("keydown", handleApprovalQuickCancel, { capture: true });
    return () => window.removeEventListener("keydown", handleApprovalQuickCancel, { capture: true });
  }, [conversation, disabled, onResolveApproval, resolvingApprovalIds]);
}
