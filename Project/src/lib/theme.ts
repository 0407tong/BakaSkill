import { useEffect } from "react";
import { useViewStore } from "@/store/viewStore";
import type { ThemeMode } from "@/store/viewStore";

/** 依据主题模式与系统偏好，判断当前是否应为暗色 */
export function resolveIsDark(mode: ThemeMode): boolean {
  if (mode === "dark") return true;
  if (mode === "light") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

/**
 * 把主题模式同步到 <html class="dark">。
 *
 * 必须在应用根部挂载一次。首屏防闪烁由 index.html 的内联脚本负责，
 * 此 hook 负责运行期的切换与系统偏好监听。
 */
export function useThemeEffect(): void {
  const themeMode = useViewStore((s) => s.themeMode);

  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const apply = () => {
      root.classList.toggle("dark", resolveIsDark(themeMode));
    };

    apply();

    // 仅在「跟随系统」时需要监听系统偏好变化
    if (themeMode !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [themeMode]);
}
