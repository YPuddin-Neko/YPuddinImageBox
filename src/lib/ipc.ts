import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";

export type Source = "danbooru" | "gelbooru" | "e621" | "rule34" | "kemono" | "yandere" | "pixiv" | "x" | "custom";
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
  e621: "e621",
  rule34: "Rule34.xxx",
  kemono: "Kemono",
  yandere: "Yande.re",
  pixiv: "Pixiv",
  x: "X",
  custom: "自定义导入",
};

/** 可直接请求接口的来源；X 使用单独的浏览器采集窗口。 */
export const SOURCES: Source[] = ["danbooru", "gelbooru", "e621", "rule34", "kemono", "yandere", "pixiv"];
export const SOURCE_OPTIONS = SOURCES.map((value) => ({ value, label: SOURCE_LABEL[value] }));

/** 几个站点的名字：全部站点时写「全部平台」，否则按固定顺序写站点名。 */
export function sourcesLabel(sources: Source[]): string {
  const chosen = SOURCES.filter((source) => sources.includes(source));
  if (chosen.length === SOURCES.length) return t("全部平台");
  return chosen.map((source) => SOURCE_LABEL[source]).join(t("、::list"));
}

export const ratingOptions = () => RATINGS.map((value) => ({ value, label: ratingLabel(value) }));

/** 站点搜索的排序；默认按上传先后，新的在前。 */
export type RemoteSort = "newest" | "oldest" | "score" | "favorites" | "popular" | "resolution" | "filesize";

const REMOTE_SORTS: { value: RemoteSort; label: Msg; hint?: Msg }[] = [
  { value: "newest", label: "最新上传" },
  { value: "oldest", label: "最早上传" },
  { value: "score", label: "分数最高" },
  { value: "favorites", label: "收藏最多" },
  { value: "popular", label: "近期热门", hint: "近两天" },
  { value: "resolution", label: "分辨率最高" },
  { value: "filesize", label: "文件最大" },
];

/** 各站点支持的排序，和 Rust 端的 `Sort::term` 一致。 */
const SITE_SORTS: Record<Source, RemoteSort[]> = {
  danbooru: ["newest", "oldest", "score", "favorites", "popular", "resolution", "filesize"],
  gelbooru: ["newest", "oldest", "score"],
  e621: ["newest", "oldest", "score", "favorites", "popular", "resolution", "filesize"],
  rule34: ["newest", "oldest", "score"],
  kemono: ["newest"],
  yandere: ["newest", "oldest", "score", "resolution"],
  pixiv: ["newest", "oldest"],
  x: [],
  custom: [],
};

/** 几个站点一起搜时能合在一起排的，和 Rust 端的 `combined::can_merge` 一致。 */
const MERGEABLE_SORTS: RemoteSort[] = ["newest", "oldest", "score", "resolution"];

/** 所选站点都支持的排序；几个站点一起搜时还要能合在一起排。 */
export function remoteSorts(sources: Source[]) {
  const usable = (sort: RemoteSort) =>
    sources.every((source) => SITE_SORTS[source].includes(sort)) && (sources.length < 2 || MERGEABLE_SORTS.includes(sort));
  return REMOTE_SORTS.filter((sort) => usable(sort.value)).map(({ value, label, hint }) => ({
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
  /** Pixiv 的多页作品有几页；只有一张图时没有。 */
  pages?: number | null;
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

/** Pixiv 的帖子 id 是「作品 id × 1000 + 页码」（和 Rust 端的 `pixiv::post_id` 一致）。 */
const PIXIV_PAGE_FACTOR = 1000;
/** Kemono 的帖子 id 是「服务序号 × 10¹³ + 帖子 id × 1000 + 第几张」（和 Rust 端的 `kemono::split_id` 一致）。 */
const KEMONO_SERVICE_FACTOR = 10_000_000_000_000;

/**
 * 界面上显示的编号（前面的 # 由文案自己写），和 Rust 端的 `Post::label` 一致：
 * Pixiv 显示作品 id、Kemono 显示帖子 id，第二页（张）起再写页码。`grouped` 时数字带千位分隔（详情面板的标题用）。
 */
export function postNumber(post: Pick<Post, "source" | "id">, { grouped = false } = {}): string {
  if (post.source === "x") return `x-${post.id.toString(16).padStart(16, "0")}`;
  const [id, page] =
    post.source === "pixiv"
      ? [Math.floor(post.id / PIXIV_PAGE_FACTOR), post.id % PIXIV_PAGE_FACTOR]
      : post.source === "kemono"
        ? [Math.floor((post.id % KEMONO_SERVICE_FACTOR) / 1000), post.id % 1000]
        : [post.id, 0];
  const number = grouped ? id.toLocaleString("en-US") : String(id);
  return page === 0 ? number : `${number} p${page + 1}`;
}

/** 站点没给尺寸的帖子（Kemono、部分 X 图片）宽高记为 1 × 1。 */
export const hasSize = (post: Pick<Post, "width" | "height">) => post.width > 1 && post.height > 1;

/** 帖子在界面上的唯一键。 */
export const postKey = (post: Pick<Post, "source" | "id">) => `${post.source}-${post.id}`;

/** 远程图片统一经 Rust 的 ibx:// 协议加载（代理、限速、缓存都在那一侧）。 */
export function imageSrc(url: string | null | undefined): string | undefined {
  return url ? convertFileSrc(url, "ibx") : undefined;
}

/** 查看器里的原图：同样经 ibx:// 加载，但不写进缓存（原图常有几十 MB），大小上限也放宽。 */
export function originalSrc(url: string | null | undefined): string | undefined {
  return url ? convertFileSrc(`full/${url}`, "ibx") : undefined;
}

/** 查看器要显示的原图地址：Pixiv 的作品要按页查一次，其他站点就是 fileUrl。没有原图时为 null。 */
export const originalUrl = (post: Post) => invoke<string | null>("original_url", { post });

/** 查看器能显示的格式，和下载时认作图片的扩展名一致。 */
const IMAGE_EXTS = ["jpg", "jpeg", "png", "gif", "webp", "avif"];

/** 原图是不是图片：视频、Ugoira 压缩包之类的查看器显示不了。没有扩展名时看地址。 */
export function originalIsImage(post: Pick<Post, "fileExt" | "fileUrl">): boolean {
  const name = (post.fileUrl ?? "").split(/[?#]/)[0].split("/").pop() ?? "";
  const ext = post.fileExt || (name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "");
  return IMAGE_EXTS.includes(ext.toLowerCase());
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
