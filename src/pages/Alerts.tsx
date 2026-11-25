import { toast } from "sonner";
import { api, type Disk, type Settings, type TitleMode } from "@/lib/api";
import { bytes, cn } from "@/lib/utils";
import { Card, CardDescription, CardTitle } from "@/components/ui/card";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { PageHeader } from "./Dashboard";

const INTERVALS = [5, 15, 30, 60];
const TITLES: { id: TitleMode; label: string }[] = [{ id: "free", label: "Free space" }, { id: "percent", label: "Percent used" }, { id: "icon", label: "Icon only" }];
// True 0-100% scale, shared by the usage bar and the threshold slider. Limits mirror the backend (`Settings::sanitized`).
const WARN_MIN = 50, WARN_MAX = 98, CRIT_MAX = 99;
export const clampLevels = (w: number, c: number): [number, number] => {
  const warn = Math.min(Math.max(w, WARN_MIN), WARN_MAX);
  return [warn, Math.min(Math.max(c, warn + 1), CRIT_MAX)];
};
const pctOf = (v: number) => `${Math.min(Math.max(v, 0), 100)}%`;
const THUMB = 16; // slider thumb width in px; Radix insets the thumb centre by half of it at each end

function UsageScale({ pct, warn, crit }: { pct: number; warn: number; crit: number }) {
  return (
    <div className="mt-2 space-y-1.5">
      {/* inset by half a thumb so zone edges sit exactly under the slider thumbs */}
      <div style={{ paddingInline: THUMB / 2 }}>
        <div className="bg-secondary relative h-3 overflow-hidden rounded-full">
          <div className="bg-ok/30 absolute inset-y-0 left-0" style={{ width: pctOf(warn) }} />
          <div className="bg-warn/40 absolute inset-y-0" style={{ left: pctOf(warn), width: `${crit - warn}%` }} />
          <div className="bg-crit/40 absolute inset-y-0 right-0" style={{ left: pctOf(crit) }} />
          <div className="bg-foreground absolute inset-y-0 w-1 -translate-x-1/2 rounded-full" style={{ left: pctOf(pct) }} />
        </div>
      </div>
      <div className="text-muted-foreground num flex justify-between text-xs" style={{ paddingInline: THUMB / 2 }}><span>0%</span><span>50%</span><span>100%</span></div>
    </div>
  );
}

export default function Alerts({ disk, settings, onChange }: { disk: Disk | null; settings: Settings | null; onChange: (s: Settings) => void }) {
  if (!settings) return null;
  const save = async (patch: Partial<Settings>) => {
    const next = { ...settings, ...patch };
    onChange(next); // optimistic
    try { onChange(await api.settingsSet(next)); } catch (e) { toast.error(String(e)); onChange(settings); }
  };
  const pct = disk?.used_pct ?? 0;
  return (
    <div className="pb-10">
      <PageHeader title="Alerts" subtitle="Get warned from the menu bar before your disk fills up" />
      <div className="max-w-2xl space-y-5 px-8">
        <Card>
          <div className="flex items-center justify-between">
            <div><CardTitle>Storage alerts</CardTitle><CardDescription className="mt-1">Native notifications when usage crosses a threshold. One alert per crossing; repeats at most daily.</CardDescription></div>
            <Switch checked={settings.alerts_enabled} onCheckedChange={(v) => save({ alerts_enabled: v })} />
          </div>
          <UsageScale pct={pct} warn={settings.warn_pct} crit={settings.critical_pct} />
          <div className="num text-sm">Now <b>{pct.toFixed(1)}%</b> used{disk && <span className="text-muted-foreground"> · {bytes(disk.free)} free of {bytes(disk.total)}</span>}</div>
        </Card>
        <Card className={cn(!settings.alerts_enabled && "opacity-50 pointer-events-none")}>
          <div className="space-y-3">
            <div className="flex justify-between text-sm">
              <span className="font-medium">Thresholds</span>
              <span className="num text-muted-foreground"><span className="text-warn">Warn {settings.warn_pct}%</span> · <span className="text-crit">Critical {settings.critical_pct}%</span></span>
            </div>
            <Slider min={0} max={100} step={1} value={[settings.warn_pct, settings.critical_pct]}
              onValueChange={([w, c]) => { const [cw, cc] = clampLevels(w, c); onChange({ ...settings, warn_pct: cw, critical_pct: cc }); }}
              onValueCommit={([w, c]) => { const [cw, cc] = clampLevels(w, c); save({ warn_pct: cw, critical_pct: cc }); }} />
            <div className="text-muted-foreground flex justify-between text-xs" style={{ paddingInline: THUMB / 2 }}><span>0%</span><span>50%</span><span>100%</span></div>
            {disk && (
              <div className="text-muted-foreground num grid grid-cols-2 gap-3 text-xs">
                <div><span className="text-warn font-medium">Warning</span> at {settings.warn_pct}% used = when free space falls below {bytes(disk.total * (1 - settings.warn_pct / 100))}</div>
                <div><span className="text-crit font-medium">Critical</span> at {settings.critical_pct}% used = below {bytes(disk.total * (1 - settings.critical_pct / 100))}</div>
              </div>
            )}
          </div>
          <div className="space-y-3">
            <div className="text-sm font-medium">Check every</div>
            <div className="flex gap-2">{INTERVALS.map((m) => <Button key={m} size="sm" variant={settings.check_minutes === m ? "default" : "outline"} onClick={() => save({ check_minutes: m })}>{m} min</Button>)}</div>
          </div>
        </Card>
        <Card className="flex-row items-center justify-between">
          <div><CardTitle>Launch at login</CardTitle><CardDescription className="mt-1">Keep Tidy in the menu bar so alerts work after a restart.</CardDescription></div>
          <Switch checked={settings.launch_at_login} onCheckedChange={(v) => save({ launch_at_login: v })} />
        </Card>
        <Card>
          <div><CardTitle>Menu bar</CardTitle><CardDescription className="mt-1">Click the Tidy icon for status, Quick Clean and snooze; right-click for the menu. The icon shows a ring when storage is getting full and a dot when it is critical.</CardDescription></div>
          <div className="flex items-center justify-between gap-4">
            <div className="text-sm font-medium">Show next to the icon</div>
            <div className="flex gap-1" role="radiogroup" aria-label="Menu bar title">
              {TITLES.map((t) => <Button key={t.id} size="sm" role="radio" aria-checked={settings.tray_title === t.id} variant={settings.tray_title === t.id ? "default" : "outline"} onClick={() => save({ tray_title: t.id })}>{t.label}</Button>)}
            </div>
          </div>
          <div className="flex items-center justify-between gap-4">
            <label htmlFor="menu-bar-only"><div className="text-sm font-medium">Menu bar only</div><div className="text-muted-foreground text-sm">Hide the Dock icon. Open this window from the menu bar icon.</div></label>
            <Switch id="menu-bar-only" checked={settings.menu_bar_only} onCheckedChange={(v) => save({ menu_bar_only: v })} />
          </div>
        </Card>
      </div>
    </div>
  );
}
