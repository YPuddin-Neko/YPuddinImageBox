import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Source = "danbooru" | "gelbooru";
export type Rating = "general" | "sensitive" | "questionable" | "explicit";

export const RATINGS: Rating[] = ["general", "sensitive", "questionable", "explicit"];

export const RATING_LABEL: Record<Rating, string> = {
  general: "一般",
  sensitive: "敏感",
  questionable: "存疑",
  explicit: "成人",
};

export const SOURCE_LABEL: Record<Source, string> = {
  danbooru: "Danbooru",
  gelbooru: "Gelbooru",
};

export interface PostTags {
  artist: string[];
  copyright: string[];
  character: string[];
  general: string[];
  meta: string[];
}

export interface Post {
  source: Source;
  id: number;
  md5: string | null;
  width: number;
  height: number;
  rating: Rating | null;
  score: number;
  favCount: number | null;
  fileExt: string;
  fileSize: number | null;
  fileUrl: string | null;
  sampleUrl: string | null;
  thumbUrl: string | null;
  createdAt: string | null;
  postUrl: string;
  tags: PostTags;
}

export interface SearchParams {
  source: Source;
  tags: string;
  ratings: Rating[];
  page: number;
}

export interface SearchPage {
  posts: Post[];
  page: number;
  hasMore: boolean;
  /** 实际发给站点的查询串。 */
  query: string;
}

export function searchRemote(params: SearchParams): Promise<SearchPage> {
  return invoke<SearchPage>("search_remote", { params });
}

/** 远程图片统一经 Rust 的 ibx:// 协议加载（代理、限速、缓存都在那一侧）。 */
export function imageSrc(url: string | null | undefined): string | undefined {
  return url ? convertFileSrc(url, "ibx") : undefined;
}

/** Rust 端错误序列化为 { code, message }。 */
export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}
