import { invoke } from "@tauri-apps/api/core";

import { formatCount } from "./format";
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

/** 文件夹、分组卡片上扇形展开的封面，按下载时间从新到旧。 */
export interface Cover {
  source: Source;
  postId: number;
  width: number;
  height: number;
  thumbUrl: string;
}

/** 图库首页按来源分的文件夹。 */
export interface Folder {
  source: Source;
  count: number;
  /** 最近一次下载的时间；文件夹是空的时为 null。 */
  latestAt: number | null;
  covers: Cover[];
}

/** 文件夹里按哪类 tag 分组。 */
export type GroupKind = "artist" | "copyright" | "character" | "general";
export type GroupSort = "recent" | "count" | "name";

export interface Group {
  name: string;
  count: number;
  latestAt: number;
  covers: Cover[];
}

export interface GroupPage {
  groups: Group[];
  /** 这个文件夹里一共有多少组。 */
  total: number;
  hasMore: boolean;
}

const GROUP_KINDS: { value: GroupKind; label: Msg; total: Msg }[] = [
  { value: "artist", label: "画师", total: "{n} 位画师" },
  { value: "copyright", label: "作品", total: "{n} 部作品" },
  { value: "character", label: "角色", total: "{n} 个角色" },
  { value: "general", label: "一般 tag", total: "{n} 个 tag" },
];

const GROUP_SORTS: { value: GroupSort; label: Msg }[] = [
  { value: "recent", label: "最近下载" },
  { value: "count", label: "图片最多" },
  { value: "name", label: "名称" },
];

/** 各来源能怎么分组：Gelbooru、Yande.re 的 tag 不分类别，只能按一般 tag；Pixiv 只分画师和作品上的 tag。 */
const SOURCE_GROUP_KINDS: Record<Source, GroupKind[]> = {
  danbooru: ["artist", "copyright", "character", "general"],
  gelbooru: ["general"],
  yandere: ["general"],
  pixiv: ["artist", "general"],
};

export function groupKinds(source: Source) {
  const kinds = SOURCE_GROUP_KINDS[source];
  return GROUP_KINDS.filter(({ value }) => kinds.includes(value)).map(({ value, label }) => ({
    value,
    label: t(label),
  }));
}

export const groupSorts = () => GROUP_SORTS.map(({ value, label }) => ({ value, label: t(label) }));

/** 「128 位画师」这样的组数说明。 */
export function groupTotal(kind: GroupKind, total: number): string {
  const entry = GROUP_KINDS.find((item) => item.value === kind);
  return entry ? t(entry.total, { n: formatCount(total) }) : "";
}

export const libraryFolders = () => invoke<Folder[]>("library_folders");
export const libraryGroups = (query: { source: Source; kind: GroupKind; sort: GroupSort; offset: number; limit: number }) =>
  invoke<GroupPage>("library_groups", { query });

/** 从图库删除；keepFiles 为 false 时图片移到废纸篓（回收站）。 */
export const libraryDelete = (posts: PostRef[], keepFiles: boolean) =>
  invoke<DeleteOutcome>("library_delete", { posts, keepFiles });
