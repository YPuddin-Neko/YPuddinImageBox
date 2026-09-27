import { useEffect, useState } from "react";

import { Toast, useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/ipc";
import { isMac } from "../../lib/platform";
import { generalInfo, generalSave, type GeneralSettings as General } from "../../lib/settings";

const TRAY = isMac ? "菜单栏" : "任务栏托盘";

const CLOSE_OPTIONS: { value: boolean; title: string; description: string }[] = [
  {
    value: true,
    title: "后台继续运行",
    description: `订阅检查和下载不中断，从${TRAY}里的图标重新打开窗口。`,
  },
  {
    value: false,
    title: "退出软件",
    description: "关掉窗口就退出，订阅只在软件打开时检查。",
  },
];

export function GeneralSettings() {
  const [settings, setSettings] = useState<General | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    generalInfo().then(setSettings, (err) => setError(errorMessage(err)));
  }, []);

  const save = async (next: General, message: string) => {
    setBusy(true);
    setError(null);
    try {
      setSettings(await generalSave(next));
      setNotice(message);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>通用</h1>
        <p>软件在后台怎么运行。订阅要按时检查，需要软件一直开着。</p>
      </header>

      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
        </div>
      )}

      {settings && (
        <div className="set-list">
          <section className="set-card" aria-labelledby="close-title">
            <div className="set-title">
              <h2 id="close-title">关闭窗口时</h2>
            </div>
            <div className="options" role="radiogroup" aria-labelledby="close-title">
              {CLOSE_OPTIONS.map((option) => (
                <label key={String(option.value)} className="option" data-checked={settings.closeToTray === option.value || undefined}>
                  <input
                    type="radio"
                    name="close-to-tray"
                    checked={settings.closeToTray === option.value}
                    disabled={busy}
                    onChange={() =>
                      void save({ ...settings, closeToTray: option.value }, option.value ? "关闭窗口后会在后台继续运行" : "关闭窗口时会退出软件")
                    }
                  />
                  <span className="option-title">{option.title}</span>
                  <span className="option-desc">{option.description}</span>
                </label>
              ))}
            </div>
          </section>

          <section className="set-card" aria-labelledby="login-title">
            <div className="set-title">
              <h2 id="login-title">开机启动</h2>
              <button
                type="button"
                role="switch"
                className="switch"
                aria-checked={settings.launchAtLogin}
                aria-labelledby="login-title"
                disabled={busy}
                onClick={() =>
                  void save(
                    { ...settings, launchAtLogin: !settings.launchAtLogin },
                    settings.launchAtLogin ? "已关闭开机启动" : "登录系统后会自动在后台启动",
                  )
                }
              >
                <span />
              </button>
            </div>
            <p className="set-desc">登录系统后自动在后台启动，不弹出窗口，订阅照常按时检查。</p>
          </section>
        </div>
      )}

      <Toast message={notice} />
    </div>
  );
}
