import { AlertTriangle, Loader2, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/Button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/Dialog";
import { useUninstallSkills } from "@/lib/queries";
import type { SkillSummary } from "@/types/ipc";

export interface UninstallDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 待卸载的 Skill（来自扫描结果） */
  skills: readonly SkillSummary[];
  libraryPath: string;
}

/**
 * 卸载确认框。
 *
 * # 为什么必须逐条列出路径
 *
 * 这是本项目里**唯一一处会把目录搬离中央库**的操作，且常有"全部卸载"这样的
 * 批量用法。确认框的作用不是走个形式，而是让用户在按下按钮前能核对
 * "我要动的到底是哪几个目录"——名字可能重名，**绝对路径才是身份**。
 *
 * # 为什么要把"会被跳过的"也列出来
 *
 * 选中项里可能混着不在中央库的 Skill（只是某个 Agent 目录里的实体副本）。
 * 那些不会被卸载。若不明说，用户会以为全清了，回头发现某个还在，
 * 只会怀疑工具坏了。
 */
export function UninstallDialog({
  open,
  onOpenChange,
  skills,
  libraryPath,
}: UninstallDialogProps) {
  const uninstall = useUninstallSkills();

  const removable = skills.filter((s) => s.inCentralLibrary);
  const skippable = skills.filter((s) => !s.inCentralLibrary);

  /** 这个 Skill 在各 Agent 里已有的链接（kind === managed 才是我们建的） */
  const linksOf = (skill: SkillSummary) =>
    skill.instances.filter((i) => i.kind === "managed");

  const confirm = () => {
    uninstall.mutate(
      {
        dirNames: removable.map((s) => s.dirName),
        path: libraryPath,
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <div className="mb-4 space-y-2 pr-8">
          <DialogTitle>卸载 Skill</DialogTitle>
          <p className="text-sm leading-relaxed text-fg-muted">
            卸载会把 Skill 从中央库移除，并摘掉它在各 Agent 里的链接。
            目录不会直接销毁，而是
            <strong className="text-fg">送进 Windows 回收站</strong>
            ，需要时可以从那里还原。
          </p>
        </div>

        <div className="space-y-3">
          <div className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning-subtle p-2.5 text-xs leading-relaxed">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-warning" />
            <span className="text-fg-muted">
              即将卸载 <strong className="text-fg">{removable.length}</strong>{" "}
              个 Skill。请核对下面的路径——重名的 Skill 靠路径区分。
            </span>
          </div>

          <ul className="max-h-72 space-y-1.5 overflow-y-auto">
            {removable.map((skill) => {
              const links = linksOf(skill);
              return (
                <li
                  key={skill.id}
                  className="rounded-md border border-border bg-bg-subtle p-2"
                >
                  <div className="flex items-baseline gap-2">
                    <span className="min-w-0 truncate text-xs font-medium text-fg">
                      {skill.name}
                    </span>
                    <span className="shrink-0 font-mono text-[10px] text-fg-subtle">
                      {skill.dirName}
                    </span>
                  </div>

                  {skill.centralPath ? (
                    <code
                      className="mt-1 block truncate font-mono text-[10px] text-fg-muted"
                      title={skill.centralPath}
                    >
                      {skill.centralPath}
                    </code>
                  ) : null}

                  <div className="mt-1 text-[10px] text-fg-subtle">
                    {links.length > 0 ? (
                      <span className="text-warning">
                        将摘掉 {links.length} 个链接：
                        {links.map((l) => l.agentName).join("、")}
                      </span>
                    ) : (
                      <span>没有已接入的 Agent</span>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>

          {/* 做不到的那些必须说出来 */}
          {skippable.length > 0 ? (
            <div className="rounded-md border border-border bg-bg-subtle p-2.5 text-[11px]">
              <div className="mb-1 font-medium text-fg-muted">
                {skippable.length} 个不在中央库，将被跳过
              </div>
              <ul className="space-y-0.5 text-fg-subtle">
                {skippable.slice(0, 5).map((skill) => (
                  <li key={skill.id} className="truncate">
                    {skill.name}：只存在于 Agent 目录里，卸载不会动它
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>

        <DialogFooter>
          <Button
            variant="danger"
            disabled={removable.length === 0 || uninstall.isPending}
            onClick={confirm}
          >
            {uninstall.isPending ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Trash2 className="size-4" />
            )}
            卸载 {removable.length} 个
          </Button>
          <Button
            variant="ghost"
            disabled={uninstall.isPending}
            onClick={() => onOpenChange(false)}
          >
            取消
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
