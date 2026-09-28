import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";

export type Source = "danbooru" | "gelbooru";
export type Rating = "general" | "sensitive" | "questionable" | "explicit";

export const RATINGS: Rating[] = ["general", "sensitive", "questionable", "explicit"];

const RATING_LABEL: Record<Rating, Msg> = {
  general: "一般",
  sensitive: "敏感",
  questionable: "存疑",
  explicit: "成人",
};

export const ratingLabel = (rating: Rating) => t(RATING_LABEL[rating]);

export const SOURCE_LABEL: Record<Source, string> = {
  danbooru: "Danbooru",
  gelbooru: "Gelbooru",
};

export const SOURCES = Object.keys(SOURCE_LABEL) as Source[];
export const SOURCE_OPTIONS = SOURCES.map((value) => ({ value, label: SOURCE_LABEL[value] }));

/** 搜哪里：一个站点，或者全部平台一起搜（聚合搜索）。 */
export type Scope = Source | "all";

export const scopeLabel = (scope: Scope) => (scope === "all" ? t("全部平台") : SOURCE_LABEL[scope]);

/** 搜索框里选来源：第一项是全部平台（聚合搜索），下面是各个站点。 */
export const scopeOptions = (): { value: Scope; label: string; hint?: string; divider?: boolean }[] => [
  { value: "all", label: t("全部平台"), hint: t("聚合搜索"), divider: true },
  ...SOURCE_OPTIONS,
];
export const ratingOptions = () => RATINGS.map((value) => ({ value, label: ratingLabel(value) }));

/** 站点搜索的排序；默认按上传先后，新的在前。 */
export type RemoteSort = "newest" | "oldest" | "score" | "favorites" | "popular" | "resolution" | "filesize";

const REMOTE_SORTS: { value: RemoteSort; label: Msg; hint?: Msg; danbooruOnly?: boolean }[] = [
  { value: "newest", label: "最新上传" },
  { value: "oldest", label: "最早上传" },
  { value: "score", label: "分数最高" },
  { value: "favorites", label: "收藏最多", danbooruOnly: true },
  { value: "popular", label: "近期热门", hint: "近两天", danbooruOnly: true },
  { value: "resolution", label: "分辨率最高", danbooruOnly: true },
  { value: "filesize", label: "文件最大", danbooruOnly: true },
];

/** 站点支持的排序：Gelbooru 只能按上传先后和分数排，聚合搜索也只能用各站点都支持的这几种。 */
export function remoteSorts(scope: Scope) {
  return REMOTE_SORTS.filter((sort) => scope === "danbooru" || !sort.danbooruOnly).map(({ value, label, hint }) => ({
    value,
    label: t(label),
    hint: hint && t(hint),
  }));
}

export function remoteSortLabel(sort: RemoteSort): string {
  const option = REMOTE_SORTS.find((item) => item.value === sort);
  return option ? t(option.label) : "";
}

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

/** 聚合搜索里一个站点这一页的情况。 */
export interface SiteStatus {
  source: Source;
  query: string;
  localFilter: string;
  error: { code: string; message: string } | null;
  /** 出错的站点往下翻时还会再试（网络问题）；账号、条件不对时这个站点不再往下翻。 */
  retry: boolean;
}

export interface SitesSearchParams {
  sources: Source[];
  tags: string;
  ratings: Rating[];
  sort?: RemoteSort;
  cursor?: string | null;
}

export interface SitesPage {
  /** 各站点的结果按所选排序合在一起。 */
  posts: Post[];
  /** 下一页的位置，原样传回；没有更多时为 null。 */
  next: string | null;
  /** 这一页搜了的站点；有图在等着显示的站点这一页不用搜，不在里面。 */
  sites: SiteStatus[];
  /** 这一页里已在图库中的帖子。 */
  owned: { source: Source; postId: number }[];
}

/** 聚合搜索：同样的条件同时搜几个站点，按所选排序合成一列。 */
export function searchSites(params: SitesSearchParams): Promise<SitesPage> {
  return invoke<SitesPage>("search_sites", { params });
}

/** 查询条件一共能搜到多少张；站点不给数字时为 null。 */
export function countRemote(params: SearchParams): Promise<number | null> {
  return invoke<number | null>("count_remote", { params });
}

/** Danbooru 的受限 tag：带这些 tag 的帖子只对 Gold 及以上等级开放原图（和 Rust 端一致）。 */
const GOLD_ONLY_TAGS = ["loli", "shota", "toddlercon"];

/** 没有原图地址是不是因为账号等级不够；其余情况（画师被封禁、图片下架）连 Gold 也拿不到。 */
export const goldOnly = (post: Post) =>
  post.source === "danbooru" && post.tags.general.some((tag) => GOLD_ONLY_TAGS.includes(tag));

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
