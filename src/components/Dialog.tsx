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
    const dialog = panel.current;
    if (!open || !dialog) return;
    const previous = document.activeElement as HTMLElement | null;
    dialog.dataset.focusTrap = "true";
    const topmost = () => {
      const dialogs = document.querySelectorAll<HTMLElement>('.dialog[data-focus-trap="true"]');
      return dialogs[dialogs.length - 1] === dialog;
    };
    const available = (element: HTMLElement) => !element.matches(":disabled")
      && !element.closest('[hidden], [inert], [aria-hidden="true"]')
      && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
    const focusable = () => Array.from(dialog.querySelectorAll<HTMLElement>(
      'a[href], button, input:not([type="hidden"]), select, textarea, summary, [tabindex]',
    )).filter((element) => element.tabIndex >= 0 && available(element));
    const focusInitial = () => {
      const buttons = Array.from(dialog.querySelectorAll<HTMLElement>(".dialog-actions button")).filter(available);
      const button = buttons[initialFocus === "last" ? buttons.length - 1 : 0];
      (button ?? focusable()[0] ?? dialog).focus({ preventScroll: true });
    };
    focusInitial();
    const onKey = (event: KeyboardEvent) => {
      if (!topmost() || event.defaultPrevented || event.isComposing) return;
      if (event.key === "Escape") {
        event.preventDefault();
        closeRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const elements = focusable();
      const first = elements[0];
      const last = elements[elements.length - 1];
      const active = document.activeElement;
      if (!first) {
        event.preventDefault();
        dialog.focus({ preventScroll: true });
      } else if (active === dialog || !dialog.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus({ preventScroll: true });
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus({ preventScroll: true });
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus({ preventScroll: true });
      }
    };
    const onFocus = (event: FocusEvent) => {
      if (topmost() && !dialog.contains(event.target as Node | null)) focusInitial();
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("focusin", onFocus);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("focusin", onFocus);
      delete dialog.dataset.focusTrap;
      previous?.focus({ preventScroll: true });
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
            tabIndex={-1}
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
