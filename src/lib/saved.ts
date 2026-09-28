import { invoke } from "@tauri-apps/api/core";

import { t } from "./i18n";
import { ratingLabel, RATINGS, remoteSortLabel, scopeLabel, type Rating, type RemoteSort, type Scope } from "./ipc";

/** 收藏的搜索条件。`ratings` 为空表示全选；`source` 为 all 表示聚合搜索。 */
export interface SavedSearch {
  id: number;
  source: Scope;
  tags: string;
  ratings: Rating[];
  sort: RemoteSort;
  createdAt: number;
}

export const savedSearchesList = () => invoke<SavedSearch[]>("saved_searches_list");
export const savedSearchAdd = (params: { source: Scope; tags: string; ratings: Rating[]; sort: RemoteSort }) =>
  invoke<SavedSearch[]>("saved_search_add", { params });
export const savedSearchRemove = (id: number) => invoke<SavedSearch[]>("saved_search_remove", { id });

/** 分级按固定顺序排好；全选和都不选都记成空，和 Rust 端存的一致。 */
const normalizedRatings = (ratings: Rating[]) => {
  const chosen = RATINGS.filter((rating) => ratings.includes(rating));
  return chosen.length === RATINGS.length ? [] : chosen;
};

/** 和收藏里的条件是否相同：tag 之间的空格、分级顺序都不影响。 */
export function sameSearch(saved: SavedSearch, criteria: { scope: Scope; tags: string; ratings: Rating[]; sort: RemoteSort }) {
  return (
    saved.source === criteria.scope &&
    saved.sort === criteria.sort &&
    saved.tags === criteria.tags.split(/\s+/).filter(Boolean).join(" ") &&
    normalizedRatings(saved.ratings).join(",") === normalizedRatings(criteria.ratings).join(",")
  );
}

export const savedTitle = (saved: SavedSearch) => saved.tags || t("全部帖子");

/** 菜单里标在右侧的说明：站点、分级，排序不是默认时也写上。 */
export function savedHint(saved: SavedSearch) {
  const ratings = saved.ratings.length ? saved.ratings.map(ratingLabel).join(t("、::list")) : t("全部分级");
  const parts = [scopeLabel(saved.source), ratings];
  if (saved.sort !== "newest") parts.push(remoteSortLabel(saved.sort));
  return parts.join(" · ");
}
