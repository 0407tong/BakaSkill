import { useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/**
 * 订阅窗口的原生文件拖放，返回"是否正有东西被拖在上面"。
 *
 * # 为什么不用 HTML5 的 `onDrop` / `DataTransfer`
 *
 * Tauri 的 `dragDropEnabled` **默认为 true**，此时它替换了 WebView2 自己的
 * 拖放处理器——按官方文档，要用 HTML5 拖放得先把它关掉。而我们真正需要的不是
 * `File` 对象，是**真实文件系统路径**（后端要照着路径去读目录、解压 ZIP），
 * 原生事件恰好直接给路径。
 *
 * 走 HTML5 那条路的失败形态很隐蔽：`onDrop` 根本不触发，界面上什么都不会发生，
 * 看起来像"功能没写"，而不是像"用错了 API"。
 *
 * # 回调为什么放进 ref
 *
 * 若把 `onFiles` 放进订阅的依赖里，拖动过程中每次渲染都会解绑重绑监听器
 * ——而拖动时 `enter` / `over` 事件会连续触发渲染，监听器就在最需要它稳定的
 * 时候被反复换掉，拖拽状态随之丢失。放进 ref、每次渲染后更新，订阅只建立一次。
 */
export function useFileDrop(onFiles: (paths: string[]) => void): boolean {
  const [isDragging, setDragging] = useState(false);
  const handler = useRef(onFiles);

  useEffect(() => {
    handler.current = onFiles;
  });

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragging(true);
          return;
        }
        if (payload.type === "leave") {
          setDragging(false);
          return;
        }
        // drop
        setDragging(false);
        if (payload.paths.length > 0) handler.current(payload.paths);
      })
      .then((fn) => {
        // 订阅是异步建立的：组件在这期间卸载了就直接取消订阅，
        // 否则会留下一个指向已卸载组件的监听器
        if (cancelled) void fn();
        else unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return isDragging;
}
