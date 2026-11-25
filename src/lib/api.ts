import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

export type Disk = { total: number; free: number; used: number; used_pct: number };
export type Overview = { disk: Disk | null; full_disk_access: boolean; lifetime_freed: number; home: string; volume: string; history_available: boolean };
export type Item = { id: string; name: string; path: string; size: number };
export type Category = {
  id: string; name: string; description: string; group: string;
  safety: "safe" | "review"; disposal: "delete" | "trash" | "emptytrash";
  needs_fda: boolean; blocked: boolean; total: number; items: Item[];
};
export type Report = { freed: number; removed: number; failed: { path: string; reason: string }[] };
export type Entry = { name: string; path: string; size: number; is_dir: boolean; blocked: boolean; modified: number | null; delta: number | null };
export type Listing = { path: string; total: number; entries: Entry[]; hidden: number; unreadable: boolean; since: number | null };
export type Kind = "video" | "images" | "audio" | "documents" | "archives" | "code" | "apps" | "other";
export type Insights = { path: string; total: number; by_kind: [Kind, number][]; top_files: Entry[]; unreadable_dirs: number };
export type TitleMode = "free" | "percent" | "icon";
export type Settings = {
  alerts_enabled: boolean; warn_pct: number; critical_pct: number; check_minutes: number; launch_at_login: boolean; lifetime_freed: number;
  tray_title: TitleMode; menu_bar_only: boolean; snoozed_until: number;
};
export type Level = "Ok" | "Warn" | "Critical";
export type Sample = { ts: number; used: number; total: number };
export type Forecast = { bytes_per_day: number; days_to_full: number | null; span_days: number };
export type UsageHistory = { samples: Sample[]; forecast: Forecast | null };
export type HistoryRow = { id: number; ts: number; source: "smart" | "quick" | "explorer" | "tool"; path: string; size: number; disposal: string; ok: boolean; reason: string | null };
export type Hog = { id: string; label: string; note: string; size: number; path: string | null; blocked: boolean };
export type SpaceBreakdown = { hogs: Hog[]; measured_at: number | null; local_snapshots: number };
export type Tool = { id: "tm-snapshots" | "simulators" | "homebrew"; available: boolean; command: string; needs_admin: boolean; found: string; bytes: number | null; choices: string[] };
export type QuickPreview = { total: number; count: number; categories: [string, number][] };
export type PopoverState = { disk: Disk | null; level: Level; forecast: Forecast | null; hogs: Hog[]; hogs_at: number | null; settings: Settings; full_disk_access: boolean };

export const api = {
  overview: () => invoke<Overview>("overview"),
  scanStart: () => invoke<void>("scan_start"),
  scanCancel: () => invoke<void>("scan_cancel"),
  clean: (ids: string[]) => invoke<Report>("clean", { ids }),
  list: (path: string) => invoke<Listing>("explorer_list", { path }),
  insights: (path: string) => invoke<Insights>("explorer_insights", { path }),
  explorerCancel: () => invoke<void>("explorer_cancel"),
  trash: (path: string) => invoke<Report>("explorer_trash", { path }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  openFda: () => invoke<void>("open_full_disk_access"),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<Settings>("settings_set", { settings }),
  snooze: (until: "hour" | "tomorrow" | null) => invoke<Settings>("snooze", { until }),
  usageHistory: (days: number) => invoke<UsageHistory>("usage_history", { days }),
  history: () => invoke<HistoryRow[]>("history_list"),
  space: (refresh: boolean) => invoke<SpaceBreakdown>("space_breakdown", { refresh }),
  tools: () => invoke<Tool[]>("tools_status"),
  toolRun: (id: Tool["id"], arg?: string) => invoke<void>("tool_run", { id, arg: arg ?? null }),
  quickPreview: () => invoke<QuickPreview>("quick_preview"),
  quickCommit: () => invoke<Report>("quick_clean_commit"),
  popoverState: () => invoke<PopoverState>("popover_state"),
  openMain: (page?: string) => invoke<void>("open_main", { page: page ?? null }),
  quit: () => invoke<void>("quit"),
};

/** Subscribe to a backend event for the lifetime of the component. */
export function useEvent<T>(name: string, fn: (p: T) => void) {
  useEffect(() => {
    let un: UnlistenFn | undefined, dead = false;
    listen<T>(name, (e) => fn(e.payload)).then((u) => (dead ? u() : (un = u)));
    return () => { dead = true; un?.(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name]);
}
