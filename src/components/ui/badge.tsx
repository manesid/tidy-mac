import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "@/lib/utils";

const badgeVariants = cva("inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium whitespace-nowrap", {
  variants: {
    variant: {
      default: "border-transparent bg-secondary text-secondary-foreground",
      outline: "text-muted-foreground",
      ok: "border-transparent bg-ok/15 text-ok",
      warn: "border-transparent bg-warn/20 text-[oklch(0.5_0.12_70)] dark:text-warn",
      crit: "border-transparent bg-crit/15 text-crit",
    },
  },
  defaultVariants: { variant: "default" },
});
export const Badge = ({ className, variant, ...p }: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) => (
  <span className={cn(badgeVariants({ variant }), className)} {...p} />
);
