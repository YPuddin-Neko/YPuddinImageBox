import { Icon, type IconName } from "./Icon";

export type View = "discover" | "settings";

const ITEMS: { view: View; label: string; icon: IconName }[] = [{ view: "discover", label: "发现", icon: "compass" }];

export function Rail({ view, onChange }: { view: View; onChange: (view: View) => void }) {
  const button = (item: { view: View; label: string; icon: IconName }) => (
    <button
      key={item.view}
      type="button"
      className="rail-btn"
      aria-label={item.label}
      title={item.label}
      aria-current={view === item.view ? "page" : undefined}
      onClick={() => onChange(item.view)}
    >
      <Icon name={item.icon} size={20} />
    </button>
  );
  return (
    <nav className="rail" aria-label="主导航" data-tauri-drag-region>
      <span className="rail-logo" aria-hidden="true">
        <Icon name="box" size={20} />
      </span>
      {ITEMS.map(button)}
      <span className="rail-space" data-tauri-drag-region />
      {button({ view: "settings", label: "设置", icon: "gear" })}
    </nav>
  );
}
