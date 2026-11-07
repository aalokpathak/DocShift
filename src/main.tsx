import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";

async function boot() {
  // Dev-only: `npm run dev` then open /?mock in a normal browser to review the UI against a fake backend.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    (await import("./lib/mock")).installMock();
  }
  if (import.meta.env.DEV) {
    // handles for E2E scripts
    const w = window as unknown as Record<string, unknown>;
    w.__store = (await import("./lib/store")).useApp;
    w.__catalog = await import("./lib/catalog");
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
void boot();
