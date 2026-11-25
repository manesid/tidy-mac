import * as D from "@radix-ui/react-alert-dialog";
import { cn } from "@/lib/utils";
import { buttonVariants } from "./button";

export const AlertDialog = D.Root;
export const AlertDialogTrigger = D.Trigger;
export const AlertDialogContent = ({ className, ...p }: React.ComponentProps<typeof D.Content>) => (
  <D.Portal>
    <D.Overlay className="fixed inset-0 z-50 bg-black/50 backdrop-blur-[2px] data-[state=open]:animate-in data-[state=open]:fade-in-0" />
    <D.Content className={cn("bg-card fixed top-1/2 left-1/2 z-50 grid w-full max-w-[28rem] -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl border p-6 shadow-2xl", className)} {...p} />
  </D.Portal>
);
export const AlertDialogTitle = ({ className, ...p }: React.ComponentProps<typeof D.Title>) => <D.Title className={cn("text-lg font-semibold", className)} {...p} />;
export const AlertDialogDescription = ({ className, ...p }: React.ComponentProps<typeof D.Description>) => <D.Description className={cn("text-muted-foreground text-sm", className)} {...p} />;
export const AlertDialogFooter = ({ className, ...p }: React.ComponentProps<"div">) => <div className={cn("flex justify-end gap-2", className)} {...p} />;
export const AlertDialogAction = ({ className, ...p }: React.ComponentProps<typeof D.Action>) => <D.Action className={cn(buttonVariants(), className)} {...p} />;
export const AlertDialogCancel = ({ className, ...p }: React.ComponentProps<typeof D.Cancel>) => <D.Cancel className={cn(buttonVariants({ variant: "outline" }), className)} {...p} />;
