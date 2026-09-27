import type { ComponentType } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Icon, type IconName } from "../../components/Icon";
import { EASE_OUT } from "../../lib/motion";
import { AccountsSettings } from "./AccountsSettings";
import { Appearance } from "./Appearance";
import { NetworkSettings } from "./NetworkSettings";
import { StorageSettings } from "./StorageSettings";

export type SettingsSection = "appearance" | "accounts" | "network" | "storage";

const SECTIONS: { id: SettingsSection; label: string; icon: IconName }[] = [
  { id: "appearance", label: "外观", icon: "palette" },
  { id: "accounts", label: "账号", icon: "user" },
  { id: "network", label: "网络", icon: "globe" },
  { id: "storage", label: "存储", icon: "folder" },
];

const PAGES: Record<SettingsSection, ComponentType> = {
  appearance: Appearance,
  accounts: AccountsSettings,
  network: NetworkSettings,
  storage: StorageSettings,
};

/** 当前栏由外层管理，其他页面可以直接打开某一栏（例如「去填写账号」）。 */
export function Settings({
  section,
  onSectionChange,
}: {
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
}) {
  const Page = PAGES[section];
  return (
    <div className="settings">
      <nav className="settings-nav" aria-label="设置分类">
        <p className="settings-nav-title" data-tauri-drag-region>
          设置
        </p>
        {SECTIONS.map((item) => (
          <button
            key={item.id}
            type="button"
            className="settings-nav-item"
            aria-current={section === item.id ? "page" : undefined}
            onClick={() => onSectionChange(item.id)}
          >
            <Icon name={item.icon} size={17} />
            {item.label}
          </button>
        ))}
      </nav>
      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={section}
          className="settings-body"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, transition: { duration: 0.1 } }}
          transition={{ duration: 0.18, ease: EASE_OUT }}
        >
          <Page />
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
