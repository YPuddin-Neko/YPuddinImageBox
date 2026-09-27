import { invoke } from "@tauri-apps/api/core";

import type { Source } from "./ipc";

export interface AccountView {
  source: Source;
  /** 用户名（Gelbooru 是 User ID）；未登录时为 null。 */
  name: string | null;
  level: string | null;
  /** 设置里记着账号，但钥匙串里找不到 API Key。 */
  keyMissing: boolean;
}

export interface AccountsInfo {
  accounts: AccountView[];
  /** 启动时读取钥匙串失败的原因。 */
  error: string | null;
}

export type ProxyMode = "system" | "none" | "manual";

export interface ProxySettings {
  mode: ProxyMode;
  url: string;
}

export const accountsInfo = () => invoke<AccountsInfo>("accounts_info");
export const accountSave = (source: Source, name: string, apiKey: string) =>
  invoke<AccountsInfo>("account_save", { source, name, apiKey });
export const accountRemove = (source: Source) => invoke<AccountsInfo>("account_remove", { source });

export const proxyInfo = () => invoke<ProxySettings>("proxy_info");
export const proxySave = (proxy: ProxySettings) => invoke<ProxySettings>("proxy_save", { proxy });
/** 用还没保存的设置试连一次，返回耗时（毫秒）。 */
export const proxyTest = (proxy: ProxySettings) => invoke<number>("proxy_test", { proxy });
