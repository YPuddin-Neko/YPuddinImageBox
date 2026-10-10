import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";

export type Source = "danbooru" | "gelbooru" | "e621" | "rule34" | "kemono" | "yandere" | "pixiv" | "fanbox" | "x" | "custom";
export type PostId = number | string;
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
  fanbox: "FANBOX",
  x: "X",
  custom: "自定义导入",
};

/** 可直接请求接口的来源；X 使用单独的浏览器采集窗口。 */
export const SOURCES: Source[] = ["danbooru", "gelbooru", "e621", "rule34", "kemono", "yandere", "pixiv", "fanbox"];
export const SOURCE_OPTIONS = SOURCES.map((value) => ({ value, label: SOURCE_LABEL[value] }));
/** 聚合标签搜索的默认来源；FANBOX 只接受作者和投稿查询。 */
export const TAG_SEARCH_SOURCES = SOURCES.filter((source) => source !== "fanbox");

/** 几个站点的名字：全部站点时写「全部平台」，否则按固定顺序写站点名。 */
export function sourcesLabel(sources: Source[]): string {
  const chosen = SOURCES.filter((source) => sources.includes(source));
  if (chosen.length === SOURCES.length || (chosen.length === TAG_SEARCH_SOURCES.length && TAG_SEARCH_SOURCES.every((source) => chosen.includes(source)))) return t("全部平台");
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
  fanbox: ["newest"],
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
  id: PostId;
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
  fileName?: string | null;
  downloadIndex?: number | null;
  title?: string | null;
  tags: PostTags;
  /** Pixiv 的多页作品有几页；只有一张图时没有。 */
  pages?: number | null;
}

export interface SearchParams {
  pixivInput?: boolean;
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
  owned: PostId[];
  creators?: PixivCreator[];
  creatorError?: SearchError | null;
  artworkError?: SearchError | null;
}

export interface PixivCreator {
  id: string;
  name: string;
  avatarUrl: string | null;
}

export interface SearchError {
  code: string;
  message: string;
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
  pixivInput?: boolean;
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
  owned: { source: Source; postId: PostId }[];
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

/** 界面编号与下载资源的内部身份分开；FANBOX 使用投稿链接和下载顺序。 */
export function postNumber(post: Pick<Post, "source" | "id"> & Partial<Pick<Post, "fileUrl" | "postUrl" | "downloadIndex">>, { grouped = false } = {}): string {
  const raw = BigInt(post.id);
  if (post.source === "x") return `x-${raw.toString(16).padStart(16, "0")}`;
  let id = raw;
  let page = 0;
  if (post.source === "pixiv") {
    id = raw / 1000n;
    page = Number(raw % 1000n);
  } else if (post.source === "kemono") {
    id = raw % 10_000_000_000_000n / 1000n;
    page = Number(raw % 1000n);
  } else if (post.source === "fanbox") {
    const match = post.postUrl?.match(/\/posts\/(\d+)(?:[/?#]|$)/);
    id = match ? BigInt(match[1]) : raw / 1000n;
    page = post.downloadIndex == null ? Number(raw % 1000n) : Math.max(0, post.downloadIndex - 1);
  }
  const number = grouped ? id.toLocaleString("en-US") : id.toString();
  if (post.source === "fanbox" && (post.downloadIndex === 0 || raw % 1000n === 999n)) {
    if (!("fileUrl" in post)) return number;
    try {
      const url = new URL(post.fileUrl ?? "");
      if (url.protocol === "https:" && url.hostname === "pixiv.pximg.net" && !url.port && !url.username && !url.password && !url.hash
        && url.pathname.startsWith(`/fanbox/public/images/post/${id}/cover/`)) {
        return `${number} ${t("封面")}`;
      }
    } catch { /* 缺少有效地址时仍按普通资源显示。 */ }
  }
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

/** 图库里这张图的本地原图：按帖子找，找不到再按 md5 找同一张图（可能是从别的站点下载的）。 */
export function localFileSrc(post: Pick<Post, "source" | "id" | "md5">): string | undefined {
  const md5 = post.md5 && /^[0-9a-f]{32}$/i.test(post.md5) ? `/${post.md5.toLowerCase()}` : "";
  return imageSrc(`local/file/${post.source}/${post.id}${md5}`);
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

/** FANBOX 的附件保留文件名，以文件操作打开，不送入图片查看器。 */
export const isFanboxFile = (post: Pick<Post, "source" | "fileName" | "fileExt" | "fileUrl">): boolean =>
  post.source === "fanbox" && !!post.fileName && !originalIsImage(post);

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
