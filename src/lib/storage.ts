import { invoke } from "@tauri-apps/api/core";

export type StorageKind = "images" | "database" | "data" | "cache";
export type ChangeMode = "move" | "leave";

export interface LocationInfo {
  kind: StorageKind;
  label: string;
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
