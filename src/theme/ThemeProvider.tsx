import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import {
  findTheme,
  loadPrefs,
  resolveThemeId,
  savePrefs,
  switchTheme,
  systemDarkQuery,
  type AppearancePrefs,
} from "./themes";
import { ThemeContext } from "./context";

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<AppearancePrefs>(loadPrefs);
  const [systemDark, setSystemDark] = useState(() => systemDarkQuery().matches);

  useEffect(() => {
    const query = systemDarkQuery();
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const activeId = resolveThemeId(prefs, systemDark);

  useEffect(() => {
    switchTheme(activeId);
  }, [activeId]);

  useEffect(() => {
    savePrefs(prefs);
  }, [prefs]);

  const chooseTheme = useCallback((id: string) => {
    const theme = findTheme(id);
    if (!theme) return;
    setPrefs((prev) => {
      if (!prev.followSystem) return { ...prev, themeId: id };
      return theme.mode === "dark" ? { ...prev, darkId: id } : { ...prev, lightId: id };
    });
  }, []);

  const setFollowSystem = useCallback((follow: boolean) => {
    setPrefs((prev) => ({ ...prev, followSystem: follow }));
  }, []);

  const value = useMemo(
    () => ({ prefs, activeId, chooseTheme, setFollowSystem }),
    [prefs, activeId, chooseTheme, setFollowSystem],
  );
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
