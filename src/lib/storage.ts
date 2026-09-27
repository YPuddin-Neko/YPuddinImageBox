import { invoke } from "@tauri-apps/api/core";

import { t, type Msg } from "./i18n";

export type StorageKind = "images" | "database" | "data" | "cache";
export type ChangeMode = "move" | "leave";

const LABEL: Record<StorageKind, Msg> = { images: "图片", database: "数据库", data: "软件数据", cache: "缓存" };
/** 放在句子中间时的叫法（英文是小写、带冠词）。 */
const NOUN: Record<StorageKind, Msg> = {
  images: "图片::noun",
  database: "数据库::noun",
  data: "软件数据::noun",
  cache: "缓存::noun",
};

export const storageLabel = (kind: StorageKind) => t(LABEL[kind]);
export const storageNoun = (kind: StorageKind) => t(NOUN[kind]);

export interface LocationInfo {
  kind: StorageKind;
  path: string;
  defaultPath: string;
  isDefault: boolean;
  /** 当前位置里是否已有内容（决定修改时要不要问怎么处理）。 */
  hasData: boolean;
  /** 数据库、软件数据：修改后重启才生效。 */
  appliesOnRestart: boolean;
  pending: { to: string; mode: ChangeMode } | null;
}

export interface StorageInfo {
  locations: LocationInfo[];
  configFile: string;
  lastError: string | null;
}

export interface ChangeOutcome {
  applied: boolean;
  info: StorageInfo;
}

export const storageInfo = () => invoke<StorageInfo>("storage_info");
export const storageUsage = (kind: StorageKind) => invoke<number>("storage_usage", { kind });
export const storageChange = (kind: StorageKind, path: string | null, mode: ChangeMode) =>
  invoke<ChangeOutcome>("storage_change", { kind, path, mode });
export const storageCancelPending = (kind: StorageKind) => invoke<StorageInfo>("storage_cancel_pending", { kind });
export const storageDismissError = () => invoke<StorageInfo>("storage_dismiss_error");
export const storagePrepare = (kind: StorageKind) => invoke<string>("storage_prepare", { kind });
export const restartApp = () => invoke<void>("restart_app");
