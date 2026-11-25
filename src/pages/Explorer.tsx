import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight, FileIcon, Folder, FolderOpen, FolderSearch, Home, HardDrive, Loader2, Lock, Trash2, ArrowUp, ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import { api, type Insights, type Kind, type Listing, type Overview } from "@/lib/api";
import { ago, bytes, cn, signedBytes, when } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardTitle } from "@/components/ui/card";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { PageHeader } from "./Dashboard";

const KIND_COLOR: Record<Kind, string> = {
  video: "bg-[oklch(0.65_0.2_20)]", images: "bg-[oklch(0.72_0.15_70)]", audio: "bg-[oklch(0.68_0.16_150)]", documents: "bg-[oklch(0.65_0.14_240)]",
  archives: "bg-[oklch(0.62_0.18_300)]", code: "bg-[oklch(0.7_0.12_190)]", apps: "bg-[oklch(0.6_0.2_265)]", other: "bg-muted-foreground/50",
};

/** Changes smaller than this are noise (logs, caches ticking) and are not highlighted. */
const DELTA_MIN = 10_000_000;
/** Rows rendered before "Show all": keeps huge folders responsive. */
const ROWS = 300;

export default function Explorer({ ov, onChanged, jump }: { ov: Overview | null; onChanged: () => void; jump?: { path: string; n: number } | null }) {
  const [path, setPath] = useState<string | null>(null);
  const [list, setList] = useState<Listing | null>(null);
  const [ins, setIns] = useState<Insights | null>(null);
  const [loading, setLoading] = useState(false);
  const [target, setTarget] = useState<{ path: string; name: string; size: number } | null>(null);
  const [showAll, setShowAll] = useState(false);
  const seq = useRef(0);

  const go = useCallback(async (p: string) => {
    const my = ++seq.current;
    await api.explorerCancel();
    setPath(p); setLoading(true); setList(null); setIns(null); setShowAll(false);
    try {
      const l = await api.list(p);
      if (my !== seq.current) return;
      setList(l); setLoading(false);
      const i = await api.insights(p); // second pass: file kinds + biggest files
      if (my === seq.current) setIns(i);
    } catch (e) { if (my === seq.current) { toast.error(String(e)); setLoading(false); } }
  }, []);

  // PRIVACY: never walk anything on mount. Reading ~/Desktop, ~/Documents, ~/Music, ~/Pictures... makes
  // macOS show consent prompts, so the first walk only starts from an explicit click (or a jump the user clicked).
  useEffect(() => { if (jump) go(jump.path); }, [jump, go]);

  const parent = path && path !== "/" ? path.replace(/\/[^/]+\/?$/, "") || "/" : null;
  const crumbs = path ? path.split("/").filter(Boolean) : [];
  const max = list?.entries[0]?.size || 1;

  const trash = async () => {
    if (!target) return;
    const t = target; setTarget(null);
    try {
      const r = await api.trash(t.path);
      if (r.failed.length) toast.error("Couldn't move it to the Trash", { description: r.failed[0].reason });
      else { toast.success(`Moved ${t.name} to Trash`, { description: bytes(t.size) }); onChanged(); }
    } catch (e) { toast.error("Couldn't move it to the Trash", { description: String(e) }); }
    if (path) go(path);
  };

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Storage Explorer" subtitle="See exactly what is taking space — anywhere you can read">
        <Button variant="outline" size="sm" onClick={() => ov && go(ov.home)}><Home />Home</Button>
        <Button variant="outline" size="sm" onClick={() => ov && go(ov.volume)}><HardDrive />Whole disk</Button>
        <Button variant="outline" size="sm" onClick={() => go("/Applications")}>Applications</Button>
      </PageHeader>
      <div className="flex items-center gap-1 px-8 pb-3 text-sm">
        <Button size="icon" variant="ghost" aria-label="Up one folder" disabled={!parent} onClick={() => parent && go(parent)}><ArrowUp /></Button>
        <button className="text-muted-foreground hover:text-foreground" onClick={() => go("/")}>/</button>
        {crumbs.map((c, i) => (
          <span key={i} className="flex items-center gap-1"><ChevronRight className="text-muted-foreground size-3" />
            <button className={cn("hover:text-foreground", i === crumbs.length - 1 ? "font-medium" : "text-muted-foreground")} onClick={() => go("/" + crumbs.slice(0, i + 1).join("/"))}>{c}</button></span>
        ))}
        {loading && <Loader2 className="text-muted-foreground ml-2 size-4 animate-spin" />}
        {list?.since && <span className="text-muted-foreground ml-auto shrink-0 text-xs">Changes since {when(list.since)}</span>}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[1fr_20rem] gap-5 px-8 pb-8">
        <Card className="min-h-0 gap-0 overflow-y-auto p-0">
          {!path && (
            <div className="mx-auto flex max-w-md flex-col items-center gap-3 px-6 py-14 text-center text-sm">
              <FolderSearch className="text-muted-foreground size-7" aria-hidden />
              <div className="font-medium">Choose where to look</div>
              <div className="text-muted-foreground">Tidy reads file sizes only; nothing changes until you choose Move to Trash. The first time it looks inside Desktop, Documents, Downloads, Music or Photos, macOS asks whether Tidy may read them. Saying no just leaves those folders out.</div>
              <div className="flex flex-wrap justify-center gap-2 pt-1">
                <Button size="sm" onClick={() => ov && go(ov.home)} disabled={!ov}><Home />Home folder</Button>
                <Button size="sm" variant="outline" onClick={() => go("/Applications")}>Applications</Button>
                <Button size="sm" variant="outline" onClick={() => ov && go(ov.volume)} disabled={!ov}><HardDrive />Whole disk</Button>
              </div>
            </div>
          )}
          {list?.unreadable && (
            <div className="flex flex-col items-center gap-3 py-16 text-center text-sm"><Lock className="text-muted-foreground size-6" /><div>macOS blocked access to this folder.</div>
              <Button size="sm" variant="outline" onClick={api.openFda}><ShieldAlert />Grant Full Disk Access</Button></div>
          )}
          {path && !list && <div className="space-y-px">{Array.from({ length: 8 }).map((_, i) => <div key={i} className="bg-muted/50 m-3 h-9 animate-pulse rounded-md" />)}</div>}
          {list?.entries.slice(0, showAll ? undefined : ROWS).map((e) => (
            <div key={e.path} className="group hover:bg-accent/50 relative flex items-center gap-3 border-b px-4 py-2.5 last:border-0">
              <div className="bg-brand-b/10 pointer-events-none absolute inset-y-0 left-0" style={{ width: `${(e.size / max) * 100}%` }} />
              <button className="relative flex min-w-0 flex-1 items-center gap-3 text-left" onDoubleClick={() => e.is_dir && go(e.path)} onClick={() => e.is_dir && go(e.path)}>
                {e.is_dir ? <Folder className="text-brand-b size-4 shrink-0" /> : <FileIcon className="text-muted-foreground size-4 shrink-0" />}
                <span className="truncate text-sm">{e.name}</span>
                {e.blocked && <span title="Some contents are unreadable without Full Disk Access"><Lock className="text-warn size-3.5" /></span>}
              </button>
              <span className="text-muted-foreground relative hidden text-xs group-hover:hidden lg:block">{ago(e.modified)}</span>
              <div className="relative hidden gap-1 group-focus-within:flex group-hover:flex">
                <Button size="icon" variant="ghost" title="Reveal in Finder" aria-label={`Reveal ${e.name} in Finder`} onClick={() => api.reveal(e.path)}><FolderOpen /></Button>
                <Button size="icon" variant="ghost" title="Move to Trash" aria-label={`Move ${e.name} to Trash`} onClick={() => setTarget({ path: e.path, name: e.name, size: e.size })}><Trash2 /></Button>
              </div>
              {list?.since && (
                <span className={cn("num relative w-16 shrink-0 text-right text-xs", (e.delta ?? 0) > 0 ? "text-warn" : "text-ok")} title={`Change since ${when(list.since)}`}>
                  {e.delta !== null && Math.abs(e.delta) >= DELTA_MIN ? signedBytes(e.delta) : ""}
                </span>
              )}
              <div className="num relative w-20 text-right text-sm font-medium">{bytes(e.size)}</div>
            </div>
          ))}
          {list && !showAll && list.entries.length > ROWS && (
            <button className="text-muted-foreground hover:text-foreground w-full py-3 text-sm" onClick={() => setShowAll(true)}>Show {list.entries.length - ROWS} more</button>
          )}
          {list && list.hidden > 0 && (showAll || list.entries.length <= ROWS) && (
            <div className="text-muted-foreground py-3 text-center text-xs">and {list.hidden.toLocaleString()} smaller items (included in the total)</div>
          )}
          {list && !list.unreadable && !list.entries.length && <div className="text-muted-foreground py-16 text-center text-sm">Empty folder</div>}
        </Card>
        <div className="flex min-h-0 flex-col gap-5 overflow-y-auto">
          <Card>
            <div><CardTitle>By file type</CardTitle><CardDescription className="mt-1">{ins ? `${bytes(ins.total)} in this folder` : list ? "Analyzing…" : path ? "" : "Pick a folder to see what kinds of files fill it."}</CardDescription></div>
            {ins && (<>
              <div className="flex h-3 overflow-hidden rounded-full">{ins.by_kind.map(([k, v]) => <div key={k} className={KIND_COLOR[k]} style={{ width: `${(v / (ins.total || 1)) * 100}%` }} />)}</div>
              <div className="space-y-2">{ins.by_kind.map(([k, v]) => (
                <div key={k} className="flex items-center gap-2 text-sm"><span className={cn("size-2.5 rounded-sm", KIND_COLOR[k])} /><span className="flex-1 capitalize">{k}</span><span className="num text-muted-foreground">{bytes(v)}</span></div>
              ))}</div>
              {ins.unreadable_dirs > 0 && <div className="text-warn text-xs">{ins.unreadable_dirs} locations unreadable — grant Full Disk Access for the full picture.</div>}
            </>)}
          </Card>
          <Card>
            <CardTitle>Biggest files</CardTitle>
            {!ins && path && <div className="bg-muted/50 h-24 animate-pulse rounded-md" />}
            {ins && !ins.top_files.length && <div className="text-muted-foreground text-sm">No files here.</div>}
            <div className="-mx-2 space-y-0.5">{ins?.top_files.map((f) => (
              <button key={f.path} onClick={() => api.reveal(f.path)} className="hover:bg-accent flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm" title={f.path}>
                <span className="min-w-0 flex-1 truncate">{f.name}</span><span className="num text-muted-foreground text-xs">{bytes(f.size)}</span>
              </button>
            ))}</div>
          </Card>
        </div>
      </div>
      <AlertDialog open={!!target} onOpenChange={(o) => !o && setTarget(null)}>
        <AlertDialogContent>
          <AlertDialogTitle>Move “{target?.name}” to Trash?</AlertDialogTitle>
          <AlertDialogDescription>{target && bytes(target.size)} will be moved to the Trash. You can restore it until you empty the Trash. Items outside your home folder are protected and can't be removed from here.</AlertDialogDescription>
          <AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction onClick={trash}>Move to Trash</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
