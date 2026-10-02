import { Loader2 } from "lucide-react";

import { cn } from "@/lib/cn";
import { Switch } from "@/components/ui/Switch";
import { Tooltip } from "@/components/ui/Tooltip";
import { useSetLinkEnabled } from "@/lib/queries";
import type { SkillSummary } from "@/types/ipc";

export interface SkillToggleProps {
  skill: SkillSummary;
  /** 限定只操作这些 Agent；不传表示全部已检测的 Agent */
  agentIds?: string[];
  className?: string;
}

/**
 * Skill 的启停开关（三态）。
 *
 * - **全部启用**：开关为开
 * - **部分启用**：开关为开但半透明，点击会「全部关闭」
 * - **未启用**：开关为关，点击会「全部启用」
 *
 * 主开关的语义是"对该 Skill 在所有已检测 Agent 上批量操作"。
 * 按 Agent 的细分开关在详情面板中单独提供。
 *
 * 不使用乐观更新：链接操作能否成功取决于磁盘真实状态
 * （目标被占用、Agent 锁定目录等），预测错误会让用户看到
 * "界面显示成功但实际没生效"。这里如实等待结果。
 */
export function SkillToggle({
  skill,
  agentIds = [],
  className,
}: SkillToggleProps) {
  const setEnabled = useSetLinkEnabled();

  // 可切换的前提：中央库中有实体（才能建立指向它的链接）
  const canToggle = skill.inCentralLibrary;

  const managed = skill.managedCount;
  const total = skill.instances.length;
  const state: "all" | "partial" | "none" =
    managed === 0 ? "none" : total > 0 && managed === total ? "all" : "partial";

  const nextEnabled = state !== "all";

  const reason = !canToggle
    ? "该 Skill 不在中央库中，无法建立链接。请先把它纳入中央库。"
    : nextEnabled
      ? "启用：在 Agent 技能目录中建立指向中央库的链接"
      : "禁用：删除链接（中央库中的 Skill 文件不受影响）";

  const busy = setEnabled.isPending;

  return (
    <Tooltip label={busy ? "正在处理…" : reason}>
      <span
        className={cn("inline-flex shrink-0 items-center", className)}
        onClick={(e) => e.stopPropagation()}
      >
        {busy ? (
          <Loader2 className="size-4 animate-spin text-fg-subtle" />
        ) : (
          <Switch
            size="sm"
            checked={state === "all"}
            disabled={!canToggle}
            aria-label={reason}
            aria-checked={state === "partial" ? "mixed" : state === "all"}
            onCheckedChange={() =>
              setEnabled.mutate({
                skillDirName: skill.dirName,
                agentIds,
                enabled: nextEnabled,
              })
            }
            className={cn(
              // 部分启用用半透明表达"介于两者之间"，而不是简单等同于关闭
              state === "partial" && "opacity-60",
            )}
          />
        )}
      </span>
    </Tooltip>
  );
}
