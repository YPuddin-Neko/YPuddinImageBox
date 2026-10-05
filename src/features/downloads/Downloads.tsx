import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Icon, type IconName } from "../../components/Icon";
import {
  isActive,
  jobNotes,
  processed,
  statusLabel,
  type ItemNote,
  type Job,
  type JobAction,
} from "../../lib/downloads";
import { formatCount, formatTime } from "../../lib/format";
import { t, tx, type Msg } from "../../lib/i18n";
import { errorMessage, postNumber, SOURCE_LABEL, type Source } from "../../lib/ipc";
import { EASE_OUT } from "../../lib/motion";
import type { Navigate } from "../../lib/nav";
import { useDownloads } from "./context";

type Notes = ItemNote[] | "loading" | "failed";

interface ActionButton {
  action: JobAction;
  label: string;
  icon: IconName;
  ghost?: boolean;
  title?: string;
}

const actionButton = (action: JobAction, label: Msg, icon: IconName, ghost = false): ActionButton => ({
  action,
  label: t(label),
  icon,
  ghost,
});

function actionsFor(job: Job): ActionButton[] {
  const remove: ActionButton = {
    ...actionButton("remove", "移除", "trash", true),
    title: t(job.source === "fanbox" ? "只移除这条任务，已下载的文件不受影响" : "只移除这条任务，已下载的图片不受影响"),
  };
  switch (job.status) {
    case "queued":
    case "running":
      return [actionButton("pause", "暂停", "pause"), actionButton("cancel", "取消", "close", true)];
    case "paused":
      return [actionButton("resume", "继续", "play"), actionButton("cancel", "取消", "close", true)];
    case "failed":
    case "canceled":
      return [actionButton("resume", "继续", "play"), remove];
    case "done":
      return job.failed > 0
        ? [{ action: "retry", label: t(job.source === "fanbox" ? "重试失败的 {n} 项" : "重试失败的 {n} 张", { n: formatCount(job.failed) }), icon: "retry" }, remove]
        : [remove];
  }
}

function summary(jobs: Job[]): string {
  const running = jobs.filter((job) => job.status === "running").length;
  const queued = jobs.filter((job) => job.status === "queued").length;
  if (running + queued === 0) {
    return jobs.length === 0 ? t("下载的图片保存在「设置 → 存储」里的图片位置。") : t("没有进行中的任务。");
  }
  const parts = [
    running > 0 ? t("{n} 个正在下载", { n: running }) : "",
    queued > 0 ? t("{n} 个排队中", { n: queued }) : "",
  ];
  return t("{status}。一次下载一个任务，其余按加入顺序排队。", { status: parts.filter(Boolean).join(t("，::list")) });
}

function Progress({ job }: { job: Job }) {
  const done = processed(job);
  // 按条件下载且站点没给总数时，下载中显示来回滑动的进度条。
  const indeterminate = job.total == null && job.status === "running";
  // 已处理的部分和右下角的张数一致（跳过的也算），里面按已保存、跳过、失败的张数分段。
  const parts = [
    { className: "bar-saved", n: job.saved },
    { className: "bar-skipped", n: job.skipped },
    { className: "bar-failed", n: job.failed },
  ];
  return (
    <div
      className={`bar${indeterminate ? " is-indeterminate" : ""}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={job.total ?? undefined}
      aria-valuenow={done}
    >
      {indeterminate ? (
        <i className="bar-indeterminate" />
      ) : (
        <i className="bar-fill" style={{ width: `${job.total ? Math.min(100, (done / job.total) * 100) : 0}%` }}>
          {parts.map(
            ({ className, n }) => n > 0 && <i key={className} className={className} style={{ flexGrow: n }} />,
          )}
        </i>
      )}
    </div>
  );
}

function countLabel(job: Job): string {
  const done = formatCount(processed(job));
  return job.total != null ? `${done} / ${formatCount(job.total)}` : t(job.source === "fanbox" ? "{n} 项" : "{n} 张", { n: done });
}

function NoteList({ source, notes }: { source: Source; notes: Notes | undefined }) {
  if (notes === undefined || notes === "loading") return <p className="job-notes-hint">{t("正在读取…")}</p>;
  if (notes === "failed") return <p className="job-notes-hint">{t("读取失败，请稍后再试。")}</p>;
  if (notes.length === 0) return <p className="job-notes-hint">{t(source === "fanbox" ? "没有跳过或失败的文件。" : "没有跳过或失败的图。")}</p>;
  return (
    <ul className="job-notes">
      {notes.map((note) => (
        <li key={`${note.postId}-${note.status}`}>
          <span className="mono">#{postNumber({ source, id: note.postId })}</span>
          <span className={`note-status ${note.status}`}>{note.status === "failed" ? t("失败") : t("跳过")}</span>
          <span>{note.note ?? "—"}</span>
        </li>
      ))}
    </ul>
  );
}

export function Downloads({ onNavigate }: { onNavigate: Navigate }) {
  const { jobs, loaded, loadError, act, clearFinished } = useDownloads();
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const [notes, setNotes] = useState<Record<number, Notes>>({});

  const openJob = jobs.find((job) => job.id === open);
  const noteCount = openJob ? openJob.skipped + openJob.failed : 0;

  // 展开的任务有新的跳过或失败时重新读取原因。
  useEffect(() => {
    if (open === null) return;
    let stale = false;
    jobNotes(open).then(
      (list) => !stale && setNotes((prev) => ({ ...prev, [open]: list })),
      () => !stale && setNotes((prev) => ({ ...prev, [open]: "failed" })),
    );
    return () => {
      stale = true;
    };
  }, [open, noteCount]);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const finished = jobs.filter((job) => job.status === "done" || job.status === "canceled").length;

  return (
    <div className="page">
      <header className="page-head" data-tauri-drag-region>
        <div className="page-title">
          <h1>{t("下载::page")}</h1>
          <p>{summary(jobs)}</p>
        </div>
        <button type="button" className="btn ghost" onClick={() => void run(clearFinished)} disabled={finished === 0}>
          <Icon name="trash" size={15} />
          {t("清除已完成")}
        </button>
      </header>

      {(error ?? loadError) && (
        <div className="alert page-block" role="alert">
          <span>{error ?? loadError}</span>
          {error && (
            <button type="button" className="btn" onClick={() => setError(null)}>
              {t("关闭")}
            </button>
          )}
        </div>
      )}

      {loaded && jobs.length === 0 && !loadError && (
        <div className="empty page-block">
          <p className="empty-title">{t("还没有下载任务")}</p>
          <p>{t("在「发现」里勾选图片，或者点「下载全部结果」，任务会排在这里。")}</p>
          <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
            <Icon name="compass" size={15} />
            {t("去发现")}
          </button>
        </div>
      )}

      <ul className="jobs page-block">
        <AnimatePresence initial={false}>
          {jobs.map((job) => (
            <motion.li
              key={job.id}
              className="job"
              data-status={job.status}
              layout="position"
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, transition: { duration: 0.12 } }}
              transition={{ duration: 0.2, ease: EASE_OUT }}
            >
              <div className="job-head">
                <div className="job-title">
                  <span className="badge">{SOURCE_LABEL[job.source]}</span>
                  <h2 title={job.title}>{job.title}</h2>
                  <span className="job-status" data-status={job.status}>
                    {statusLabel(job.status)}
                  </span>
                </div>
                <div className="job-actions">
                  {actionsFor(job).map((button) => (
                    <button
                      key={button.action}
                      type="button"
                      className={`btn${button.ghost ? " ghost" : ""}`}
                      title={button.title}
                      onClick={() => void run(() => act(job.id, button.action))}
                    >
                      <Icon name={button.icon} size={15} />
                      {button.label}
                    </button>
                  ))}
                </div>
              </div>
              {job.query !== null && (
                <code className="job-query" title={t("发给站点的查询")}>
                  {job.query || t("全部帖子")}
                  {job.localFilter ? t(" · 本地筛选 {filter}", { filter: job.localFilter }) : ""}
                  {job.maxPosts ? t(job.source === "fanbox" ? " · 最多 {n} 项" : " · 最多 {n} 张", { n: formatCount(job.maxPosts) }) : ""}
                </code>
              )}
              <Progress job={job} />
              <div className="job-meta">
                <span>{tx("已保存 {n}", { n: <b>{formatCount(job.saved)}</b> })}</span>
                <span>{tx("跳过 {n}", { n: <b>{formatCount(job.skipped)}</b> })}</span>
                <span>{tx("失败 {n}", { n: <b>{formatCount(job.failed)}</b> })}</span>
                <span>{t("{time} 加入", { time: formatTime(job.createdAt) })}</span>
                {job.skipped + job.failed > 0 && (
                  <button
                    type="button"
                    className="link"
                    aria-expanded={open === job.id}
                    onClick={() => setOpen(open === job.id ? null : job.id)}
                  >
                    {open === job.id ? t("收起原因") : t("查看跳过和失败的原因")}
                  </button>
                )}
                <span className="job-count">{countLabel(job)}</span>
              </div>
              {job.error && !isActive(job) && <p className="job-error">{job.error}</p>}
              {open === job.id && <NoteList source={job.source} notes={notes[job.id]} />}
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>
    </div>
  );
}
