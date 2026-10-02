import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  FileArchive,
  FileText,
  FolderInput,
  GitBranch,
  Loader2,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/Dialog";
import { cn } from "@/lib/cn";
import { useImportApply, useImportPreview } from "@/lib/queries";
import type {
  ImportAction,
  ImportCandidate,
  ImportPreview,
  ImportSource,
} from "@/types/ipc";

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  libraryPath: string;
  /**
   * 由外部**已经准备好**的预览（拖拽安装走这条路）。
   *
   * 提供时对话框跳过"选来源"那一步，直接进逐项决定。预览由调用方持有，
   * 因此关闭对话框时也要由调用方清掉它——否则下次以"选来源"方式打开时
   * 会拿上一次拖拽的预览冒充新预览。
   */
  presetPreview?: ImportPreview | null;
}

/**
 * 未显式选择时的默认动作。
 *
 * # 为什么默认「跳过」而不是「替换」
 *
 * 同名冲突的默认动作是**跳过**。用户点开这个对话框是想"把东西拿进来"，
 * 不是想"覆盖已有的东西"；默认替换会让一次随手点击就改动他已有的 Skill。
 * 要替换必须由他逐项（或显式批量）选。
 *
 * **算出来而不是预先把每个候选都写进 `actions`**：拖拽那条路的预览是外部传进来的，
 * 若还把默认值播种成 state，就得在"外部预览何时变化"这件事上做同步，
 * 而同步点正是最容易出错的地方。现算则没有状态可失同步。
 */
function defaultAction(candidate: ImportCandidate): ImportAction {
  return candidate.conflict ? "skip" : "overwrite";
}

/**
 * 导入向导：选来源 → 预览 → 逐项决定 → 落盘。
 *
 * 两条入口共用同一个对话框：点「导入」自己挑来源，或者由外部把**拖拽**产生的
 * 预览通过 `presetPreview` 递进来（后者跳过"选来源"，直接进逐项决定）。
 */
export function ImportDialog({
  open: isOpen,
  onOpenChange,
  libraryPath,
  presetPreview = null,
}: ImportDialogProps) {
  const previewMutation = useImportPreview();
  const applyMutation = useImportApply();

  // 自己发起的那条路产生的预览（"选来源"）；拖拽那条由调用方持有
  const [ownedPreview, setOwnedPreview] = useState<ImportPreview | null>(null);
  const preview = presetPreview ?? ownedPreview;

  /** 只记录用户**显式改过**的项，其余走 `defaultAction` */
  const [actions, setActions] = useState<Record<string, ImportAction>>({});
  const [gitUrl, setGitUrl] = useState("");
  const [showGit, setShowGit] = useState(false);

  const actionOf = (candidate: ImportCandidate): ImportAction =>
    actions[candidate.relativePath] ?? defaultAction(candidate);

  const reset = () => {
    setOwnedPreview(null);
    setActions({});
    setShowGit(false);
    setGitUrl("");
  };

  const load = async (source: ImportSource) => {
    try {
      const result = await previewMutation.mutateAsync({
        source,
        path: libraryPath,
      });
      setOwnedPreview(result);
      setActions({});
    } catch {
      // 错误提示由 mutation 的 onError 统一给出
    }
  };

  const pickFolder = async () => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: "选择要导入的文件夹",
    });
    if (typeof picked === "string")
      await load({ kind: "folder", path: picked });
  };

  const pickZip = async () => {
    const picked = await open({
      multiple: false,
      title: "选择要导入的 ZIP",
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (typeof picked === "string") await load({ kind: "zip", path: picked });
  };

  const pickSkillFile = async () => {
    const picked = await open({
      multiple: false,
      title: "选择 SKILL.md",
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (typeof picked === "string")
      await load({ kind: "skillFile", path: picked });
  };

  const confirm = () => {
    if (!preview) return;
    const decisions = preview.candidates.map((c) => ({
      relativePath: c.relativePath,
      action: actionOf(c),
    }));
    applyMutation.mutate(
      { token: preview.token, decisions, path: libraryPath },
      {
        onSuccess: () => {
          reset();
          onOpenChange(false);
        },
      },
    );
  };

  const setAllConflicts = (action: ImportAction) => {
    if (!preview) return;
    setActions((current) => {
      const next = { ...current };
      for (const c of preview.candidates) {
        if (c.conflict) next[c.relativePath] = action;
      }
      return next;
    });
  };

  const willImport = preview
    ? preview.candidates.filter((c) => actionOf(c) !== "skip").length
    : 0;
  const busy = previewMutation.isPending || applyMutation.isPending;

  // 冲突分两种来源，提示语不能混为一谈
  const libraryConflicts =
    preview?.candidates.filter((c) => c.conflictKind === "library").length ?? 0;
  const batchConflicts =
    preview?.candidates.filter((c) => c.conflictKind === "batch").length ?? 0;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(next) => {
        if (!next) reset();
        onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>导入 Skill</DialogTitle>
          <DialogDescription>
            从文件夹、ZIP、Git 仓库或单个 SKILL.md 把 Skill 纳入中央库。
            既有的同名 Skill 默认<strong>不动</strong>，要替换必须你自己选。
          </DialogDescription>
        </DialogHeader>

        {!preview ? (
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-2">
              <SourceButton
                icon={<FolderInput />}
                title="文件夹"
                desc="递归查找其中的 SKILL.md"
                disabled={busy}
                onClick={pickFolder}
              />
              <SourceButton
                icon={<FileArchive />}
                title="ZIP"
                desc="解压后按文件夹导入"
                disabled={busy}
                onClick={pickZip}
              />
              <SourceButton
                icon={<FileText />}
                title="单个 SKILL.md"
                desc="以文件名作为目录名"
                disabled={busy}
                onClick={pickSkillFile}
              />
              <SourceButton
                icon={<GitBranch />}
                title="Git 仓库"
                desc="克隆后按文件夹导入"
                disabled={busy}
                onClick={() => setShowGit((v) => !v)}
              />
            </div>

            {showGit ? (
              <div className="flex gap-2">
                <Input
                  value={gitUrl}
                  onChange={(e) => setGitUrl(e.target.value)}
                  placeholder="https://github.com/用户名/仓库.git"
                  aria-label="Git 仓库地址"
                  spellCheck={false}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && gitUrl.trim()) {
                      void load({ kind: "git", url: gitUrl.trim() });
                    }
                  }}
                />
                <Button
                  disabled={!gitUrl.trim() || busy}
                  onClick={() => void load({ kind: "git", url: gitUrl.trim() })}
                >
                  读取
                </Button>
              </div>
            ) : null}

            {previewMutation.isPending ? (
              <div className="flex items-center gap-2 text-xs text-fg-muted">
                <Loader2 className="size-3.5 animate-spin" />
                正在读取来源并检查冲突…
              </div>
            ) : null}

            <p className="text-[11px] leading-relaxed text-fg-subtle">
              ZIP 会被解压到系统临时目录；包含越界路径（<code>..</code>{" "}
              或绝对路径）的压缩包会被
              <strong>整体拒绝</strong>，不会解压出任何文件。
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {libraryConflicts + batchConflicts > 0 ? (
              <div className="flex flex-wrap items-center gap-2 rounded-md border border-warning/40 bg-warning-subtle p-2.5 text-xs">
                <AlertTriangle className="size-3.5 shrink-0 text-warning" />
                <span className="text-fg-muted">
                  {libraryConflicts > 0
                    ? `${libraryConflicts} 个与中央库里的 Skill 同名`
                    : null}
                  {libraryConflicts > 0 && batchConflicts > 0 ? "；" : null}
                  {batchConflicts > 0
                    ? `${batchConflicts} 个与本次导入的另一个条目重名`
                    : null}
                  。冲突项一律：
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => setAllConflicts("skip")}
                >
                  跳过
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => setAllConflicts("rename")}
                >
                  改名导入
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => setAllConflicts("overwrite")}
                >
                  替换
                </Button>
              </div>
            ) : null}

            <ul className="max-h-72 space-y-1.5 overflow-y-auto">
              {preview.candidates.map((candidate) => (
                <li
                  key={candidate.relativePath}
                  className="rounded-md border border-border bg-bg-subtle p-2"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate text-xs font-medium text-fg">
                        {candidate.name}
                        <span className="ml-1.5 font-mono text-[10px] text-fg-subtle">
                          {candidate.dirName}
                        </span>
                      </div>
                      {candidate.description ? (
                        <div className="truncate text-[11px] text-fg-subtle">
                          {candidate.description}
                        </div>
                      ) : null}
                    </div>

                    <div className="flex shrink-0 gap-1">
                      {(["skip", "rename", "overwrite"] as const).map(
                        (action) => (
                          <button
                            key={action}
                            type="button"
                            disabled={
                              busy ||
                              (!candidate.conflict &&
                                action !== "skip" &&
                                action !== "overwrite")
                            }
                            onClick={() =>
                              setActions((current) => ({
                                ...current,
                                [candidate.relativePath]: action,
                              }))
                            }
                            className={cn(
                              "rounded-sm border px-1.5 py-0.5 text-[10px] transition-colors",
                              actionOf(candidate) === action
                                ? "border-accent bg-accent-subtle text-accent"
                                : "border-border text-fg-subtle hover:text-fg",
                              !candidate.conflict &&
                                action === "rename" &&
                                "cursor-not-allowed opacity-40",
                            )}
                          >
                            {ACTION_LABEL[action]}
                          </button>
                        ),
                      )}
                    </div>
                  </div>

                  {candidate.conflict ? (
                    <div className="mt-1 text-[10px] text-warning">
                      {candidate.conflictKind === "batch"
                        ? `与本次导入的「${candidate.conflict.name}」重名——落盘时只能留下一个`
                        : `中央库里已有「${candidate.conflict.name}」`}
                      {candidate.conflictKind !== "batch" &&
                      candidate.conflict.description
                        ? `（${candidate.conflict.description}）`
                        : ""}
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>

            {/* 拖进来的东西里认不出、因而**整个没参与导入**的项。
                单独列出来而不是让它消失：用户拖了 3 样、只进来 2 样时必须说得出为什么。 */}
            {preview.rejected.length > 0 ? (
              <div className="rounded-md border border-warning/40 bg-warning-subtle p-2.5 text-[11px]">
                <div className="mb-1 flex items-center gap-1 font-medium text-warning">
                  <AlertTriangle className="size-3" />
                  {preview.rejected.length} 项没能识别，未参与本次导入
                </div>
                <ul className="space-y-0.5 text-fg-muted">
                  {preview.rejected.slice(0, 5).map((item) => (
                    <li key={item.path} className="truncate" title={item.path}>
                      {item.path}：{item.reason}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}

            {preview.invalid.length > 0 ? (
              <div className="rounded-md border border-danger/40 bg-danger-subtle p-2.5 text-[11px]">
                <div className="mb-1 flex items-center gap-1 font-medium text-danger">
                  <AlertTriangle className="size-3" />
                  {preview.invalid.length} 项无法导入
                </div>
                <ul className="space-y-0.5 text-fg-muted">
                  {preview.invalid.slice(0, 5).map((item) => (
                    <li key={item.relativePath} className="truncate">
                      {item.relativePath}：{item.reason}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}

            <DialogFooter>
              <Button disabled={busy || willImport === 0} onClick={confirm}>
                {applyMutation.isPending ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : null}
                导入 {willImport} 个
              </Button>
              {presetPreview ? (
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() => onOpenChange(false)}
                >
                  取消
                </Button>
              ) : (
                <Button variant="ghost" disabled={busy} onClick={reset}>
                  换个来源
                </Button>
              )}
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

const ACTION_LABEL: Record<ImportAction, string> = {
  skip: "跳过",
  rename: "改名",
  overwrite: "替换",
};

function SourceButton({
  icon,
  title,
  desc,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  desc: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex items-start gap-2 rounded-md border border-border bg-surface p-2.5 text-left",
        "transition-colors hover:border-border-strong hover:bg-surface-hover",
        disabled && "cursor-not-allowed opacity-50",
      )}
    >
      <span className="mt-0.5 text-fg-subtle [&>svg]:size-4">{icon}</span>
      <span className="min-w-0">
        <span className="block text-xs font-medium text-fg">{title}</span>
        <span className="block text-[11px] text-fg-subtle">{desc}</span>
      </span>
    </button>
  );
}
