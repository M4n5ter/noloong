import { invoke } from "@tauri-apps/api/core";

type ElementRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type AppRenderProbeReport = {
  surface: string;
  viewportWidth: number;
  viewportHeight: number;
  desktopMediaQuery: boolean;
  primaryHeading: string | null;
  primaryHeadingRect: ElementRect | null;
  surfaceRect: ElementRect | null;
};

export function scheduleRenderProbe(): () => void {
  if (!("__TAURI_INTERNALS__" in window)) {
    return () => {};
  }

  const timer = window.setTimeout(() => {
    void reportRenderProbeIfEnabled();
  }, 500);

  return () => {
    window.clearTimeout(timer);
  };
}

async function reportRenderProbeIfEnabled(): Promise<void> {
  try {
    const enabled = await invoke<boolean>("app_render_probe_enabled");
    if (!enabled) {
      return;
    }
  } catch {
    return;
  }
  await reportRenderProbe();
}

async function reportRenderProbe(): Promise<void> {
  const surface = currentSurface();
  const heading = primaryHeading(surface);
  const report: AppRenderProbeReport = {
    surface,
    viewportWidth: window.innerWidth,
    viewportHeight: window.innerHeight,
    desktopMediaQuery: window.matchMedia("(min-width: 901px)").matches,
    primaryHeading: heading?.textContent ?? null,
    primaryHeadingRect: rectFor(heading),
    surfaceRect: rectFor(surfaceElement(surface)),
  };

  try {
    await invoke("app_render_probe_report", { report });
  } catch {
    // Render probes are test-only diagnostics and must never affect the UI.
  }
}

function primaryHeading(surface: string): Element | null {
  return surfaceElement(surface)?.querySelector("[data-render-heading]") ?? document.querySelector("h1, h2");
}

function currentSurface(): string {
  return surfaceElement("environment")?.getAttribute("data-render-surface")
    ?? surfaceElement("sessions")?.getAttribute("data-render-surface")
    ?? surfaceElement("chat")?.getAttribute("data-render-surface")
    ?? "unknown";
}

function surfaceElement(surface: string): Element | null {
  const explicit = document.querySelector(`[data-render-surface="${surface}"]`);
  if (explicit) {
    return explicit;
  }
  return document.querySelector(".app-shell");
}

function rectFor(element: Element | null): ElementRect | null {
  if (!element) {
    return null;
  }
  const rect = element.getBoundingClientRect();
  return {
    x: rect.x,
    y: rect.y,
    width: rect.width,
    height: rect.height,
  };
}
