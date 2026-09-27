import { useEffect, useId, useRef, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";

import { EASE_OUT } from "../lib/motion";

interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  actions: ReactNode;
  /** 打开时焦点落在哪个按钮。删除这类操作用 "last"（取消），免得一按回车就执行。 */
  initialFocus?: "first" | "last";
}

/** 应用内的确认对话框：遮罩淡入、面板轻微放大；Esc 或点遮罩关闭。 */
export function Dialog({ open, title, onClose, children, actions, initialFocus = "first" }: DialogProps) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  // 调用方一般传内联函数，每次渲染都是新的；放进 ref，免得对话框里一输入、一选择就把焦点抢回按钮上。
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  });

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const buttons = panel.current?.querySelectorAll<HTMLElement>(".dialog-actions button");
    buttons?.[initialFocus === "last" ? buttons.length - 1 : 0]?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previous?.focus();
    };
  }, [open, initialFocus]);

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="dialog-backdrop"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.16 }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onClose();
          }}
        >
          <motion.div
            ref={panel}
            className="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            initial={{ opacity: 0, scale: 0.96, y: 8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.98, y: 4 }}
            transition={{ duration: 0.2, ease: EASE_OUT }}
          >
            <h2 id={titleId}>{title}</h2>
            <div className="dialog-body">{children}</div>
            <div className="dialog-actions">{actions}</div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
