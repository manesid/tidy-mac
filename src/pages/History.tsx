import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, Loader2, RefreshCw } from "lucide-react";
import { api, type HistoryRow } from "@/lib/api";
import { bytes, cn, when } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardDescription } from "@/components/ui/card";
import { PageHeader } from "./Dashboard";

const SOURCE: Record<HistoryRow["source"], string> = { smart: "Smart Clean", quick: "Quick Clean", explorer: "Explorer", tool: "Tools" };
const DISPOSAL: Record<string, string> = { delete: "Deleted", trash: "Moved to Trash", emptytrash: "Emptied Trash", command: "Ran" };

/** Audit log: everything Tidy removed, trashed or ran, grouped by run. */
export default function History({ active }: { active: boolean }) {
  const [rows, setRows] = useState<HistoryRow[] | null>(null);
  const [loading, setLoading] = useState(false);
  const load = useCallback(async () => { setLoading(true); try { setRows(await api.history()); } catch { setRows([]); } finally { setLoading(false); } }, []);
  useEffect(() => { if (active) load(); }, [active, load]);

  const runs = useMemo(() => {
    const m = new Map<string, HistoryRow[]>();
    for (const r of rows ?? []) { const k = `${r.ts}:${r.source}`; m.set(k, [...(m.get(k) ?? []), r]); }
    return [...m.values()];
  }, [rows]);
  const freed = (rows ?? []).filter((r) => r.ok).reduce((a, r) => a + r.size, 0);

  return (
    <div className="pb-10">
      <PageHeader title="History" subtitle={rows?.length ? `${bytes(freed)} reclaimed across ${runs.length} run${runs.length === 1 ? "" : "s"} (last 12 months)` : "Every item Tidy removes is recorded here"}>
        <Button variant="outline" size="sm" onClick={load} disabled={loading}>{loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}Refresh</Button>
      </PageHeader>
      <div className="max-w-3xl space-y-4 px-8">
        {rows && !rows.length && <Card className="text-muted-foreground items-center py-16 text-center text-sm">Nothing cleaned yet. Runs from Smart Clean, Quick Clean, the Explorer and Tools appear here.</Card>}
        {!rows && <div className="bg-muted h-40 animate-pulse rounded-xl" />}
        {runs.map((run) => {
          const head = run[0];
          const ok = run.filter((r) => r.ok);
          const failed = run.length - ok.length;
          return (
            <Card key={`${head.ts}:${head.source}`} className="gap-3">
              <div className="flex items-center gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 text-sm font-medium">{SOURCE[head.source] ?? head.source}{failed > 0 && <Badge variant="warn">{failed} skipped</Badge>}</div>
                  <CardDescription className="text-xs">{when(head.ts)} · {ok.length} item{ok.length === 1 ? "" : "s"}</CardDescription>
                </div>
                <div className="num text-sm font-semibold">{bytes(ok.reduce((a, r) => a + r.size, 0))}</div>
              </div>
              <ul className="divide-y rounded-md border">
                {run.slice(0, 50).map((r) => (
                  <li key={r.id} className="flex items-center gap-3 px-3 py-1.5 text-xs">
                    {r.ok ? <CheckCircle2 className="text-ok size-3.5 shrink-0" aria-label="Done" /> : <AlertCircle className="text-warn size-3.5 shrink-0" aria-label="Skipped" />}
                    <span className={cn("min-w-0 flex-1 truncate font-mono select-text", !r.ok && "text-muted-foreground")} title={r.reason ? `${r.path} (${r.reason})` : r.path}>{r.path}</span>
                    <span className="text-muted-foreground shrink-0">{r.ok ? DISPOSAL[r.disposal] ?? r.disposal : r.reason}</span>
                    {r.size > 0 && <span className="num w-16 shrink-0 text-right">{bytes(r.size)}</span>}
                  </li>
                ))}
                {run.length > 50 && <li className="text-muted-foreground px-3 py-1.5 text-xs">and {run.length - 50} more</li>}
              </ul>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
