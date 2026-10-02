import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { cn } from "@/lib/cn";

export const TooltipProvider = TooltipPrimitive.Provider;

export interface TooltipProps {
  /** 提示内容。为空时不渲染 Tooltip，直接返回 children */
  label: React.ReactNode;
  children: React.ReactElement;
  side?: "top" | "right" | "bottom" | "left";
  align?: "start" | "center" | "end";
  /** 内容较长时用于分行的宽度上限 */
  className?: string;
}

/**
 * 轻量 Tooltip。用于展示链接目标路径等「悬停可见」的透明化信息。
 */
export function Tooltip({
  label,
  children,
  side = "bottom",
  align = "center",
  className,
}: TooltipProps) {
  if (label === null || label === undefined || label === "") return children;

  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          align={align}
          sideOffset={6}
          collisionPadding={8}
          className={cn(
            "z-50 max-w-96 rounded-md border border-border bg-surface px-2.5 py-1.5",
            "text-xs text-fg shadow-lg select-text",
            className,
          )}
        >
          {label}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
