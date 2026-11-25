import { useCallback, useEffect, useState } from "react";
import { Activity, Bell, FolderSearch, HardDrive, History as HistoryIcon, Sparkles, Wrench } from "lucide-react";
import { api, useEvent, type Disk, type Overview, type Settings } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ThemeToggle } from "@/components/ThemeToggle";
import Dashboard from "@/pages/Dashboard";
import Cleaner from "@/pages/Cleaner";
import Explorer from "@/pages/Explorer";
import Alerts from "@/pages/Alerts";
import Tools from "@/pages/Tools";
import History from "@/pages/History";

export type Page = "dashboard" | "cleaner" | "explorer" | "tools" | "history" | "alerts";
const NAV: { id: Page; label: string; icon: typeof Activity }[] = [
  { id: "dashboard", label: "Overview", icon: Activity },
  { id: "cleaner", label: "Smart Clean", icon: Sparkles },
  { id: "explorer", label: "Storage Explorer", icon: FolderSearch },
  { id: "tools", label: "Tools", icon: Wrench },
  { id: "history", label: "History", icon: HistoryIcon },
  { id: "alerts", label: "Alerts & Menu Bar", icon: Bell },
];

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [ov, setOv] = useState<Overview | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [scanTick, setScanTick] = useState(0); // bump to auto-start a scan
  const [jump, setJump] = useState<{ path: string; n: number } | null>(null); // open a folder in the Explorer

  const reload = useCallback(() => { api.overview().then(setOv); api.settingsGet().then(setSettings); }, []);
  useEffect(reload, [reload]);
  useEvent<Disk | null>("disk:update", (disk) => setOv((o) => (o ? { ...o, disk } : o)));
  useEvent<void>("settings:changed", reload);
  useEvent<string>("nav", (id) => {
    if (id === "scan") { setPage("cleaner"); setScanTick((t) => t + 1); }
    else if (id === "explore") setPage("explorer");
    else if (NAV.some((n) => n.id === id)) setPage(id as Page);
  });
  // FDA can be granted while the app runs; re-check when the window regains focus.
  useEffect(() => { window.addEventListener("focus", reload); return () => window.removeEventListener("focus", reload); }, [reload]);

  const startScan = () => { setPage("cleaner"); setScanTick((t) => t + 1); };
  const explorePath = (path: string) => { setPage("explorer"); setJump((j) => ({ path, n: (j?.n ?? 0) + 1 })); };
  const pct = ov?.disk?.used_pct ?? 0;

  return (
    <div className="flex h-full">
      <aside className="bg-sidebar flex w-60 shrink-0 flex-col border-r">
        <div data-tauri-drag-region className="h-14 shrink-0" />
        <div className="flex items-center gap-2.5 px-5 pb-5">
          <div className="bg-primary text-primary-foreground grid size-8 place-items-center rounded-lg"><HardDrive className="size-4" /></div>
          <div className="text-[15px] font-semibold tracking-tight">Tidy</div>
        </div>
        <nav className="flex flex-col gap-0.5 px-3">
          {NAV.map(({ id, label, icon: Icon }) => (
            <button key={id} onClick={() => setPage(id)} aria-current={page === id ? "page" : undefined}
              className={cn("flex items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm font-medium transition-colors", page === id ? "bg-card text-foreground shadow-xs ring-1 ring-border" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground")}>
              <Icon className="size-4" />{label}
            </button>
          ))}
        </nav>
        <div className="mt-auto space-y-3 p-4">
          <ThemeToggle />
          <div className="bg-card rounded-lg border p-3">
            <div className="text-muted-foreground mb-1.5 flex justify-between text-xs"><span>Macintosh HD</span><span className="num">{pct.toFixed(0)}%</span></div>
            <div className="bg-secondary h-1.5 overflow-hidden rounded-full">
              <div className={cn("h-full rounded-full transition-all", pct >= (settings?.critical_pct ?? 90) ? "bg-crit" : pct >= (settings?.warn_pct ?? 80) ? "bg-warn" : "bg-ok")} style={{ width: `${pct}%` }} />
            </div>
          </div>
        </div>
      </aside>
      <main className="bg-background relative min-w-0 flex-1">
        <div data-tauri-drag-region className="absolute inset-x-0 top-0 z-10 h-10" />
        {/* all pages stay mounted so scan results and explorer position survive navigation */}
        <div className={cn("h-full overflow-y-auto", page !== "dashboard" && "hidden")}><Dashboard ov={ov} settings={settings} onScan={startScan} onExplore={() => setPage("explorer")} onExplorePath={explorePath} /></div>
        <div className={cn("h-full", page !== "cleaner" && "hidden")}><Cleaner scanTick={scanTick} onFreed={reload} /></div>
        <div className={cn("h-full", page !== "explorer" && "hidden")}><Explorer ov={ov} onChanged={reload} jump={jump} /></div>
        <div className={cn("h-full overflow-y-auto", page !== "tools" && "hidden")}>{page === "tools" && <Tools />}</div>
        <div className={cn("h-full overflow-y-auto", page !== "history" && "hidden")}><History active={page === "history"} /></div>
        <div className={cn("h-full overflow-y-auto", page !== "alerts" && "hidden")}><Alerts disk={ov?.disk ?? null} settings={settings} onChange={setSettings} /></div>
      </main>
    </div>
  );
}
