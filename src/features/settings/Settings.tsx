import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Icon, type IconName } from "../../components/Icon";
import { EASE_OUT } from "../../lib/motion";
import { Appearance } from "./Appearance";
import { StorageSettings } from "./StorageSettings";

type Section = "appearance" | "storage";

const SECTIONS: { id: Section; label: string; icon: IconName }[] = [
  { id: "appearance", label: "外观", icon: "palette" },
  { id: "storage", label: "存储", icon: "folder" },
];

export function Settings() {
  const [section, setSection] = useState<Section>("appearance");
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
            onClick={() => setSection(item.id)}
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
          {section === "appearance" ? <Appearance /> : <StorageSettings />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
