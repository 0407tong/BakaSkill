import { useState } from "react";
import { toast } from "sonner";

import { DEFAULT_BACKGROUND, useBackgroundEffect } from "@/lib/background";
import { useBackground, useBackgroundImage } from "@/lib/queries";
import { useViewStore } from "@/store/viewStore";

/**
 * 铺在最底层的背景图。
 *
 * 它本身不画任何面板——面板的半透明由 `globals.css` 里
 * `:root[data-bg-image="on"]` 那段令牌覆盖负责，这里只负责：
 *   1. 决定**用哪张图**（内置默认 / 用户自选 / 不用）
 *   2. 把「有没有背景」这件事同步给 `<html>`（`useBackgroundEffect`）
 *   3. 图读不出来时**退回去并说出来**，而不是留一片空白
 *
 * 之所以把面板透明度交给 CSS 令牌而不是在这里算：面板颜色统一来自
 * `--sh-surface` 那几个变量，改一处就全站生效；在这里算就得去动几十个组件。
 */
export function BackgroundLayer() {
  const mode = useViewStore((s) => s.backgroundMode);
  const opacity = useViewStore((s) => s.backgroundOpacity);

  const background = useBackground();
  const hasCustomFile = Boolean(background.data?.path);
  const wantCustom = mode === "custom" && hasCustomFile;

  // 只有真要用自定义图时才去取内容——默认图是打包进前端的，不必走 IPC
  const custom = useBackgroundImage(wantCustom);

  // 渲染失败的兜底。**记的是"哪一张失败了"而不是一个布尔量**：
  // 布尔量一旦置上就再也下不来，用户换了张好图也仍然算失败，得重启才行。
  // 按内容比对则天然自愈——新图的 data URL 与失败的那张不同，就重新可见。
  const [failedSrc, setFailedSrc] = useState<string | null>(null);

  const usingCustom =
    wantCustom && Boolean(custom.data) && custom.data !== failedSrc;
  const source =
    mode === "none"
      ? null
      : usingCustom
        ? (custom.data ?? null)
        : DEFAULT_BACKGROUND;

  const active = source !== null && opacity > 0;

  useBackgroundEffect(active, opacity);

  if (!active || !source) return null;

  return (
    <div className="pointer-events-none fixed inset-0 -z-10" aria-hidden="true">
      <img
        src={source}
        alt=""
        className="size-full object-cover"
        style={{ opacity: opacity / 100 }}
        onError={() => {
          // 只在自定义图失败时处理；默认图是打包进来的，不该失败
          if (!usingCustom) return;
          setFailedSrc(source);
          toast.error("自定义背景图读不出来，已退回默认背景", {
            description:
              "那张图可能被删掉了。到「设置 → 外观」重新选一张即可。",
          });
        }}
      />
    </div>
  );
}
