import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { Check, Minus } from "lucide-react";
import { cn } from "@/lib/utils";

export const Checkbox = ({ className, ...p }: React.ComponentProps<typeof CheckboxPrimitive.Root>) => (
  <CheckboxPrimitive.Root className={cn("peer border-input data-[state=checked]:bg-primary data-[state=indeterminate]:bg-primary data-[state=checked]:text-primary-foreground data-[state=indeterminate]:text-primary-foreground size-4 shrink-0 rounded-[4px] border shadow-xs transition-shadow outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-50", className)} {...p}>
    <CheckboxPrimitive.Indicator className="flex items-center justify-center text-current">
      {p.checked === "indeterminate" ? <Minus className="size-3.5" /> : <Check className="size-3.5" />}
    </CheckboxPrimitive.Indicator>
  </CheckboxPrimitive.Root>
);
