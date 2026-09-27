import { useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { MOD } from "../../lib/hotkeys";
import { errorMessage } from "../../lib/ipc";
import { isMac, revealLabel } from "../../lib/platform";
import { generalInfo, generalSave, type GeneralInfo, type GeneralSettings as General } from "../../lib/settings";

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

const SHORTCUTS: { keys: string[]; action: string }[] = [
  { keys: [MOD, "F"], action: "跳到搜索框" },
  { keys: [MOD, "1～4"], action: "切换到发现、图库、订阅、下载" },
  { keys: [MOD, ","], action: "打开设置" },
  { keys: ["←", "→"], action: "上一张、下一张" },
  { keys: ["空格"], action: "勾选或取消勾选当前这张" },
  { keys: [MOD, "A"], action: "勾选已加载的全部图片" },
  { keys: ["Esc"], action: "取消勾选" },
  { keys: [MOD, "D"], action: "发现页：下载当前这张，有勾选时下载勾选的" },
  { keys: [isMac ? "⌫" : "Delete"], action: "图库：删除当前这张，有勾选时删除勾选的" },
];

export function GeneralSettings() {
  const [settings, setSettings] = useState<GeneralInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    generalInfo().then(setSettings, (err) => setError(errorMessage(err)));
  }, []);

  const revealLog = async (path: string) => {
    setError(null);
    try {
      await revealItemInDir(path);
    } catch (err) {
      setError(`${revealLabel}失败：${errorMessage(err)}`);
    }
  };

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

          <section className="set-card" aria-labelledby="keys-title">
            <div className="set-title">
              <h2 id="keys-title">快捷键</h2>
            </div>
            <dl className="shortcuts">
              {SHORTCUTS.map((shortcut) => (
                <div key={shortcut.action} className="shortcut">
                  <dt>
                    {shortcut.keys.map((key) => (
                      <kbd key={key}>{key}</kbd>
                    ))}
                  </dt>
                  <dd>{shortcut.action}</dd>
                </div>
              ))}
            </dl>
          </section>

          <section className="set-card" aria-labelledby="log-title">
            <div className="set-title">
              <h2 id="log-title">日志</h2>
            </div>
            <p className="set-desc">记录下载任务、订阅检查和出错的情况，文件超过 2 MB 会换新的。</p>
            <div className="set-line">
              <code className="storage-path" title={settings.logFile}>
                <span>{settings.logFile}</span>
              </code>
              <div className="set-actions">
                <button type="button" className="btn ghost" onClick={() => void revealLog(settings.logFile)}>
                  <Icon name="folder" size={15} />
                  {revealLabel}
                </button>
              </div>
            </div>
          </section>
        </div>
      )}

      <Toast message={notice} />
    </div>
  );
}
