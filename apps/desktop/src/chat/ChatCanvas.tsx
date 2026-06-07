import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
} from "../interaction/client";
import type { AppLaunchOptions } from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { CenteredStatus } from "./CenteredStatus";
import { RuntimeBanner, SessionList, TranscriptView } from "./TranscriptComponents";
import type { BootstrapState } from "./types";
import { useInteractionRuntime } from "./useInteractionRuntime";

export function ChatCanvas({
  bootstrap,
  connectDisplayStream,
  createInteractionClient,
  i18n,
  onOpenSettings,
}: {
  bootstrap: BootstrapState;
  connectDisplayStream: typeof connectDefaultInteractionDisplayStream;
  createInteractionClient: typeof createDefaultInteractionClient;
  i18n: AppI18n;
  onOpenSettings: () => void;
}) {
  if (bootstrap.status === "loading") {
    return (
      <CenteredStatus
        detail={i18n.t("bootstrap.loadingDetail")}
        title={i18n.t("bootstrap.loadingTitle")}
      />
    );
  }

  if (bootstrap.status === "failed") {
    return <CenteredStatus title={i18n.t("bootstrap.failedTitle")} detail={bootstrap.error} />;
  }

  return (
    <InteractionCanvas
      connectDisplayStream={connectDisplayStream}
      createInteractionClient={createInteractionClient}
      i18n={i18n}
      onOpenSettings={onOpenSettings}
      options={bootstrap.options}
    />
  );
}

function InteractionCanvas({
  connectDisplayStream,
  createInteractionClient,
  i18n,
  onOpenSettings,
  options,
}: {
  connectDisplayStream: typeof connectDefaultInteractionDisplayStream;
  createInteractionClient: typeof createDefaultInteractionClient;
  i18n: AppI18n;
  onOpenSettings: () => void;
  options: AppLaunchOptions;
}) {
  const runtime = useInteractionRuntime({
    connectDisplayStream,
    createInteractionClient,
    i18n,
    options,
  });

  return (
    <section className="chat-shell">
      <aside className="session-rail">
        <div className="section-heading">
          <span>{i18n.t("sessions.title")}</span>
          <button
            aria-label={i18n.t("sessions.create")}
            className="icon-button"
            disabled={runtime.interaction.status !== "ready"}
            onClick={() => void runtime.createSession()}
            type="button"
          >
            +
          </button>
        </div>
        <SessionList
          i18n={i18n}
          interaction={runtime.interaction}
          onSelect={runtime.selectSession}
        />
      </aside>
      <section className="transcript-pane">
        <RuntimeBanner
          i18n={i18n}
          interaction={runtime.interaction}
          profileConfigPath={options.profileConfigPath ?? i18n.t("header.starterDraft")}
        />
        <TranscriptView
          i18n={i18n}
          interaction={runtime.interaction}
          onAbortRun={runtime.abortCurrentRun}
          onOpenSettings={onOpenSettings}
          onResolveApproval={runtime.resolveApproval}
          onSubmitPrompt={runtime.submitPrompt}
          onToggleReasoning={runtime.toggleReasoning}
        />
      </section>
    </section>
  );
}
