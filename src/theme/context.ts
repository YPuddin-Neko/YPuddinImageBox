import { createContext, useContext } from "react";

import type { AppearancePrefs } from "./themes";

// 和下载状态一样，context 单独放一个文件，热更新 ThemeProvider 时不会生成第二个 context。

export interface ThemeContextValue {
  prefs: AppearancePrefs;
  activeId: string;
  /** 不跟随系统时直接换主题；跟随系统时替换该主题所属明暗那一套。 */
  chooseTheme: (id: string) => void;
  setFollowSystem: (follow: boolean) => void;
}

export const ThemeContext = createContext<ThemeContextValue | null>(null);

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (!value) throw new Error("useTheme 需要放在 ThemeProvider 里面");
  return value;
}
