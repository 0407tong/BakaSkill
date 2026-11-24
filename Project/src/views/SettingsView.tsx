import { AgentDirSettings } from "@/components/settings/AgentDirSettings";
import { AppearanceSettings } from "@/components/settings/AppearanceSettings";
import { CentralPathPicker } from "@/components/settings/CentralPathPicker";
import { GitHubSyncPanel } from "@/components/settings/GitHubSyncPanel";

/**
 * 设置页。
 *
 * 承载四块配置：外观（背景图）、中央库位置、Agent 技能目录，
 * 以及 GitHub 登录与仓库选择。
 *
 * 这里不放"尚未落地"的说明卡片：要么指向一个已存在的入口，
 * 要么就不写——占位文案一旦没人维护，就会变成一句与事实相反的话。
 */
export function SettingsView() {
  return (
    <div className="flex-1 overflow-y-auto px-6 py-5">
      <div className="mx-auto max-w-3xl space-y-5">
        <header className="sh-plate rounded-xl border border-border bg-surface px-4 py-3">
          <h2 className="text-sm font-semibold text-fg">应用设置</h2>
          <p className="mt-0.5 text-xs text-fg-muted">
            统一管理外观主题、中央库存储位置、各 Agent 技能目录与 GitHub 同步。
          </p>
        </header>

        {/* 外观：背景图。放在最前——它是"第一眼看到的东西" */}
        <AppearanceSettings />

        {/* 中央库位置：一级入口，不藏在弹窗里 */}
        <CentralPathPicker />

        {/* Agent 技能目录：决定哪些 Agent 能出现在「启用」的目标里 */}
        <AgentDirSettings />

        {/* 登录 + 选择仓库两个面向用户的接口 */}
        <GitHubSyncPanel />
      </div>
    </div>
  );
}
