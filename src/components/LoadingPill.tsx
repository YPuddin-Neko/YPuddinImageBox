import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, type Transition } from "motion/react";

import { t } from "../lib/i18n";

/** 加载很快时不出现，免得一闪而过；出现后至少停留这么久再收回。 */
const SHOW_DELAY_MS = 180;
const MIN_VISIBLE_MS = 450;
/** 从底边以下弹上来：离底边的距离加上自身高度，起点完全在区域外。 */
const TRAVEL = 64;

const ENTER: Transition = { y: { type: "spring", stiffness: 520, damping: 30 }, opacity: { duration: 0.14 } };
const EXIT: Transition = { y: { duration: 0.22, ease: [0.4, 0, 1, 1] }, opacity: { duration: 0.16, delay: 0.06 } };

/** `on` 持续超过 SHOW_DELAY_MS 才变成 true，变成 true 后至少保持 MIN_VISIBLE_MS。 */
function useSettled(on: boolean): boolean {
  const [visible, setVisible] = useState(false);
  const shownAt = useRef(0);
  useEffect(() => {
    if (on === visible) return;
    const wait = on ? SHOW_DELAY_MS : Math.max(0, MIN_VISIBLE_MS - (performance.now() - shownAt.current));
    const timer = window.setTimeout(() => {
      if (on) shownAt.current = performance.now();
      setVisible(on);
    }, wait);
    return () => window.clearTimeout(timer);
  }, [on, visible]);
  return visible;
}

interface LoadingPillProps {
  loading: boolean;
  /** 加载失败的说明：立即出现并停住，不再转圈。 */
  failed?: string | null;
}

/** 加载提示：从所在区域的底边弹出，加载完成后退回底边以下。放在哪个容器里、离底边多高由样式决定。 */
export function LoadingPill({ loading, failed = null }: LoadingPillProps) {
  const settled = useSettled(loading);
  return (
    <AnimatePresence>
      {(settled || failed) && (
        <motion.div
          className="loading-pill"
          data-failed={failed ? true : undefined}
          role="status"
          initial={{ opacity: 0, y: TRAVEL }}
          animate={{ opacity: 1, y: 0, transition: ENTER }}
          exit={{ opacity: 0, y: TRAVEL, transition: EXIT }}
        >
          {!failed && <span className="spinner" aria-hidden="true" />}
          <span>{failed ?? t("正在加载…")}</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
