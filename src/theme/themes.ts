import data from "./themes.json";

export type ThemeMode = "dark" | "light";

export interface ThemeInfo {
  id: string;
  name: string;
  mode: ThemeMode;
  mood: string;
}

export const THEMES: ThemeInfo[] = data.themes.map(({ id, name, mode, mood }) => ({
  id,
  name,
  mode: mode as ThemeMode,
  mood,
}));

export const DEFAULT_THEME: Record<ThemeMode, string> = {
  dark: data.defaults.dark,
  light: data.defaults.light,
};

export function findTheme(id: string | null | undefined): ThemeInfo | undefined {
  return THEMES.find((theme) => theme.id === id);
}

export interface AppearancePrefs {
  /** 不跟随系统时使用的主题。 */
  themeId: string;
  followSystem: boolean;
  /** 跟随系统时，系统为深色 / 浅色分别使用的主题。 */
  darkId: string;
  lightId: string;
}

const STORAGE_KEY = "imagebox:appearance";

export const DEFAULT_PREFS: AppearancePrefs = {
  themeId: DEFAULT_THEME.dark,
  followSystem: false,
  darkId: DEFAULT_THEME.dark,
  lightId: DEFAULT_THEME.light,
};

function validId(id: unknown, mode?: ThemeMode): id is string {
  const theme = typeof id === "string" ? findTheme(id) : undefined;
  return Boolean(theme && (!mode || theme.mode === mode));
}

export function loadPrefs(): AppearancePrefs {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<AppearancePrefs> | null;
    if (!raw) return DEFAULT_PREFS;
    return {
      themeId: validId(raw.themeId) ? raw.themeId : DEFAULT_PREFS.themeId,
      followSystem: raw.followSystem === true,
      darkId: validId(raw.darkId, "dark") ? raw.darkId : DEFAULT_PREFS.darkId,
      lightId: validId(raw.lightId, "light") ? raw.lightId : DEFAULT_PREFS.lightId,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function savePrefs(prefs: AppearancePrefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // 存储不可用时本次运行仍按内存里的设置显示。
  }
}

export function resolveThemeId(prefs: AppearancePrefs, systemDark: boolean): string {
  if (!prefs.followSystem) return prefs.themeId;
  return systemDark ? prefs.darkId : prefs.lightId;
}

export function applyTheme(id: string): void {
  document.documentElement.dataset.theme = id;
}

/** 换主题：支持 View Transitions 时整窗淡入新配色；不支持时给颜色加一段短暂过渡。 */
export function switchTheme(id: string): void {
  const root = document.documentElement;
  if (root.dataset.theme === id) return;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    applyTheme(id);
    return;
  }
  if (typeof document.startViewTransition === "function") {
    document.startViewTransition(() => applyTheme(id));
    return;
  }
  root.classList.add("theme-fade");
  applyTheme(id);
  window.setTimeout(() => root.classList.remove("theme-fade"), 260);
}

export const systemDarkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");
