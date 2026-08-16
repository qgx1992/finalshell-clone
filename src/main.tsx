import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";

// 注意:不用 StrictMode,避免 dev 模式下 effect 双执行导致 SSH 连接被重复建立/断开
ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
