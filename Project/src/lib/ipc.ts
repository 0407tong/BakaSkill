import { invoke } from "@tauri-apps/api/core";
import type { AppErrorKind, AppErrorPayload } from "@/types/ipc";

/**
 * 前端侧的结构化错误。后端返回的 AppError 会被归一化成此类型，
 * 使 UI 能按错误类别给出不同引导（例如 PermissionDenied 提示关闭 Agent）。
 */
export class IpcError extends Error {
  readonly kind: AppErrorKind;
  readonly detail: string | null;

  constructor(
    kind: AppErrorKind,
    message: string,
    detail: string | null = null,
  ) {
    super(message);
    this.name = "IpcError";
    this.kind = kind;
    this.detail = detail;
  }
}

function isAppErrorPayload(value: unknown): value is AppErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as AppErrorPayload).kind === "string" &&
    typeof (value as AppErrorPayload).message === "string"
  );
}

/** 把 invoke 抛出的任意值归一化为 IpcError */
export function normalizeIpcError(raw: unknown): IpcError {
  if (raw instanceof IpcError) return raw;

  if (isAppErrorPayload(raw)) {
    return new IpcError(raw.kind, raw.message, raw.detail ?? null);
  }

  if (raw instanceof Error) {
    return new IpcError("Internal", raw.message);
  }

  if (typeof raw === "string") {
    // Tauri 在命令未注册 / 权限不足时可能返回纯字符串
    return new IpcError("Internal", raw);
  }

  return new IpcError("Internal", "未知错误", JSON.stringify(raw));
}

/**
 * 所有后端调用的统一入口。
 *
 * 不要直接在组件里使用 `invoke`：绕过此处会丢失错误归一化，
 * 导致 UI 拿到无法识别的错误对象。
 */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    const err = normalizeIpcError(raw);
    console.error(`[ipc] ${command} 失败:`, err.kind, err.message, err.detail);
    throw err;
  }
}
