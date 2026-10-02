import { useMemo } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  CircleSlash,
  Download,
  Link2,
  Loader2,
  RefreshCw,
  Unlink,
} from "lucide-react";

import { cn } from "@/lib/cn";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { EmptyState } from "@/components/EmptyState";
import { call } from "@/lib/ipc";
import { toast } from "sonner";
import { useLinkMatrix, useSetLinkEnabled } from "@/lib/queries";
import type { LinkCell, LinkMatrix, LinkState } from "@/types/ipc";

/** 每种链接状态的符号、配色与说明 */
const STATE_META: Record<
  LinkState,
  { symbol: string; icon: React.ElementType; className: string; label: string }
> = {
  valid: {
    symbol: "●",
    icon: Check,
    className: "text-success",
    label: "链接有效",
  },
  absent: {
    symbol: "○",
    icon: CircleSlash,
    className: "text-fg-subtle",
    label: "未链接",
  },
  dangling: {
    symbol: "⚠",
    icon: Unlink,
    className: "text-danger",
    label: "断链（目标不可达）",
  },
  conflict: {
    symbol: "✖",
    icon: AlertTriangle,
    className: "text-warning",
    label: "冲突（被真实目录占用）",
  },
  foreignLink: {
    symbol: "◐",
    icon: Link2,
    className: "text-info",
    label: "外部链接（目标不在中央库）",
  },
  foreignSymlink: {
    symbol: "◐",
    icon: Link2,
    className: "text-info",
    label: "符号链接（本项目不代管）",
  },
};

/**
 * 链接映射矩阵 —— **链接透明化的核心载体**。
 *
 * 参考项目的链接机制不透明：用户只能看到"目录在不在"，看不到它到底是
 * 链接、副本还是断链。本视图把「Skill × Agent」的每一格摊开，
 * 并明确标出中央库与各 Agent 技能目录的**绝对路径**，
 * 让用户能自己核对而不是只能相信工具。
 *
 * 这也是本项目最核心的差异点，因此它必须是一个**独立可访问的页面**，
 * 而不是卡片上的一个小徽章。
 */
export function LinkMappingView() {
  const queryClient = useQueryClient();
  const matrix = useLinkMatrix();
  const setEnabled = useSetLinkEnabled();

  const data = matrix.data;

  const rowsWithProblems = useMemo(
    () =>
      (data?.rows ?? []).filter((row) =>
        row.cells.some((c) => c.state === "dangling" || c.state === "conflict"),
      ),
    [data],
  );

  const exportReport = async (matrixData: LinkMatrix) => {
    const target = await save({
      title: "导出映射报告",
      defaultPath: `bakaskill-链接映射-${new Date().toISOString().slice(0, 10)}.md`,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (typeof target !== "string") return;

    try {
      await call<string>("write_text_file", {
        path: target,
        content: buildReport(matrixData),
      });
      toast.success("映射报告已导出", { description: target });
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "导出失败");
    }
  };

  if (matrix.isPending) {
    return (
      <EmptyState
        icon={<Loader2 className="animate-spin" />}
        title="正在校验链接…"
        description="逐个检查每个 Skill 在每个 Agent 中的链接状态。"
      />
    );
  }

  if (matrix.isError) {
    return (
      <EmptyState
        icon={<AlertTriangle />}
        title="无法读取链接状态"
        description={
          matrix.error instanceof Error ? matrix.error.message : "未知错误"
        }
      />
    );
  }

  if (!data?.centralLibraryPath) {
    return (
      <EmptyState
        icon={<Link2 />}
        title="尚未配置中央库"
        description="链接是「中央库 → Agent 技能目录」的映射。请先在设置中指定中央库位置。"
      />
    );
  }

  if (data.agents.length === 0) {
    return (
      <EmptyState
        icon={<Link2 />}
        title="没有可映射的 Agent"
        description="尚未检测到任何带有技能目录的 Agent。可以在设置中为 Agent 手动指定技能目录。"
      />
    );
  }

  const { summary } = data;
  const hasProblems = summary.dangling > 0 || summary.conflict > 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* 路径透明化：中央库与各 Agent 技能目录的绝对路径 */}
      <div className="sh-plate shrink-0 space-y-1.5 border-b border-border px-4 py-3">
        <PathLine label="中央库" path={data.centralLibraryPath} />
        {data.agents.map((agent) => (
          <PathLine
            key={agent.id}
            label={agent.displayName}
            path={agent.skillDir}
          />
        ))}
      </div>

      {/* 概览 + 操作 */}
      <div className="sh-plate flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-4 py-2">
        <SummaryChip state="valid" count={summary.valid} />
        <SummaryChip state="absent" count={summary.absent} />
        {summary.dangling > 0 ? (
          <SummaryChip state="dangling" count={summary.dangling} />
        ) : null}
        {summary.conflict > 0 ? (
          <SummaryChip state="conflict" count={summary.conflict} />
        ) : null}
        {summary.foreignLink + summary.foreignSymlink > 0 ? (
          <SummaryChip
            state="foreignLink"
            count={summary.foreignLink + summary.foreignSymlink}
          />
        ) : null}

        <div className="ml-auto flex items-center gap-1">
          <Tooltip label="重新逐格校验（不修改任何文件）">
            <Button
              variant="ghost"
              size="sm"
              onClick={() =>
                void queryClient.invalidateQueries({ queryKey: ["links"] })
              }
              disabled={matrix.isFetching}
            >
              <RefreshCw
                className={cn("size-3.5", matrix.isFetching && "animate-spin")}
              />
              刷新校验
            </Button>
          </Tooltip>
          <Tooltip label="导出当前矩阵为 Markdown，便于排查">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void exportReport(data)}
            >
              <Download className="size-3.5" />
              导出报告
            </Button>
          </Tooltip>
        </div>
      </div>

      {hasProblems ? (
        <div className="shrink-0 border-b border-border bg-warning-subtle px-4 py-1.5 text-xs text-warning">
          有 {rowsWithProblems.length} 个 Skill 存在断链或冲突。
          断链通常意味着中央库被移动过；冲突表示 Agent 目录里有同名真实目录。
        </div>
      ) : null}

      {/* 矩阵 */}
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="w-full border-collapse text-xs">
          <thead className="sticky top-0 z-10 bg-bg-subtle">
            <tr>
              <th className="border-b border-border px-3 py-2 text-left font-medium text-fg-muted">
                Skill
              </th>
              <th className="border-b border-border px-3 py-2 text-left font-medium text-fg-muted">
                中央库
              </th>
              {data.agents.map((agent) => (
                <th
                  key={agent.id}
                  className="border-b border-border px-3 py-2 text-center font-medium text-fg-muted"
                >
                  {agent.displayName}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {/* 每行的 sh-row：铺了背景图时表格行也要有自己的底，
                否则每一行的文字都直接压在插画上（见 globals.css） */}
            {data.rows.map((row) => (
              <tr key={row.skillId} className="sh-row hover:bg-surface-hover">
                <td className="border-b border-border px-3 py-1.5">
                  <div className="flex flex-col">
                    <span className="font-medium text-fg">{row.name}</span>
                    <span className="font-mono text-[10px] text-fg-subtle">
                      {row.dirName}
                    </span>
                  </div>
                </td>
                <td className="border-b border-border px-3 py-1.5">
                  {row.inCentralLibrary ? (
                    <Badge variant="accent">在库中</Badge>
                  ) : (
                    <Badge variant="outline">不在库中</Badge>
                  )}
                </td>
                {row.cells.map((cell) => (
                  <MatrixCell
                    key={cell.agentId}
                    cell={cell}
                    dirName={row.dirName}
                    canToggle={row.inCentralLibrary}
                    busy={setEnabled.isPending}
                    onToggle={(enabled) =>
                      setEnabled.mutate({
                        skillDirName: row.dirName,
                        agentIds: [cell.agentId],
                        enabled,
                      })
                    }
                  />
                ))}
              </tr>
            ))}
          </tbody>
        </table>

        {data.rows.length === 0 ? (
          <EmptyState
            icon={<Link2 />}
            title="没有可映射的 Skill"
            description="中央库与各 Agent 技能目录中都没有 Skill。"
          />
        ) : null}
      </div>

      {/* 图例 */}
      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t border-border px-4 py-2 text-[11px] text-fg-subtle">
        {(Object.keys(STATE_META) as LinkState[]).map((state) => {
          const meta = STATE_META[state];
          return (
            <span key={state} className="flex items-center gap-1">
              <span className={cn("font-mono", meta.className)}>
                {meta.symbol}
              </span>
              {meta.label}
            </span>
          );
        })}
        <span className="ml-auto">点击单元格可直接切换该格状态</span>
      </div>
    </div>
  );
}

function PathLine({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex items-baseline gap-2 text-[11px]">
      <span className="w-20 shrink-0 text-fg-subtle">{label}</span>
      <code
        className="min-w-0 flex-1 truncate rounded-xs bg-bg-subtle px-1.5 py-0.5 font-mono text-fg-muted"
        title={path}
      >
        {path}
      </code>
    </div>
  );
}

function SummaryChip({ state, count }: { state: LinkState; count: number }) {
  const meta = STATE_META[state];
  return (
    <span className="flex items-center gap-1.5 text-xs">
      <span className={cn("font-mono", meta.className)}>{meta.symbol}</span>
      <span className="text-fg-muted">{meta.label}</span>
      <span className="font-medium tabular-nums text-fg">{count}</span>
    </span>
  );
}

interface MatrixCellProps {
  cell: LinkCell;
  dirName: string;
  canToggle: boolean;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
}

function MatrixCell({
  cell,
  dirName,
  canToggle,
  busy,
  onToggle,
}: MatrixCellProps) {
  const meta = STATE_META[cell.state];

  // 可点击的情形：未链接（去启用）或链接有效（去禁用）。
  // 断链允许点击重建；冲突/外部链接不代管，点击无意义。
  const clickable =
    canToggle &&
    (cell.state === "absent" ||
      cell.state === "valid" ||
      cell.state === "dangling");

  const targetEnabled = cell.state !== "valid";

  const hint = [
    meta.label,
    `链接位置：${cell.linkPath}`,
    cell.targetPath ? `目标：${cell.targetPath}` : null,
    clickable
      ? targetEnabled
        ? "点击启用"
        : "点击禁用（不会删除中央库中的文件）"
      : canToggle
        ? "该状态不代为管理"
        : "该 Skill 不在中央库中",
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <td className="border-b border-border px-3 py-1.5 text-center">
      <Tooltip label={hint}>
        <button
          type="button"
          disabled={!clickable || busy}
          onClick={() => onToggle(targetEnabled)}
          aria-label={`${dirName} @ ${cell.agentId}：${meta.label}`}
          className={cn(
            "inline-flex size-7 items-center justify-center rounded-sm font-mono text-base transition-colors",
            meta.className,
            clickable
              ? "cursor-pointer hover:bg-surface-hover"
              : "cursor-help opacity-70",
          )}
        >
          {meta.symbol}
        </button>
      </Tooltip>
    </td>
  );
}

/** 生成便于排查的 Markdown 报告 */
function buildReport(matrix: LinkMatrix): string {
  const lines: string[] = [];
  const now = new Date().toLocaleString("zh-CN");

  lines.push("# BakaSkill 链接映射报告");
  lines.push("");
  lines.push(`导出时间：${now}`);
  lines.push("");
  lines.push("## 路径");
  lines.push("");
  lines.push(`- 中央库：\`${matrix.centralLibraryPath ?? "（未配置）"}\``);
  for (const agent of matrix.agents) {
    lines.push(`- ${agent.displayName}：\`${agent.skillDir}\``);
  }
  lines.push("");
  lines.push("## 概览");
  lines.push("");
  lines.push(`- 链接有效：${matrix.summary.valid}`);
  lines.push(`- 未链接：${matrix.summary.absent}`);
  lines.push(`- 断链：${matrix.summary.dangling}`);
  lines.push(`- 冲突：${matrix.summary.conflict}`);
  lines.push(
    `- 外部链接/符号链接：${matrix.summary.foreignLink + matrix.summary.foreignSymlink}`,
  );
  lines.push("");
  lines.push("## 明细");
  lines.push("");

  const header = [
    "Skill",
    "中央库",
    ...matrix.agents.map((a) => a.displayName),
  ];
  lines.push(`| ${header.join(" | ")} |`);
  lines.push(`| ${header.map(() => "---").join(" | ")} |`);

  for (const row of matrix.rows) {
    const cells = row.cells.map((c) => STATE_META[c.state].symbol);
    lines.push(
      `| ${row.name} | ${row.inCentralLibrary ? "在库中" : "不在库中"} | ${cells.join(" | ")} |`,
    );
  }

  lines.push("");
  lines.push("## 图例");
  lines.push("");
  for (const state of Object.keys(STATE_META) as LinkState[]) {
    const meta = STATE_META[state];
    lines.push(`- \`${meta.symbol}\` ${meta.label}`);
  }
  lines.push("");

  return lines.join("\n");
}
