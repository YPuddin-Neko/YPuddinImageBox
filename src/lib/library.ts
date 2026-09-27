import { invoke } from "@tauri-apps/api/core";

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

export interface LibraryQuery {
  source: Source | null;
  /** 空格分隔，`-tag` 表示排除。 */
  tags: string;
  ratings: Rating[];
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
