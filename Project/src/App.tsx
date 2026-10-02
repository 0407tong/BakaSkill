import { AppShell } from "@/components/layout/AppShell";
import { BackgroundLayer } from "@/components/layout/BackgroundLayer";
import { LibraryHealthBanner } from "@/components/layout/LibraryHealthBanner";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { SkillsView } from "@/views/SkillsView";
import { TagsView } from "@/views/TagsView";
import { MappingView } from "@/views/MappingView";
import { SettingsView } from "@/views/SettingsView";
import { useThemeEffect } from "@/lib/theme";
import { useViewStore } from "@/store/viewStore";

function ActiveView() {
  const activeView = useViewStore((s) => s.activeView);

  switch (activeView) {
    case "skills":
      return <SkillsView />;
    case "tags":
      return <TagsView />;
    case "mapping":
      return <MappingView />;
    case "settings":
      return <SettingsView />;
  }
}

export default function App() {
  useThemeEffect();

  return (
    <>
      {/* 铺在最底层。面板的半透明由 globals.css 的令牌覆盖负责，不在这里画。 */}
      <BackgroundLayer />
      <AppShell banner={<LibraryHealthBanner />}>
        <ActiveView />
      </AppShell>
      {/* 只在「中央库路径未配置」时自动出现，老用户升级不会看到它 */}
      <OnboardingWizard />
    </>
  );
}
