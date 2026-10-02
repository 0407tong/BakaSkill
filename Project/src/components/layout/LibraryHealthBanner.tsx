import { FolderCog } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { GlobalBanner } from "@/components/layout/GlobalBanner";
import { useConfig, useLibraryStats } from "@/lib/queries";
import { useViewStore } from "@/store/viewStore";

/**
 * 中央库健康横幅：路径不可达时告知用户，并给出**可执行的**恢复入口。
 *
 * 横幅本身只做展示；恢复动作（重新定位、重写全部链接）由它跳转到的迁移向导
 * 完成——那个向导就在设置页的「中央库位置」卡片里
 * （「更改位置…」→ 迁移中央库到此位置）。见 `docs/LINKING.md`。
 *
 * 刻意不做任何**自动**修复：自动删链接或自动重建，在路径只是临时不可达
 * （比如移动硬盘没插）时会造成真正的数据损坏。把决定权留给用户是有意的。
 */
export function LibraryHealthBanner() {
  const { data: config } = useConfig();
  const configuredPath = config?.centralLibraryPath ?? null;
  const stats = useLibraryStats(configuredPath);
  const setActiveView = useViewStore((s) => s.setActiveView);

  if (!configuredPath) return null;

  // 路径被移动/删除/盘符变化 —— stats 会返回错误
  if (stats.isError) {
    return (
      <GlobalBanner
        severity="error"
        message={
          <>
            中央库路径当前不可访问：
            <code className="font-mono">{configuredPath}</code>
            。如果是可移动磁盘，请先接入该磁盘。
            <span className="text-fg-muted">
              （已有链接不会被自动改动，也不会被自动删除）
            </span>
          </>
        }
        actions={
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setActiveView("settings")}
          >
            <FolderCog className="size-3.5" />
            重新定位中央库
          </Button>
        }
      />
    );
  }

  return null;
}
