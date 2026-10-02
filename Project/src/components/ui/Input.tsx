import type * as React from "react";
import { cn } from "@/lib/cn";

export interface InputProps extends React.ComponentPropsWithRef<"input"> {
  /** 左侧图标（通常传 lucide 图标元素） */
  leading?: React.ReactNode;
  /** 右侧内容，例如清除按钮 */
  trailing?: React.ReactNode;
}

export function Input({
  className,
  leading,
  trailing,
  ref,
  ...props
}: InputProps) {
  const field = (
    <input
      ref={ref}
      className={cn(
        "h-8.5 w-full min-w-0 bg-transparent text-sm text-fg outline-none",
        "placeholder:text-fg-subtle",
        "disabled:cursor-not-allowed disabled:opacity-50",
        leading ? "pl-7.5" : "pl-2.5",
        trailing ? "pr-8" : "pr-2.5",
        className,
      )}
      {...props}
    />
  );

  if (!leading && !trailing) {
    return (
      <div className="relative w-full rounded-md border border-border bg-surface transition-colors focus-within:border-accent">
        {field}
      </div>
    );
  }

  return (
    <div className="relative w-full rounded-md border border-border bg-surface transition-colors focus-within:border-accent">
      {leading ? (
        <span className="pointer-events-none absolute top-1/2 left-2.5 flex -translate-y-1/2 text-fg-subtle [&_svg]:size-3.5">
          {leading}
        </span>
      ) : null}
      {field}
      {trailing ? (
        <span className="absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center">
          {trailing}
        </span>
      ) : null}
    </div>
  );
}
