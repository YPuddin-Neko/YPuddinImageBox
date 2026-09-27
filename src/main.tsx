import React from "react";
import ReactDOM from "react-dom/client";
import { error as logError } from "@tauri-apps/plugin-log";
import "@fontsource-variable/manrope";

import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles/themes.css";
import "./styles/app.css";
import { applyTheme, loadPrefs, resolveThemeId, systemDarkQuery } from "./theme/themes";

// 首帧前套上主题并标记平台，避免启动时闪一下默认配色。
applyTheme(resolveThemeId(loadPrefs(), systemDarkQuery().matches));
document.documentElement.classList.toggle("is-mac", /Mac/.test(navigator.userAgent));

// 界面里没接住的错误也记进日志文件，方便排查。
window.addEventListener("error", (event) => {
  logError(`界面错误：${event.message}`).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  logError(`未处理的异常：${String(event.reason)}`).catch(() => {});
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
