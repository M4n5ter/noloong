import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { AppLaunchOptions } from "./generated/contracts";
import { ChatCanvas } from "./chat/ChatCanvas";
import type { BootstrapState } from "./chat/types";
import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
} from "./interaction/client";
import { createI18n, resolveUiLocale, type AppI18n } from "./i18n";
import { SettingsView } from "./settings/SettingsView";
import "./styles.css";

export type AppShellDependencies = {
  bootstrap?: () => Promise<AppLaunchOptions>;
  createInteractionClient?: typeof createDefaultInteractionClient;
  connectInteractionDisplayStream?: typeof connectDefaultInteractionDisplayStream;
};

type AppRoute = "chat" | "settings";

export function App({ dependencies = {} }: { dependencies?: AppShellDependencies }) {
  const [bootstrap, setBootstrap] = useState<BootstrapState>({ status: "loading" });
  const [route, setRoute] = useState<AppRoute>("chat");
  const bootstrapApp = dependencies.bootstrap ?? defaultBootstrap;
  const createClient = dependencies.createInteractionClient ?? createDefaultInteractionClient;
  const connectDisplayStream =
    dependencies.connectInteractionDisplayStream ?? connectDefaultInteractionDisplayStream;
  const locale = resolveUiLocale(
    bootstrap.status === "ready" ? bootstrap.options.locale : null,
  );
  const i18n = useMemo(() => createI18n(locale), [locale]);

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

  return (
    <main className="app-shell">
      <header className="title-bar" data-tauri-drag-region="deep">
        <div className="brand-mark" aria-hidden="true">
          N
        </div>
        <div className="title-copy">
          <strong>{i18n.t("app.brand")}</strong>
          <span>{headerSubtitle(bootstrap, i18n)}</span>
        </div>
        <div className="title-drag-spacer" />
        {bootstrap.status === "ready" ? (
          <nav className="title-actions">
            <button
              className={route === "chat" ? "title-action active" : "title-action"}
              data-tauri-drag-region="false"
              onClick={() => setRoute("chat")}
              type="button"
            >
              {i18n.t("nav.chat")}
            </button>
            <button
              className={route === "settings" ? "title-action active" : "title-action"}
              data-tauri-drag-region="false"
              onClick={() => setRoute("settings")}
              type="button"
            >
              {i18n.t("nav.settings")}
            </button>
          </nav>
        ) : null}
      </header>
      {route === "settings" && bootstrap.status === "ready" ? (
        <SettingsView i18n={i18n} onBack={() => setRoute("chat")} />
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

function headerSubtitle(bootstrap: BootstrapState, i18n: AppI18n): string {
  if (bootstrap.status === "ready") {
    return i18n.headerSubtitle({
      status: "ready",
      appVersion: bootstrap.options.appVersion,
      profileConfigPath: bootstrap.options.profileConfigPath,
    });
  }
  return i18n.headerSubtitle({ status: bootstrap.status });
}

function defaultBootstrap(): Promise<AppLaunchOptions> {
  return invoke<AppLaunchOptions>("app_bootstrap");
}
