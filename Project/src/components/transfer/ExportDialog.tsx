import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { FileArchive, FolderOutput, Loader2 } from "lucide-react";

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
import { useExportSkills } from "@/lib/queries";
import type { ExportFormat } from "@/types/ipc";

interface ExportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  libraryPath: string;
  /** 要导出的 Skill 目录名 */
  dirNames: string[];
}

/**
 * 导出对话框。
 *
 * 两种格式对应用户的两种目的，因此文案要说清区别而不是罗列参数：
 * - **ZIP**：自己留档、换台机器再导入（带 `manifest.json`）
 * - **纯目录**：直接把 Skill 递给不用 BakaSkill 的人
 */
export function ExportDialog({
  open: isOpen,
  onOpenChange,
  libraryPath,
  dirNames,
}: ExportDialogProps) {
  const exportMutation = useExportSkills();
  const [format, setFormat] = useState<ExportFormat>("zip");
  const [includeManifest, setIncludeManifest] = useState(true);
  const [dest, setDest] = useState<string | null>(null);

  const chooseDest = async () => {
    if (format === "zip") {
      const picked = await save({
        title: "导出为 ZIP",
        defaultPath: "bakaskill-skills.zip",
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (picked) setDest(picked);
      return;
    }
    const picked = await open({
      directory: true,
      multiple: false,
      title: "选择导出到的文件夹",
    });
    if (typeof picked === "string") setDest(picked);
  };

  const confirm = () => {
    if (!dest) return;
    exportMutation.mutate(
      { dirNames, dest, format, includeManifest, path: libraryPath },
      {
        onSuccess: () => {
          setDest(null);
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>导出 {dirNames.length} 个 Skill</DialogTitle>
          <DialogDescription>
            导出的目录结构是 <code>skills/&lt;目录名&gt;/…</code>
            ，与中央库同构—— 再导入回来（无论哪种格式）都能原样还原。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="grid grid-cols-2 gap-2">
            <FormatButton
              icon={<FileArchive />}
              title="打包成 ZIP"
              desc="自己留档、换台机器再导入"
              selected={format === "zip"}
              onClick={() => {
                setFormat("zip");
                setDest(null);
              }}
            />
            <FormatButton
              icon={<FolderOutput />}
              title="纯目录结构"
              desc="直接递给不用 BakaSkill 的人"
              selected={format === "folder"}
              onClick={() => {
                setFormat("folder");
                setDest(null);
              }}
            />
          </div>

          <label className="flex items-center gap-2 text-xs text-fg-muted">
            <input
              type="checkbox"
              checked={includeManifest}
              onChange={(e) => setIncludeManifest(e.target.checked)}
              className="size-3.5 accent-[var(--sh-accent)]"
            />
            包含 <code>manifest.json</code>（导出时间、BakaSkill 版本、Skill
            清单）
          </label>

          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={chooseDest}
              disabled={exportMutation.isPending}
            >
              {dest ? "重新选择位置" : "选择导出位置"}
            </Button>
            <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-subtle">
              {dest ?? "尚未选择"}
            </span>
          </div>
        </div>

        <DialogFooter>
          <Button
            disabled={!dest || exportMutation.isPending}
            onClick={confirm}
          >
            {exportMutation.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : null}
            导出
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function FormatButton({
  icon,
  title,
  desc,
  selected,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  desc: string;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-start gap-2 rounded-md border p-2.5 text-left transition-colors",
        selected
          ? "border-accent bg-accent-subtle"
          : "border-border bg-surface hover:border-border-strong",
      )}
    >
      <span
        className={cn(
          "mt-0.5 [&>svg]:size-4",
          selected ? "text-accent" : "text-fg-subtle",
        )}
      >
        {icon}
      </span>
      <span className="min-w-0">
        <span className="block text-xs font-medium text-fg">{title}</span>
        <span className="block text-[11px] text-fg-subtle">{desc}</span>
      </span>
    </button>
  );
}
