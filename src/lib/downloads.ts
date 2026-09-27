import { invoke } from "@tauri-apps/api/core";

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
  /** 总张数；按条件下载且站点没给数字时，翻完之前为 null。 */
  total: number | null;
  saved: number;
  skipped: number;
  failed: number;
  error: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ItemNote {
  postId: number;
  status: "failed" | "skipped";
  note: string | null;
}

export const STATUS_LABEL: Record<JobStatus, string> = {
  queued: "排队中",
  running: "下载中",
  paused: "已暂停",
  done: "已完成",
  failed: "出错",
  canceled: "已取消",
};

/** 还在队列里（会继续自动下载）的任务。 */
export const isActive = (job: Job) => job.status === "queued" || job.status === "running";

export const processed = (job: Job) => job.saved + job.skipped + job.failed;

export const downloadPosts = (posts: Post[]) => invoke<Job>("download_posts", { posts });
export const downloadQuery = (params: SearchParams, maxPosts: number | null) =>
  invoke<Job>("download_query", { params, maxPosts });
export const listJobs = () => invoke<Job[]>("list_jobs");
export const jobAction = (id: number, action: JobAction) => invoke<void>("job_action", { id, action });
export const jobNotes = (id: number) => invoke<ItemNote[]>("job_notes", { id });
export const clearFinishedJobs = () => invoke<void>("clear_finished_jobs");

/** Rust 端推送的事件名。 */
export const EVENTS = {
  jobUpdated: "job-updated",
  jobRemoved: "job-removed",
  librarySaved: "library-changed",
} as const;

export interface SavedPayload {
  source: Source;
  postId: number;
}
