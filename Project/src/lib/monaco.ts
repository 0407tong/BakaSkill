// ⚠️ 导入路径不能带 `esm/vs` 前缀。
// monaco-editor 的 package.json `exports` 映射是 `"./*": "./esm/vs/*.js"`，
// 写 `monaco-editor/esm/vs/editor/editor.worker` 会被拼成
// `./esm/vs/esm/vs/editor/editor.worker.js`（双重前缀）而解析失败。
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/language/json/json.worker?worker";

/**
 * Monaco 本地打包配置。
 *
 * **这是硬性要求，不是优化项。** `@monaco-editor/react` 默认从 CDN
 * （jsdelivr）加载 monaco 的 loader 脚本，对于一个桌面应用意味着：
 * 离线环境下编辑器整个白屏，而且首屏要等一次外部网络请求。
 *
 * 这里改为：
 * 1. 直接 import 本地的 `monaco-editor` 包；
 * 2. 通过 `loader.config({ monaco })` 让 `@monaco-editor/react` 使用这份本地实例，
 *    不再去 CDN 取；
 * 3. 显式注册 worker（Vite 的 `?worker` 语法会把它们打成独立 chunk）。
 *
 * 断网下编辑器必须能正常渲染，本文件是它的实现前提。
 */

declare global {
  interface Window {
    MonacoEnvironment?: monaco.Environment;
  }
}

window.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    // 只有 JSON 有独立 worker；Markdown 等基础语言走通用 worker。
    // monaco 0.57 未提供 markdown / yaml 专用 worker，不要凭空 import。
    if (label === "json") return new JsonWorker();
    return new EditorWorker();
  },
};

loader.config({ monaco });

export { monaco };
