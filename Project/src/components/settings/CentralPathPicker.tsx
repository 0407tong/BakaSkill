import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowRightLeft,
  CheckCircle2,
  FolderOpen,
  HardDrive,
  Info,
  Loader2,
  RefreshCw,
  XCircle,
} from "lucide-react";

import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { Tooltip } from "@/components/ui/Tooltip";
import { formatBytes, formatVolumeKind } from "@/lib/format";
import {
  useConfig,
  useInitializeLibrary,
  useLibraryStats,
  useRebuildIndex,
  useRelocateLibrary,
  useSetConfig,
  useValidateLibraryPath,
} from "@/lib/queries";
import type { DiagnosisIssue, IssueSeverity, PathDiagnosis } from "@/types/ipc";

const SEVERITY_STYLE: Record<
  IssueSeverity,
  { icon: React.ElementType; className: string }
> = {
  error: { icon: XCircle, className: "text-danger" },
  warning: { icon: AlertTriangle, className: "text-warning" },
  info: { icon: Info, className: "text-fg-subtle" },
};

/**
 * 中央库路径选择器 —— **自定义中央库位置的核心入口**。
 *
 * 参考项目不支持自定义存储位置；本项目把它做成设置页的一级入口，
 * 并且不是"选完就算"，而是当场给出这个位置的体检报告：
 * 文件系统是否支持链接、是不是云同步目录、剩余空间够不够、路径会不会过长。
 * 这些风险如果在选择时不讲清楚，用户会在很久以后才以"链接莫名其妙失效"的形式撞上。
 */
export function CentralPathPicker() {
  const { data: config, isPending: configLoading } = useConfig();
  const setConfig = useSetConfig();
  const initialize = useInitializeLibrary();
  const rebuildIndex = useRebuildIndex();
  const relocate = useRelocateLibrary();

  // 用户新选但尚未确认的路径
  const [candidate, setCandidate] = useState<string | null>(null);
  // 已确认生效的路径（来自配置）
  const [confirmedPath, setConfirmedPath] = useState<string | null>(null);

  const activePath = confirmedPath ?? config?.centralLibraryPath ?? null;

  const stats = useLibraryStats(activePath);
  const diagnosis = useValidateLibraryPath(candidate);

  const pickFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择中央库位置",
    });
    if (typeof selected === "string") {
      setCandidate(selected);
    }
  };

  const applyCandidate = async (path: string) => {
    // 先初始化目录骨架（幂等），成功后再写入配置。
    // 顺序反了会在初始化失败时留下一个指向空目录的配置。
    await initialize.mutateAsync(path);
    if (!config) throw new Error("配置尚未加载完成");

    await setConfig.mutateAsync({ ...config, centralLibraryPath: path });
    setConfirmedPath(path);
    setCandidate(null);
  };

  return (
    <section
      className="rounded-lg border border-border bg-surface"
      aria-labelledby="central-library-heading"
    >
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <HardDrive className="size-4 text-fg-subtle" />
        <h2
          id="central-library-heading"
          className="text-sm font-medium text-fg"
        >
          中央库位置
        </h2>
        <Badge variant="accent">可自定义位置</Badge>
      </header>

      <div className="space-y-4 p-4">
        <p className="text-sm leading-relaxed text-fg-muted">
          Skill 集中存放在这里，再通过目录链接映射到各个 Agent。
          你可以把它放在任意盘符，不必挤占 C 盘。
        </p>

        {/* 当前生效的位置 */}
        <div className="space-y-1.5">
          <div className="text-xs font-medium text-fg-subtle">当前位置</div>
          {configLoading ? (
            <div className="flex items-center gap-2 text-sm text-fg-muted">
              <Loader2 className="size-3.5 animate-spin" /> 读取配置…
            </div>
          ) : activePath ? (
            <div className="space-y-1">
              <code
                className="block truncate rounded-md border border-border bg-bg-subtle px-2.5 py-1.5 font-mono text-xs text-fg"
                title={activePath}
              >
                {activePath}
              </code>
              <div className="flex items-center gap-3 text-xs text-fg-muted">
                {stats.data?.initialized ? (
                  <>
                    <span className="flex items-center gap-1">
                      <CheckCircle2 className="size-3.5 text-success" />
                      已就绪 · {stats.data.skillCount} 个 Skill ·{" "}
                      {formatBytes(stats.data.totalBytes)}
                    </span>
                  </>
                ) : (
                  <span className="flex items-center gap-1 text-warning">
                    <AlertTriangle className="size-3.5" />
                    该位置尚未初始化
                  </span>
                )}
              </div>
            </div>
          ) : (
            <div className="text-sm text-fg-muted">尚未设置</div>
          )}
        </div>

        {/* 选择新位置 */}
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => void pickFolder()}>
            <FolderOpen className="size-4" />
            {activePath ? "更换位置…" : "选择文件夹…"}
          </Button>

          {activePath && stats.data?.initialized ? (
            <Button
              variant="ghost"
              onClick={() => rebuildIndex.mutate(activePath)}
              disabled={rebuildIndex.isPending}
            >
              <RefreshCw
                className={cn(
                  "size-4",
                  rebuildIndex.isPending && "animate-spin",
                )}
              />
              重建索引
            </Button>
          ) : null}
        </div>

        {/* 候选路径的体检报告 */}
        {candidate ? (
          <CandidateReport
            candidate={candidate}
            diagnosis={diagnosis.data}
            loading={diagnosis.isPending}
            errorMessage={
              diagnosis.error instanceof Error ? diagnosis.error.message : null
            }
            applying={initialize.isPending || setConfig.isPending}
            existingLibraryPath={
              activePath && activePath !== candidate ? activePath : null
            }
            onApply={() => void applyCandidate(candidate)}
            onRelocate={(moveData) =>
              relocate.mutate(
                { newPath: candidate, moveData },
                {
                  onSettled: () => {
                    setCandidate(null);
                    setConfirmedPath(null);
                  },
                },
              )
            }
            relocating={relocate.isPending}
            onCancel={() => setCandidate(null)}
          />
        ) : null}
      </div>
    </section>
  );
}

interface CandidateReportProps {
  candidate: string;
  diagnosis: PathDiagnosis | undefined;
  loading: boolean;
  errorMessage: string | null;
  applying: boolean;
  /** 已配置的旧中央库路径；非空时提供"迁移"而非"新建" */
  existingLibraryPath: string | null;
  onApply: () => void;
  onRelocate: (moveData: boolean) => void;
  relocating: boolean;
  onCancel: () => void;
}

function CandidateReport({
  candidate,
  diagnosis,
  loading,
  errorMessage,
  applying,
  existingLibraryPath,
  onApply,
  onRelocate,
  relocating,
  onCancel,
}: CandidateReportProps) {
  const [confirmingRelocate, setConfirmingRelocate] = useState(false);
  if (loading) {
    return (
      <div className="flex items-center gap-2 rounded-md border border-border bg-bg-subtle p-3 text-sm text-fg-muted">
        <Loader2 className="size-4 animate-spin" />
        正在检查该位置…
      </div>
    );
  }

  if (errorMessage) {
    return (
      <div className="space-y-3 rounded-md border border-danger/40 bg-danger-subtle p-3">
        <div className="flex items-start gap-2 text-sm text-danger">
          <XCircle className="mt-0.5 size-4 shrink-0" />
          <span>{errorMessage}</span>
        </div>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          取消
        </Button>
      </div>
    );
  }

  if (!diagnosis) return null;

  const errors = diagnosis.issues.filter((i) => i.severity === "error");
  const others = diagnosis.issues.filter((i) => i.severity !== "error");

  return (
    <div className="space-y-3 rounded-md border border-border bg-bg-subtle p-3">
      <div className="text-xs font-medium text-fg-subtle">新位置检查结果</div>

      <code
        className="block truncate font-mono text-xs text-fg"
        title={candidate}
      >
        {diagnosis.normalizedPath}
      </code>

      {/* 客观事实 */}
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs sm:grid-cols-4">
        <Fact label="文件系统" value={diagnosis.filesystem ?? "未知"} />
        <Fact label="卷类型" value={formatVolumeKind(diagnosis.volumeKind)} />
        <Fact label="可用空间" value={formatBytes(diagnosis.freeBytes)} />
        <Fact
          label="路径长度"
          value={`${diagnosis.pathLength} 字符`}
          warn={diagnosis.pathLength > 160}
        />
      </div>

      {/* 结论性问题 */}
      {errors.length > 0 ? (
        <ul className="space-y-1.5">
          {errors.map((issue) => (
            <IssueRow key={issue.code} issue={issue} />
          ))}
        </ul>
      ) : (
        <div className="flex items-center gap-2 text-xs text-success">
          <CheckCircle2 className="size-3.5" />
          该位置可以安全地作为中央库
        </div>
      )}

      {others.length > 0 ? (
        <ul className="space-y-1.5">
          {others.map((issue) => (
            <IssueRow key={issue.code} issue={issue} />
          ))}
        </ul>
      ) : null}

      {/* 已有中央库时，这条路是"迁移"而不是"新建"——
          两者差别很大：迁移会重写所有链接，且对执行顺序有硬性要求。 */}
      {existingLibraryPath && diagnosis.canInitialize ? (
        <div className="space-y-2 rounded-sm border border-border bg-surface p-2.5">
          <p className="text-[11px] leading-relaxed text-fg-muted">
            当前中央库在{" "}
            <code className="font-mono">{existingLibraryPath}</code>
            。迁移会把数据复制过来，并<strong>重写所有指向旧位置的链接</strong>
            ；只有在全部链接重写成功后，才会按你的选择决定是否删除旧数据。
          </p>

          {confirmingRelocate ? (
            <div className="flex flex-wrap gap-1.5">
              <Button
                size="sm"
                disabled={relocating}
                onClick={() => onRelocate(true)}
              >
                {relocating ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : null}
                迁移并删除旧数据
              </Button>
              <Button
                size="sm"
                variant="secondary"
                disabled={relocating}
                onClick={() => onRelocate(false)}
              >
                迁移但保留旧数据
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={relocating}
                onClick={() => setConfirmingRelocate(false)}
              >
                返回
              </Button>
            </div>
          ) : (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setConfirmingRelocate(true)}
            >
              <ArrowRightLeft className="size-3.5" />
              迁移中央库到此位置
            </Button>
          )}
        </div>
      ) : null}

      <div className="flex flex-wrap gap-2 pt-1">
        <Tooltip
          label={
            diagnosis.canInitialize ? "" : "存在阻断性问题，无法使用该位置"
          }
        >
          <span>
            <Button
              onClick={onApply}
              disabled={!diagnosis.canInitialize || applying}
            >
              {applying ? <Loader2 className="size-4 animate-spin" /> : null}
              {existingLibraryPath
                ? "改用它，但不迁移数据"
                : diagnosis.isInitialized
                  ? "设为中央库"
                  : "创建并设为中央库"}
            </Button>
          </span>
        </Tooltip>
        <Button variant="ghost" onClick={onCancel} disabled={applying}>
          取消
        </Button>
      </div>
    </div>
  );
}

function Fact({
  label,
  value,
  warn = false,
}: {
  label: string;
  value: string;
  warn?: boolean;
}) {
  return (
    <div className="min-w-0">
      <div className="text-fg-subtle">{label}</div>
      <div
        className={cn(
          "truncate font-medium",
          warn ? "text-warning" : "text-fg",
        )}
      >
        {value}
      </div>
    </div>
  );
}

function IssueRow({ issue }: { issue: DiagnosisIssue }) {
  const { icon: Icon, className } = SEVERITY_STYLE[issue.severity];
  return (
    <li className="flex items-start gap-2 text-xs leading-relaxed">
      <Icon className={cn("mt-0.5 size-3.5 shrink-0", className)} />
      <span className="text-fg-muted">{issue.message}</span>
    </li>
  );
}
