import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";
import { SwapLabel } from "../../components/SwapLabel";
import { Toast, useToast } from "../../components/Toast";
import { useTauriEvent } from "../../lib/events";
import { formatCount, formatTime } from "../../lib/format";
import { t, tx } from "../../lib/i18n";
import { errorMessage, SOURCE_LABEL } from "../../lib/ipc";
import { EASE_OUT } from "../../lib/motion";
import type { Navigate } from "../../lib/nav";
import {
  intervalOptions,
  SUBSCRIPTION_EVENT,
  subscriptionCheck,
  subscriptionDelete,
  subscriptionsCheckAll,
  subscriptionsList,
  subscriptionTitle,
  subscriptionUpdate,
  type Subscription,
} from "../../lib/subscriptions";

function lastResult(sub: Subscription): string {
  if (sub.activeJob !== null) return t("正在检查新图…");
  if (sub.lastCheckedAt === null) return t("还没检查过");
  const found = sub.lastNew > 0 ? t("找到 {n} 张新图", { n: formatCount(sub.lastNew) }) : t("没有新图");
  return t("{time} 检查，{found}", { time: formatTime(sub.lastCheckedAt), found });
}

function nextCheck(sub: Subscription): string | null {
  if (!sub.enabled || sub.activeJob !== null || sub.lastCheckedAt === null) return null;
  return t("下次 {time}", { time: formatTime(sub.lastCheckedAt + sub.intervalMinutes * 60_000) });
}

export function Subscriptions({ onNavigate }: { onNavigate: Navigate }) {
  const [subs, setSubs] = useState<Subscription[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Subscription | null>(null);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    subscriptionsList().then(setSubs, (err) => setError(errorMessage(err)));
  }, []);

  const upsert = (sub: Subscription) =>
    setSubs((prev) => {
      if (!prev) return [sub];
      return prev.some((s) => s.id === sub.id) ? prev.map((s) => (s.id === sub.id ? sub : s)) : [sub, ...prev];
    });

  useTauriEvent<Subscription>(SUBSCRIPTION_EVENT, upsert);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const check = (sub: Subscription) =>
    run(async () => {
      const job = await subscriptionCheck(sub.id);
      setNotice(job ? t("开始检查「{title}」", { title: subscriptionTitle(sub) }) : t("这个订阅已经在检查中"));
    });

  const checkAll = () =>
    run(async () => {
      const started = await subscriptionsCheckAll();
      setNotice(started > 0 ? t("开始检查 {n} 个订阅", { n: started }) : t("订阅都在检查中，或者没有启用的订阅"));
    });

  const remove = (sub: Subscription) =>
    run(async () => {
      setDeleting(null);
      await subscriptionDelete(sub.id);
      setSubs((prev) => prev?.filter((s) => s.id !== sub.id) ?? null);
      setNotice(t("已删除订阅「{title}」", { title: subscriptionTitle(sub) }));
    });

  const enabledCount = subs?.filter((sub) => sub.enabled).length ?? 0;

  return (
    <div className="page">
      <header className="page-head" data-tauri-drag-region>
        <div className="page-title">
          <h1>{t("订阅::page")}</h1>
          <p>{t("按设定的间隔检查新图并自动下载。关掉窗口后也会在后台继续，可以在「设置 → 通用」里修改。")}</p>
        </div>
        <button type="button" className="btn ghost" onClick={() => void checkAll()} disabled={enabledCount === 0}>
          <Icon name="retry" size={15} />
          {t("全部立即检查")}
        </button>
      </header>

      {error && (
        <div className="alert page-block" role="alert">
          <span>{error}</span>
          <button type="button" className="btn" onClick={() => setError(null)}>
            {t("关闭")}
          </button>
        </div>
      )}

      {subs && subs.length === 0 && (
        <div className="empty page-block">
          <p className="empty-title">{t("还没有订阅")}</p>
          <p>{t("在「发现」里搜索后点「订阅」，以后有新图会自动下载。")}</p>
          <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
            <Icon name="compass" size={15} />
            {t("去发现")}
          </button>
        </div>
      )}

      {/* 和下载页的任务行共用一套排版：标题行与按钮同高，右侧按钮对齐到同一条边。 */}
      <ul className="jobs page-block">
        <AnimatePresence initial={false}>
          {subs?.map((sub) => (
            <motion.li
              key={sub.id}
              className="job"
              layout="position"
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, transition: { duration: 0.12 } }}
              transition={{ duration: 0.2, ease: EASE_OUT }}
            >
              <div className="job-head">
                <div className="job-title">
                  <span className="badge">{SOURCE_LABEL[sub.source]}</span>
                  <h2 title={subscriptionTitle(sub)}>{subscriptionTitle(sub)}</h2>
                  {sub.activeJob !== null ? (
                    <span className="job-status" data-status="running">
                      {t("检查中")}
                    </span>
                  ) : (
                    !sub.enabled && <span className="job-status">{t("已暂停")}</span>
                  )}
                </div>
                <div className="job-actions">
                  <Select
                    className="select"
                    name={t("检查间隔")}
                    value={sub.intervalMinutes}
                    options={intervalOptions(sub.intervalMinutes)}
                    onChange={(intervalMinutes) =>
                      void run(async () => upsert(await subscriptionUpdate(sub.id, { intervalMinutes })))
                    }
                  />
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void check(sub)}
                    disabled={sub.activeJob !== null || !sub.enabled}
                  >
                    <Icon name="retry" size={15} />
                    {t("立即检查")}
                  </button>
                  <button
                    type="button"
                    className="btn ghost"
                    onClick={() => void run(async () => upsert(await subscriptionUpdate(sub.id, { enabled: !sub.enabled })))}
                  >
                    <Icon name={sub.enabled ? "pause" : "play"} size={15} />
                    <SwapLabel labels={[t("暂停"), t("恢复")]} active={sub.enabled ? 0 : 1} />
                  </button>
                  <button
                    type="button"
                    className="btn ghost icon-only"
                    aria-label={t("删除订阅")}
                    title={t("删除订阅（已下载的图不受影响）")}
                    onClick={() => setDeleting(sub)}
                  >
                    <Icon name="trash" size={15} />
                  </button>
                </div>
              </div>
              <code className="job-query" title={t("发给站点的查询")}>
                {sub.query || t("全部帖子")}
                {sub.localFilter ? t(" · 本地筛选 {filter}", { filter: sub.localFilter }) : ""}
              </code>
              <div className="job-meta">
                <span>{lastResult(sub)}</span>
                {nextCheck(sub) && <span>{nextCheck(sub)}</span>}
                <span>{tx("已处理到 {id}", { id: <b>#{sub.lastSeenId}</b> })}</span>
                {sub.activeJob !== null && (
                  <button type="button" className="link" onClick={() => onNavigate("downloads")}>
                    {t("查看下载")}
                  </button>
                )}
              </div>
              {sub.lastError && <p className="job-error">{sub.lastError}</p>}
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>

      <Dialog
        open={deleting !== null}
        title={t("删除这个订阅？")}
        onClose={() => setDeleting(null)}
        initialFocus="last"
        actions={
          <>
            <button type="button" className="btn danger" onClick={() => deleting && void remove(deleting)}>
              <Icon name="trash" size={15} />
              {t("删除订阅")}
            </button>
            <button type="button" className="btn ghost" onClick={() => setDeleting(null)}>
              {t("取消")}
            </button>
          </>
        }
      >
        <p className="dialog-note">
          {t("以后不再检查「{title}」的新图。已经下载的图和进行中的下载都保留。", {
            title: deleting ? subscriptionTitle(deleting) : "",
          })}
        </p>
      </Dialog>

      <Toast message={notice} />
    </div>
  );
}
