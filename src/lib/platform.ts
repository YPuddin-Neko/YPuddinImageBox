export const isMac = /Mac/.test(navigator.userAgent);

/** 在系统文件管理器里显示的按钮文字。 */
export const revealLabel = isMac ? "在访达中显示" : "在资源管理器中显示";

/** 系统的废纸篓叫法。 */
export const trashLabel = isMac ? "废纸篓" : "回收站";
