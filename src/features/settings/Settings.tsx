import { Icon } from "../../components/Icon";
import { useTheme } from "../../theme/ThemeProvider";
import { findTheme, THEMES, type ThemeInfo } from "../../theme/themes";

const MODE_LABEL = { dark: "深色", light: "浅色" } as const;

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

export function Settings() {
  const { prefs, activeId, chooseTheme, setFollowSystem } = useTheme();
  const darkName = findTheme(prefs.darkId)?.name;
  const lightName = findTheme(prefs.lightId)?.name;
  const darkCount = THEMES.filter((t) => t.mode === "dark").length;

  return (
    <div className="settings">
      <header className="settings-head" data-tauri-drag-region>
        <p className="eyebrow">设置</p>
        <h1>外观</h1>
        <p>主题和跟随系统，改动立即生效。</p>
      </header>

      <div className="section-head">
        <h2 id="theme-title">主题</h2>
        <span>
          {darkCount} 套深色 · {THEMES.length - darkCount} 套浅色
        </span>
      </div>
      <div className="theme-grid" role="group" aria-labelledby="theme-title">
        {THEMES.map((theme) => {
          const slot = prefs.followSystem
            ? theme.id === prefs.darkId
              ? "深色时使用"
              : theme.id === prefs.lightId
                ? "浅色时使用"
                : null
            : null;
          const inUse = prefs.followSystem ? slot !== null : theme.id === prefs.themeId;
          return (
            <button
              key={theme.id}
              type="button"
              className="theme-card"
              aria-pressed={inUse}
              aria-label={`${theme.name}，${MODE_LABEL[theme.mode]}，${theme.mood}`}
              title={theme.mood}
              onClick={() => chooseTheme(theme.id)}
            >
              <MiniPreview theme={theme} />
              <span className="theme-name">
                <b>{theme.name}</b>
                <small>{slot ?? MODE_LABEL[theme.mode]}</small>
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
          <b id="follow-system-label">跟随系统</b>
          <span>
            系统切换深浅色时自动换主题：深色用{darkName}，浅色用{lightName}。开启后点主题卡片，替换的是对应明暗的那一套。
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
