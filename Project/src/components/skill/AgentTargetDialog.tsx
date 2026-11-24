import { useMemo, useState } from "react";
import { Check, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/Button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/Dialog";
import { cn } from "@/lib/cn";
import type { AgentDetection } from "@/types/ipc";

export interface AgentTargetDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel: string;
  agents: readonly AgentDetection[];
  /** 初始勾选的 Agent id；不传则默认全选 */
  initialSelectedIds?: readonly string[];
  /** 每个 Agent 当前是否已启用（仅单条管理时用于展示状态） */
  enabledAgentIds?: ReadonlySet<string>;
  busy?: boolean;
  onConfirm: (agentIds: string[]) => void;
}

/**
 * 指定目标 Agent 对话框。
 *
 * 用于在「全部」总览视角下启用或禁用 Skill 时，让用户勾选要生效的
 * 具体 Agent，而不是一刀切操作所有已安装的 Agent。
 */
export function AgentTargetDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  agents,
  initialSelectedIds,
  enabledAgentIds,
  busy = false,
  onConfirm,
}: AgentTargetDialogProps) {
  const defaultIds = useMemo(
    () =>
      initialSelectedIds ? [...initialSelectedIds] : agents.map((a) => a.id),
    [initialSelectedIds, agents],
  );

  const [selected, setSelected] = useState<string[] | null>(null);
  const current = selected ?? defaultIds;

  const toggle = (id: string) => {
    setSelected(
      current.includes(id) ? current.filter((x) => x !== id) : [...current, id],
    );
  };

  const allSelected =
    agents.length > 0 && agents.every((a) => current.includes(a.id));

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setSelected(null);
        onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>

        <div className="space-y-2">
          <div className="flex items-center justify-between text-xs text-fg-subtle">
            <span>选择目标 Agent</span>
            <button
              type="button"
              className="text-accent hover:underline"
              onClick={() =>
                setSelected(allSelected ? [] : agents.map((a) => a.id))
              }
            >
              {allSelected ? "取消全选" : "全选"}
            </button>
          </div>

          <ul className="max-h-64 space-y-1.5 overflow-y-auto">
            {agents.map((agent) => {
              const checked = current.includes(agent.id);
              const isEnabled = enabledAgentIds?.has(agent.id) ?? false;
              return (
                <li key={agent.id}>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => toggle(agent.id)}
                    className={cn(
                      "flex w-full items-center gap-2.5 rounded-lg border px-3 py-2 text-left transition-colors",
                      checked
                        ? "border-accent bg-accent-subtle/60"
                        : "border-border bg-bg-subtle hover:border-border-strong hover:bg-surface-hover",
                    )}
                  >
                    <span
                      className={cn(
                        "flex size-4 shrink-0 items-center justify-center rounded-xs border transition-colors",
                        checked
                          ? "border-accent bg-accent text-accent-fg"
                          : "border-border-strong bg-surface",
                      )}
                    >
                      {checked ? <Check className="size-3" /> : null}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-xs font-medium text-fg">
                          {agent.displayName}
                        </span>
                        {enabledAgentIds ? (
                          <span
                            className={cn(
                              "text-[10px]",
                              isEnabled ? "text-success" : "text-fg-subtle",
                            )}
                          >
                            {isEnabled ? "已启用" : "未启用"}
                          </span>
                        ) : null}
                      </div>
                      {agent.skillDir ? (
                        <div
                          className="truncate font-mono text-[10px] text-fg-subtle"
                          title={agent.skillDir}
                        >
                          {agent.skillDir}
                        </div>
                      ) : null}
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>

        <DialogFooter>
          <Button
            disabled={busy || current.length === 0}
            onClick={() => onConfirm(current)}
          >
            {busy ? <Loader2 className="size-4 animate-spin" /> : null}
            {confirmLabel}
            {current.length > 0 ? ` (${current.length})` : ""}
          </Button>
          <Button
            variant="ghost"
            disabled={busy}
            onClick={() => {
              setSelected(null);
              onOpenChange(false);
            }}
          >
            取消
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
