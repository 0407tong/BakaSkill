import { AlertTriangle, Database, Link2, Unlink } from "lucide-react";

import { Badge, type BadgeVariant } from "@/components/ui/Badge";
import { cn } from "@/lib/cn";
import type { InstanceKind, SkillSummary } from "@/types/ipc";

export const INSTANCE_KIND_META: Record<
  InstanceKind,
  { label: string; variant: BadgeVariant; hint: string }
> = {
  managed: {
    label: "已接入",
    variant: "success",
    hint: "指向中央库的链接，启用状态正常",
  },
  external: {
    label: "外部",
    variant: "outline",
    hint: "Agent 目录中的真实副本，尚未纳入中央库",
  },
  externalDuplicate: {
    label: "冲突",
    variant: "warning",
    hint: "Agent 目录中的真实副本，但中央库已有同名 Skill",
  },
  dangling: {
    label: "断链",
    variant: "danger",
    hint: "链接目标不可达——中央库可能已被移动或所在磁盘未接入",
  },
  foreignLink: {
    label: "外部链接",
    variant: "info",
    hint: "是链接，但目标不在本项目中央库内，不代为管理",
  },
};

/**
 * 一个 Skill 的状态徽章组。
 *
 * 明面上是"徽章"，实际承担**链接透明化**的信息前置职责：
 * 用户应当不点进详情就能看出这个 Skill 是接入状态、外部副本还是断链。
 */
export function SkillBadges({
  skill,
  className,
}: {
  skill: SkillSummary;
  className?: string;
}) {
  const counts = new Map<InstanceKind, number>();
  for (const instance of skill.instances) {
    counts.set(instance.kind, (counts.get(instance.kind) ?? 0) + 1);
  }

  // 展示顺序：先问题后正常，让异常更容易被注意到
  const order: InstanceKind[] = [
    "dangling",
    "externalDuplicate",
    "foreignLink",
    "managed",
    "external",
  ];

  return (
    <div className={cn("flex flex-wrap items-center gap-1", className)}>
      {skill.inCentralLibrary ? (
        <Badge variant="accent" title="位于中央库">
          <Database />
          中央库
        </Badge>
      ) : null}

      {order.map((kind) => {
        const count = counts.get(kind);
        if (!count) return null;
        const meta = INSTANCE_KIND_META[kind];
        return (
          <Badge key={kind} variant={meta.variant} title={meta.hint}>
            {kind === "dangling" ? (
              <Unlink />
            ) : kind === "externalDuplicate" ? (
              <AlertTriangle />
            ) : (
              <Link2 />
            )}
            {meta.label}
            {count > 1 ? ` ×${count}` : ""}
          </Badge>
        );
      })}
    </div>
  );
}
