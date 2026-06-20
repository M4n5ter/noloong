import { invoke } from "@tauri-apps/api/core";

export const CONVERSATION_COMMANDS = [
  "focus-composer",
  "send-message",
  "stop-response",
  "clear-composer",
] as const;

export type ConversationCommand = (typeof CONVERSATION_COMMANDS)[number];

export const CONVERSATION_COMMAND_EVENT = "noloong-conversation-command";

export type ConversationMenuState = {
  canFocusComposer: boolean;
  canSendMessage: boolean;
  canStopResponse: boolean;
  canClearComposer: boolean;
};

export const DISABLED_CONVERSATION_MENU_STATE: ConversationMenuState = {
  canFocusComposer: false,
  canSendMessage: false,
  canStopResponse: false,
  canClearComposer: false,
};

export function conversationMenuStateForChatSurface({
  composerState,
  documentTargetAvailable,
  ready,
  sessionsPanelVisible,
}: {
  composerState: ConversationMenuState;
  documentTargetAvailable: boolean;
  ready: boolean;
  sessionsPanelVisible: boolean;
}): ConversationMenuState {
  return ready && documentTargetAvailable && !sessionsPanelVisible
    ? composerState
    : DISABLED_CONVERSATION_MENU_STATE;
}

export function dispatchConversationCommand(command: ConversationCommand): void {
  window.dispatchEvent(
    new CustomEvent<ConversationCommand>(CONVERSATION_COMMAND_EVENT, { detail: command }),
  );
}

export function conversationCommandFromPayload(payload: unknown): ConversationCommand | null {
  return typeof payload === "string" && isConversationCommand(payload) ? payload : null;
}

export function syncConversationMenuState(state: ConversationMenuState): void {
  if (!("__TAURI_INTERNALS__" in window)) {
    return;
  }
  void invoke("app_update_conversation_menu_state", { state }).catch(() => undefined);
}

function isConversationCommand(value: string): value is ConversationCommand {
  return CONVERSATION_COMMANDS.includes(value as ConversationCommand);
}
