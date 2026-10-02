import type * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cn } from "@/lib/cn";

export type ButtonVariant =
  "primary" | "secondary" | "ghost" | "danger" | "link";
export type ButtonSize = "sm" | "md" | "lg" | "icon" | "icon-sm";

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover shadow-sm",
  secondary:
    "bg-surface text-fg border border-border hover:bg-surface-hover hover:border-border-strong shadow-sm",
  ghost: "text-fg-muted hover:bg-surface-hover hover:text-fg",
  danger: "bg-danger text-white hover:opacity-90 shadow-sm",
  link: "text-accent underline-offset-4 hover:underline p-0 h-auto",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  sm: "h-7 px-2.5 text-xs gap-1.5 rounded-sm",
  md: "h-8.5 px-3 text-sm gap-2 rounded-md",
  lg: "h-10 px-4 text-sm gap-2 rounded-md",
  icon: "h-8.5 w-8.5 rounded-md",
  "icon-sm": "h-7 w-7 rounded-sm",
};

export interface ButtonProps extends React.ComponentPropsWithRef<"button"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** 将样式套用到子元素（Radix Slot），用于把按钮外观赋予 <a> 等元素 */
  asChild?: boolean;
}

export function Button({
  className,
  variant = "primary",
  size = "md",
  asChild = false,
  type,
  ref,
  ...props
}: ButtonProps) {
  const Comp = asChild ? Slot : "button";
  return (
    <Comp
      ref={ref}
      type={asChild ? undefined : (type ?? "button")}
      className={cn(
        "inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap",
        "transition-colors duration-100",
        "disabled:pointer-events-none disabled:opacity-45",
        VARIANT_CLASS[variant],
        SIZE_CLASS[size],
        className,
      )}
      {...props}
    />
  );
}
