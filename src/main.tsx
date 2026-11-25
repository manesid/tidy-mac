import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import Popover from "./pages/Popover";
import "./index.css";
import { initTheme } from "./lib/theme";

initTheme();
document.addEventListener("contextmenu", (e) => e.preventDefault());

/** The menu bar popover is a second window loading the same bundle. `?popover` is for previews. */
function isPopover(): boolean {
  if (new URLSearchParams(location.search).has("popover")) return true;
  try { return getCurrentWindow().label === "popover"; } catch { return false; }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {isPopover() ? <Popover /> : <>
      <App />
      <Toaster position="bottom-right" richColors closeButton />
    </>}
  </StrictMode>,
);
