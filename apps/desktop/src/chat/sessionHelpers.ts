import type {
  AppDisplayEvent,
  AppInteractionSessionDescriptor,
  AppInteractionSessionStatus,
  InteractionInitializeResult,
} from "../generated/contracts";
import type { InteractionClient } from "../interaction/client";
import { textFromMessage } from "../interaction/contentText";

export async function listSessionsForProfiles(
  client: InteractionClient,
  initializeResult: InteractionInitializeResult,
): Promise<AppInteractionSessionDescriptor[]> {
  const profileIds = (initializeResult.profiles ?? []).map((profile) => profile.profileId);
  if (profileIds.length === 0) {
    return [];
  }
  const sessions = (
    await Promise.all(profileIds.map((profileId) => client.listSessions({ profileId })))
  ).flat();
  const seen = new Set<string>();
  return sessions.filter((session) => {
    if (seen.has(session.sessionId)) {
      return false;
    }
    seen.add(session.sessionId);
    return true;
  });
}

export function sessionTitle(session: AppInteractionSessionDescriptor): string {
  const firstText = (session.state.messages ?? [])
    .map((message) => textFromMessage(message))
    .find((text) => text.length > 0)
    ?.split("\n")[0];
  return firstText?.slice(0, 40) || session.sessionId;
}

export function sessionStatusFromDisplayEvent(
  event: AppDisplayEvent,
): AppInteractionSessionStatus | null {
  switch (event.type) {
    case "run_started":
      return "running";
    case "run_paused":
      return "paused";
    case "run_completed":
      return "completed";
    case "run_failed":
      return "failed";
    case "run_aborted":
      return "aborted";
    default:
      return null;
  }
}

export function updateSessionStatus(
  sessions: AppInteractionSessionDescriptor[],
  sessionId: string,
  status: AppInteractionSessionStatus,
): AppInteractionSessionDescriptor[] {
  return sessions.map((session) =>
    session.sessionId === sessionId ? { ...session, status } : session,
  );
}
