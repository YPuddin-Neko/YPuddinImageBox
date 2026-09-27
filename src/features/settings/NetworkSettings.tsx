import { useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { t, type Msg } from "../../lib/i18n";
import { errorMessage } from "../../lib/ipc";
import { proxyInfo, proxySave, proxyTest, type ProxyMode, type ProxySettings } from "../../lib/settings";

const OPTIONS: { mode: ProxyMode; title: Msg; description?: Msg }[] = [
  { mode: "system", title: "跟随系统", description: "使用系统设置里的代理，没设置就直接连接" },
  { mode: "none", title: "不使用代理", description: "总是直接连接站点" },
  { mode: "manual", title: "手动设置" },
];

type TestState = { state: "idle" } | { state: "testing" } | { state: "ok"; ms: number } | { state: "failed"; message: string };

const same = (a: ProxySettings, b: ProxySettings) => a.mode === b.mode && a.url.trim() === b.url.trim();

export function NetworkSettings() {
  const [saved, setSaved] = useState<ProxySettings | null>(null);
  const [draft, setDraft] = useState<ProxySettings>({ mode: "system", url: "" });
  const [test, setTest] = useState<TestState>({ state: "idle" });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    proxyInfo().then(
      (proxy) => {
        setSaved(proxy);
        setDraft(proxy);
      },
      (err) => setError(errorMessage(err)),
    );
  }, []);

  const change = (next: ProxySettings) => {
    setDraft(next);
    setTest({ state: "idle" });
    setError(null);
  };

  const runTest = async () => {
    setTest({ state: "testing" });
    try {
      setTest({ state: "ok", ms: await proxyTest(draft) });
    } catch (err) {
      setTest({ state: "failed", message: errorMessage(err) });
    }
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const next = await proxySave(draft);
      setSaved(next);
      setDraft(next);
      setNotice(t("代理设置已保存，之后的请求立即使用"));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const dirty = saved !== null && !same(saved, draft);

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>{t("网络")}</h1>
        <p>{t("代理用于搜索、预览图和原图下载，保存后立即生效。")}</p>
      </header>

      <div className="set-list">
        <section className="set-card" aria-labelledby="proxy-title">
          <div className="set-title">
            <h2 id="proxy-title">{t("代理")}</h2>
          </div>
          <div className="options" role="radiogroup" aria-labelledby="proxy-title">
            {OPTIONS.map((option) => (
              <label key={option.mode} className="option" data-checked={draft.mode === option.mode || undefined}>
                <input
                  type="radio"
                  name="proxy-mode"
                  checked={draft.mode === option.mode}
                  onChange={() => change({ ...draft, mode: option.mode })}
                />
                <span className="option-title">{t(option.title)}</span>
                {option.mode === "manual" ? (
                  <input
                    className="field-input option-input"
                    aria-label={t("代理地址")}
                    value={draft.url}
                    placeholder={t("http://127.0.0.1:7890 或 socks5://127.0.0.1:1080")}
                    spellCheck={false}
                    autoComplete="off"
                    onFocus={() => draft.mode !== "manual" && change({ ...draft, mode: "manual" })}
                    onChange={(event) => change({ mode: "manual", url: event.target.value })}
                  />
                ) : (
                  <span className="option-desc">{option.description && t(option.description)}</span>
                )}
              </label>
            ))}
          </div>
          <div className="set-line">
            <button type="button" className="btn" onClick={() => void runTest()} disabled={test.state === "testing"}>
              <Icon name="globe" size={15} />
              {test.state === "testing" ? t("正在连接…") : t("测试连接")}
            </button>
            <span className="test-result" data-state={test.state} role="status">
              {test.state === "ok" && t("连接正常，访问 Danbooru 用了 {ms} ms", { ms: test.ms })}
              {test.state === "failed" && test.message}
            </span>
            <button type="button" className="btn primary" onClick={() => void save()} disabled={!dirty || saving}>
              {saving ? t("正在保存…") : t("保存")}
            </button>
          </div>
          {error && <p className="form-error">{error}</p>}
        </section>
      </div>

      <Toast message={notice} />
    </div>
  );
}
