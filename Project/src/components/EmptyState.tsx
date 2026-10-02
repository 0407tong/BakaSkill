import type * as React from "react";
import { cn } from "@/lib/cn";

export interface EmptyStateProps {
  icon: React.ReactNode;
  title: string;
  description?: React.ReactNode;
  /** 引导用户执行的下一步动作按钮 */
  action?: React.ReactNode;
  className?: string;
}

/**
 * 空态。用于「无 Skill」「无搜索结果」「未配置中央库」三种情形。
 * 空态必须给出下一步动作，而不是只描述"没有内容"。
 */
export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        // sh-plate：空态是"一整片只有文字"的区域，铺了背景图之后没有底就会看不清
        "sh-plate flex flex-1 flex-col items-center justify-center gap-3 p-10 text-center",
        className,
      )}
    >
      <div className="flex size-11 items-center justify-center rounded-full bg-bg-subtle text-fg-subtle [&_svg]:size-5.5">
        {icon}
      </div>
      <div className="max-w-md space-y-1">
        <h2 className="text-sm font-semibold text-fg">{title}</h2>
        {description ? (
          <div className="text-sm leading-relaxed text-fg-muted">
            {description}
          </div>
        ) : null}
      </div>
      {action ? <div className="mt-1 flex gap-2">{action}</div> : null}
    </div>
  );
}
