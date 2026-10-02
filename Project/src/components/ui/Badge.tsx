import type * as React from "react";
import { cn } from "@/lib/cn";

export type BadgeVariant =
  "default" | "accent" | "success" | "warning" | "danger" | "info" | "outline";

const VARIANT_CLASS: Record<BadgeVariant, string> = {
  default: "bg-bg-subtle text-fg-muted border-transparent",
  accent: "bg-accent-subtle text-accent border-transparent",
  success: "bg-success-subtle text-success border-transparent",
  warning: "bg-warning-subtle text-warning border-transparent",
  danger: "bg-danger-subtle text-danger border-transparent",
  info: "bg-info-subtle text-info border-transparent",
  outline: "bg-transparent text-fg-muted border-border-strong",
};

export interface BadgeProps extends React.ComponentPropsWithRef<"span"> {
  variant?: BadgeVariant;
}

export function Badge({
  className,
  variant = "default",
  ref,
  ...props
}: BadgeProps) {
  return (
    <span
      ref={ref}
      className={cn(
        "inline-flex max-w-full items-center gap-1 rounded-xs border px-1.5 py-0.5",
        "text-[11px] leading-4 font-medium whitespace-nowrap",
        "[&_svg]:size-3 [&_svg]:shrink-0",
        VARIANT_CLASS[variant],
        className,
      )}
      {...props}
    />
  );
}
