import type {
  AppInteractionSessionDescriptor,
  AppInteractionSessionStatus,
  AppLaunchOptions,
  InteractionInitializeResult,
} from "../generated/contracts";
import type { ConversationState } from "../interaction/conversationState";

export type BootstrapState =
  | { status: "loading" }
  | { status: "failed"; error: string }
  | { status: "ready"; options: AppLaunchOptions };

export type InteractionState =
  | { status: "disconnected"; launchStatus: AppLaunchOptions["interactionStatus"] }
  | { status: "loading" }
  | { status: "failed"; error: string }
  | InteractionReadyState;

export type InteractionReadyState = {
  status: "ready";
  initializeResult: InteractionInitializeResult;
  sessions: AppInteractionSessionDescriptor[];
  selectedSessionId: string | null;
  selectedSession: AppInteractionSessionDescriptor | null;
  conversation: ConversationState;
  refreshing: boolean;
  sending: boolean;
  streamStatus: "connecting" | "ready" | "failed";
  streamError: string | null;
};

export type SessionStatus = AppInteractionSessionStatus;
