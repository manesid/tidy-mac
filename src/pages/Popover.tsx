import { useCallback, useEffect, useState } from "react";
import { BellOff, BellRing, Loader2, Power, Sparkles, SquareArrowOutUpRight, TrendingUp } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, useEvent, type Disk, type Level, type PopoverState, type QuickPreview } from "@/lib/api";
import { ago, bytes, cn, inDays } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { SpaceList } from "@/components/SpaceList";

const LEVEL = {
  Ok: { label: "Healthy", badge: "ok", bar: "bg-ok" },
  Warn: { label: "Getting full", badge: "warn", bar: "bg-warn" },
  Critical: { label: "Critically low", badge: "crit", bar: "bg-crit" },
} as const;

type Quick = { phase: "idle" } | { phase: "checking" } | { phase: "ready"; p: QuickPreview } | { phase: "cleaning"; p: QuickPreview } | { phase: "done"; freed: number; skipped: number };

const clock = (secs: number) => new Date(secs * 1000).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" });

/** Menu bar popover: status at a glance, preview-then-confirm Quick Clean, alert snooze. */
export default function Popover() {
  const [s, setS] = useState<PopoverState | null>(null);
  const [quick, setQuick] = useState<Quick>({ phase: "idle" });
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => { api.popoverState().then((x) => { setS(x); setError(null); }).catch((e) => setError(String(e))); }, []);
  useEffect(load, [load]);
  // The popover window is transparent with a native material behind it; the page must not paint over it.
  useEffect(() => { document.documentElement.style.background = "transparent"; document.body.style.background = "transparent"; }, []);
  useEvent<void>("popover:open", () => { load(); setQuick((q) => (q.phase === "done" || q.phase === "ready" ? { phase: "idle" } : q)); });
  useEvent<void>("settings:changed", load);
  // The scheduler emits this every 30 s even while the popover is hidden: patch the numbers locally instead of
  // re-querying the backend (FDA probe, forecast) each time.
  useEvent<Disk | null>("disk:update", (disk) => setS((x) => {
    if (!x || !disk) return x;
    const level: Level = disk.used_pct >= x.settings.critical_pct ? "Critical" : disk.used_pct >= x.settings.warn_pct ? "Warn" : "Ok";
    return { ...x, disk, level };
  }));
  useEffect(() => {
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") getCurrentWindow().hide().catch(() => {}); };
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, []);

  const check = async () => {
    setQuick({ phase: "checking" });
    try { setQuick({ phase: "ready", p: await api.quickPreview() }); } catch (e) { setError(String(e)); setQuick({ phase: "idle" }); }
  };
  const commit = async (p: QuickPreview) => {
    setQuick({ phase: "cleaning", p });
    try { const r = await api.quickCommit(); setQuick({ phase: "done", freed: r.freed, skipped: r.failed.length }); load(); }
    catch (e) { setError(String(e)); setQuick({ phase: "idle" }); }
  };
  const save = async (patch: Partial<PopoverState["settings"]>) => { if (s) { await api.settingsSet({ ...s.settings, ...patch }); load(); } };
  const snooze = async (u: "hour" | "tomorrow" | null) => { await api.snooze(u); load(); };

  const d = s?.disk;
  const lv = LEVEL[s?.level ?? "Ok"];
  const snoozed = s && s.settings.snoozed_until * 1000 > Date.now();
  const f = s?.forecast;

  return (
    <div className="text-foreground flex h-screen flex-col gap-3 overflow-y-auto p-4 text-sm">
      {/* status */}
      <section aria-label="Disk status" className="space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-muted-foreground text-xs font-medium">Macintosh HD</div>
          {s && <Badge variant={lv.badge}>{lv.label}</Badge>}
        </div>
        {d ? (
          <>
            <div className="flex items-baseline gap-1.5"><span className="num text-2xl font-semibold tracking-tight">{bytes(d.free)}</span><span className="text-muted-foreground">free of {bytes(d.total)}</span></div>
            <div className="bg-secondary h-2 overflow-hidden rounded-full" role="meter" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(d.used_pct)} aria-label="Disk used">
              <div className={cn("h-full rounded-full transition-all", lv.bar)} style={{ width: `${d.used_pct}%` }} />
            </div>
            <div className="text-muted-foreground num flex justify-between text-xs">
              <span>{d.used_pct.toFixed(0)}% used</span>
              {f?.days_to_full != null ? <span className={cn(f.days_to_full < 30 && "text-crit")}><TrendingUp className="mr-1 inline size-3" aria-hidden />Full {inDays(f.days_to_full)}</span>
                : f ? <span>Usage stable</span> : null}
            </div>
          </>
        ) : <div className="bg-secondary h-16 animate-pulse rounded-lg" />}
      </section>

      {/* quick clean: always preview first, then an explicit button labelled with the size */}
      <section aria-label="Quick Clean" className="bg-card/60 space-y-2 rounded-xl border p-3">
        <div className="flex items-center justify-between gap-2">
          <div className="font-medium">Quick Clean</div>
          <span className="text-muted-foreground text-xs">Safe, regenerable items only</span>
        </div>
        {quick.phase === "idle" && <Button className="w-full" variant="outline" onClick={check}><Sparkles />Check what can be cleaned</Button>}
        {quick.phase === "checking" && <Button className="w-full" variant="outline" disabled><Loader2 className="animate-spin" />Checking caches and logs…</Button>}
        {(quick.phase === "ready" || quick.phase === "cleaning") && (
          quick.p.total === 0 ? (
            <div className="text-muted-foreground py-1 text-center text-xs">Nothing safe to clean right now.</div>
          ) : (
            <>
              <ul className="space-y-1 text-xs">
                {quick.p.categories.slice(0, 4).map(([n, v]) => <li key={n} className="flex justify-between"><span className="text-muted-foreground truncate">{n}</span><span className="num">{bytes(v)}</span></li>)}
              </ul>
              <div className="flex gap-2">
                <Button variant="ghost" className="flex-1" disabled={quick.phase === "cleaning"} onClick={() => setQuick({ phase: "idle" })}>Cancel</Button>
                <Button variant="brand" className="flex-[2]" disabled={quick.phase === "cleaning"} onClick={() => commit(quick.p)}>
                  {quick.phase === "cleaning" ? <Loader2 className="animate-spin" /> : <Sparkles />}Clean {bytes(quick.p.total)}
                </Button>
              </div>
            </>
          )
        )}
        {quick.phase === "done" && (
          <div className="flex items-center justify-between text-xs">
            <span>Freed <b className="num">{bytes(quick.freed)}</b>{quick.skipped ? ` · ${quick.skipped} skipped` : ""}</span>
            <Button size="sm" variant="ghost" onClick={() => setQuick({ phase: "idle" })}>Done</Button>
          </div>
        )}
      </section>

      {/* top space users from the last measurement */}
      <section aria-label="Biggest space users" className="space-y-1">
        <div className="flex items-center justify-between">
          <div className="font-medium">Biggest space users</div>
          {s?.hogs_at && <span className="text-muted-foreground text-xs">{ago(s.hogs_at)}</span>}
        </div>
        {s && s.hogs.length > 0 ? <SpaceList hogs={s.hogs} compact />
          : <button className="text-muted-foreground hover:text-foreground w-full rounded-md py-2 text-left text-xs underline-offset-2 hover:underline" onClick={() => api.openMain("dashboard")}>Not measured yet. Measure in Tidy</button>}
      </section>

      {/* alerts */}
      {s && (
        <section aria-label="Storage alerts" className="space-y-2 border-t pt-3">
          <div className="flex items-center justify-between">
            <label htmlFor="pop-alerts" className="flex items-center gap-2 font-medium">{snoozed ? <BellOff className="size-4" aria-hidden /> : <BellRing className="size-4" aria-hidden />}Storage alerts</label>
            <Switch id="pop-alerts" checked={s.settings.alerts_enabled} onCheckedChange={(v) => save({ alerts_enabled: v })} />
          </div>
          {s.settings.alerts_enabled && (snoozed ? (
            <div className="text-muted-foreground flex items-center justify-between text-xs">
              <span>Snoozed until {clock(s.settings.snoozed_until)}</span>
              <Button size="sm" variant="ghost" onClick={() => snooze(null)}>Resume</Button>
            </div>
          ) : (
            <div className="flex gap-2">
              <Button size="sm" variant="outline" className="flex-1" onClick={() => snooze("hour")}>Snooze 1 hour</Button>
              <Button size="sm" variant="outline" className="flex-1" onClick={() => snooze("tomorrow")}>Until tomorrow</Button>
            </div>
          ))}
        </section>
      )}

      {error && <div className="text-crit text-xs" role="alert">{error}</div>}

      <footer className="mt-auto flex gap-2 border-t pt-3">
        <Button variant="secondary" className="flex-1" onClick={() => api.openMain()}><SquareArrowOutUpRight />Open Tidy</Button>
        <Button variant="ghost" onClick={() => api.quit()} aria-label="Quit Tidy"><Power />Quit</Button>
      </footer>
    </div>
  );
}
