import { invoke } from "@tauri-apps/api/core";

import type { Msg } from "./i18n";
import type { Source } from "./ipc";
import type { AccountView } from "./settings";

/** 收藏页列出的站点，顺序就是下拉框里的顺序。Rule34.xxx 的收藏只有网页、没有接口，暂不支持。 */
export const FAVORITE_SITES = ["danbooru", "gelbooru", "yandere", "e621", "pixiv", "fanbox", "kemono", "x"] as const satisfies readonly Source[];
export type FavoriteSite = (typeof FAVORITE_SITES)[number];

/** 同一站点里的几种收藏；FANBOX 列出关注或赞助的创作者。 */
export type FavoriteMode = "public" | "private" | "posts" | "creators" | "following" | "supporting" | "likes" | "bookmarks";

export const FAVORITE_MODES: Partial<Record<FavoriteSite, { value: FavoriteMode; label: Msg }[]>> = {
  pixiv: [
    { value: "public", label: "公开" },
    { value: "private", label: "非公开" },
  ],
  fanbox: [
    { value: "following", label: "关注的创作者" },
    { value: "supporting", label: "赞助的创作者" },
  ],
  kemono: [
    { value: "posts", label: "帖子" },
    { value: "creators", label: "作者" },
  ],
  x: [
    { value: "likes", label: "喜欢" },
    { value: "bookmarks", label: "书签" },
  ],
};

/** 能按分级筛选的站点：站点搜索认 rating:，或者适配器自己筛。 */
export const RATED_FAVORITES: readonly FavoriteSite[] = ["danbooru", "gelbooru", "yandere", "pixiv", "fanbox"];

/** 填好了账号才能列收藏。Yande.re 只存用户名；X 的登录在采集窗口里，这里不判断。 */
export const signedIn = (account: AccountView | undefined) => !!account?.name && !account.keyMissing;

/**
 * 收藏页发给搜索接口的条件。Danbooru、Gelbooru、Yande.re 用站点自己的写法（按收藏先后排）；
 * e621、Pixiv、Kemono 的 `favorites:`、`bookmarks:` 由各自的适配器换成站点的收藏接口。
 */
export function favoritesQuery(site: FavoriteSite, account: AccountView | undefined, mode: FavoriteMode): string | null {
  if (site === "x" || site === "fanbox" || !account?.name || account.keyMissing) return null;
  const name = account.name;
  const queries: Record<Exclude<FavoriteSite, "x" | "fanbox">, string> = {
    danbooru: `ordfav:${name}`,
    gelbooru: `fav:${name}`,
    yandere: `vote:3:${name} order:vote`,
    e621: "favorites:",
    pixiv: mode === "private" ? "bookmarks:private" : "bookmarks:",
    kemono: "favorites:",
  };
  return queries[site];
}

/** Kemono 收藏的作者，或 FANBOX 关注、赞助的创作者。 */
export interface FavoriteCreator {
  id: string;
  name: string;
  service: string;
  avatarUrl?: string | null;
  /** 最近更新的时间（不带时区的 UTC）。 */
  updated: string | null;
}

export const kemonoFavoriteCreators = () => invoke<FavoriteCreator[]>("kemono_favorite_creators");
export const fanboxFavoriteCreators = (mode: "following" | "supporting") =>
  invoke<FavoriteCreator[]>("fanbox_favorite_creators", { mode });
