import type { ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

import { formatCount } from "../lib/format";
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
          aria-label="已选的图片"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 12 }}
          transition={PANEL_ENTER}
        >
          <span className="dock-count">已选 {formatCount(count)} 张</span>
          <button type="button" className="btn ghost" onClick={onPickAll} disabled={count === total}>
            全选已加载的 {formatCount(total)} 张
          </button>
          <button type="button" className="btn ghost" onClick={onClear}>
            取消选择
          </button>
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
