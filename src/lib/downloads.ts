import { invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";
import type { Post, SearchParams, Source } from "./ipc";

export type JobKind = "posts" | "query";
export type JobStatus = "queued" | "running" | "paused" | "done" | "failed" | "canceled";
export type JobAction = "pause" | "resume" | "cancel" | "retry" | "remove";

export interface Job {
  id: number;
  kind: JobKind;
  source: Source;
  title: string;
  query: string | null;
  maxPosts: number | null;
  status: JobStatus;
  /** 总资源数；按条件下载且站点没给数字时，翻完之前为 null。 */
  total: number | null;
  /** 已读取并加入任务的资源数，包含尚未处理的资源。 */
  discovered: number;
  saved: number;
  skipped: number;
  failed: number;
  error: string | null;
  createdAt: number;
  updatedAt: number;
  /** 超出 tag 上限、在本地筛选的 tag。 */
  localFilter: string | null;
}

export interface ItemNote {
  postId: number;
  status: "failed" | "skipped";
  note: string | null;
}

const STATUS_LABEL: Record<JobStatus, Msg> = {
  queued: "排队中",
  running: "下载中",
  paused: "已暂停",
  done: "已完成",
  failed: "出错",
  canceled: "已取消",
};

export const statusLabel = (status: JobStatus) => t(STATUS_LABEL[status]);

/** 还在队列里（会继续自动下载）的任务。 */
export const isActive = (job: Job) => job.status === "queued" || job.status === "running";

export const processed = (job: Job) => job.saved + job.skipped + job.failed;

/** 下载选中的图；来自几个站点时每个站点各建一个任务。 */
export const downloadPosts = (posts: Post[]) => invoke<Job[]>("download_posts", { posts });
export const downloadQuery = (params: SearchParams, maxPosts: number | null, title?: string) =>
  invoke<Job>("download_query", { params, maxPosts, title });
export const listJobs = () => invoke<Job[]>("list_jobs");
export const jobAction = (id: number, action: JobAction) => invoke<void>("job_action", { id, action });
export const jobNotes = (id: number) => invoke<ItemNote[]>("job_notes", { id });
export const clearFinishedJobs = () => invoke<void>("clear_finished_jobs");

/** Rust 端推送的事件名。 */
export const EVENTS = {
  jobUpdated: "job-updated",
  jobRemoved: "job-removed",
  librarySaved: "library-changed",
  /** 图库里删掉了一些图，payload 是 PostRef[]。 */
  libraryRemoved: "library-removed",
} as const;

export interface SavedPayload {
  source: Source;
  postId: number;
}
