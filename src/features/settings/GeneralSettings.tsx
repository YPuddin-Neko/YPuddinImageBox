import { useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";
import { Toast, useToast } from "../../components/Toast";
import { MOD } from "../../lib/hotkeys";
import { setLanguage, t, type LanguageSetting, type Msg } from "../../lib/i18n";
import { errorMessage } from "../../lib/ipc";
import { isMac, revealLabel } from "../../lib/platform";
import { generalInfo, generalSave, type GeneralInfo, type GeneralSettings as General } from "../../lib/settings";

const tray = () => (isMac ? t("菜单栏") : t("任务栏托盘"));

const closeOptions = (): { value: boolean; title: string; description: string }[] => [
  {
    value: true,
    title: t("后台继续运行"),
    description: t("订阅检查和下载不中断，从{tray}里的图标重新打开窗口。", { tray: tray() }),
  },
  {
    value: false,
    title: t("退出软件"),
    description: t("关掉窗口就退出，订阅只在软件打开时检查。"),
  },
];

/** 语言名称总用它自己的文字写，切错了也认得出来。 */
const languageOptions = (): { value: LanguageSetting; label: string }[] => [
  { value: "system", label: t("跟随系统") },
  { value: "zh", label: "简体中文" },
  { value: "en", label: "English" },
];

const SHORTCUTS: { keys: string[]; action: Msg }[] = [
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

/** 快捷键里需要翻译的按键名称。 */
const keyLabel = (key: string) => (key === "空格" || key === "1～4" ? t(key) : key);

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
      setError(t("{action}失败：{error}", { action: revealLabel(), error: errorMessage(err) }));
    }
  };

  /** `message` 在保存之后才生成，切换语言时提示用的就是新语言。 */
  const save = async (next: General, message: () => string) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await generalSave(next);
      setLanguage(saved.resolvedLanguage);
      setSettings(saved);
      setNotice(message());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>{t("通用")}</h1>
        <p>{t("界面语言，以及软件在后台怎么运行。订阅要按时检查，需要软件一直开着。")}</p>
      </header>

      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
        </div>
      )}

      {settings && (
        <div className="set-list">
          <section className="set-card" aria-labelledby="language-title">
            <div className="set-title">
              <h2 id="language-title">{t("语言")}</h2>
              <Select
                className="select"
                name={t("语言")}
                value={settings.language}
                options={languageOptions()}
                disabled={busy}
                onChange={(language) => void save({ ...settings, language }, () => t("界面语言已切换"))}
              />
            </div>
            <p className="set-desc">{t("界面、提示信息、{tray}菜单和系统通知使用的语言。", { tray: tray() })}</p>
          </section>

          <section className="set-card" aria-labelledby="close-title">
            <div className="set-title">
              <h2 id="close-title">{t("关闭窗口时")}</h2>
            </div>
            <div className="options" role="radiogroup" aria-labelledby="close-title">
              {closeOptions().map((option) => (
                <label key={String(option.value)} className="option" data-checked={settings.closeToTray === option.value || undefined}>
                  <input
                    type="radio"
                    name="close-to-tray"
                    checked={settings.closeToTray === option.value}
                    disabled={busy}
                    onChange={() =>
                      void save({ ...settings, closeToTray: option.value }, () =>
                        option.value ? t("关闭窗口后会在后台继续运行") : t("关闭窗口时会退出软件"),
                      )
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
              <h2 id="login-title">{t("开机启动")}</h2>
              <button
                type="button"
                role="switch"
                className="switch"
                aria-checked={settings.launchAtLogin}
                aria-labelledby="login-title"
                disabled={busy}
                onClick={() =>
                  void save({ ...settings, launchAtLogin: !settings.launchAtLogin }, () =>
                    settings.launchAtLogin ? t("已关闭开机启动") : t("登录系统后会自动在后台启动"),
                  )
                }
              >
                <span />
              </button>
            </div>
            <p className="set-desc">{t("登录系统后自动在后台启动，不弹出窗口，订阅照常按时检查。")}</p>
          </section>

          <section className="set-card" aria-labelledby="keys-title">
            <div className="set-title">
              <h2 id="keys-title">{t("快捷键")}</h2>
            </div>
            <dl className="shortcuts">
              {SHORTCUTS.map((shortcut) => (
                <div key={shortcut.action} className="shortcut">
                  <dt>
                    {shortcut.keys.map((key) => (
                      <kbd key={key}>{keyLabel(key)}</kbd>
                    ))}
                  </dt>
                  <dd>{t(shortcut.action)}</dd>
                </div>
              ))}
            </dl>
          </section>

          <section className="set-card" aria-labelledby="log-title">
            <div className="set-title">
              <h2 id="log-title">{t("日志")}</h2>
            </div>
            <p className="set-desc">{t("记录下载任务、订阅检查和出错的情况，文件超过 2 MB 会换新的。")}</p>
            <div className="set-line">
              <code className="storage-path" title={settings.logFile}>
                <span>{settings.logFile}</span>
              </code>
              <div className="set-actions">
                <button type="button" className="btn ghost" onClick={() => void revealLog(settings.logFile)}>
                  <Icon name="folder" size={15} />
                  {revealLabel()}
                </button>
              </div>
            </div>
          </section>
        </div>
      )}

      <footer className="settings-version">
        {t("构建版本")} {__BUILD_VERSION__}
      </footer>

      <Toast message={notice} />
    </div>
  );
}
