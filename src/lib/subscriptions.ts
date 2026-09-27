import { invoke } from "@tauri-apps/api/core";

import type { Job } from "./downloads";
import type { Rating, SearchParams, Source } from "./ipc";

export interface Subscription {
  id: number;
  source: Source;
  /** 用户填写的 tag（不含分级）。 */
  tags: string;
  ratings: Rating[];
  /** 发给站点的查询串。 */
  query: string;
  enabled: boolean;
  intervalMinutes: number;
  /** 已经处理到的最大帖子 id。 */
  lastSeenId: number;
  lastCheckedAt: number | null;
  /** 最近一次检查找到的新图张数。 */
  lastNew: number;
  lastError: string | null;
  createdAt: number;
  updatedAt: number;
  /** 还在排队、下载或暂停中的检查任务。 */
  activeJob: number | null;
  /** 超出 tag 上限、在本地筛选的 tag。 */
  localFilter: string | null;
}

/** 可选的检查间隔（分钟）。 */
export const INTERVALS: { minutes: number; label: string }[] = [
  { minutes: 60, label: "每小时" },
  { minutes: 180, label: "每 3 小时" },
  { minutes: 360, label: "每 6 小时" },
  { minutes: 720, label: "每 12 小时" },
  { minutes: 1440, label: "每天" },
];

export const intervalLabel = (minutes: number) =>
  INTERVALS.find((option) => option.minutes === minutes)?.label ?? `每 ${minutes} 分钟`;

/** 下拉框的选项；当前间隔不在列表里时（例如旧版本设的）也列出来。 */
export function intervalOptions(current?: number) {
  const options = INTERVALS.map((option) => ({ value: option.minutes, label: option.label }));
  if (current !== undefined && !INTERVALS.some((option) => option.minutes === current)) {
    options.unshift({ value: current, label: intervalLabel(current) });
  }
  return options;
}

export const subscriptionTitle = (sub: Pick<Subscription, "tags">) => sub.tags.trim() || "全部帖子";

export const SUBSCRIPTION_EVENT = "subscription-updated";

export const subscriptionsList = () => invoke<Subscription[]>("subscriptions_list");
export const subscriptionCreate = (
  params: SearchParams,
  intervalMinutes: number,
  downloadExisting: boolean,
  maxPosts: number | null,
) => invoke<Subscription>("subscription_create", { params, intervalMinutes, downloadExisting, maxPosts });
/** 订阅实际使用的条件：订阅按上传先后找新图、不带排序，超出 tag 上限时的拆分可能和当前搜索不同。 */
export const subscriptionPreview = (params: SearchParams) =>
  invoke<{ query: string; localFilter: string }>("subscription_preview", { params });
export const subscriptionUpdate = (id: number, change: { enabled?: boolean; intervalMinutes?: number }) =>
  invoke<Subscription>("subscription_update", { id, ...change });
export const subscriptionDelete = (id: number) => invoke<void>("subscription_delete", { id });
/** 立即检查；已有检查任务在队列里时返回 null。 */
export const subscriptionCheck = (id: number) => invoke<Job | null>("subscription_check", { id });
export const subscriptionsCheckAll = () => invoke<number>("subscriptions_check_all");
