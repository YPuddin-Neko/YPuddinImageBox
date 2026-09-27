import { Icon } from "../../components/Icon";
import { t } from "../../lib/i18n";
import { useTheme } from "../../theme/context";
import { findTheme, themeMood, themeName, THEMES, type ThemeInfo, type ThemeMode } from "../../theme/themes";

const modeLabel = (mode: ThemeMode) => (mode === "dark" ? t("深色") : t("浅色"));

function MiniPreview({ theme }: { theme: ThemeInfo }) {
  return (
    <span className="mini" data-theme={theme.id} aria-hidden="true">
      <i className="mini-rail" />
      <i className="mini-bar" />
      <i className="mini-grid">
        <i />
        <i />
        <i />
      </i>
      <i className="mini-panel">
        <i />
        <i />
        <i />
        <b />
      </i>
    </span>
  );
}

export function Appearance() {
  const { prefs, activeId, chooseTheme, setFollowSystem } = useTheme();
  const dark = findTheme(prefs.darkId);
  const light = findTheme(prefs.lightId);
  const darkCount = THEMES.filter((t) => t.mode === "dark").length;

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>{t("外观")}</h1>
        <p>{t("主题和跟随系统，改动立即生效。")}</p>
      </header>

      <div className="section-head">
        <h2 id="theme-title">{t("主题")}</h2>
        <span>{t("{dark} 套深色 · {light} 套浅色", { dark: darkCount, light: THEMES.length - darkCount })}</span>
      </div>
      <div className="theme-grid" role="group" aria-labelledby="theme-title">
        {THEMES.map((theme) => {
          const slot = prefs.followSystem
            ? theme.id === prefs.darkId
              ? t("深色时使用")
              : theme.id === prefs.lightId
                ? t("浅色时使用")
                : null
            : null;
          const inUse = prefs.followSystem ? slot !== null : theme.id === prefs.themeId;
          return (
            <button
              key={theme.id}
              type="button"
              className="theme-card"
              aria-pressed={inUse}
              aria-label={t("{name}，{mode}，{mood}", {
                name: themeName(theme),
                mode: modeLabel(theme.mode),
                mood: themeMood(theme),
              })}
              title={themeMood(theme)}
              onClick={() => chooseTheme(theme.id)}
            >
              <MiniPreview theme={theme} />
              <span className="theme-name">
                <b>{themeName(theme)}</b>
                <small>{slot ?? modeLabel(theme.mode)}</small>
              </span>
              {theme.id === activeId && (
                <span className="theme-check" aria-hidden="true">
                  <Icon name="check" size={12} />
                </span>
              )}
            </button>
          );
        })}
      </div>

      <div className="setting-row">
        <div>
          <b id="follow-system-label">{t("跟随系统")}</b>
          <span>
            {t("系统切换深浅色时自动换主题：深色用{dark}，浅色用{light}。开启后点主题卡片，替换的是对应明暗的那一套。", {
              dark: dark ? themeName(dark) : "",
              light: light ? themeName(light) : "",
            })}
          </span>
        </div>
        <button
          type="button"
          id="follow-system"
          role="switch"
          className="switch"
          aria-checked={prefs.followSystem}
          aria-labelledby="follow-system-label"
          onClick={() => setFollowSystem(!prefs.followSystem)}
        >
          <span />
        </button>
      </div>
    </div>
  );
}
