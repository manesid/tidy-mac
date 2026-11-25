import * as SliderPrimitive from "@radix-ui/react-slider";
import { cn } from "@/lib/utils";

export const Slider = ({ className, ...p }: React.ComponentProps<typeof SliderPrimitive.Root>) => (
  <SliderPrimitive.Root className={cn("relative flex w-full touch-none items-center select-none", className)} {...p}>
    <SliderPrimitive.Track className="bg-secondary relative h-1.5 w-full grow overflow-hidden rounded-full">
      <SliderPrimitive.Range className="bg-primary absolute h-full" />
    </SliderPrimitive.Track>
    {(p.value ?? p.defaultValue ?? [0]).map((_, i) => (
      <SliderPrimitive.Thumb key={i} className="border-primary bg-background block size-4 shrink-0 rounded-full border shadow-sm outline-none focus-visible:ring-4 focus-visible:ring-ring/50" />
    ))}
  </SliderPrimitive.Root>
);
