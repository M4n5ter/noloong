import { createRoot } from "react-dom/client";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) {
  throw new Error("root element is missing");
}

try {
  createRoot(root).render(<App />);
} catch (error) {
  root.replaceChildren(renderBootError(error));
}

function renderBootError(error: unknown): HTMLElement {
  const container = document.createElement("div");
  container.className = "boot-fallback";

  const title = document.createElement("strong");
  title.textContent = "Noloong failed to start";

  const detail = document.createElement("span");
  detail.textContent = error instanceof Error ? error.message : String(error);

  container.append(title, detail);
  return container;
}
