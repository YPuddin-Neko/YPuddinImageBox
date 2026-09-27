import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/manrope";

import App from "./App";
import "./styles/themes.css";
import "./styles/app.css";
import { applyTheme, loadPrefs, resolveThemeId, systemDarkQuery } from "./theme/themes";

// 首帧前套上主题并标记平台，避免启动时闪一下默认配色。
applyTheme(resolveThemeId(loadPrefs(), systemDarkQuery().matches));
document.documentElement.classList.toggle("is-mac", /Mac/.test(navigator.userAgent));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
