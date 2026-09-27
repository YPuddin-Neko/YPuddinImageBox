import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { formatBytes } from "../../lib/format";
import { t, tx, type Msg } from "../../lib/i18n";
import { errorMessage } from "../../lib/ipc";
import { revealLabel } from "../../lib/platform";
import {
  restartApp,
  storageCancelPending,
  storageChange,
  storageDismissError,
  storageInfo,
  storageLabel,
  storageNoun,
  storagePrepare,
  storageUsage,
  type ChangeMode,
  type LocationInfo,
  type StorageInfo,
  type StorageKind,
} from "../../lib/storage";

const DESCRIPTION: Record<StorageKind, Msg> = {
  images: "下载的原图",
  database: "图库索引、下载任务和订阅",
  data: "设置和日志",
  cache: "缩略图和预览图，删掉后浏览时会重新下载",
};

const CHOICES: Record<StorageKind, { move: Msg; leave: Msg; leaveHint: Msg }> = {
  images: {
    move: "移动已有图片",
    leave: "已有图片留在原处",
    leaveHint: "新下载的图片存到新位置，已有图片还在原来的文件夹里。",
  },
  database: {
    move: "移动数据库",
    leave: "使用新位置的数据库",
    leaveHint: "新位置里已有数据库就直接使用，没有就新建一个空的。",
  },
  data: {
    move: "移动软件数据",
    leave: "新位置从空开始",
    leaveHint: "设置恢复默认，旧数据留在原处。",
  },
  cache: {
    move: "移动缓存",
    leave: "清空旧缓存",
    leaveHint: "缓存会在浏览时重新下载。",
  },
};

type Usage = number | "loading" | "failed";

interface Choice {
  location: LocationInfo;
  /** `null` 表示恢复默认位置。 */
  target: string | null;
}

function usageText(usage: Usage | undefined): string {
  if (usage === undefined || usage === "loading") return t("正在统计…");
  if (usage === "failed") return t("无法统计");
  return t("占用 {size}", { size: formatBytes(usage) });
}

export function StorageSettings() {
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [usage, setUsage] = useState<Partial<Record<StorageKind, Usage>>>({});
  const [busy, setBusy] = useState<StorageKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();
  const [choice, setChoice] = useState<Choice | null>(null);

  const measure = useCallback((kind: StorageKind) => {
    setUsage((prev) => ({ ...prev, [kind]: "loading" }));
    storageUsage(kind).then(
      (bytes) => setUsage((prev) => ({ ...prev, [kind]: bytes })),
      () => setUsage((prev) => ({ ...prev, [kind]: "failed" })),
    );
  }, []);

  useEffect(() => {
    storageInfo().then(
      (next) => {
        setInfo(next);
        next.locations.forEach((location) => measure(location.kind));
      },
      (err) => setError(errorMessage(err)),
    );
  }, [measure]);

  const apply = async (location: LocationInfo, target: string | null, mode: ChangeMode) => {
    setChoice(null);
    setBusy(location.kind);
    setError(null);
    try {
      const outcome = await storageChange(location.kind, target, mode);
      setInfo(outcome.info);
      const label = storageNoun(location.kind);
      setNotice(outcome.applied ? t("{label}的位置已更新", { label }) : t("{label}会在重启后移到新位置", { label }));
      outcome.info.locations.forEach((l) => measure(l.kind));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  // 旧位置里有内容才需要问怎么处理；没有内容时直接改用新位置（新位置里已有的东西照常使用）。
  const request = (location: LocationInfo, target: string | null) => {
    if (location.hasData) setChoice({ location, target });
    else void apply(location, target, "leave");
  };

  const pick = async (location: LocationInfo) => {
    try {
      const dir = await open({
        directory: true,
        multiple: false,
        defaultPath: location.path,
        title: t("选择{label}的位置", { label: storageNoun(location.kind) }),
      });
      if (typeof dir === "string") request(location, dir);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const reveal = async (location: LocationInfo) => {
    try {
      await revealItemInDir(await storagePrepare(location.kind));
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const update = async (action: () => Promise<StorageInfo>) => {
    try {
      setInfo(await action());
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const hasPending = info?.locations.some((location) => location.pending) ?? false;
  const current = choice?.location;

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>{t("存储")}</h1>
        <p>{t("图片、数据库、软件数据和缓存各自的位置。修改时可以把已有内容一起移过去。")}</p>
      </header>

      {info?.lastError && (
        <div className="alert" role="alert">
          <span>{info.lastError}</span>
          <button type="button" className="btn" onClick={() => void update(storageDismissError)}>
            {t("知道了")}
          </button>
        </div>
      )}
      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
          <button type="button" className="btn" onClick={() => setError(null)}>
            {t("关闭")}
          </button>
        </div>
      )}
      {hasPending && (
        <div className="notice-bar" role="status">
          <span>{t("有位置修改要重启后才生效。")}</span>
          <button type="button" className="btn primary" onClick={() => void restartApp()}>
            <Icon name="retry" size={15} />
            {t("立即重启")}
          </button>
        </div>
      )}

      <div className="set-list">
        {info?.locations.map((location) => (
          <section key={location.kind} className="set-card" aria-labelledby={`storage-${location.kind}`}>
            <div className="set-title">
              <h2 id={`storage-${location.kind}`}>{storageLabel(location.kind)}</h2>
              {/* 「恢复默认」放在标题行，每行右侧的按钮保持一样，路径框才能上下对齐。 */}
              {location.isDefault ? (
                <span className="badge">{t("默认")}</span>
              ) : (
                <button
                  type="button"
                  className="link"
                  onClick={() => request(location, null)}
                  disabled={busy !== null}
                >
                  {t("恢复默认")}
                </button>
              )}
              <span className="storage-usage">{usageText(usage[location.kind])}</span>
            </div>
            <p className="set-desc">
              {t(DESCRIPTION[location.kind])}
              {location.appliesOnRestart ? t("，修改后重启生效") : ""}
            </p>
            <div className="set-line">
              <code className="storage-path" title={location.path}>
                <span>{location.path}</span>
              </code>
              <div className="set-actions">
                <button type="button" className="btn" onClick={() => void pick(location)} disabled={busy !== null}>
                  {busy === location.kind ? t("正在处理…") : t("更改位置…")}
                </button>
                <button type="button" className="btn ghost" onClick={() => void reveal(location)}>
                  <Icon name="folder" size={15} />
                  {revealLabel()}
                </button>
              </div>
            </div>
            {location.pending && (
              <p className="storage-pending">
                {tx(location.pending.mode === "move" ? "重启后移到{path}" : "重启后改用{path}", {
                  path: <code>{location.pending.to}</code>,
                })}
                <button
                  type="button"
                  className="link"
                  onClick={() => void update(() => storageCancelPending(location.kind))}
                >
                  {t("撤销")}
                </button>
              </p>
            )}
          </section>
        ))}
      </div>

      {info && (
        <p className="storage-foot">
          {tx("位置设置本身保存在 {file}，这个文件的位置不能修改。", { file: <code>{info.configFile}</code> })}
        </p>
      )}

      <Toast message={notice} />

      <Dialog
        open={choice !== null}
        title={current ? t("把已有的{label}移到新位置？", { label: storageNoun(current.kind) }) : ""}
        onClose={() => setChoice(null)}
        actions={
          choice && current ? (
            <>
              <button type="button" className="btn primary" onClick={() => void apply(current, choice.target, "move")}>
                {t(CHOICES[current.kind].move)}
              </button>
              <button type="button" className="btn" onClick={() => void apply(current, choice.target, "leave")}>
                {t(CHOICES[current.kind].leave)}
              </button>
              <button type="button" className="btn ghost" onClick={() => setChoice(null)}>
                {t("取消")}
              </button>
            </>
          ) : null
        }
      >
        {choice && current && (
          <>
            <dl className="dialog-paths">
              <dt>{t("当前")}</dt>
              <dd>
                <code>{current.path}</code>
              </dd>
              <dt>{t("新位置")}</dt>
              <dd>
                <code>{choice.target ?? current.defaultPath}</code>
              </dd>
            </dl>
            <ul className="dialog-options">
              <li>
                {tx(
                  current.appliesOnRestart
                    ? "{title}：新位置需要是空文件夹。{label}正在使用，下次启动时再移动。"
                    : "{title}：新位置需要是空文件夹。",
                  { title: <b>{t(CHOICES[current.kind].move)}</b>, label: storageNoun(current.kind) },
                )}
              </li>
              <li>
                {tx("{title}：{text}", {
                  title: <b>{t(CHOICES[current.kind].leave)}</b>,
                  text: t(CHOICES[current.kind].leaveHint),
                })}
              </li>
            </ul>
          </>
        )}
      </Dialog>
    </div>
  );
}
