import { invoke } from "@tauri-apps/api/core";

import type { Post, Rating, Source } from "./ipc";

/** 图库里的一张图。thumbUrl / sampleUrl 指向本地路由，照常用 imageSrc 加载。 */
export interface LocalPost extends Post {
  path: string;
  downloadedAt: number;
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
