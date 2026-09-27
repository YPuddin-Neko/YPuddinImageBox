import React from "react";
import ReactDOM from "react-dom/client";
import { error as logError } from "@tauri-apps/plugin-log";
import "@fontsource-variable/manrope";

import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles/themes.css";
import "./styles/app.css";
import { setLanguage } from "./lib/i18n";
import { isMac, isWindows } from "./lib/platform";
import { languageCurrent } from "./lib/settings";
import { applyTheme, loadPrefs, resolveThemeId, systemDarkQuery } from "./theme/themes";

// 首帧前套上主题并标记平台，避免启动时闪一下默认配色。
applyTheme(resolveThemeId(loadPrefs(), systemDarkQuery().matches));
document.documentElement.classList.toggle("is-mac", isMac);
document.documentElement.classList.toggle("is-windows", isWindows);

// 界面里没接住的错误也记进日志文件，方便排查。
window.addEventListener("error", (event) => {
  logError(`界面错误：${event.message}`).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  logError(`未处理的异常：${String(event.reason)}`).catch(() => {});
});

// 先问 Rust 端用哪种语言（设置里选的，或者跟随系统）再渲染，免得先闪一下另一种语言。
languageCurrent()
  .catch(() => "zh" as const)
  .then((language) => {
    setLanguage(language);
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
        <ErrorBoundary>
          <App />
        </ErrorBoundary>
      </React.StrictMode>,
    );
  });
