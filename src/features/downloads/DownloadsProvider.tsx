import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import {
  clearFinishedJobs,
  downloadPosts,
  downloadQuery,
  EVENTS,
  isActive,
  jobAction,
  listJobs,
  type Job,
  type JobAction,
} from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { errorMessage, type Post, type SearchParams } from "../../lib/ipc";

interface DownloadsValue {
  /** 新加入的在前。 */
  jobs: Job[];
  loaded: boolean;
  loadError: string | null;
  /** 排队中和下载中的任务数，侧栏角标用。 */
  activeCount: number;
  act: (id: number, action: JobAction) => Promise<void>;
  clearFinished: () => Promise<void>;
  addPosts: (posts: Post[]) => Promise<Job>;
  addQuery: (params: SearchParams, maxPosts: number | null) => Promise<Job>;
}

const DownloadsContext = createContext<DownloadsValue | null>(null);

/** 只接受不比手上旧的数据：列表请求和实时事件可能交错到达。 */
function upsert(jobs: Map<number, Job>, job: Job): Map<number, Job> {
  const current = jobs.get(job.id);
  if (current && current.updatedAt > job.updatedAt) return jobs;
  return new Map(jobs).set(job.id, job);
}

/** 下载任务的全局状态：启动时读一次列表，之后跟着 Rust 端推送的事件更新。 */
export function DownloadsProvider({ children }: { children: ReactNode }) {
  const [jobs, setJobs] = useState<Map<number, Job>>(() => new Map());
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    listJobs().then(
      (list) => {
        setJobs((prev) => list.reduce(upsert, prev));
        setLoaded(true);
      },
      (err) => {
        setLoadError(errorMessage(err));
        setLoaded(true);
      },
    );
  }, []);

  useTauriEvent<Job>(EVENTS.jobUpdated, (job) => setJobs((prev) => upsert(prev, job)));
  useTauriEvent<number>(EVENTS.jobRemoved, (id) =>
    setJobs((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Map(prev);
      next.delete(id);
      return next;
    }),
  );

  const addPosts = useCallback(async (posts: Post[]) => {
    const job = await downloadPosts(posts);
    setJobs((prev) => upsert(prev, job));
    return job;
  }, []);

  const addQuery = useCallback(async (params: SearchParams, maxPosts: number | null) => {
    const job = await downloadQuery(params, maxPosts);
    setJobs((prev) => upsert(prev, job));
    return job;
  }, []);

  const value = useMemo<DownloadsValue>(() => {
    const list = [...jobs.values()].sort((a, b) => b.id - a.id);
    return {
      jobs: list,
      loaded,
      loadError,
      activeCount: list.filter(isActive).length,
      act: jobAction,
      clearFinished: clearFinishedJobs,
      addPosts,
      addQuery,
    };
  }, [jobs, loaded, loadError, addPosts, addQuery]);

  return <DownloadsContext.Provider value={value}>{children}</DownloadsContext.Provider>;
}

export function useDownloads(): DownloadsValue {
  const value = useContext(DownloadsContext);
  if (!value) throw new Error("useDownloads 需要放在 DownloadsProvider 里面");
  return value;
}
