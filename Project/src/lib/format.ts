import type { VolumeKind } from "@/types/ipc";

/** 人类可读的字节数 */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

const VOLUME_KIND_LABEL: Record<VolumeKind, string> = {
  fixed: "本地磁盘",
  removable: "可移动磁盘",
  network: "网络位置",
  cdRom: "光盘",
  ramDisk: "内存盘",
  unknown: "未知",
};

export function formatVolumeKind(kind: VolumeKind | null): string {
  return kind ? VOLUME_KIND_LABEL[kind] : "未知";
}

/**
 * 时间戳格式化。后端给出的是 epoch 毫秒。
 */
export function formatTimestamp(ms: number | null | undefined): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * 展示用的路径截断：保留头尾，中间省略。
 * 用于在窄容器里仍能看出盘符与末尾目录名。
 */
export function shortenPath(path: string, maxLength = 56): string {
  if (path.length <= maxLength) return path;
  const head = path.slice(0, Math.ceil(maxLength / 2) - 2);
  const tail = path.slice(-Math.floor(maxLength / 2));
  return `${head}…${tail}`;
}
