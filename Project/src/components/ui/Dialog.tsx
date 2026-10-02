import type * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/cn";

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

function Overlay({
  className,
  ...props
}: React.ComponentPropsWithRef<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      className={cn(
        "fixed inset-0 z-50 bg-black/45 backdrop-blur-[1px]",
        className,
      )}
      {...props}
    />
  );
}

export interface DialogContentProps extends React.ComponentPropsWithRef<
  typeof DialogPrimitive.Content
> {
  /** 是否显示右上角关闭按钮 */
  showClose?: boolean;
}

export function DialogContent({
  className,
  children,
  showClose = true,
  ...props
}: DialogContentProps) {
  return (
    <DialogPrimitive.Portal>
      <Overlay />
      <DialogPrimitive.Content
        className={cn(
          "fixed top-1/2 left-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2",
          "rounded-lg border border-border bg-surface p-5 shadow-lg",
          "max-h-[85vh] overflow-y-auto",
          className,
        )}
        {...props}
      >
        {children}
        {showClose ? (
          <DialogPrimitive.Close
            className={cn(
              "absolute top-3.5 right-3.5 rounded-sm p-1 text-fg-subtle",
              "transition-colors hover:bg-surface-hover hover:text-fg",
            )}
            aria-label="关闭"
          >
            <X className="size-4" />
          </DialogPrimitive.Close>
        ) : null}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export function DialogHeader({
  className,
  ...props
}: React.ComponentPropsWithRef<"div">) {
  return <div className={cn("mb-4 space-y-1 pr-8", className)} {...props} />;
}

export function DialogFooter({
  className,
  ...props
}: React.ComponentPropsWithRef<"div">) {
  return (
    <div
      className={cn(
        "mt-5 flex flex-row-reverse items-center gap-2 border-t border-border pt-4",
        className,
      )}
      {...props}
    />
  );
}

export function DialogTitle({
  className,
  ...props
}: React.ComponentPropsWithRef<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      className={cn("text-base font-semibold text-fg", className)}
      {...props}
    />
  );
}

export function DialogDescription({
  className,
  ...props
}: React.ComponentPropsWithRef<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      className={cn("text-sm leading-relaxed text-fg-muted", className)}
      {...props}
    />
  );
}
