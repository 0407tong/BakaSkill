import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * 合并 Tailwind 类名：clsx 处理条件与数组，twMerge 消解冲突的同类工具类
 * （例如 cn("p-2", "p-4") -> "p-4"）。
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
