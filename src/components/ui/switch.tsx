import * as SwitchPrimitive from "@radix-ui/react-switch";
import { cn } from "@/lib/utils";

export const Switch = ({ className, ...p }: React.ComponentProps<typeof SwitchPrimitive.Root>) => (
  <SwitchPrimitive.Root className={cn("peer data-[state=checked]:bg-primary data-[state=unchecked]:bg-input inline-flex h-[1.25rem] w-9 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-50", className)} {...p}>
    <SwitchPrimitive.Thumb className="bg-background pointer-events-none block size-4 rounded-full ring-0 transition-transform data-[state=checked]:translate-x-[calc(100%-2px)] data-[state=unchecked]:translate-x-0" />
  </SwitchPrimitive.Root>
);
