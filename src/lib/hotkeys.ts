import { useEffect, useLayoutEffect, useRef } from "react";

import { isMac } from "./platform";

/** 快捷键里的修饰键：macOS 用 ⌘，其他系统用 Ctrl。 */
export const MOD = isMac ? "⌘" : "Ctrl";

export const hasMod = (event: KeyboardEvent) => (isMac ? event.metaKey : event.ctrlKey) && !event.altKey;

/** 开着对话框或图片查看器时，除了它们自己的按键，快捷键都不生效。 */
export const dialogOpen = () => document.querySelector(".dialog-backdrop, .image-viewer") !== null;

/** 正在输入（包括输入法选字）、开着对话框或下拉菜单时，列表上的快捷键不生效。 */
function blocked(event: KeyboardEvent): boolean {
  if (event.defaultPrevented || event.isComposing) return true;
  const target = event.target as HTMLElement | null;
  if (target?.closest('input, textarea, select, [contenteditable="true"], [role="listbox"]')) return true;
  return dialogOpen();
}

/** 视图在前台时监听键盘；`handler` 返回 true 表示处理了这个按键，会阻止默认行为。 */
export function useHotkeys(active: boolean, handler: (event: KeyboardEvent) => boolean) {
  const latest = useRef(handler);
  useLayoutEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (!blocked(event) && latest.current(event)) event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active]);
}

/** 空格留给焦点所在的按钮（例如「订阅」），只有焦点在卡片上或页面本身时才用来勾选。 */
export const spaceForButton = (event: KeyboardEvent) =>
  event.key === " " && (event.target as HTMLElement | null)?.closest("button:not(.card-hit), a") != null;
