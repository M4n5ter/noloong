import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import type { AppLaunchOptions, AppRuntimeRestartResult } from "./generated/contracts";
import { ChatCanvas } from "./chat/ChatCanvas";
import type { BootstrapState } from "./chat/types";
import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
} from "./interaction/client";
import { createI18n, resolveUiLocale, type AppI18n } from "./i18n";
import { SettingsView } from "./settings/SettingsView";
import { devLaunchOptions, isTauriRuntime } from "./devFallback";
import { scheduleRenderProbe } from "./renderProbe";
import "./styles.css";

export type AppShellDependencies = {
  bootstrap?: () => Promise<AppLaunchOptions>;
  createInteractionClient?: typeof createDefaultInteractionClient;
  connectInteractionDisplayStream?: typeof connectDefaultInteractionDisplayStream;
};

type AppRoute = "chat" | "settings";
const OPEN_SETTINGS_EVENT = "noloong-open-settings";

export function App({ dependencies = {} }: { dependencies?: AppShellDependencies }) {
  const [bootstrap, setBootstrap] = useState<BootstrapState>({ status: "loading" });
  const [route, setRoute] = useState<AppRoute>("chat");
  const bootstrapApp = dependencies.bootstrap ?? defaultBootstrap;
  const createClient = dependencies.createInteractionClient ?? createDefaultInteractionClient;
  const connectDisplayStream =
    dependencies.connectInteractionDisplayStream ?? connectDefaultInteractionDisplayStream;
  const locale = resolveUiLocale(
    bootstrap.status === "ready" ? bootstrap.options.locale ?? "en" : null,
  );
  const i18n = useMemo(() => createI18n(locale), [locale]);
  const applyRuntimeRestart = (result: AppRuntimeRestartResult) => {
    setBootstrap((current) =>
      current.status === "ready"
        ? {
            status: "ready",
            options: {
              ...current.options,
              interactionEndpoint: result.interactionEndpoint,
              interactionStatus: result.interactionStatus,
            },
          }
        : current,
    );
  };

  useEffect(() => {
    let active = true;

    bootstrapApp()
      .then((options) => {
        if (active) {
          setBootstrap({ status: "ready", options });
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setBootstrap({ status: "failed", error: String(error) });
        }
      });

    return () => {
      active = false;
    };
  }, [bootstrapApp]);

  useEffect(() => scheduleRenderProbe(), [bootstrap.status, route]);

  useEffect(() => {
    if (bootstrap.status !== "ready" || !isTauriRuntime()) {
      return;
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    void listen(OPEN_SETTINGS_EVENT, () => {
      setRoute("settings");
    }).then((dispose) => {
      if (!active) {
        dispose();
        return;
      }
      unlisten = dispose;
    });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [bootstrap.status]);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || bootstrap.status !== "ready") {
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        setRoute("settings");
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [bootstrap.status]);

  const title = bootstrap.status === "ready" ? null : headerTitle(route, bootstrap, i18n);
  const subtitle = headerSubtitle(bootstrap, i18n);

  return (
    <main className="app-shell">
      <header className="title-bar" data-tauri-drag-region="deep">
        {title ? (
          <div className="title-copy">
            <strong>{title}</strong>
            {subtitle ? <span>{subtitle}</span> : null}
          </div>
        ) : null}
        <div className="title-drag-spacer" />
      </header>
      {route === "settings" && bootstrap.status === "ready" ? (
        <SettingsView
          i18n={i18n}
          launchOptions={bootstrap.options}
          onBack={() => setRoute("chat")}
          onRuntimeRestart={applyRuntimeRestart}
        />
      ) : (
        <ChatCanvas
          bootstrap={bootstrap}
          connectDisplayStream={connectDisplayStream}
          createInteractionClient={createClient}
          i18n={i18n}
          onOpenSettings={() => setRoute("settings")}
        />
      )}
    </main>
  );
}

function headerTitle(route: AppRoute, bootstrap: BootstrapState, i18n: AppI18n): string {
  if (bootstrap.status !== "ready") {
    return i18n.t("app.brand");
  }
  return route === "settings" ? i18n.t("settings.title") : i18n.t("nav.chat");
}

function headerSubtitle(bootstrap: BootstrapState, i18n: AppI18n): string | null {
  if (bootstrap.status === "ready") {
    return null;
  }
  return i18n.headerSubtitle({ status: bootstrap.status });
}

function defaultBootstrap(): Promise<AppLaunchOptions> {
  if (!isTauriRuntime()) {
    return Promise.resolve(devLaunchOptions());
  }
  return invoke<AppLaunchOptions>("app_bootstrap");
}
