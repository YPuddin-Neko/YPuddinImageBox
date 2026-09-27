import type { Transition, Variants } from "motion/react";

/** 界面统一的缓出曲线：起步快、收尾柔和。 */
export const EASE_OUT = [0.2, 0.8, 0.2, 1] as const;

/** 视图切换：新视图淡入并上移 6px，旧视图淡出后隐藏。 */
export const VIEW_VARIANTS: Variants = {
  shown: { opacity: 1, y: 0, visibility: "visible", transition: { duration: 0.22, ease: EASE_OUT } },
  hidden: { opacity: 0, y: 6, transition: { duration: 0.14, ease: "easeIn" }, transitionEnd: { visibility: "hidden" } },
};

/** 瀑布流卡片入场。同一页内依次错开，最多错开 14 张，避免长列表等待过久。 */
export function cardEnter(index: number, pageSize: number): Transition {
  return { duration: 0.24, ease: EASE_OUT, delay: Math.min(index % pageSize, 14) * 0.018 };
}

/** 详情面板换图时的淡入。 */
export const PANEL_ENTER: Transition = { duration: 0.18, ease: EASE_OUT };
