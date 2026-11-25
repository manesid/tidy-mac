import * as ProgressPrimitive from "@radix-ui/react-progress";
import { cn } from "@/lib/utils";

export function Progress({ className, value, barClassName, ...p }: React.ComponentProps<typeof ProgressPrimitive.Root> & { barClassName?: string }) {
  return (
    <ProgressPrimitive.Root className={cn("bg-secondary relative h-2 w-full overflow-hidden rounded-full", className)} value={value} {...p}>
      <ProgressPrimitive.Indicator className={cn("h-full rounded-full bg-primary transition-all duration-500", barClassName)} style={{ width: `${value ?? 0}%` }} />
    </ProgressPrimitive.Root>
  );
}
