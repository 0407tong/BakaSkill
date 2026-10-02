import { useEffect } from "react";

import defaultBackgroundUrl from "@/assets/default-background.png";

/** 内置默认背景的地址（Vite 会把这张图作为独立资源产出） */
export const DEFAULT_BACKGROUND = defaultBackgroundUrl;

/**
 * 滑杆值 → 面板不透明度。
 *
 * 背景越明显，面板越透明——两者必须**连动**。只让滑杆控制图片的话，
 * 拉到很小时图片几乎看不见、面板却还半透明，界面会显得发灰，
 * 而且用户说不清是哪里不对。
 *
 * 0.68 是"背景最清楚"时的面板不透明度：再低正文就开始难读了；
 * 0.98 是"背景几乎看不见"时的值：接近不透明，但不设成 1，
 * 免得滑杆刚离开 0 就出现一刀切的跳变。
 */
export function panelAlpha(opacity: number): number {
  const p = Math.min(100, Math.max(0, opacity)) / 100;
  return 0.68 + 0.3 * (1 - p);
}

/**
 * 把"当前有没有背景图"同步到 `<html>` 上。
 *
 * 用 data 属性而不是 class：它就是一份**状态**，CSS 侧只按属性选择即可，
 * 不必知道背后的业务（是默认图还是自定义图、滑杆多少）。面板的半透明
 * 完全由 `globals.css` 里 `[data-bg-image="on"]` 那段负责，这里只负责开关它。
 *
 * 关掉背景时**必须把属性摘掉**——否则会留下一层半透明，界面就回不到原样了。
 */
export function useBackgroundEffect(active: boolean, opacity: number) {
  useEffect(() => {
    const root = document.documentElement;

    if (!active) {
      root.removeAttribute("data-bg-image");
      root.style.removeProperty("--sh-panel-alpha");
      return;
    }

    root.setAttribute("data-bg-image", "on");
    root.style.setProperty("--sh-panel-alpha", panelAlpha(opacity).toFixed(3));
  }, [active, opacity]);
}
