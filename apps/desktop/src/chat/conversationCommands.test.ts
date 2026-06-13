import { describe, expect, it } from "vitest";
import {
  conversationCommandFromPayload,
  conversationMenuStateForChatSurface,
  DISABLED_CONVERSATION_MENU_STATE,
} from "./conversationCommands";

describe("conversation commands", () => {
  it("accepts only known native menu command payloads", () => {
    expect(conversationCommandFromPayload("focus-composer")).toBe("focus-composer");
    expect(conversationCommandFromPayload("send-message")).toBe("send-message");
    expect(conversationCommandFromPayload("stop-response")).toBe("stop-response");
    expect(conversationCommandFromPayload("delete-session")).toBeNull();
    expect(conversationCommandFromPayload(null)).toBeNull();
  });

  it("disables native commands when the chat surface cannot receive them", () => {
    const composerState = {
      canFocusComposer: true,
      canSendMessage: true,
      canStopResponse: false,
    };

    expect(
      conversationMenuStateForChatSurface({
        composerState,
        documentTargetAvailable: true,
        ready: true,
        sessionsPanelVisible: false,
      }),
    ).toEqual(composerState);
    expect(
      conversationMenuStateForChatSurface({
        composerState,
        documentTargetAvailable: false,
        ready: true,
        sessionsPanelVisible: false,
      }),
    ).toEqual(DISABLED_CONVERSATION_MENU_STATE);
    expect(
      conversationMenuStateForChatSurface({
        composerState,
        documentTargetAvailable: true,
        ready: false,
        sessionsPanelVisible: false,
      }),
    ).toEqual(DISABLED_CONVERSATION_MENU_STATE);
    expect(
      conversationMenuStateForChatSurface({
        composerState,
        documentTargetAvailable: true,
        ready: true,
        sessionsPanelVisible: true,
      }),
    ).toEqual(DISABLED_CONVERSATION_MENU_STATE);
  });
});
