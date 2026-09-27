import { invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";
import type { Post, Rating, Source } from "./ipc";

/** 图库里的一张图。thumbUrl / sampleUrl 指向本地路由，照常用 imageSrc 加载。 */
export interface LocalPost extends Post {
  path: string;
  downloadedAt: number;
  /** 文件已经不在记录的位置（被移动或删除）。 */
  missing: boolean;
}

export interface PostRef {
  source: Source;
  postId: number;
}

export interface DeleteOutcome {
  removed: PostRef[];
  failed: { postId: number; message: string }[];
}

/** 图库的排序；默认最近下载的在前。 */
export type LibrarySort =
  | "downloaded"
  | "downloadedAsc"
  | "newest"
  | "oldest"
  | "score"
  | "favorites"
  | "resolution"
  | "filesize";

const LIBRARY_SORTS: { value: LibrarySort; label: Msg }[] = [
  { value: "downloaded", label: "最近下载" },
  { value: "downloadedAsc", label: "最早下载" },
  { value: "newest", label: "最新上传" },
  { value: "oldest", label: "最早上传" },
  { value: "score", label: "分数最高" },
  { value: "favorites", label: "收藏最多" },
  { value: "resolution", label: "分辨率最高" },
  { value: "filesize", label: "文件最大" },
];

export const librarySorts = () => LIBRARY_SORTS.map(({ value, label }) => ({ value, label: t(label) }));

export interface LibraryQuery {
  source: Source | null;
  /** 空格分隔，`-tag` 表示排除。 */
  tags: string;
  ratings: Rating[];
  sort: LibrarySort;
  offset: number;
  limit: number;
}

export interface LibraryPage {
  posts: LocalPost[];
  total: number;
  offset: number;
  hasMore: boolean;
}

export const libraryList = (query: LibraryQuery) => invoke<LibraryPage>("library_list", { query });

/** 从图库删除；keepFiles 为 false 时图片移到废纸篓（回收站）。 */
export const libraryDelete = (posts: PostRef[], keepFiles: boolean) =>
  invoke<DeleteOutcome>("library_delete", { posts, keepFiles });
