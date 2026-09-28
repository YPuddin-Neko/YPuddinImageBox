import { invoke } from "@tauri-apps/api/core";

import type { Language, LanguageSetting } from "./i18n";
import type { Source } from "./ipc";

export interface AccountView {
  source: Source;
  /** 用户名（Gelbooru 是 User ID，Pixiv 是账号昵称）；未登录时为 null。 */
  name: string | null;
  level: string | null;
  /** 设置里记着账号，但钥匙串里找不到 API Key。 */
  keyMissing: boolean;
}

/** API Key 的保存方式：系统钥匙串，或加密后存在设置文件里。 */
export type KeyStorage = "keychain" | "file";

export interface AccountsInfo {
  accounts: AccountView[];
  keyStorage: KeyStorage;
  /** 启动时读取 API Key 失败的原因。 */
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
/** 切换保存方式，已保存的 Key 一起搬过去。 */
export const accountKeyStorage = (storage: KeyStorage) => invoke<AccountsInfo>("account_key_storage", { storage });

/** Pixiv 登录窗口的情况：还在等、被关掉了，或者已经登录（账号已保存）。 */
export type PixivLogin = { status: "waiting" } | { status: "closed" } | { status: "signedIn"; info: AccountsInfo };

/** 打开 Pixiv 的登录页；已经开着时切到前面。 */
export const pixivLoginOpen = () => invoke<void>("pixiv_login_open");
export const pixivLoginCheck = () => invoke<PixivLogin>("pixiv_login_check");

export const proxyInfo = () => invoke<ProxySettings>("proxy_info");
export const proxySave = (proxy: ProxySettings) => invoke<ProxySettings>("proxy_save", { proxy });
/** 用还没保存的设置试连一次，返回耗时（毫秒）。 */
export const proxyTest = (proxy: ProxySettings) => invoke<number>("proxy_test", { proxy });

export interface GeneralSettings {
  /** 关闭窗口后在后台继续运行。 */
  closeToTray: boolean;
  /** 登录系统后自动在后台启动。 */
  launchAtLogin: boolean;
  language: LanguageSetting;
}

export interface GeneralInfo extends GeneralSettings {
  /** 实际使用的语言（跟随系统时是系统语言对应的那一种）。 */
  resolvedLanguage: Language;
  /** 日志文件的完整路径。 */
  logFile: string;
}

export const generalInfo = () => invoke<GeneralInfo>("general_info");
export const generalSave = ({ closeToTray, launchAtLogin, language }: GeneralSettings) =>
  invoke<GeneralInfo>("general_save", { closeToTray, launchAtLogin, language });
/** 当前界面语言，启动时渲染前先问一次。 */
export const languageCurrent = () => invoke<Language>("language_current");
