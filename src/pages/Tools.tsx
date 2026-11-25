import { useCallback, useEffect, useState } from "react";
import { Check, Copy, Loader2, RefreshCw, ShieldCheck, Terminal } from "lucide-react";
import { toast } from "sonner";
import { api, type Tool } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardTitle } from "@/components/ui/card";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogTitle } from "@/components/ui/alert-dialog";
import { PageHeader } from "./Dashboard";

const INFO: Record<Tool["id"], { title: string; what: string; action: string }> = {
  "tm-snapshots": {
    title: "Time Machine local snapshots",
    what: "Time Machine keeps snapshots on this disk between backups. macOS deletes them on its own when space runs low, so they show as purgeable. Delete one now if you need the space immediately. macOS asks for an administrator password.",
    action: "Delete snapshot",
  },
  simulators: {
    title: "Unavailable iOS simulators",
    what: "Simulator devices whose iOS runtime is no longer installed and can never boot again. Removed with Apple's own simctl.",
    action: "Remove unavailable simulators",
  },
  homebrew: {
    title: "Homebrew cleanup",
    what: "Removes old versions of installed packages and downloads older than 120 days (Homebrew's default). Current versions are never touched.",
    action: "Run brew cleanup",
  },
};

function CommandLine({ cmd }: { cmd: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="bg-muted/60 flex items-center gap-2 rounded-md border px-3 py-1.5">
      <Terminal className="text-muted-foreground size-3.5 shrink-0" aria-hidden />
      <code className="min-w-0 flex-1 truncate font-mono text-xs select-text" title={cmd}>{cmd}</code>
      <Button size="icon" variant="ghost" className="size-7" aria-label="Copy command"
        onClick={() => navigator.clipboard.writeText(cmd).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); })}>
        {copied ? <Check /> : <Copy />}
      </Button>
    </div>
  );
}

export default function Tools() {
  const [tools, setTools] = useState<Tool[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState<{ tool: Tool; arg?: string } | null>(null);
  const [running, setRunning] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try { setTools(await api.tools()); } catch (e) { toast.error(String(e)); } finally { setLoading(false); }
  }, []);
  useEffect(() => { load(); }, [load]);

  const run = async () => {
    if (!pending) return;
    const { tool, arg } = pending;
    setPending(null); setRunning(tool.id + (arg ?? ""));
    try { await api.toolRun(tool.id, arg); toast.success("Done", { description: INFO[tool.id].title }); }
    catch (e) { toast.error("Didn't complete", { description: String(e) }); }
    finally { setRunning(null); load(); }
  };
  const cmdFor = (t: Tool, arg?: string) => (arg ? t.command.replace("<date>", arg) : t.command);

  return (
    <div className="pb-10">
      <PageHeader title="Tools" subtitle="Space that only Apple's and your package manager's own tools can safely reclaim">
        <Button variant="outline" size="sm" onClick={load} disabled={loading}>{loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}Refresh</Button>
      </PageHeader>
      <div className="max-w-3xl space-y-5 px-8">
        <Card className="flex-row items-start gap-3 text-sm">
          <ShieldCheck className="text-ok mt-0.5 size-5 shrink-0" aria-hidden />
          <div className="text-muted-foreground">Each action runs the official command shown, exactly as written, only after you confirm. Tidy never uses sudo; when administrator rights are needed, macOS shows its own password dialog. Every run is recorded in History.</div>
        </Card>
        {!tools && <div className="space-y-5">{[0, 1, 2].map((i) => <div key={i} className="bg-muted h-36 animate-pulse rounded-xl" />)}</div>}
        {tools?.map((t) => {
          const info = INFO[t.id];
          const busy = running?.startsWith(t.id) ?? false;
          return (
            <Card key={t.id}>
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <CardTitle className="flex items-center gap-2">{info.title}{t.needs_admin && <Badge variant="outline">Admin password</Badge>}</CardTitle>
                  <CardDescription className="mt-1.5">{info.what}</CardDescription>
                </div>
                <div className="num shrink-0 text-right text-sm font-medium">{t.found}</div>
              </div>
              {t.id === "tm-snapshots" ? (
                t.choices.length > 0 && (
                  <ul className="divide-y rounded-md border" aria-label="Local snapshots">
                    {t.choices.map((d) => (
                      <li key={d} className="flex items-center gap-3 px-3 py-2 text-sm">
                        <span className="num flex-1">{d.slice(0, 10)} <span className="text-muted-foreground">{d.slice(11, 13)}:{d.slice(13, 15)}:{d.slice(15, 17)}</span></span>
                        <Button size="sm" variant="outline" disabled={!!running} onClick={() => setPending({ tool: t, arg: d })}>
                          {running === t.id + d ? <Loader2 className="animate-spin" /> : null}Delete…
                        </Button>
                      </li>
                    ))}
                  </ul>
                )
              ) : (
                <div className="flex items-center gap-3">
                  <div className="min-w-0 flex-1"><CommandLine cmd={t.command} /></div>
                  <Button variant="outline" disabled={!t.available || !!running || /^No |Nothing/.test(t.found)} onClick={() => setPending({ tool: t })}>
                    {busy && <Loader2 className="animate-spin" />}{info.action}…
                  </Button>
                </div>
              )}
              {t.id === "tm-snapshots" && <CommandLine cmd={t.command} />}
            </Card>
          );
        })}
      </div>
      <AlertDialog open={!!pending} onOpenChange={(o) => !o && setPending(null)}>
        <AlertDialogContent>
          <AlertDialogTitle>{pending && INFO[pending.tool.id].action}?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="space-y-3">
              <div>Tidy will run exactly this command{pending?.tool.needs_admin ? ", and macOS will ask for an administrator password" : ""}:</div>
              {pending && <CommandLine cmd={cmdFor(pending.tool, pending.arg)} />}
              <div>This can't be undone.</div>
            </div>
          </AlertDialogDescription>
          <AlertDialogFooter><AlertDialogCancel>Cancel</AlertDialogCancel><AlertDialogAction onClick={run}>Run</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
