import { useEffect, useMemo, useState } from "react";
import { ChevronRight, Loader2, Search, ShieldAlert, Trash2, Undo2 } from "lucide-react";
import { toast } from "sonner";
import { api, useEvent, type Category } from "@/lib/api";
import { bytes, cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Card } from "@/components/ui/card";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { PageHeader } from "./Dashboard";

const GROUPS = ["System", "Developer", "AI", "Files"];
/** Items rendered per open category before "Show all" (caches can have thousands of children). */
const ROWS = 200;

export default function Cleaner({ scanTick, onFreed }: { scanTick: number; onFreed: () => void }) {
  const [cats, setCats] = useState<Category[]>([]);
  const [scanning, setScanning] = useState(false);
  const [current, setCurrent] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [full, setFull] = useState<Set<string>>(new Set()); // categories showing every item
  const [confirm, setConfirm] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [scanned, setScanned] = useState(false);

  const scan = () => {
    setCats([]); setSelected(new Set()); setFull(new Set()); setScanning(true); setScanned(true);
    api.scanStart();
  };
  // Only an explicit request (scanTick > 0) starts a scan; never on mount (StrictMode re-runs mount effects).
  useEffect(() => { if (scanTick > 0) scan(); /* eslint-disable-next-line */ }, [scanTick]);

  useEvent<string>("scan:progress", setCurrent);
  useEvent<Category>("scan:category", (c) => {
    setCats((p) => [...p.filter((x) => x.id !== c.id), c]);
    // Safe items start selected; Review items never do.
    if (c.safety === "safe" && !c.blocked) setSelected((s) => new Set([...s, ...c.items.map((i) => i.id)]));
  });
  useEvent<void>("scan:done", () => { setScanning(false); setCurrent(null); });

  const sizeOf = useMemo(() => new Map(cats.flatMap((c) => c.items.map((i) => [i.id, { size: i.size, disposal: c.disposal }] as const))), [cats]);
  const totals = useMemo(() => {
    let all = 0, perm = 0, trash = 0;
    for (const id of selected) { const x = sizeOf.get(id); if (!x) continue; all += x.size; x.disposal === "trash" ? (trash += x.size) : (perm += x.size); }
    return { all, perm, trash };
  }, [selected, sizeOf]);
  const reclaimable = cats.reduce((a, c) => a + c.total, 0);

  const toggleCat = (c: Category, on: boolean) => setSelected((s) => { const n = new Set(s); c.items.forEach((i) => (on ? n.add(i.id) : n.delete(i.id))); return n; });
  const toggleItem = (id: string) => setSelected((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });

  const doClean = async () => {
    setConfirm(false); setCleaning(true);
    const ids = new Set(selected);
    try {
      const r = await api.clean([...ids]);
      toast.success(`Freed ${bytes(r.freed)}`, { description: `${r.removed} item${r.removed === 1 ? "" : "s"} cleaned${r.failed.length ? ` · ${r.failed.length} skipped` : ""}` });
      if (r.failed.length) console.warn("skipped", r.failed);
      setSelected(new Set());
      setCats((p) => p.map((c) => { const left = c.items.filter((i) => !ids.has(i.id)); return { ...c, items: left, total: left.reduce((a, i) => a + i.size, 0) }; }).filter((c) => c.items.length || c.blocked));
      onFreed();
    } catch (e) { toast.error(String(e)); } finally { setCleaning(false); }
  };

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Smart Clean" subtitle={scanning ? `Scanning${current ? ` · ${cats.length ? "" : ""}${current}` : "…"}` : scanned ? `${bytes(reclaimable)} can be reclaimed` : "Find everything that's safe to remove"}>
        {scanning ? <Button variant="outline" onClick={() => { api.scanCancel(); setScanning(false); }}><Loader2 className="animate-spin" />Stop</Button>
          : <Button variant={scanned ? "outline" : "brand"} onClick={scan}><Search />{scanned ? "Rescan" : "Scan"}</Button>}
      </PageHeader>
      <div className="flex-1 space-y-6 overflow-y-auto px-8 pb-28">
        {!scanned && (
          <Card className="text-muted-foreground items-center gap-2 py-14 text-center text-sm">
            <div className="text-foreground font-medium">Nothing is deleted during a scan. You review and confirm every item.</div>
            <div className="max-w-lg">To find large files, old installers and stale projects, the scan reads your home folder. The first time, macOS may ask whether Tidy can access Desktop, Documents or Downloads. Tidy only reads sizes; if you decline, those folders are simply skipped.</div>
          </Card>
        )}
        {GROUPS.map((g) => {
          const list = cats.filter((c) => c.group === g).sort((a, b) => b.total - a.total);
          if (!list.length) return null;
          return (
            <section key={g} className="space-y-2">
              <h2 className="text-muted-foreground px-1 text-xs font-medium tracking-wide uppercase">{g}</h2>
              <Card className="gap-0 divide-y p-0">
                {list.map((c) => {
                  const n = c.items.filter((i) => selected.has(i.id)).length;
                  const state = n === 0 ? false : n === c.items.length ? true : "indeterminate";
                  const isOpen = open.has(c.id);
                  return (
                    <div key={c.id}>
                      <div className="flex items-center gap-3 px-4 py-3">
                        <Checkbox checked={state} disabled={!c.items.length} onCheckedChange={(v) => toggleCat(c, v === true)} />
                        <button className="flex min-w-0 flex-1 items-center gap-3 text-left" onClick={() => setOpen((s) => { const x = new Set(s); x.has(c.id) ? x.delete(c.id) : x.add(c.id); return x; })} disabled={!c.items.length}>
                          <ChevronRight className={cn("text-muted-foreground size-4 shrink-0 transition-transform", isOpen && "rotate-90", !c.items.length && "opacity-0")} />
                          <div className="min-w-0">
                            <div className="flex items-center gap-2 text-sm font-medium">{c.name}
                              <Badge variant={c.safety === "safe" ? "ok" : "warn"}>{c.safety === "safe" ? "Safe" : "Review"}</Badge>
                              {c.disposal === "trash" && <Badge variant="outline"><Undo2 className="size-3" />To Trash</Badge>}
                            </div>
                            <div className="text-muted-foreground truncate text-xs" title={c.description}>{c.blocked ? "Needs Full Disk Access to read this" : c.items.length ? c.description : "Nothing found"}</div>
                          </div>
                        </button>
                        {c.blocked && <Button size="sm" variant="outline" onClick={api.openFda}><ShieldAlert />Grant access</Button>}
                        <div className="num w-20 text-right text-sm font-medium">{c.total ? bytes(c.total) : "–"}</div>
                      </div>
                      {isOpen && (
                        <div className="bg-muted/40 divide-y border-t">
                          {c.items.slice(0, full.has(c.id) ? undefined : ROWS).map((i) => (
                            <div key={i.id} className="flex items-center gap-3 py-2 pr-4 pl-14">
                              <Checkbox checked={selected.has(i.id)} onCheckedChange={() => toggleItem(i.id)} />
                              <button className="min-w-0 flex-1 text-left" onClick={() => api.reveal(i.path)} title="Reveal in Finder">
                                <div className="truncate text-sm">{i.name}</div><div className="text-muted-foreground truncate text-xs">{i.path}</div>
                              </button>
                              <div className="num text-muted-foreground text-sm">{bytes(i.size)}</div>
                            </div>
                          ))}
                          {!full.has(c.id) && c.items.length > ROWS && (
                            <button className="text-muted-foreground hover:text-foreground w-full py-2 pl-14 text-left text-xs" onClick={() => setFull((s) => new Set(s).add(c.id))}>Show {c.items.length - ROWS} more</button>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </Card>
            </section>
          );
        })}
      </div>
      {scanned && (
        <div className="bg-background/80 absolute inset-x-0 bottom-0 flex items-center justify-between border-t px-8 py-4 backdrop-blur-xl">
          <div className="text-sm"><span className="num text-lg font-semibold">{bytes(totals.all)}</span> <span className="text-muted-foreground">selected · {selected.size} item{selected.size === 1 ? "" : "s"}</span></div>
          <Button variant="brand" size="lg" disabled={!selected.size || cleaning || scanning} onClick={() => setConfirm(true)}>
            {cleaning ? <Loader2 className="animate-spin" /> : <Trash2 />}Clean {bytes(totals.all)}
          </Button>
        </div>
      )}
      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogTitle>Clean {bytes(totals.all)}?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="space-y-2">
              {totals.perm > 0 && <div className="flex justify-between"><span>Deleted permanently (regenerable)</span><b className="num text-foreground">{bytes(totals.perm)}</b></div>}
              {totals.trash > 0 && <div className="flex justify-between"><span>Moved to Trash (restorable)</span><b className="num text-foreground">{bytes(totals.trash)}</b></div>}
              <div>Permanently deleted items cannot be recovered. Anything outside your home folder is never touched.</div>
            </div>
          </AlertDialogDescription>
          <AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction onClick={doClean}>Clean</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
