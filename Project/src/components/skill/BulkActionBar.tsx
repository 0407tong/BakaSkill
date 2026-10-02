import { useState } from "react";
import { Download, Loader2, PowerOff, Power, Trash2, X } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { ExportDialog } from "@/components/transfer/ExportDialog";
import { UninstallDialog } from "@/components/skill/UninstallDialog";
import { useConfig, useSetLinkEnabled } from "@/lib/queries";
import { useSkillStore } from "@/store/skillStore";
import type { SkillSummary } from "@/types/ipc";

export interface BulkActionBarProps {
  /** 当前可见（已过滤）的 Skill，用于「全选」的语义 */
  visible: readonly SkillSummary[];
  allSkills: readonly SkillSummary[];
}

/**
 * 批量操作栏：多选后一次性启用/禁用。
 *
 * # 部分失败的措辞
 *
 * `useSetLinkEnabled` 会按「成功 / 跳过 / 失败」三类分别给出 Toast。
 * 这是刻意的：批量操作里"3 个成功 1 个失败"如果只说一句"完成"，
 * 用户会以为全部生效，之后发现某个 Agent 里没有该 Skill 时会认为工具坏了。
 *
 * 同时，**已成功的项不会因为其他项失败而回滚**——回滚本身也是文件系统操作，
 * 可能再失败一次，把状态弄得更乱。失败项会被明确列出。
 */
export function BulkActionBar({ visible, allSkills }: BulkActionBarProps) {
  const selectedIds = useSkillStore((s) => s.selectedIds);
  const clearSelection = useSkillStore((s) => s.clearSelection);
  const setSelectedIds = useSkillStore((s) => s.setSelectedIds);
  const setEnabled = useSetLinkEnabled();
  const { data: config } = useConfig();
  const libraryPath = config?.centralLibraryPath ?? null;
  const [exportOpen, setExportOpen] = useState(false);
  const [uninstallOpen, setUninstallOpen] = useState(false);

  // 只有「在中央库中」的 Skill 才能建立链接
  const actionable = allSkills.filter(
    (s) => selectedIds.includes(s.id) && s.inCentralLibrary,
  );
  const notInLibrary = selectedIds.length - actionable.length;

  const hasSelection = selectedIds.length > 0;

  const run = async (enabled: boolean) => {
    // 逐个调用而不是一次传多个：后端每个 Agent 的结果互不影响，
    // 且这样能让进度可见、单个失败不阻塞其余。
    for (const skill of actionable) {
      await setEnabled.mutateAsync({
        skillDirName: skill.dirName,
        agentIds: [],
        enabled,
      });
    }
    clearSelection();
  };

  const allVisibleSelected =
    visible.length > 0 && visible.every((s) => selectedIds.includes(s.id));

  // 只能导出中央库里的 Skill：Agent 目录里的外部副本随时可能被 Agent 自己改动，
  // 拿它当"导出源"会给出一个来源不明、后续不可复现的包。
  const exportable = actionable.map((s) => s.dirName);

  return (
    <div className="sh-plate-accent flex shrink-0 flex-wrap items-center gap-2 border-b border-border bg-accent-subtle/40 px-3 py-2">
      {/* 这一栏**常驻**（不再"选了才出现"）：它同时是"这里能做什么"的说明，
          按钮时隐时现会让整页布局跳动，也让用户不知道有这些操作存在。 */}
      {hasSelection ? (
        <span className="text-xs font-medium text-fg">
          已选 {selectedIds.length} 个
        </span>
      ) : (
        <span className="text-xs text-fg-subtle">勾选 Skill 后可批量操作</span>
      )}

      {notInLibrary > 0 ? (
        <span className="text-[11px] text-warning">
          其中 {notInLibrary} 个不在中央库中，将被忽略
        </span>
      ) : null}

      <div className="ml-auto flex items-center gap-1.5">
        {/* 这一栏常驻，所以「全选」在没选任何东西时也要可用——
            从它开始选正是这一栏存在的意义 */}
        <Button
          size="sm"
          variant="primary"
          disabled={visible.length === 0}
          onClick={() =>
            setSelectedIds(allVisibleSelected ? [] : visible.map((s) => s.id))
          }
        >
          {allVisibleSelected ? "取消全选" : `全选当前 ${visible.length} 项`}
        </Button>

        {/* 下面四个统一为同一套样式（accent 底 + accent 前景）。
            用户明确要求它们看起来一致，不再用"实心/描边"区分主次。 */}
        <Button
          size="sm"
          variant="primary"
          disabled={
            !hasSelection || actionable.length === 0 || setEnabled.isPending
          }
          onClick={() => void run(true)}
        >
          {setEnabled.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Power className="size-3.5" />
          )}
          全部启用
        </Button>

        <Button
          size="sm"
          variant="primary"
          disabled={
            !hasSelection || actionable.length === 0 || setEnabled.isPending
          }
          onClick={() => void run(false)}
        >
          {setEnabled.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <PowerOff className="size-3.5" />
          )}
          全部禁用
        </Button>

        <Button
          size="sm"
          variant="primary"
          disabled={exportable.length === 0}
          onClick={() => setExportOpen(true)}
        >
          <Download className="size-3.5" />
          导出
        </Button>

        {/* 卸载与「全部禁用」是两件事：禁用只摘链接，卸载把目录也搬离中央库。
            **这一颗保持危险色**——它与上面四个不是同类操作，混成一样会让人
            把"取消接入"和"移出中央库"看成一回事。若你希望它也统一，说一声。 */}
        <Button
          size="sm"
          variant="danger"
          disabled={!hasSelection}
          onClick={() => setUninstallOpen(true)}
        >
          <Trash2 className="size-3.5" />
          全部卸载
        </Button>

        <Button
          variant="ghost"
          size="icon-sm"
          onClick={clearSelection}
          aria-label="清除选择"
        >
          <X className="size-3.5" />
        </Button>
      </div>

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
            // 把**选中的全部**给它（含不在中央库的）——确认框要能如实说出
            // "哪几个会被跳过"，而不是悄悄少列几个
            skills={allSkills.filter((s) => selectedIds.includes(s.id))}
            libraryPath={libraryPath}
          />
        </>
      ) : null}
    </div>
  );
}
