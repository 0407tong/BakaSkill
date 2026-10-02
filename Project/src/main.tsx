import React from "react";
import ReactDOM from "react-dom/client";

import "@/styles/globals.css";
import { Root } from "@/Root";

const container = document.getElementById("root");
if (!container) {
  throw new Error("未找到 #root 挂载点，index.html 可能被改动");
}

ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
