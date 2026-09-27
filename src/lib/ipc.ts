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

export const SOURCES = Object.keys(SOURCE_LABEL) as Source[];
export const SOURCE_OPTIONS = SOURCES.map((value) => ({ value, label: SOURCE_LABEL[value] }));
export const RATING_OPTIONS = RATINGS.map((value) => ({ value, label: RATING_LABEL[value] }));

/** 站点搜索的排序；默认按上传先后，新的在前。 */
export type RemoteSort = "newest" | "oldest" | "score" | "favorites" | "popular" | "resolution" | "filesize";

const REMOTE_SORTS: { value: RemoteSort; label: string; hint?: string; danbooruOnly?: boolean }[] = [
  { value: "newest", label: "最新上传" },
  { value: "oldest", label: "最早上传" },
  { value: "score", label: "分数最高" },
  { value: "favorites", label: "收藏最多", danbooruOnly: true },
  { value: "popular", label: "近期热门", hint: "近两天", danbooruOnly: true },
  { value: "resolution", label: "分辨率最高", danbooruOnly: true },
  { value: "filesize", label: "文件最大", danbooruOnly: true },
];

/** 站点支持的排序：Gelbooru 只能按上传先后和分数排。 */
export function remoteSorts(source: Source) {
  return REMOTE_SORTS.filter((sort) => source === "danbooru" || !sort.danbooruOnly).map(({ value, label, hint }) => ({
    value,
    label,
    hint,
  }));
}

export const remoteSortLabel = (sort: RemoteSort) => REMOTE_SORTS.find((option) => option.value === sort)?.label ?? "";

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
  /** 订阅和统计张数时忽略（近期热门除外，它同时限定了时间范围）。 */
  sort?: RemoteSort;
  /** 上一次返回的 next；不传表示第一页。 */
  cursor?: string | null;
}

export interface SearchPage {
  posts: Post[];
  /** 下一页的位置，没有更多时为 null。 */
  next: string | null;
  /** 实际发给站点的查询串。 */
  query: string;
  /** 超出 tag 上限、在本地筛选的 tag；没有时为空字符串。 */
  localFilter: string;
  /** 这一页里已在图库中的帖子 id。 */
  owned: number[];
}

export function searchRemote(params: SearchParams): Promise<SearchPage> {
  return invoke<SearchPage>("search_remote", { params });
}

/** 查询条件一共能搜到多少张；站点不给数字时为 null。 */
export function countRemote(params: SearchParams): Promise<number | null> {
  return invoke<number | null>("count_remote", { params });
}

/** 帖子在界面上的唯一键。 */
export const postKey = (post: Pick<Post, "source" | "id">) => `${post.source}-${post.id}`;

/** 远程图片统一经 Rust 的 ibx:// 协议加载（代理、限速、缓存都在那一侧）。 */
export function imageSrc(url: string | null | undefined): string | undefined {
  return url ? convertFileSrc(url, "ibx") : undefined;
}

/** Rust 端错误的类别，例如 credentials_missing、bad_credentials。 */
export function errorCode(error: unknown): string | null {
  if (error && typeof error === "object" && "code" in error) {
    return String((error as { code: unknown }).code);
  }
  return null;
}

/** Rust 端错误序列化为 { code, message }。 */
export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}
