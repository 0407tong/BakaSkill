import {
  AlertTriangle,
  Database,
  FileText,
  FolderTree,
  Loader2,
  PackagePlus,
  PanelRightClose,
  Trash2,
} from "lucide-react";

import { cn } from "@/lib/cn";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { Suspense, lazy, useState } from "react";

import { EmptyState } from "@/components/EmptyState";
import { INSTANCE_KIND_META } from "@/components/skill/SkillBadges";
import { UninstallDialog } from "@/components/skill/UninstallDialog";
import { useAdoptSkill, useConfig, useSkillsScan } from "@/lib/queries";
import { useSkillStore } from "@/store/skillStore";
import { useViewStore } from "@/store/viewStore";
import type { ScannedInstance, SkillSummary } from "@/types/ipc";

/**
 * Monaco 按需加载。
 *
 * 全量 monaco 打包后主 chunk 约 4.5 MB，若随首屏一起加载会直接拖垮
 * 「1000 条列表首屏 < 1s」的性能目标。这里改成用户真正选中某个 Skill、
 * 需要看内容时才加载编辑器代码。
 */
const SkillEditor = lazy(() =>
  import("@/components/skill/SkillEditor").then((m) => ({
    default: m.SkillEditor,
  })),
);

/**
 * 右侧 Skill 详情面板。
 *
 * **链接透明化的关键落点**：这里必须显示每个 Agent 的**真实路径**与链接目标，
 * 而不只是 Skill 名称。参考项目的链接机制不透明，用户无法确认
 * "这个目录到底是链接还是副本"——本面板就是回答这个问题的界面。
 */
export function DetailPanel() {
  const closeDetail = useViewStore((s) => s.closeDetail);
  const selectedId = useSkillStore((s) => s.selectedId);
  const { data: config } = useConfig();
  const scan = useSkillsScan(config?.centralLibraryPath ?? null);

  const skill = selectedId
    ? scan.data?.skills.find((s) => s.id === selectedId)
    : undefined;

  return (
    <section
      className="sh-frost flex h-full min-w-0 flex-col border-l border-border bg-surface"
      aria-label="Skill 详情"
    >
      <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="size-4 shrink-0 text-fg-subtle" />
          <span className="truncate text-sm font-medium text-fg">
            {skill?.name ?? "详情"}
          </span>
        </div>
        <Tooltip label="关闭面板">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={closeDetail}
            aria-label="关闭详情面板"
          >
            <PanelRightClose className="size-4" />
          </Button>
        </Tooltip>
      </header>

      {skill ? (
        <SkillDetail
          skill={skill}
          libraryPath={config?.centralLibraryPath ?? null}
        />
      ) : (
        <EmptyState
          icon={<FileText />}
          title="未选择 Skill"
          description="在左侧列表中选择一个 Skill，这里会显示它的元数据、各 Agent 中的真实路径与 SKILL.md 内容。"
        />
      )}
    </section>
  );
}

function SkillDetail({
  skill,
  libraryPath,
}: {
  skill: SkillSummary;
  libraryPath: string | null;
}) {
  // 优先选中央库中的实体用于编辑；否则退回第一个可读的实例
  const editableDir =
    skill.centralPath ??
    skill.instances.find((i) => !i.parseError && i.kind !== "dangling")?.path ??
    null;

  const [uninstallOpen, setUninstallOpen] = useState(false);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="space-y-3 p-3">
        <div>
          <div className="text-base font-semibold text-fg">{skill.name}</div>
          <div className="font-mono text-[11px] text-fg-subtle">{skill.id}</div>
        </div>

        <p className="text-sm leading-relaxed text-fg-muted">
          {skill.description ?? "（无描述）"}
        </p>

        {skill.tags.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {skill.tags.map((tag) => (
              <Badge key={tag} variant="default">
                {tag}
              </Badge>
            ))}
          </div>
        ) : null}

        {/* 中央库位置 */}
        <section className="space-y-1.5">
          <h3 className="flex items-center gap-1.5 text-xs font-medium text-fg-subtle">
            <Database className="size-3.5" />
            中央库
          </h3>
          {skill.inCentralLibrary && skill.centralPath ? (
            <code
              className="block truncate rounded-md border border-border bg-bg-subtle px-2 py-1.5 font-mono text-[11px] text-fg"
              title={skill.centralPath}
            >
              {skill.centralPath}
            </code>
          ) : (
            <p className="text-xs text-fg-muted">
              该 Skill 不在中央库中，仅存在于 Agent 目录里。
            </p>
          )}

          {/* 卸载只对中央库里的实体有意义：它把目录搬离中央库。
              外部副本不在这里处理（用户已明确：跳过并说明原因）。 */}
          {skill.inCentralLibrary && libraryPath ? (
            <Button
              size="sm"
              variant="ghost"
              className="text-danger hover:bg-danger-subtle"
              onClick={() => setUninstallOpen(true)}
            >
              <Trash2 className="size-3.5" />
              卸载（移出中央库）
            </Button>
          ) : null}
        </section>

        {/* 各 Agent 中的实例 —— 链接映射矩阵的核心信息 */}
        <section className="space-y-1.5">
          <h3 className="flex items-center gap-1.5 text-xs font-medium text-fg-subtle">
            <FolderTree className="size-3.5" />
            Agent 中的实例（{skill.instances.length}）
          </h3>
          {skill.instances.length === 0 ? (
            <p className="text-xs text-fg-muted">尚未在任何 Agent 中启用。</p>
          ) : (
            <ul className="space-y-1.5">
              {skill.instances.map((instance) => (
                <InstanceRow key={instance.path} instance={instance} />
              ))}
            </ul>
          )}
        </section>
      </div>

      {libraryPath ? (
        <UninstallDialog
          open={uninstallOpen}
          onOpenChange={setUninstallOpen}
          skills={[skill]}
          libraryPath={libraryPath}
        />
      ) : null}

      {editableDir ? (
        <div className="mt-2 flex min-h-0 flex-1 flex-col border-t border-border">
          {/* key 让切换 Skill 时编辑器重新挂载，未保存的草稿自然丢弃 */}
          <Suspense
            fallback={
              <div className="flex items-center justify-center gap-2 p-6 text-sm text-fg-muted">
                <Loader2 className="size-4 animate-spin" />
                正在加载编辑器…
              </div>
            }
          >
            <SkillEditor key={editableDir} dirPath={editableDir} />
          </Suspense>
        </div>
      ) : (
        <div className="border-t border-border p-3 text-xs text-fg-subtle">
          没有可读取的 SKILL.md。
        </div>
      )}
    </div>
  );
}

function InstanceRow({ instance }: { instance: ScannedInstance }) {
  const meta = INSTANCE_KIND_META[instance.kind];
  const adopt = useAdoptSkill();
  const [confirming, setConfirming] = useState(false);

  // 只有「外部」可以纳入：它尚未与中央库同名。
  // 「冲突」表示中央库已有同名 Skill，需要用户先决定改名还是替换，
  // 不能一键纳入——那会覆盖中央库里的现有版本。
  const canAdopt = instance.kind === "external" && !instance.parseError;

  return (
    <li className="rounded-md border border-border bg-bg-subtle p-2">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-xs font-medium text-fg">
          {instance.agentName}
        </span>
        <Badge variant={meta.variant} title={meta.hint}>
          {meta.label}
        </Badge>
      </div>

      <code
        className="mt-1 block truncate font-mono text-[10px] text-fg-subtle"
        title={instance.path}
      >
        {instance.path}
      </code>

      {instance.linkTarget ? (
        <div className="mt-0.5 flex items-start gap-1">
          <span className="shrink-0 font-mono text-[10px] text-fg-subtle">
            →
          </span>
          <code
            className={cn(
              "truncate font-mono text-[10px]",
              instance.kind === "dangling" ? "text-danger" : "text-fg-muted",
            )}
            title={instance.linkTarget}
          >
            {instance.linkTarget}
          </code>
        </div>
      ) : null}

      {instance.group ? (
        <div className="mt-0.5 text-[10px] text-fg-subtle">
          分组：{instance.group}
          {instance.isSystemGroup ? "（内置）" : ""}
        </div>
      ) : null}

      {instance.parseError ? (
        <div className="mt-1 flex items-start gap-1 text-[10px] text-warning">
          <AlertTriangle className="mt-0.5 size-3 shrink-0" />
          <span>{instance.parseError}</span>
        </div>
      ) : null}

      {canAdopt ? (
        <div className="mt-1.5">
          {confirming ? (
            /* 移动文件是不可逆操作，先确认再执行 */
            <div className="space-y-1.5 rounded-sm border border-border bg-surface p-2">
              <p className="text-[11px] leading-relaxed text-fg-muted">
                将把该目录**移动**到中央库，并在原位置建立链接。 之后它由
                BakaSkill 统一管理。
              </p>
              <div className="flex gap-1.5">
                <Button
                  size="sm"
                  disabled={adopt.isPending}
                  onClick={() =>
                    adopt.mutate(
                      {
                        agentId: instance.agentId,
                        dirName: instance.dirName,
                      },
                      { onSettled: () => setConfirming(false) },
                    )
                  }
                >
                  {adopt.isPending ? (
                    <Loader2 className="size-3 animate-spin" />
                  ) : null}
                  确认纳入
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={adopt.isPending}
                  onClick={() => setConfirming(false)}
                >
                  取消
                </Button>
              </div>
            </div>
          ) : (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setConfirming(true)}
            >
              <PackagePlus className="size-3.5" />
              纳入中央库
            </Button>
          )}
        </div>
      ) : null}
    </li>
  );
}
