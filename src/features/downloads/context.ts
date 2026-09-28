import { createContext, useContext } from "react";

import type { Job, JobAction } from "../../lib/downloads";
import type { Post, SearchParams } from "../../lib/ipc";

// context 单独放在这个文件里：开发时热更新 DownloadsProvider 不会生成第二个 context，
// 否则还挂着的旧 Provider 和新的 useDownloads 对不上，整个界面会报错白屏。

export interface DownloadsValue {
  /** 新加入的在前。 */
  jobs: Job[];
  loaded: boolean;
  loadError: string | null;
  /** 排队中和下载中的任务数，侧栏角标用。 */
  activeCount: number;
  act: (id: number, action: JobAction) => Promise<void>;
  clearFinished: () => Promise<void>;
  /** 来自几个站点的图（聚合搜索）每个站点各建一个任务。 */
  addPosts: (posts: Post[]) => Promise<Job[]>;
  addQuery: (params: SearchParams, maxPosts: number | null) => Promise<Job>;
}

export const DownloadsContext = createContext<DownloadsValue | null>(null);

export function useDownloads(): DownloadsValue {
  const value = useContext(DownloadsContext);
  if (!value) throw new Error("useDownloads 需要放在 DownloadsProvider 里面");
  return value;
}
