import * as SwitchPrimitive from "@radix-ui/react-switch";
import { cn } from "@/lib/cn";

export type SwitchSize = "sm" | "md";

const TRACK_SIZE: Record<SwitchSize, string> = {
  sm: "h-4 w-7",
  md: "h-5 w-9",
};

const THUMB_SIZE: Record<SwitchSize, string> = {
  sm: "size-3 data-[state=checked]:translate-x-3",
  md: "size-4 data-[state=checked]:translate-x-4",
};

export interface SwitchProps extends React.ComponentPropsWithRef<
  typeof SwitchPrimitive.Root
> {
  size?: SwitchSize;
}

/**
 * 启停开关的底层控件。三态（全启用 / 部分启用 / 未启用）的语义
 * 由 `SkillToggle` 在其上层实现。
 */
export function Switch({ className, size = "md", ref, ...props }: SwitchProps) {
  return (
    <SwitchPrimitive.Root
      ref={ref}
      className={cn(
        "peer relative inline-flex shrink-0 cursor-pointer items-center rounded-full",
        "border border-transparent transition-colors duration-150",
        "data-[state=unchecked]:bg-border-strong",
        "data-[state=checked]:bg-accent",
        "disabled:cursor-not-allowed disabled:opacity-45",
        TRACK_SIZE[size],
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        className={cn(
          "pointer-events-none block rounded-full bg-white shadow-sm",
          "transition-transform duration-150 data-[state=unchecked]:translate-x-0.5",
          THUMB_SIZE[size],
        )}
      />
    </SwitchPrimitive.Root>
  );
}
