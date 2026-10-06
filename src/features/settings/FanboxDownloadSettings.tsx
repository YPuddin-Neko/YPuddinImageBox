import { useEffect, useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Toast, useToast } from "../../components/Toast";
import { t, type Msg } from "../../lib/i18n";
import { errorMessage } from "../../lib/ipc";
import {
  fanboxDownloadInfo,
  fanboxDownloadSave,
  type FanboxDownloadInfo,
  type FanboxDownloadSettings as DownloadSettings,
} from "../../lib/settings";
import { FANBOX_NAMING_DEFAULTS, fanboxDownloadExamples } from "./fanboxDownloadExamples";

const FIELDS: { key: keyof typeof FANBOX_NAMING_DEFAULTS; label: Msg }[] = [
  { key: "folderTemplate", label: "投稿文件夹" },
  { key: "imageTemplate", label: "图片文件名" },
  { key: "attachmentTemplate", label: "附件文件名" },
];

const same = (a: DownloadSettings, b: DownloadSettings) => a.directory === b.directory
  && FIELDS.every(({ key }) => a[key] === b[key]);

export function FanboxDownloadSettings({ imagesDirectory }: { imagesDirectory?: string }) {
  const [info, setInfo] = useState<FanboxDownloadInfo | null>(null);
  const [draft, setDraft] = useState<DownloadSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    let stale = false;
    setLoading(true);
    setError(null);
    fanboxDownloadInfo().then((next) => {
      if (stale) return;
      setInfo(next);
      setDraft((prev) => prev ?? next.settings);
    }, (err) => { if (!stale) setError(errorMessage(err)); })
      .finally(() => { if (!stale) setLoading(false); });
    return () => { stale = true; };
  }, [imagesDirectory, retry]);

  const change = (next: DownloadSettings) => {
    setDraft(next);
    setError(null);
  };

  const chooseDirectory = async () => {
    if (!draft || !info) return;
    try {
      const directory = await open({
        directory: true,
        multiple: false,
        defaultPath: draft.directory ?? info.defaultDirectory,
        title: t("选择 FANBOX 保存位置"),
      });
      if (typeof directory === "string") setDraft((prev) => prev && { ...prev, directory });
      setError(null);
    } catch (err) { setError(errorMessage(err)); }
  };

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!draft || !info || same(draft, info.settings) || saving || loading) return;
    setSaving(true);
    setError(null);
    try {
      const next = await fanboxDownloadSave(draft);
      setInfo(next);
      setDraft(next.settings);
      setNotice(t("FANBOX 下载设置已保存"));
    } catch (err) { setError(errorMessage(err)); }
    finally { setSaving(false); }
  };

  const disabled = loading || saving;
  const directory = draft?.directory ?? info?.defaultDirectory ?? "";
  const examples = draft && info ? fanboxDownloadExamples(draft, info.defaultDirectory) : null;

  return (
    <section className="set-card fanbox-download-settings" aria-labelledby="fanbox-download-title">
      <div className="set-title"><h2 id="fanbox-download-title">{t("FANBOX 下载")}</h2></div>
      {loading && !info && <p className="set-desc">{t("正在读取…")}</p>}
      {draft && info && (
        <form onSubmit={(event) => void save(event)}>
          <div className="fanbox-setting-heading">
            <span id="fanbox-directory-label">{t("保存位置")}</span>
            {draft.directory === null ? <span className="badge">{t("默认")}</span> : (
              <button type="button" className="link" disabled={disabled} onClick={() => change({ ...draft, directory: null })}>
                {t("恢复默认")}
              </button>
            )}
          </div>
          <div className="set-line">
            <code className="storage-path" aria-labelledby="fanbox-directory-label" title={directory}><span>{directory}</span></code>
            <button type="button" className="btn" disabled={disabled} onClick={() => void chooseDirectory()}>{t("更改位置…")}</button>
          </div>
          {draft.directory === null && <p className="set-desc">{t("默认使用图片位置下的 fanbox 文件夹。")}</p>}

          <div className="fanbox-setting-heading">
            <span>{t("命名规则")}</span>
            <button type="button" className="link" disabled={disabled || FIELDS.every(({ key }) => draft[key] === FANBOX_NAMING_DEFAULTS[key])}
              onClick={() => change({ ...draft, ...FANBOX_NAMING_DEFAULTS })}>{t("恢复默认")}</button>
          </div>
          <div className="fanbox-template-fields">
            {FIELDS.map(({ key, label }) => (
              <label key={key}>
                <span>{t(label)}</span>
                <input id={`fanbox-${key}`} className="field-input" value={draft[key]} disabled={disabled}
                  spellCheck={false} autoComplete="off" aria-describedby="fanbox-template-help"
                  onChange={(event) => change({ ...draft, [key]: event.target.value })} />
              </label>
            ))}
          </div>
          <p id="fanbox-template-help" className="set-desc">{t("文件夹用 / 分层；文件名不含路径和扩展名，扩展名会自动添加。")}</p>
          <details className="fanbox-template-help">
            <summary>{t("可用字段")}</summary>
            <p className="set-desc">{t("{user} 作者名 · {creator_id} 作者 ID · {date} 投稿日期 · {title} 投稿标题 · {postid} 投稿编号 · {index} 三位序号（封面 000） · {name} 原文件名")}</p>
          </details>

          <div className="fanbox-setting-heading"><span>{t("路径示例")}</span></div>
          {examples ? (
            <dl className="dialog-paths fanbox-path-examples">
              <dt>{t("图片")}</dt><dd><code>{examples.image}</code></dd>
              <dt>{t("封面")}</dt><dd><code>{examples.cover}</code></dd>
              <dt>{t("附件")}</dt><dd><code>{examples.attachment}</code></dd>
            </dl>
          ) : <p className="set-desc">{t("填写有效的命名规则后显示路径示例。")}</p>}
          <div className="set-line fanbox-save-line">
            <button type="submit" className="btn primary" disabled={disabled || same(draft, info.settings)}>
              {saving ? t("正在保存…") : t("保存")}
            </button>
          </div>
        </form>
      )}
      {error && <p className="form-error" role="alert">{error}</p>}
      {!info && !loading && error && <div className="set-line"><button type="button" className="btn" onClick={() => setRetry((n) => n + 1)}>{t("重试")}</button></div>}
      <Toast message={notice} />
    </section>
  );
}
