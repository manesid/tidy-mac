import { useCallback, useEffect, useState } from "react";
import { ArrowRight, FolderSearch, Loader2, RefreshCw, ShieldAlert, Sparkles, TrendingUp } from "lucide-react";
import { toast } from "sonner";
import { api, type Overview, type Settings, type SpaceBreakdown, type UsageHistory, type Hog } from "@/lib/api";
import { ago, bytes, cn, inDays } from "@/lib/utils";
import { UsageChart } from "@/components/UsageChart";
import { SpaceList } from "@/components/SpaceList";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

export function PageHeader({ title, subtitle, children }: { title: string; subtitle?: string; children?: React.ReactNode }) {
  return (
    <div className="flex items-end justify-between gap-4 px-8 pt-12 pb-5">
      <div><h1 className="text-2xl font-semibold tracking-tight">{title}</h1>{subtitle && <p className="text-muted-foreground mt-1 text-sm">{subtitle}</p>}</div>
      <div className="flex items-center gap-2">{children}</div>
    </div>
  );
}

function Gauge({ pct, free, level }: { pct: number; free: number; level: "ok" | "warn" | "crit" }) {
  const R = 84, C = 2 * Math.PI * R, arc = C * 0.75; // 270° gauge
  return (
    <div className="relative size-60">
      <svg viewBox="0 0 200 200" className="size-full -rotate-[225deg]">
        <circle cx="100" cy="100" r={R} fill="none" strokeWidth="14" strokeLinecap="round" className="stroke-secondary" strokeDasharray={`${arc} ${C}`} />
        <circle cx="100" cy="100" r={R} fill="none" strokeWidth="14" strokeLinecap="round" stroke={`var(--${level})`} strokeDasharray={`${(arc * Math.min(pct, 100)) / 100} ${C}`} className="transition-all duration-700" />
      </svg>
      <div className="absolute inset-0 grid place-content-center text-center">
        <div className="num text-4xl font-semibold tracking-tight">{bytes(free, 0)}</div>
        <div className="text-muted-foreground text-sm">available</div>
        <div className="num text-muted-foreground mt-1 text-xs">{pct.toFixed(0)}% used</div>
      </div>
    </div>
  );
}

const RANGES = [7, 30, 90] as const;

function Trend({ settings, available }: { settings: Settings | null; available: boolean }) {
  const [days, setDays] = useState<(typeof RANGES)[number]>(30);
  const [h, setH] = useState<UsageHistory | null>(null);
  const load = useCallback(() => { api.usageHistory(days).then(setH).catch(() => setH({ samples: [], forecast: null })); }, [days]);
  useEffect(load, [load]);
  useEffect(() => { window.addEventListener("focus", load); return () => window.removeEventListener("focus", load); }, [load]);
  const f = h?.forecast;
  return (
    <Card>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <CardTitle>Usage over time</CardTitle>
          <CardDescription className="mt-1">
            {!available ? "History is unavailable (the local database could not be opened)."
              : !f ? "Tidy needs about two days of samples before it can estimate a trend."
              : f.days_to_full !== null ? <>Growing about <b className="text-foreground num">{bytes(f.bytes_per_day)}/day</b>. At this rate the disk is full <b className="text-foreground">{inDays(f.days_to_full)}</b>.</>
              : <>Stable or shrinking over the last {Math.round(f.span_days)} days (<span className="num">{f.bytes_per_day >= 0 ? "+" : "−"}{bytes(Math.abs(f.bytes_per_day))}/day</span>).</>}
          </CardDescription>
        </div>
        <div className="flex shrink-0 gap-1" role="group" aria-label="Time range">
          {RANGES.map((r) => <Button key={r} size="sm" variant={days === r ? "secondary" : "ghost"} aria-pressed={days === r} onClick={() => setDays(r)}>{r}d</Button>)}
        </div>
      </div>
      {f?.days_to_full != null && f.days_to_full < 30 && <Badge variant="crit" className="self-start"><TrendingUp className="size-3" />Full {inDays(f.days_to_full)}</Badge>}
      <UsageChart samples={h?.samples ?? []} warnPct={settings?.warn_pct ?? 80} critPct={settings?.critical_pct ?? 90} />
    </Card>
  );
}

function WhereSpaceGoes({ onOpen }: { onOpen: (path: string) => void }) {
  const [b, setB] = useState<SpaceBreakdown | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.space(false).then(setB).catch(() => {}); }, []);
  const measure = async () => { setBusy(true); try { setB(await api.space(true)); } catch (e) { toast.error("Couldn't measure", { description: String(e) }); } finally { setBusy(false); } };
  return (
    <Card>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <CardTitle>Where your space goes</CardTitle>
          <CardDescription className="mt-1">Known space users that macOS lumps into “System Data” or hides in your Library.{b?.measured_at ? ` Measured ${ago(b.measured_at)}.` : ""}</CardDescription>
        </div>
        <Button size="sm" variant="outline" onClick={measure} disabled={busy}>{busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}{b?.measured_at ? "Re-measure" : "Measure"}</Button>
      </div>
      {busy && !b?.hogs.length && <div className="space-y-2">{Array.from({ length: 4 }).map((_, i) => <div key={i} className="bg-muted/60 h-10 animate-pulse rounded-md" />)}</div>}
      {b && b.hogs.length > 0 && <SpaceList hogs={b.hogs} onOpen={(h: Hog) => h.path && onOpen(h.path)} />}
      {b && !b.measured_at && !busy && <div className="text-muted-foreground text-sm">Not measured yet. Measuring reads folder sizes only and can take a minute on large disks. It looks at your Photos library, iCloud Drive and some apps' data, so macOS may ask for permission first; declined locations are marked and skipped.</div>}
      <div className="text-muted-foreground border-t pt-3 text-xs leading-relaxed">
        <b className="text-foreground font-medium">About purgeable space and snapshots.</b> macOS counts some space as “purgeable” (iCloud copies, caches, Time Machine local snapshots) and frees it automatically when needed, so it is not shown here as reclaimable.
        {b && <> This Mac has <b className="text-foreground">{b.local_snapshots}</b> Time Machine local snapshot{b.local_snapshots === 1 ? "" : "s"}; you can remove them in Tools.</>}
      </div>
    </Card>
  );
}

export default function Dashboard({ ov, settings, onScan, onExplore, onExplorePath }: { ov: Overview | null; settings: Settings | null; onScan: () => void; onExplore: () => void; onExplorePath: (p: string) => void }) {
  const d = ov?.disk;
  const level = !d || !settings ? "ok" : d.used_pct >= settings.critical_pct ? "crit" : d.used_pct >= settings.warn_pct ? "warn" : "ok";
  return (
    <div className="pb-10">
      <PageHeader title="Overview" subtitle="Your Mac's storage at a glance" />
      <div className="space-y-5 px-8">
        {ov && !ov.full_disk_access && (
          <Card className="border-warn/40 bg-warn/10 flex-row items-center gap-4">
            <ShieldAlert className="text-warn size-5 shrink-0" />
            <div className="flex-1 text-sm"><div className="font-medium">Grant Full Disk Access for complete results</div>
              <div className="text-muted-foreground">Mail, Messages, Safari and iOS backups are invisible without it. Tidy only reads; deletions stay inside your home folder.</div></div>
            <Button size="sm" variant="outline" onClick={api.openFda}>Open Settings</Button>
          </Card>
        )}
        <div className="grid grid-cols-[auto_1fr] gap-5">
          <Card className="items-center justify-center px-10 py-8">
            {d ? <Gauge pct={d.used_pct} free={d.free} level={level} /> : <div className="bg-secondary size-60 animate-pulse rounded-full" />}
            <Badge variant={level === "ok" ? "ok" : level === "warn" ? "warn" : "crit"}>{level === "ok" ? "Healthy" : level === "warn" ? "Getting full" : "Critically low"}</Badge>
          </Card>
          <div className="grid grid-rows-[auto_1fr] gap-5">
            <div className="grid grid-cols-3 gap-5">
              {[["Capacity", d ? bytes(d.total) : "–"], ["Used", d ? bytes(d.used) : "–"], ["Freed with Tidy", ov ? bytes(ov.lifetime_freed) : "–"]].map(([k, v]) => (
                <Card key={k} className="gap-1.5"><CardDescription>{k}</CardDescription><div className="num text-2xl font-semibold tracking-tight">{v}</div></Card>
              ))}
            </div>
            <Card className="justify-between">
              <CardHeader><CardTitle>Reclaim space</CardTitle><CardDescription>Smart Clean finds caches, logs, build data, stale dependencies, old installers and huge files — and explains each one before anything is removed.</CardDescription></CardHeader>
              <div className="flex gap-2">
                <Button variant="brand" size="lg" onClick={onScan}><Sparkles />Run Smart Scan<ArrowRight /></Button>
                <Button variant="outline" size="lg" onClick={onExplore}><FolderSearch />Explore storage</Button>
              </div>
            </Card>
          </div>
        </div>
        <Trend settings={settings} available={ov?.history_available ?? true} />
        <WhereSpaceGoes onOpen={onExplorePath} />
        <Card className={cn("flex-row items-center gap-4 text-sm")}>
          <div className="text-muted-foreground flex-1">Tidy lives in your menu bar and warns you at <span className="text-foreground font-medium">{settings?.warn_pct ?? 80}%</span> and <span className="text-foreground font-medium">{settings?.critical_pct ?? 90}%</span> full. {settings?.alerts_enabled ? "Alerts are on." : "Alerts are off."}</div>
        </Card>
      </div>
    </div>
  );
}
