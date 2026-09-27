import appIcon from "../assets/app-icon.png";
import { Icon, type IconName } from "./Icon";

export type View = "discover" | "library" | "subscriptions" | "downloads" | "settings";

interface Item {
  view: View;
  label: string;
  icon: IconName;
}

const ITEMS: Item[] = [
  { view: "discover", label: "发现", icon: "compass" },
  { view: "library", label: "图库", icon: "library" },
  { view: "subscriptions", label: "订阅", icon: "bell" },
  { view: "downloads", label: "下载", icon: "download" },
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
    return (
      <button
        key={item.view}
        type="button"
        className="rail-btn"
        aria-label={badge > 0 ? `${item.label}（${badge} 个进行中）` : item.label}
        title={item.label}
        aria-current={view === item.view ? "page" : undefined}
        onClick={() => onChange(item.view)}
      >
        <Icon name={item.icon} size={20} />
        {badge > 0 && <span className="rail-badge">{badge > 99 ? "99+" : badge}</span>}
      </button>
    );
  };
  return (
    <nav className="rail" aria-label="主导航" data-tauri-drag-region>
      <img className="rail-logo" src={appIcon} alt="" draggable={false} data-tauri-drag-region />
      {ITEMS.map(button)}
      <span className="rail-space" data-tauri-drag-region />
      {button(SETTINGS)}
    </nav>
  );
}
