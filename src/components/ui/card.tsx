import * as React from "react";
import { cn } from "@/lib/utils";

export const Card = ({ className, ...p }: React.ComponentProps<"div">) => (
  <div className={cn("bg-card text-card-foreground flex shrink-0 flex-col gap-4 rounded-xl border p-5 shadow-xs", className)} {...p} />
);
export const CardHeader = ({ className, ...p }: React.ComponentProps<"div">) => <div className={cn("flex flex-col gap-1", className)} {...p} />;
export const CardTitle = ({ className, ...p }: React.ComponentProps<"div">) => <div className={cn("text-sm font-semibold leading-none", className)} {...p} />;
export const CardDescription = ({ className, ...p }: React.ComponentProps<"div">) => <div className={cn("text-muted-foreground text-sm", className)} {...p} />;
