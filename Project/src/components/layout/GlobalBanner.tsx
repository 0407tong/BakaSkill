import type * as React from "react";
import { AlertTriangle, Info, X, XCircle } from "lucide-react";
import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";

export type BannerSeverity = "info" | "warning" | "error";

const SEVERITY_STYLE: Record<BannerSeverity, string> = {
  info: "bg-info-subtle text-info border-info/30",
  warning: "bg-warning-subtle text-warning border-warning/30",
  error: "bg-danger-subtle text-danger border-danger/30",
};

const SEVERITY_ICON: Record<BannerSeverity, React.ElementType> = {
  info: Info,
  warning: AlertTriangle,
  error: XCircle,
};

export interface GlobalBannerProps {
  severity: BannerSeverity;
  message: React.ReactNode;
  /** 恢复引导按钮等 */
  actions?: React.ReactNode;
  onDismiss?: () => void;
}

/**
 * 全局横幅。承载「中央库路径失效」「git 未安装」等需要用户介入的全局状态。
 *
 * 本组件只做展示：失效检测与恢复动作由调用方（如 `LibraryHealthBanner`）
 * 提供，通过 `message` / `actions` 传入。
 */
export function GlobalBanner({
  severity,
  message,
  actions,
  onDismiss,
}: GlobalBannerProps) {
  const Icon = SEVERITY_ICON[severity];

  return (
    <div
      role="status"
      className={cn(
        "flex shrink-0 items-center gap-2.5 border-b px-3 py-2 text-sm",
        SEVERITY_STYLE[severity],
      )}
    >
      <Icon className="size-4 shrink-0" />
      <div className="min-w-0 flex-1">{message}</div>
      {actions ? <div className="flex shrink-0 gap-2">{actions}</div> : null}
      {onDismiss ? (
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onDismiss}
          aria-label="关闭提示"
          className="shrink-0 hover:bg-black/5"
        >
          <X className="size-3.5" />
        </Button>
      ) : null}
    </div>
  );
}
