import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Icon, type IconName } from "../../components/Icon";
import type { View } from "../../components/Rail";
import { isActive, jobNotes, processed, STATUS_LABEL, type ItemNote, type Job, type JobAction } from "../../lib/downloads";
import { formatCount, formatTime } from "../../lib/format";
import { errorMessage, SOURCE_LABEL } from "../../lib/ipc";
import { EASE_OUT } from "../../lib/motion";
import { useDownloads } from "./DownloadsProvider";

type Notes = ItemNote[] | "loading" | "failed";

interface ActionButton {
  action: JobAction;
  label: string;
  icon: IconName;
  ghost?: boolean;
  title?: string;
}

const REMOVE: ActionButton = {
  action: "remove",
  label: "移除",
  icon: "trash",
  ghost: true,
  title: "只移除这条任务，已下载的图片不受影响",
};

function actionsFor(job: Job): ActionButton[] {
  switch (job.status) {
    case "queued":
    case "running":
      return [
        { action: "pause", label: "暂停", icon: "pause" },
        { action: "cancel", label: "取消", icon: "close", ghost: true },
      ];
    case "paused":
      return [
        { action: "resume", label: "继续", icon: "play" },
        { action: "cancel", label: "取消", icon: "close", ghost: true },
      ];
    case "failed":
    case "canceled":
      return [{ action: "resume", label: "继续", icon: "play" }, REMOVE];
    case "done":
      return job.failed > 0
        ? [{ action: "retry", label: `重试失败的 ${job.failed} 张`, icon: "retry" }, REMOVE]
        : [REMOVE];
  }
}

function summary(jobs: Job[]): string {
  const running = jobs.filter((job) => job.status === "running").length;
  const queued = jobs.filter((job) => job.status === "queued").length;
  if (running + queued === 0) {
    return jobs.length === 0 ? "下载的图片保存在「设置 → 存储」里的图片位置。" : "没有进行中的任务。";
  }
  const parts = [running > 0 ? `${running} 个正在下载` : "", queued > 0 ? `${queued} 个排队中` : ""];
  return `${parts.filter(Boolean).join("，")}。一次下载一个任务，其余按加入顺序排队。`;
}

function Progress({ job }: { job: Job }) {
  const done = processed(job);
  const percent = job.total ? Math.min(100, (done / job.total) * 100) : null;
  // 按条件下载且站点没给总数时，下载中显示来回滑动的进度条。
  const indeterminate = percent === null && job.status === "running";
  return (
    <div className="job-progress">
      <div
        className={`bar${indeterminate ? " is-indeterminate" : ""}`}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={job.total ?? undefined}
        aria-valuenow={done}
      >
        <i style={indeterminate ? undefined : { width: `${job.status === "done" ? 100 : (percent ?? 0)}%` }} />
      </div>
      <span className="job-count">
        {job.total != null ? `${formatCount(done)} / ${formatCount(job.total)}` : `${formatCount(done)} 张`}
      </span>
    </div>
  );
}

function NoteList({ notes }: { notes: Notes | undefined }) {
  if (notes === undefined || notes === "loading") return <p className="job-notes-hint">正在读取…</p>;
  if (notes === "failed") return <p className="job-notes-hint">读取失败，请稍后再试。</p>;
  if (notes.length === 0) return <p className="job-notes-hint">没有跳过或失败的图。</p>;
  return (
    <ul className="job-notes">
      {notes.map((note) => (
        <li key={`${note.postId}-${note.status}`}>
          <span className="mono">#{note.postId}</span>
          <span className={`note-status ${note.status}`}>{note.status === "failed" ? "失败" : "跳过"}</span>
          <span>{note.note ?? "—"}</span>
        </li>
      ))}
    </ul>
  );
}

export function Downloads({ onNavigate }: { onNavigate: (view: View) => void }) {
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
          <h1>下载</h1>
          <p>{summary(jobs)}</p>
        </div>
        <button type="button" className="btn ghost" onClick={() => void run(clearFinished)} disabled={finished === 0}>
          <Icon name="trash" size={15} />
          清除已完成
        </button>
      </header>

      {(error ?? loadError) && (
        <div className="alert page-block" role="alert">
          <span>{error ?? loadError}</span>
          {error && (
            <button type="button" className="btn" onClick={() => setError(null)}>
              关闭
            </button>
          )}
        </div>
      )}

      {loaded && jobs.length === 0 && !loadError && (
        <div className="empty page-block">
          <p className="empty-title">还没有下载任务</p>
          <p>在「发现」里勾选图片，或者点「下载全部结果」，任务会排在这里。</p>
          <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
            <Icon name="compass" size={15} />
            去发现
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
                    {STATUS_LABEL[job.status]}
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
                <code className="job-query" title="发给站点的查询">
                  {job.query || "全部帖子"}
                  {job.maxPosts ? ` · 最多 ${formatCount(job.maxPosts)} 张` : ""}
                </code>
              )}
              <Progress job={job} />
              <div className="job-meta">
                <span>
                  已保存 <b>{formatCount(job.saved)}</b>
                </span>
                <span>
                  跳过 <b>{formatCount(job.skipped)}</b>
                </span>
                <span>
                  失败 <b>{formatCount(job.failed)}</b>
                </span>
                <span>{formatTime(job.createdAt)} 加入</span>
                {job.skipped + job.failed > 0 && (
                  <button
                    type="button"
                    className="link"
                    aria-expanded={open === job.id}
                    onClick={() => setOpen(open === job.id ? null : job.id)}
                  >
                    {open === job.id ? "收起原因" : "查看跳过和失败的原因"}
                  </button>
                )}
              </div>
              {job.error && !isActive(job) && <p className="job-error">{job.error}</p>}
              {open === job.id && <NoteList notes={notes[job.id]} />}
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>
    </div>
  );
}
