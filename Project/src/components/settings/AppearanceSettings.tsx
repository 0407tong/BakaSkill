import { open } from "@tauri-apps/plugin-dialog";
import { Image as ImageIcon, Loader2, RotateCcw, Upload } from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { DEFAULT_BACKGROUND } from "@/lib/background";
import {
  useBackground,
  useClearBackground,
  useSetBackground,
} from "@/lib/queries";
import { useViewStore, type BackgroundMode } from "@/store/viewStore";

/**
 * 「外观」设置卡：背景图。
 *
 * 三个选项对应三种状态，而不是一个"开关"：
 * 默认背景 / 自己的图 / 不要背景。三态比布尔更贴合实际——
 * 用户既可能想换图，也可能想彻底关掉。
 *
 * 滑杆调的是**背景的明显程度**，同时会带动面板的透明程度
 * （见 `lib/background.ts` 的 `panelAlpha`）。只调图不调面板的话，
 * 拉小滑杆会让界面发灰而说不清原因。
 */
export function AppearanceSettings() {
  const mode = useViewStore((s) => s.backgroundMode);
  const setMode = useViewStore((s) => s.setBackgroundMode);
  const opacity = useViewStore((s) => s.backgroundOpacity);
  const setOpacity = useViewStore((s) => s.setBackgroundOpacity);

  const background = useBackground();
  const setBackground = useSetBackground();
  const clearBackground = useClearBackground();

  const hasCustom = Boolean(background.data?.path);

  const pickImage = async () => {
    const picked = await open({
      multiple: false,
      title: "选择背景图片",
      filters: [
        {
          name: "图片",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"],
        },
      ],
    });
    if (typeof picked !== "string") return;

    try {
      await setBackground.mutateAsync(picked);
      setMode("custom");
    } catch {
      // 错误提示由 mutation 的 onError 统一给出
    }
  };

  const restoreDefault = () => {
    // 连同后端那份副本一起清掉：留着它只会占地方，
    // 而且下次再选"自定义"时会让用户以为选了新的、其实还是旧的
    clearBackground.mutate(undefined, {
      onSuccess: () => setMode("default"),
    });
  };

  const busy = setBackground.isPending || clearBackground.isPending;

  const options: { id: BackgroundMode; label: string; hint: string }[] = [
    { id: "default", label: "默认背景", hint: "随应用内置" },
    {
      id: "custom",
      label: "自定义背景",
      hint: hasCustom ? "已选好一张" : "尚未选择",
    },
    { id: "none", label: "不显示", hint: "界面完全不透明" },
  ];

  return (
    <section
      className="rounded-lg border border-border bg-surface"
      aria-labelledby="appearance-heading"
    >
      <header className="flex items-center gap-2 border-b border-border px-4 py-3">
        <ImageIcon className="size-4 text-fg-subtle" />
        <h2 id="appearance-heading" className="text-sm font-medium text-fg">
          外观
        </h2>
      </header>

      <div className="space-y-4 p-4">
        <p className="text-sm leading-relaxed text-fg-muted">
          背景图铺在窗口最底层，面板会随之变成半透明——不这样的话，
          图会被面板完全盖住，铺了等于没铺。
        </p>

        {/* 三态选择 */}
        <ul className="grid gap-2 sm:grid-cols-3">
          {options.map((option) => {
            const selected = mode === option.id;
            const unavailable = option.id === "custom" && !hasCustom;
            return (
              <li key={option.id}>
                <button
                  type="button"
                  disabled={busy || unavailable}
                  onClick={() => setMode(option.id)}
                  className={cn(
                    "w-full rounded-md border p-2.5 text-left transition-colors",
                    "disabled:cursor-not-allowed disabled:opacity-50",
                    selected
                      ? "border-accent bg-accent-subtle"
                      : "border-border bg-bg-subtle hover:bg-surface-hover",
                  )}
                >
                  <div className="text-xs font-medium text-fg">
                    {option.label}
                  </div>
                  <div className="mt-0.5 text-[11px] text-fg-subtle">
                    {unavailable ? "先选一张图" : option.hint}
                  </div>
                </button>
              </li>
            );
          })}
        </ul>

        {/* 预览 + 操作 */}
        <div className="flex flex-wrap items-center gap-3">
          <div className="h-16 w-28 shrink-0 overflow-hidden rounded-md border border-border bg-bg-subtle">
            <img
              src={DEFAULT_BACKGROUND}
              alt="默认背景预览"
              className="size-full object-cover"
            />
          </div>

          <div className="flex flex-wrap gap-2">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => void pickImage()}
            >
              {setBackground.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Upload className="size-4" />
              )}
              {hasCustom ? "换一张图片…" : "选择图片…"}
            </Button>

            {hasCustom ? (
              <Button variant="ghost" disabled={busy} onClick={restoreDefault}>
                <RotateCcw className="size-4" />
                清除自定义图
              </Button>
            ) : null}
          </div>
        </div>

        {/* 不透明度 */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <label
              htmlFor="background-opacity"
              className="text-xs font-medium text-fg-subtle"
            >
              背景明显程度
            </label>
            <span className="flex items-center gap-2">
              <span className="tabular-nums text-xs text-fg-muted">
                {opacity}%
              </span>
              {mode === "none" ? (
                <Badge variant="default">当前未启用背景</Badge>
              ) : null}
            </span>
          </div>
          <input
            id="background-opacity"
            type="range"
            min={0}
            max={100}
            step={5}
            value={opacity}
            disabled={mode === "none"}
            onChange={(e) => setOpacity(Number(e.target.value))}
            className="w-full accent-[var(--color-accent)] disabled:opacity-50"
          />
          <p className="text-[11px] leading-relaxed text-fg-subtle">
            调大能看清背景，调小则文字更好读——
            <strong className="text-fg-muted">面板的透明度会跟着一起变</strong>
            ，所以拉到两端都有明显区别。
          </p>
        </div>
      </div>
    </section>
  );
}
