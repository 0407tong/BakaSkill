import { useMemo, useState } from "react";
import { Loader2 } from "lucide-react";

import { AgentTargetDialog } from "@/components/skill/AgentTargetDialog";
import { Switch } from "@/components/ui/Switch";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { useSetLinkEnabled } from "@/lib/queries";
import type { AgentDetection, LinkState, SkillSummary } from "@/types/ipc";

export interface SkillToggleProps {
  skill: SkillSummary;
  /** 若处于单 Agent 视角，只操作该 Agent */
  activeAgentId?: string | null;
  /** 单 Agent 视角下该 Skill 的当前链接状态 */
  activeAgentState?: LinkState;
  /** 所有已就绪的 Agent（总览视角下用于弹窗按需选择目标 Agent） */
  readyAgents?: readonly AgentDetection[];
  className?: string;
}

/**
 * Skill 的启停开关。
 *
 * - 选中单个 Agent 时：直接对该 Agent 一键启停；
 * - 总览「全部」视角且存在多个已就绪 Agent 时：点击弹出目标 Agent 选择框，
 *   由用户指定要启停的 Agent，不再强制全量操作。
 */
export function SkillToggle({
  skill,
  activeAgentId = null,
  activeAgentState,
  readyAgents = [],
  className,
}: SkillToggleProps) {
  const setEnabled = useSetLinkEnabled();
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerMode, setPickerMode] = useState<"enable" | "disable">("enable");

  const canToggle = skill.inCentralLibrary;
  const busy = setEnabled.isPending;

  const enabledAgentIds = useMemo(
    () =>
      new Set(
        skill.instances
          .filter((i) => i.kind === "managed")
          .map((i) => i.agentId),
      ),
    [skill.instances],
  );

  // 默认勾选：启用时勾「尚未启用的」，禁用时勾「已启用的」
  const initialSelectedIds = useMemo(() => {
    if (pickerMode === "enable") {
      const notYet = readyAgents
        .filter((a) => !enabledAgentIds.has(a.id))
        .map((a) => a.id);
      return notYet.length > 0 ? notYet : readyAgents.map((a) => a.id);
    }
    const active = readyAgents
      .filter((a) => enabledAgentIds.has(a.id))
      .map((a) => a.id);
    return active.length > 0 ? active : readyAgents.map((a) => a.id);
  }, [pickerMode, readyAgents, enabledAgentIds]);

  // 单 Agent 模式下的状态判定
  if (activeAgentId) {
    const isEnabled = activeAgentState === "valid";
    const toggleable =
      canToggle &&
      (activeAgentState === "valid" ||
        activeAgentState === "absent" ||
        activeAgentState === "dangling" ||
        !activeAgentState);
    const reason = !canToggle
      ? "尚未纳入中央库，请先在详情中纳入中央库"
      : !toggleable
        ? "该位置存在目录冲突或外部链接，请在映射矩阵中处理"
        : isEnabled
          ? "在当前 Agent 中禁用"
          : "在当前 Agent 中启用";

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
              checked={isEnabled}
              disabled={!toggleable}
              aria-label={reason}
              onCheckedChange={(checked) =>
                setEnabled.mutate({
                  skillDirName: skill.dirName,
                  agentIds: [activeAgentId],
                  enabled: checked,
                })
              }
            />
          )}
        </span>
      </Tooltip>
    );
  }

  // 总览「全部」模式
  const readyCount = readyAgents.length;
  const managed = skill.managedCount;
  const state: "all" | "partial" | "none" =
    managed === 0
      ? "none"
      : readyCount > 0 && managed >= readyCount
        ? "all"
        : "partial";

  const nextEnabled = state !== "all";
  const needPicker = readyCount > 1;

  const reason = !canToggle
    ? "尚未纳入中央库，请先在详情中纳入中央库"
    : needPicker
      ? nextEnabled
        ? "选择要启用的 Agent"
        : "选择要禁用的 Agent"
      : nextEnabled
        ? "启用到已就绪 Agent"
        : "禁用链接";

  const handleToggle = () => {
    if (!canToggle) return;
    if (needPicker) {
      setPickerMode(nextEnabled ? "enable" : "disable");
      setPickerOpen(true);
      return;
    }
    setEnabled.mutate({
      skillDirName: skill.dirName,
      agentIds: readyAgents.map((a) => a.id),
      enabled: nextEnabled,
    });
  };

  return (
    <>
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
              checked={state !== "none"}
              disabled={!canToggle}
              aria-label={reason}
              aria-checked={state === "partial" ? "mixed" : state === "all"}
              onCheckedChange={handleToggle}
              className={cn(state === "partial" && "opacity-65")}
            />
          )}
        </span>
      </Tooltip>

      {needPicker ? (
        <AgentTargetDialog
          open={pickerOpen}
          onOpenChange={setPickerOpen}
          title={
            pickerMode === "enable"
              ? `启用「${skill.name}」`
              : `禁用「${skill.name}」`
          }
          description={
            pickerMode === "enable"
              ? "选择要接入该 Skill 的 Agent，中央库文件只保留一份。"
              : "选择要取消接入的 Agent，仅移除链接、不影响中央库原文件。"
          }
          confirmLabel={pickerMode === "enable" ? "确认启用" : "确认禁用"}
          agents={readyAgents}
          initialSelectedIds={initialSelectedIds}
          enabledAgentIds={enabledAgentIds}
          busy={busy}
          onConfirm={(agentIds) => {
            setEnabled.mutate(
              {
                skillDirName: skill.dirName,
                agentIds,
                enabled: pickerMode === "enable",
              },
              { onSuccess: () => setPickerOpen(false) },
            );
          }}
        />
      ) : null}
    </>
  );
}
