import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

import { formatCount } from "../lib/format";
import { t } from "../lib/i18n";
import { PANEL_ENTER } from "../lib/motion";

interface SelectionDockProps {
  count: number;
  /** 当前已加载的张数，用于「全选」。 */
  total: number;
  onPickAll: () => void;
  onClear: () => void;
  /** 右侧的主要操作按钮。 */
  children: ReactNode;
}

/** 多选时出现在图片区底部的操作条。 */
export function SelectionDock({ count, total, onPickAll, onClear, children }: SelectionDockProps) {
  return (
    <AnimatePresence>
      {count > 0 && (
        <motion.div
          className="dock"
          role="toolbar"
          aria-label={t("已选的图片")}
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 12 }}
          transition={PANEL_ENTER}
        >
          <span className="dock-count">{t("已选 {n} 张", { n: formatCount(count) })}</span>
          <button type="button" className="btn ghost" onClick={onPickAll} disabled={count === total}>
            {t("全选已加载的 {n} 张", { n: formatCount(total) })}
          </button>
          <button type="button" className="btn ghost" onClick={onClear}>
            {t("取消选择")}
          </button>
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
