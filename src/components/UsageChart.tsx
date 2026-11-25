import { useMemo, useState } from "react";
import type { Sample } from "@/lib/api";
import { bytes, when } from "@/lib/utils";

/** Used space over time as a single-series area chart, in % of capacity (axis zoomed to the data). */
export function UsageChart({ samples, warnPct, critPct, height = 140 }: { samples: Sample[]; warnPct: number; critPct: number; height?: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const W = 600, H = height, PAD = 4;
  // y axis is % of capacity, zoomed to the data (±5 points) so real change is visible.
  const { pts, lo, hi } = useMemo(() => {
    if (samples.length < 2) return { pts: [], lo: 0, hi: 100 };
    const pct = (s: Sample) => (s.used / (s.total || 1)) * 100;
    const ps = samples.map(pct);
    const lo = Math.max(0, Math.floor(Math.min(...ps) / 5) * 5 - 5), hi = Math.min(100, Math.ceil(Math.max(...ps) / 5) * 5 + 5);
    const t0 = samples[0].ts, t1 = samples[samples.length - 1].ts, span = Math.max(1, t1 - t0);
    return { lo, hi, pts: samples.map((s) => ({ s, x: PAD + ((s.ts - t0) / span) * (W - 2 * PAD), y: H - PAD - ((pct(s) - lo) / (hi - lo || 1)) * (H - 2 * PAD) })) };
  }, [samples, H]);

  if (pts.length < 2) {
    return (
      <div className="text-muted-foreground grid place-items-center rounded-lg border border-dashed text-center text-sm" style={{ height }}>
        <div className="max-w-xs px-4">Tidy records disk usage every 15 minutes while it runs. The chart appears after a few samples.</div>
      </div>
    );
  }
  const line = pts.map((p, i) => `${i ? "L" : "M"}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join("");
  const area = `${line}L${pts[pts.length - 1].x},${H - PAD}L${pts[0].x},${H - PAD}Z`;
  const yAt = (pct: number) => H - PAD - ((pct - lo) / (hi - lo || 1)) * (H - 2 * PAD);
  const h = hover !== null ? pts[hover] : null;

  return (
    <div className="relative pl-9">
      <div className="text-muted-foreground num pointer-events-none absolute inset-y-0 left-0 flex w-8 flex-col justify-between text-right text-[10px]" aria-hidden><span>{hi}%</span><span>{lo}%</span></div>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="block w-full" style={{ height }} role="img"
        aria-label={`Disk usage from ${when(samples[0].ts)} to ${when(samples[samples.length - 1].ts)}`}
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          const x = ((e.clientX - r.left) / r.width) * W;
          let best = 0;
          pts.forEach((p, i) => { if (Math.abs(p.x - x) < Math.abs(pts[best].x - x)) best = i; });
          setHover(best);
        }}>
        {[[warnPct, "var(--warn)"], [critPct, "var(--crit)"]].filter(([v]) => Number(v) > lo && Number(v) < hi).map(([v, c]) => (
          <line key={String(v)} x1={0} x2={W} y1={yAt(Number(v))} y2={yAt(Number(v))} stroke={String(c)} strokeOpacity={0.5} strokeDasharray="4 4" vectorEffect="non-scaling-stroke" />
        ))}
        <path d={area} fill="var(--brand-b)" fillOpacity={0.12} />
        <path d={line} fill="none" stroke="var(--brand-b)" strokeWidth={2} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
        {h && <line x1={h.x} x2={h.x} y1={0} y2={H} stroke="currentColor" strokeOpacity={0.25} vectorEffect="non-scaling-stroke" />}
      </svg>
      {h && (
        <div className="bg-card pointer-events-none border shadow-md absolute top-1 rounded-md px-2 py-1 text-xs whitespace-nowrap"
          style={{ left: `calc(2.25rem + (100% - 2.25rem) * ${h.x / W})`, transform: `translateX(${h.x > W / 2 ? "-105%" : "5%"})` }}>
          <div className="num font-medium">{bytes(h.s.used)} used · {((h.s.used / h.s.total) * 100).toFixed(1)}%</div>
          <div className="text-muted-foreground">{when(h.s.ts)}</div>
        </div>
      )}
    </div>
  );
}
