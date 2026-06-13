import { useEffect, useRef } from "react";
import type { AppToolPermissionOutcome } from "../generated/contracts";
import type { ConversationState } from "../interaction/conversationState";

type ApprovalQuickCancelOptions = {
  conversation: ConversationState | null;
  disabled: boolean;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
};

export function useApprovalQuickCancel({
  conversation,
  disabled,
  onResolveApproval,
}: ApprovalQuickCancelOptions): void {
  const resolvingApprovalRef = useRef<string | null>(null);

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
      if (resolvingApprovalRef.current === approvalId) {
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      resolvingApprovalRef.current = approvalId;
      void onResolveApproval(approvalId, "deny").finally(() => {
        if (resolvingApprovalRef.current === approvalId) {
          resolvingApprovalRef.current = null;
        }
      });
    }

    window.addEventListener("keydown", handleApprovalQuickCancel, { capture: true });
    return () => window.removeEventListener("keydown", handleApprovalQuickCancel, { capture: true });
  }, [conversation, disabled, onResolveApproval]);
}

function pendingApprovalIdFromConversation(conversation: ConversationState): string | null {
  for (let index = conversation.timeline.length - 1; index >= 0; index -= 1) {
    const item = conversation.timeline[index];
    if (item.kind === "approval" && item.status === "pending") {
      return item.approvalId;
    }
  }
  return null;
}
