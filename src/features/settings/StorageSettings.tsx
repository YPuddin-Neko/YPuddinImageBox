import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { formatBytes } from "../../lib/format";
import { errorMessage } from "../../lib/ipc";
import { revealLabel } from "../../lib/platform";
import {
  restartApp,
  storageCancelPending,
  storageChange,
  storageDismissError,
  storageInfo,
  storagePrepare,
  storageUsage,
  type ChangeMode,
  type LocationInfo,
  type StorageInfo,
  type StorageKind,
} from "../../lib/storage";

const DESCRIPTION: Record<StorageKind, string> = {
  images: "下载的原图",
  database: "图库索引、下载任务和订阅",
  data: "设置和日志",
  cache: "缩略图和预览图，删掉后浏览时会重新下载",
};

const CHOICES: Record<StorageKind, { move: string; leave: string; leaveHint: string }> = {
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
  if (usage === undefined || usage === "loading") return "正在统计…";
  if (usage === "failed") return "无法统计";
  return `占用 ${formatBytes(usage)}`;
}

export function StorageSettings() {
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [usage, setUsage] = useState<Partial<Record<StorageKind, Usage>>>({});
  const [busy, setBusy] = useState<StorageKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
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

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3200);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const apply = async (location: LocationInfo, target: string | null, mode: ChangeMode) => {
    setChoice(null);
    setBusy(location.kind);
    setError(null);
    try {
      const outcome = await storageChange(location.kind, target, mode);
      setInfo(outcome.info);
      setNotice(outcome.applied ? `${location.label}的位置已更新` : `${location.label}会在重启后移到新位置`);
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
      const dir = await open({ directory: true, multiple: false, defaultPath: location.path, title: `选择${location.label}的位置` });
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
        <h1>存储</h1>
        <p>图片、数据库、软件数据和缓存各自的位置。修改时可以把已有内容一起移过去。</p>
      </header>

      {info?.lastError && (
        <div className="alert" role="alert">
          <span>{info.lastError}</span>
          <button type="button" className="btn" onClick={() => void update(storageDismissError)}>
            知道了
          </button>
        </div>
      )}
      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
          <button type="button" className="btn" onClick={() => setError(null)}>
            关闭
          </button>
        </div>
      )}
      {hasPending && (
        <div className="notice-bar" role="status">
          <span>有位置修改要重启后才生效。</span>
          <button type="button" className="btn primary" onClick={() => void restartApp()}>
            <Icon name="retry" size={15} />
            立即重启
          </button>
        </div>
      )}

      <div className="storage-list">
        {info?.locations.map((location) => (
          <section key={location.kind} className="storage-row" aria-labelledby={`storage-${location.kind}`}>
            <div className="storage-title">
              <h2 id={`storage-${location.kind}`}>{location.label}</h2>
              {/* 「恢复默认」放在标题行，每行右侧的按钮保持一样，路径框才能上下对齐。 */}
              {location.isDefault ? (
                <span className="badge">默认</span>
              ) : (
                <button
                  type="button"
                  className="link"
                  onClick={() => request(location, null)}
                  disabled={busy !== null}
                >
                  恢复默认
                </button>
              )}
              <span className="storage-usage">{usageText(usage[location.kind])}</span>
            </div>
            <p className="storage-desc">
              {DESCRIPTION[location.kind]}
              {location.appliesOnRestart ? "，修改后重启生效" : ""}
            </p>
            <div className="storage-line">
              <code className="storage-path" title={location.path}>
                <span>{location.path}</span>
              </code>
              <div className="storage-actions">
                <button type="button" className="btn" onClick={() => void pick(location)} disabled={busy !== null}>
                  {busy === location.kind ? "正在处理…" : "更改位置…"}
                </button>
                <button type="button" className="btn ghost" onClick={() => void reveal(location)}>
                  <Icon name="folder" size={15} />
                  {revealLabel}
                </button>
              </div>
            </div>
            {location.pending && (
              <p className="storage-pending">
                重启后{location.pending.mode === "move" ? "移到" : "改用"}
                <code>{location.pending.to}</code>
                <button
                  type="button"
                  className="link"
                  onClick={() => void update(() => storageCancelPending(location.kind))}
                >
                  撤销
                </button>
              </p>
            )}
          </section>
        ))}
      </div>

      {info && (
        <p className="storage-foot">
          位置设置本身保存在 <code>{info.configFile}</code>，这个文件的位置不能修改。
        </p>
      )}

      <AnimatePresence>
        {notice && (
          <motion.p
            className="toast"
            role="status"
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            transition={{ duration: 0.2 }}
          >
            {notice}
          </motion.p>
        )}
      </AnimatePresence>

      <Dialog
        open={choice !== null}
        title={current ? `把已有的${current.label}移到新位置？` : ""}
        onClose={() => setChoice(null)}
        actions={
          choice && current ? (
            <>
              <button type="button" className="btn primary" onClick={() => void apply(current, choice.target, "move")}>
                {CHOICES[current.kind].move}
              </button>
              <button type="button" className="btn" onClick={() => void apply(current, choice.target, "leave")}>
                {CHOICES[current.kind].leave}
              </button>
              <button type="button" className="btn ghost" onClick={() => setChoice(null)}>
                取消
              </button>
            </>
          ) : null
        }
      >
        {choice && current && (
          <>
            <dl className="dialog-paths">
              <dt>当前</dt>
              <dd>
                <code>{current.path}</code>
              </dd>
              <dt>新位置</dt>
              <dd>
                <code>{choice.target ?? current.defaultPath}</code>
              </dd>
            </dl>
            <ul className="dialog-options">
              <li>
                <b>{CHOICES[current.kind].move}</b>：新位置需要是空文件夹
                {current.appliesOnRestart ? `。${current.label}正在使用，下次启动时再移动` : ""}。
              </li>
              <li>
                <b>{CHOICES[current.kind].leave}</b>：{CHOICES[current.kind].leaveHint}
              </li>
            </ul>
          </>
        )}
      </Dialog>
    </div>
  );
}
