import { ChevronRight, Lock } from "lucide-react";
import type { Hog } from "@/lib/api";
import { bytes, cn } from "@/lib/utils";

/** Ranked list of space hogs with proportional bars. `onOpen` drills into a location. */
export function SpaceList({ hogs, onOpen, compact = false }: { hogs: Hog[]; onOpen?: (h: Hog) => void; compact?: boolean }) {
  const max = hogs[0]?.size || 1;
  return (
    <ul className={cn("-mx-2", compact ? "space-y-0" : "space-y-0.5")}>
      {hogs.map((h) => {
        const body = (
          <>
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{h.label}</span>
              {h.blocked && <span title="Partly unreadable without Full Disk Access: size is a minimum"><Lock className="text-warn size-3.5" aria-label="Needs Full Disk Access" /></span>}
              <span className="num text-muted-foreground text-sm">{h.blocked && "≥ "}{bytes(h.size)}</span>
              {onOpen && h.path && <ChevronRight className="text-muted-foreground size-4" aria-hidden />}
            </div>
            <div className="bg-secondary mt-1.5 h-1 overflow-hidden rounded-full"><div className="bg-brand-b h-full rounded-full" style={{ width: `${Math.max(2, (h.size / max) * 100)}%` }} /></div>
            {!compact && <div className="text-muted-foreground mt-1.5 text-xs leading-snug">{h.note}</div>}
          </>
        );
        return (
          <li key={h.id}>
            {onOpen && h.path ? (
              <button className="hover:bg-accent focus-visible:ring-ring/50 w-full rounded-md px-2 py-2 text-left outline-none focus-visible:ring-[3px]" onClick={() => onOpen(h)} title={h.path}>{body}</button>
            ) : (
              <div className="px-2 py-2">{body}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
