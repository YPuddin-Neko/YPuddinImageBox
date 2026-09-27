import { t } from "./i18n";

export const isMac = /Mac/.test(navigator.userAgent);
export const isWindows = /Windows/.test(navigator.userAgent);

/** 在系统文件管理器里显示的按钮文字。 */
export const revealLabel = () => (isMac ? t("在访达中显示") : t("在资源管理器中显示"));

/** 系统的废纸篓叫法。 */
export const trashLabel = () => (isMac ? t("废纸篓") : t("回收站"));
