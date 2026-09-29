import appIcon from "../assets/app-icon.png";
import { t, type Msg } from "../lib/i18n";
import { Icon, type IconName } from "./Icon";

export type View = "discover" | "favorites" | "library" | "x" | "subscriptions" | "downloads" | "settings";

interface Item {
  view: View;
  label: Msg;
  icon: IconName;
}

const ITEMS: Item[] = [
  { view: "discover", label: "发现", icon: "compass" },
  { view: "favorites", label: "收藏", icon: "heart" },
  { view: "library", label: "图库", icon: "library" },
  { view: "x", label: "X 媒体采集", icon: "globe" },
  { view: "subscriptions", label: "订阅::page", icon: "bell" },
  { view: "downloads", label: "下载::page", icon: "download" },
];

const SETTINGS: Item = { view: "settings", label: "设置", icon: "gear" };

interface RailProps {
  view: View;
  onChange: (view: View) => void;
  /** 图标右上角的数字，例如进行中的下载任务数。 */
  badges?: Partial<Record<View, number>>;
}

export function Rail({ view, onChange, badges = {} }: RailProps) {
  const button = (item: Item) => {
    const badge = badges[item.view] ?? 0;
    const label = t(item.label);
    return (
      <button
        key={item.view}
        type="button"
        className="rail-btn"
        aria-label={badge > 0 ? t("{label}（{n} 个进行中）", { label, n: badge }) : label}
        title={label}
        aria-current={view === item.view ? "page" : undefined}
        onClick={() => onChange(item.view)}
      >
        <Icon name={item.icon} size={20} />
        {badge > 0 && <span className="rail-badge">{badge > 99 ? "99+" : badge}</span>}
      </button>
    );
  };
  return (
    <nav className="rail" aria-label={t("主导航")} data-tauri-drag-region>
      <img className="rail-logo" src={appIcon} alt="" draggable={false} data-tauri-drag-region />
      {ITEMS.map(button)}
      <span className="rail-space" data-tauri-drag-region />
      {button(SETTINGS)}
    </nav>
  );
}
