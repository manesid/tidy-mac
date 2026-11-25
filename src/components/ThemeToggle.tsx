import { useEffect, useState } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import { getTheme, setTheme, type Theme } from "@/lib/theme";
import { cn } from "@/lib/utils";

const OPTIONS: { id: Theme; label: string; icon: typeof Sun }[] = [
  { id: "light", label: "Light", icon: Sun },
  { id: "system", label: "System", icon: Monitor },
  { id: "dark", label: "Dark", icon: Moon },
];

export function ThemeToggle() {
  const [theme, set] = useState<Theme>(getTheme);
  useEffect(() => {
    const sync = () => set(getTheme());
    window.addEventListener("tidy-theme", sync);
    return () => window.removeEventListener("tidy-theme", sync);
  }, []);
  return (
    <div role="radiogroup" aria-label="Appearance" className="bg-secondary flex rounded-lg p-0.5">
      {OPTIONS.map(({ id, label, icon: Icon }) => (
        <button key={id} role="radio" aria-checked={theme === id} aria-label={label} title={label} onClick={() => setTheme(id)}
          className={cn("flex h-7 flex-1 items-center justify-center rounded-md transition-colors outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
            theme === id ? "bg-card text-foreground shadow-xs" : "text-muted-foreground hover:text-foreground")}>
          <Icon className="size-3.5" />
        </button>
      ))}
    </div>
  );
}
