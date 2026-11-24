import { useState } from "react";
import { Download, Loader2, Power, PowerOff, Trash2, X } from "lucide-react";

import { AgentTargetDialog } from "@/components/skill/AgentTargetDialog";
import { UninstallDialog } from "@/components/skill/UninstallDialog";
import { ExportDialog } from "@/components/transfer/ExportDialog";
import { Button } from "@/components/ui/Button";
import { useConfig, useSetLinkEnabled } from "@/lib/queries";
import { useSkillStore } from "@/store/skillStore";
import type { AgentDetection, SkillSummary } from "@/types/ipc";

export interface BulkActionBarProps {
  /** 当前可见（已过滤）的 Skill，用于「全选」的语义 */
  visible: readonly SkillSummary[];
  allSkills: readonly SkillSummary[];
  /** 当前选中的单个 Agent（null 表示总览「全部」视角） */
  activeAgent?: AgentDetection | null;
  /** 所有已就绪 Agent（总览视角下供批量操作选择目标） */
  readyAgents?: readonly AgentDetection[];
}

/**
 * 批量操作栏：多选后一次性启用、禁用、导出或卸载。
 *
 * 在指定 Agent 视角下直接作用于该 Agent；在「全部」视角且存在多个
 * 已就绪 Agent 时，弹出目标选择器让用户指定要操作的 Agent。
 */
export function BulkActionBar({
  visible,
  allSkills,
  activeAgent = null,
  readyAgents = [],
}: BulkActionBarProps) {
  const selectedIds = useSkillStore((s) => s.selectedIds);
  const clearSelection = useSkillStore((s) => s.clearSelection);
  const setSelectedIds = useSkillStore((s) => s.setSelectedIds);
  const setEnabled = useSetLinkEnabled();
  const { data: config } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;

  const [exportOpen, setExportOpen] = useState(false);
  const [uninstallOpen, setUninstallOpen] = useState(false);
  const [targetPicker, setTargetPicker] = useState<"enable" | "disable" | null>(
    null,
  );
  const [batchBusy, setBatchBusy] = useState(false);

  const actionable = allSkills.filter(
    (s) => selectedIds.includes(s.id) && s.inCentralLibrary,
  );
  const notInLibrary = selectedIds.length - actionable.length;
  const hasSelection = selectedIds.length > 0;

  const runForAgents = async (enabled: boolean, agentIds: string[]) => {
    setBatchBusy(true);
    try {
      for (const skill of actionable) {
        await setEnabled.mutateAsync({
          skillDirName: skill.dirName,
          agentIds,
          enabled,
        });
      }
      clearSelection();
      setTargetPicker(null);
    } finally {
      setBatchBusy(false);
    }
  };

  const triggerEnable = (enabled: boolean) => {
    if (activeAgent) {
      void runForAgents(enabled, [activeAgent.id]);
      return;
    }
    if (readyAgents.length > 1) {
      setTargetPicker(enabled ? "enable" : "disable");
      return;
    }
    void runForAgents(
      enabled,
      readyAgents.map((a) => a.id),
    );
  };

  const allVisibleSelected =
    visible.length > 0 && visible.every((s) => selectedIds.includes(s.id));

  const exportable = actionable.map((s) => s.dirName);
  const busy = batchBusy || setEnabled.isPending;
  const agentDisabled =
    Boolean(activeAgent) && activeAgent?.status !== "detected";

  return (
    <div
      className={
        hasSelection
          ? "sh-plate-accent flex shrink-0 flex-wrap items-center gap-2 border-b border-accent/25 bg-accent-subtle/55 px-4 py-2 transition-colors"
          : "sh-plate flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-bg-subtle/60 px-4 py-1.5 transition-colors"
      }
    >
      {hasSelection ? (
        <span className="text-xs font-medium text-accent">
          已选 {selectedIds.length} 个 Skill
        </span>
      ) : (
        <span className="text-xs text-fg-subtle">
          {activeAgent
            ? `勾选 Skill 可批量管理 ${activeAgent.displayName}`
            : "勾选 Skill 后可批量启用、导出或卸载"}
        </span>
      )}

      {notInLibrary > 0 ? (
        <span className="text-[11px] text-warning">
          {notInLibrary} 个未纳管将被忽略
        </span>
      ) : null}

      <div className="ml-auto flex items-center gap-1.5">
        <Button
          size="sm"
          variant="secondary"
          disabled={visible.length === 0}
          onClick={() =>
            setSelectedIds(allVisibleSelected ? [] : visible.map((s) => s.id))
          }
        >
          {allVisibleSelected ? "取消全选" : `全选 ${visible.length} 项`}
        </Button>

        <Button
          size="sm"
          variant={hasSelection ? "primary" : "secondary"}
          disabled={
            !hasSelection || actionable.length === 0 || busy || agentDisabled
          }
          onClick={() => triggerEnable(true)}
        >
          {busy ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Power className="size-3.5" />
          )}
          启用
        </Button>

        <Button
          size="sm"
          variant="secondary"
          disabled={
            !hasSelection || actionable.length === 0 || busy || agentDisabled
          }
          onClick={() => triggerEnable(false)}
        >
          <PowerOff className="size-3.5" />
          禁用
        </Button>

        <Button
          size="sm"
          variant="secondary"
          disabled={exportable.length === 0}
          onClick={() => setExportOpen(true)}
        >
          <Download className="size-3.5" />
          导出
        </Button>

        <Button
          size="sm"
          variant={hasSelection ? "danger" : "ghost"}
          disabled={!hasSelection}
          onClick={() => setUninstallOpen(true)}
        >
          <Trash2 className="size-3.5" />
          卸载
        </Button>

        {hasSelection ? (
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={clearSelection}
            aria-label="清除选择"
          >
            <X className="size-3.5" />
          </Button>
        ) : null}
      </div>

      {targetPicker ? (
        <AgentTargetDialog
          open={Boolean(targetPicker)}
          onOpenChange={(open) => {
            if (!open) setTargetPicker(null);
          }}
          title={
            targetPicker === "enable"
              ? `批量启用 ${actionable.length} 个 Skill`
              : `批量禁用 ${actionable.length} 个 Skill`
          }
          description={
            targetPicker === "enable"
              ? "选择要启用到的目标 Agent。"
              : "选择要从中禁用的目标 Agent，中央库原文件不受影响。"
          }
          confirmLabel={targetPicker === "enable" ? "确认启用" : "确认禁用"}
          agents={readyAgents}
          busy={busy}
          onConfirm={(agentIds) =>
            void runForAgents(targetPicker === "enable", agentIds)
          }
        />
      ) : null}

      {libraryPath ? (
        <>
          <ExportDialog
            open={exportOpen}
            onOpenChange={setExportOpen}
            libraryPath={libraryPath}
            dirNames={exportable}
          />
          <UninstallDialog
            open={uninstallOpen}
            onOpenChange={setUninstallOpen}
            skills={allSkills.filter((s) => selectedIds.includes(s.id))}
            libraryPath={libraryPath}
          />
        </>
      ) : null}
    </div>
  );
}
