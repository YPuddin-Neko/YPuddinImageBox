import { useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { motion } from "motion/react";

import { t } from "../lib/i18n";
import { imageSrc, postKey, postNumber, SOURCE_LABEL, type Post } from "../lib/ipc";
import { cardEnter } from "../lib/motion";
import { Icon } from "./Icon";
import { ShimmerImage } from "./ShimmerImage";

/** 列宽下限和间距，与原来 CSS 多列布局的 `columns: 210px; column-gap: 12px` 一致。 */
const COLUMN_MIN = 210;
const GAP = 12;
/** 视口上下各多渲染这么多像素的卡片，快速滚动时不露白。 */
const OVERSCAN = 900;
/** 可见范围按这个步长取整，滚动时不必每一帧都重新渲染。 */
const STEP = 300;

/** 极端长宽比的图在瀑布流里按限定比例占位，图片本身居中裁切。 */
function cardRatio(post: Post): number {
  return Math.min(2.4, Math.max(0.42, post.width / post.height));
}

/** 瀑布流里这张图的卡片，至少露出一半时才返回。查看器打开时从这里放大出来，关闭时缩回这里。 */
export function visibleCard(scope: HTMLElement | null, post: Post): HTMLElement | null {
  const card = scope?.querySelector<HTMLElement>(`.card[data-key="${CSS.escape(postKey(post))}"]`);
  const scroller = card?.closest(".scroll");
  if (!card || !scroller) return null;
  const a = card.getBoundingClientRect();
  const b = scroller.getBoundingClientRect();
  const shown = Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left)) * Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
  return shown * 2 >= a.width * a.height ? card : null;
}

interface Box {
  x: number;
  y: number;
  height: number;
}

interface Layout {
  columnWidth: number;
  boxes: Box[];
  height: number;
}

/** 依次放进当前最矮的一列。追加新的一页时前面的卡片位置不变。 */
function computeLayout(posts: Post[], width: number): Layout {
  const columns = Math.max(1, Math.floor((width + GAP) / (COLUMN_MIN + GAP)));
  const columnWidth = (width - GAP * (columns - 1)) / columns;
  const heights = new Array<number>(columns).fill(0);
  const boxes = posts.map((post) => {
    let column = 0;
    for (let i = 1; i < columns; i++) if (heights[i] < heights[column]) column = i;
    const box = { x: column * (columnWidth + GAP), y: heights[column], height: Math.round(columnWidth / cardRatio(post)) };
    heights[column] += box.height + GAP;
    return box;
  });
  return { columnWidth, boxes, height: Math.max(0, Math.max(...heights) - GAP) };
}

interface PostGridProps<T extends Post> {
  posts: T[];
  selected: string | null;
  onSelect: (post: T, event: MouseEvent) => void;
  /** 图片卡片右下角的放大查看按钮。 */
  onView?: (post: T, event: MouseEvent) => void;
  /** 与每页条数一致，用于卡片入场错开。 */
  pageSize: number;
  /** 已在图库中的帖子，卡片右上角标「已下载」。 */
  owned?: ReadonlySet<string>;
  /** 图库里文件已经不在的帖子，右上角标「文件缺失」。 */
  missing?: ReadonlySet<string>;
  /** 多选时已勾选的帖子；不传 `onPick` 就不显示勾选框。 */
  picked?: ReadonlySet<string>;
  onPick?: (post: T, event: MouseEvent) => void;
  /** 结果来自好几个站点时（聚合搜索、图库的全部图片），右上角标出每张图来自哪个站点。 */
  showSource?: boolean;
}

/**
 * 瀑布流：点卡片看详情，左上角勾选框多选，右上角角标表示已下载和来自哪个站点。
 * 只渲染滚动区域里看得见（加上下各一段余量）的卡片，几千张也不卡。
 */
export function PostGrid<T extends Post>({
  posts,
  selected,
  onSelect,
  onView,
  pageSize,
  owned,
  missing,
  picked,
  onPick,
  showSource = false,
}: PostGridProps<T>) {
  const grid = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [range, setRange] = useState({ top: 0, bottom: 0 });
  /** 已经播过入场动画的卡片；滚出去再滚回来时直接显示。 */
  const entered = useRef(new Set<string>());
  const firstKey = posts[0] ? postKey(posts[0]) : null;

  const layout = useMemo(() => computeLayout(posts, width), [posts, width]);
  const layoutRef = useRef(layout);
  layoutRef.current = layout;

  useLayoutEffect(() => {
    const el = grid.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);

  // 可见范围：按所在滚动区域的位置换算成网格内的坐标。首次在绘制前算好，第一帧就是完整的一屏。
  useLayoutEffect(() => {
    const el = grid.current;
    const scroller = el?.closest<HTMLElement>(".scroll");
    if (!el || !scroller) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const offset = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      const top = Math.floor((-offset - OVERSCAN) / STEP) * STEP;
      const bottom = Math.ceil((-offset + scroller.clientHeight + OVERSCAN) / STEP) * STEP;
      setRange((prev) => (prev.top === top && prev.bottom === bottom ? prev : { top, bottom }));
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    scroller.addEventListener("scroll", schedule, { passive: true });
    const observer = new ResizeObserver(schedule);
    observer.observe(scroller);
    return () => {
      cancelAnimationFrame(frame);
      scroller.removeEventListener("scroll", schedule);
      observer.disconnect();
    };
  }, []);

  // 换了一批结果（重新搜索、换筛选）时，新卡片照样播入场动画。
  useEffect(() => {
    entered.current.clear();
  }, [firstKey]);

  // 选中的卡片不在视口里时（例如用方向键切换）滚过去；点击的卡片本来就看得见，不会动。
  useEffect(() => {
    const el = grid.current;
    const scroller = el?.closest<HTMLElement>(".scroll");
    const index = selected ? posts.findIndex((post) => postKey(post) === selected) : -1;
    const box = layoutRef.current.boxes[index];
    if (!el || !scroller || !box) return;
    const offset = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
    const top = offset + box.y;
    const bottom = top + box.height;
    if (top < scroller.scrollTop) {
      scroller.scrollTo({ top: top - GAP, behavior: "smooth" });
    } else if (bottom > scroller.scrollTop + scroller.clientHeight) {
      scroller.scrollTo({ top: bottom - scroller.clientHeight + GAP, behavior: "smooth" });
    }
    // 只在选中项变化时检查，列表追加或尺寸变化不跟着滚动。
  }, [selected]);

  const visible: number[] = [];
  if (width > 0) {
    layout.boxes.forEach((box, index) => {
      if (box.y + box.height >= range.top && box.y <= range.bottom) visible.push(index);
    });
  }

  useEffect(() => {
    visible.forEach((index) => entered.current.add(postKey(posts[index])));
  });

  const picking = (picked?.size ?? 0) > 0;
  return (
    <div
      ref={grid}
      className="grid"
      style={{ height: width > 0 ? layout.height : undefined }}
      data-picking={picking || undefined}
      data-count={posts.length}
    >
      {visible.map((index) => {
        const post = posts[index];
        const box = layout.boxes[index];
        const key = postKey(post);
        const isPicked = picked?.has(key) ?? false;
        const animate = !entered.current.has(key);
        const about = { id: postNumber(post), width: post.width, height: post.height };
        const state = missing?.has(key) ? (
          <span className="card-owned is-missing">{t("文件缺失")}</span>
        ) : (
          owned?.has(key) && (
            <span className="card-owned">
              <Icon name="check" size={11} />
              {t("已下载")}
            </span>
          )
        );
        return (
          <motion.div
            key={key}
            className="card"
            data-key={key}
            style={{ left: box.x, top: box.y, width: layout.columnWidth, height: box.height }}
            data-selected={key === selected || undefined}
            data-picked={isPicked || undefined}
            initial={animate ? { opacity: 0, y: 8 } : false}
            animate={{ opacity: 1, y: 0 }}
            transition={cardEnter(index, pageSize)}
            whileTap={{ scale: 0.985 }}
          >
            <button
              type="button"
              className="card-hit"
              aria-pressed={key === selected}
              aria-label={
                post.pages
                  ? t("#{id}，{width} × {height}，{n} 页", { ...about, n: post.pages })
                  : t("#{id}，{width} × {height}", about)
              }
              onClick={(event) => onSelect(post, event)}
            >
              <ShimmerImage src={imageSrc(post.thumbUrl)} alt="" />
            </button>
            {onPick && (
              <button
                type="button"
                className="card-pick"
                role="checkbox"
                aria-checked={isPicked}
                aria-label={t("选择 #{id}", { id: about.id })}
                onClick={(event) => onPick(post, event)}
              >
                <Icon name="check" size={13} />
              </button>
            )}
            {onView && (
              <button
                type="button"
                className="card-view"
                aria-label={t("放大查看")}
                title={t("放大查看")}
                onClick={(event) => onView(post, event)}
              >
                <Icon name="zoomIn" size={14} />
              </button>
            )}
            {(state || showSource || !!post.pages) && (
              <span className="card-badges">
                {!!post.pages && (
                  <span className="card-pages">
                    <Icon name="pages" size={11} />
                    {post.pages}
                  </span>
                )}
                {state}
                {showSource && (
                  <span className="card-source" data-source={post.source}>
                    {SOURCE_LABEL[post.source]}
                  </span>
                )}
              </span>
            )}
          </motion.div>
        );
      })}
    </div>
  );
}
