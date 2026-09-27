import { Component, type ErrorInfo, type ReactNode } from "react";
import { error as logError } from "@tauri-apps/plugin-log";

import { t } from "../lib/i18n";

interface State {
  error: Error | null;
}

/**
 * 界面渲染出错时显示提示和「重新加载」，而不是整窗空白。
 * 下载、订阅都在 Rust 端运行，界面出错不影响它们。
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("界面出错", error, info.componentStack);
    logError(`界面出错：${error.message}${info.componentStack ?? ""}`).catch(() => {});
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="crash" role="alert" data-tauri-drag-region>
        <h1>{t("界面出了点问题")}</h1>
        <p>{t("重新加载一般就能恢复。下载和订阅在后台照常进行，不受影响。")}</p>
        <button type="button" className="btn primary" onClick={() => window.location.reload()}>
          {t("重新加载")}
        </button>
        <details>
          <summary>{t("出错信息")}</summary>
          <code>{error.message}</code>
        </details>
      </div>
    );
  }
}
