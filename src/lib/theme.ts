export type Theme = "system" | "light" | "dark";
const KEY = "tidy-theme";
const mq = window.matchMedia("(prefers-color-scheme: dark)");

export function getTheme(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch { /* storage unavailable: fall back to system */ }
  return "system";
}

function apply(t: Theme) {
  const dark = t === "dark" || (t === "system" && mq.matches);
  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
  // Keep the native title bar and the popover material in the chosen theme (no-op outside the desktop app).
  import("@tauri-apps/api/window").then((m) => m.getCurrentWindow().setTheme(t === "system" ? null : t)).catch(() => {});
}

export function setTheme(t: Theme) {
  try { localStorage.setItem(KEY, t); } catch { /* ignore */ }
  apply(t);
  window.dispatchEvent(new Event("tidy-theme"));
}

/** Call once at startup: applies the saved choice and follows OS changes while on "system". */
export function initTheme() {
  apply(getTheme());
  mq.addEventListener("change", () => getTheme() === "system" && apply("system"));
  // keep other windows (e.g. the menu-bar popover) in sync: `storage` fires across webviews of the same origin
  window.addEventListener("storage", (e) => e.key === KEY && apply(getTheme()));
}
