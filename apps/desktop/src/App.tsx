import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useState } from "react";
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
import {
  connectDevInteractionDisplayStream,
  createDevInteractionClient,
} from "./devInteractionRuntime";
import { scheduleRenderProbe } from "./renderProbe";
import "./styles.css";

export type AppShellDependencies = {
  bootstrap?: () => Promise<AppLaunchOptions>;
  createInteractionClient?: typeof createDefaultInteractionClient;
  connectInteractionDisplayStream?: typeof connectDefaultInteractionDisplayStream;
};

type AppSurface = "chat" | "settings";
const MAIN_WINDOW_LABEL = "main";
const RUNTIME_RESTART_EVENT = "noloong-runtime-restarted";

export function App({ dependencies = {} }: { dependencies?: AppShellDependencies }) {
  const [bootstrap, setBootstrap] = useState<BootstrapState>({ status: "loading" });
  const surface = appSurface();
  const bootstrapApp = dependencies.bootstrap ?? defaultBootstrap;
  const createClient =
    dependencies.createInteractionClient ??
    (isTauriRuntime() ? createDefaultInteractionClient : createDevInteractionClient);
  const connectDisplayStream =
    dependencies.connectInteractionDisplayStream ??
    (isTauriRuntime()
      ? connectDefaultInteractionDisplayStream
      : connectDevInteractionDisplayStream);
  const locale = resolveUiLocale(
    bootstrap.status === "ready" ? bootstrap.options.locale ?? "en" : null,
  );
  const i18n = useMemo(() => createI18n(locale), [locale]);
  const applyRuntimeRestart = useCallback((result: AppRuntimeRestartResult) => {
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
  }, []);
  const handleRuntimeRestart = useCallback(
    (result: AppRuntimeRestartResult) => {
      applyRuntimeRestart(result);
      if (surface === "settings") {
        notifyMainRuntimeRestart(result);
      }
    },
    [applyRuntimeRestart, surface],
  );

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

  useEffect(() => scheduleRenderProbe(), [bootstrap.status, surface]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<AppRuntimeRestartResult>(RUNTIME_RESTART_EVENT, (event) => {
      applyRuntimeRestart(event.payload);
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
  }, [applyRuntimeRestart]);

  useEffect(() => {
    function handleGlobalKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || bootstrap.status !== "ready") {
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        openSettingsSurface();
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [bootstrap.status]);

  const title = bootstrap.status === "ready" ? null : headerTitle(bootstrap, i18n);
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
      {surface === "settings" && bootstrap.status === "ready" ? (
        <SettingsView
          i18n={i18n}
          launchOptions={bootstrap.options}
          onRuntimeRestart={handleRuntimeRestart}
        />
      ) : (
        <ChatCanvas
          bootstrap={bootstrap}
          connectDisplayStream={connectDisplayStream}
          createInteractionClient={createClient}
          i18n={i18n}
          onOpenSettings={openSettingsSurface}
        />
      )}
    </main>
  );
}

function appSurface(): AppSurface {
  if (typeof window === "undefined") {
    return "chat";
  }
  return new URLSearchParams(window.location.search).get("surface") === "settings"
    ? "settings"
    : "chat";
}

function openSettingsSurface(): void {
  if (isTauriRuntime()) {
    void invoke("app_open_settings_window");
    return;
  }
  window.open(settingsSurfaceUrl(), "noloong-settings", "width=920,height=720");
}

function settingsSurfaceUrl(): string {
  const url = new URL(window.location.href);
  url.searchParams.set("surface", "settings");
  return `${url.pathname}${url.search}${url.hash}`;
}

function notifyMainRuntimeRestart(result: AppRuntimeRestartResult): void {
  if (!isTauriRuntime()) {
    return;
  }
  void emitTo(MAIN_WINDOW_LABEL, RUNTIME_RESTART_EVENT, result).catch(() => undefined);
}

function headerTitle(bootstrap: BootstrapState, i18n: AppI18n): string {
  if (bootstrap.status !== "ready") {
    return i18n.t("app.brand");
  }
  return i18n.t("nav.chat");
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
