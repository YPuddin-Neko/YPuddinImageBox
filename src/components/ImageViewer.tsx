import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent, type PointerEvent } from "react";
import { AnimatePresence, useAnimate, usePresence, useReducedMotionConfig } from "motion/react";

import { t, type Msg } from "../lib/i18n";
import {
  goldOnly,
  hasSize,
  imageSrc,
  localFileSrc,
  originalIsImage,
  originalSrc,
  originalUrl,
  postKey,
  postNumber,
  type Post,
} from "../lib/ipc";
import { EASE_OUT } from "../lib/motion";
import { Icon } from "./Icon";
import { LoadingPill } from "./LoadingPill";

interface ImageViewerProps {
  post: Post | null;
  posts: Post[];
  onClose: () => void;
  onChange: (post: Post) => void;
  /** 图库里的记录：sampleUrl 就是本地的原图文件，直接显示它，不再从站点下载原图。 */
  local?: boolean;
  /** 已在图库中的帖子：直接显示下载好的文件，读不到（例如文件被移走了）才从站点加载。 */
  downloaded?: ReadonlySet<string>;
  /** 这张图在瀑布流里的卡片，看不见时为 null。打开时图片从卡片放大出来，关闭时缩回卡片。 */
  cardOf?: (post: Post) => HTMLElement | null;
}

type ViewerProps = Omit<ImageViewerProps, "post"> & { post: Post };

interface Size {
  w: number;
  h: number;
}

interface Rect extends Size {
  x: number;
  y: number;
}

/** 缩放倍数，以及图片中心相对可用区域中心的偏移。 */
interface View {
  zoom: number;
  x: number;
  y: number;
}

/** 缩放倍数都相对原图的尺寸。很大的图适配窗口时比 MIN_ZOOM 还小，这时以适配的倍数为下限。 */
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 4;
/** 工具栏按钮每次缩放的步长。 */
const ZOOM_STEP = 0.25;
/** 适应窗口时图片和可用区域边缘之间的留白。 */
const FIT_MARGIN = 16;
const OPEN = { duration: 0.34, ease: EASE_OUT };
const CLOSE = { duration: 0.28, ease: [0.4, 0, 0.2, 1] as const };
const FADE_OUT = { duration: 0.18, ease: [0.4, 0, 1, 1] as const };
/** 不位移、不裁切：打开动画的终点、关闭动画的起点。 */
const IDENTITY = { transform: "translate(0px, 0px) scale(1)", clipPath: "inset(0px 0px 0px 0px round 0px)" };

const clampZoom = (zoom: number, fit: number) => Math.min(MAX_ZOOM, Math.max(Math.min(MIN_ZOOM, fit), zoom));

/** 没有原图可显示的原因。 */
type Missing = "gold" | "none" | "video";

const MISSING_NOTE: Record<Missing, Msg> = {
  gold: "原图需要 Gold 账号，显示的是缩小图",
  none: "站点没有开放原图，显示的是缩小图",
  video: "原图是视频或动图，显示的是缩小图",
};

/** 查看器要加载的图：站点的缩小图先显示，原图加载完后盖上去。 */
interface Sources {
  /** 站点没有缩小图时（例如 Kemono）就是缩略图；和原图是同一张时没有。 */
  preview?: string;
  full?: string;
  /** 原图地址还在查（Pixiv）。 */
  pending?: boolean;
  missing?: Missing;
}

/** `original` 是原图地址：还在查时是 undefined，查询出错时是 false（和原图加载失败一样处理）。 */
function sourcesOf(post: Post, local: boolean, original: string | null | false | undefined): Sources {
  if (local) return { full: imageSrc(post.sampleUrl ?? post.thumbUrl) };
  const sample = post.sampleUrl ?? post.thumbUrl;
  const preview = sample && sample !== original ? imageSrc(sample) : undefined;
  if (post.fileExt && !originalIsImage(post)) return { preview, missing: "video" };
  if (original === undefined) return { preview, pending: true };
  if (original === false) return { preview };
  if (!original) return { preview, missing: goldOnly(post) ? "gold" : "none" };
  if (!originalIsImage({ fileExt: post.fileExt, fileUrl: original })) return { preview, missing: "video" };
  return { preview, full: originalSrc(original) };
}

function rectOf(element: Element): Rect {
  const rect = element.getBoundingClientRect();
  return { x: rect.left, y: rect.top, w: rect.width, h: rect.height };
}

/** 能完整放进可用区域的倍数，最多放大到 MAX_ZOOM。 */
const fitScale = (size: Size, area: Size) => Math.min(MAX_ZOOM, area.w / size.w, area.h / size.h);

/**
 * 让 `box` 这一层里 `from` 那块区域看起来和 `to` 一样：等比缩放到盖满 `to`，按 `to` 的比例居中裁切，
 * 再加上同样的圆角。`box` 的 transform-origin 是它的左上角。
 */
function morphTo(from: Rect, to: Rect, radius: number, box: Rect) {
  const scale = Math.max(to.w / from.w, to.h / from.h);
  const cx = from.x - box.x + from.w / 2;
  const cy = from.y - box.y + from.h / 2;
  const halfW = to.w / scale / 2;
  const halfH = to.h / scale / 2;
  const tx = to.x - box.x + to.w / 2 - scale * cx;
  const ty = to.y - box.y + to.h / 2 - scale * cy;
  return {
    transform: `translate(${tx}px, ${ty}px) scale(${scale})`,
    clipPath: `inset(${cy - halfH}px ${box.w - cx - halfW}px ${box.h - cy - halfH}px ${cx - halfW}px round ${radius / scale}px)`,
  };
}

/** 以 `rect` 的中心缩放，用在没有卡片可对应、只做淡入淡出的时候。 */
function scaleAt(rect: Rect, scale: number, box: Rect): string {
  const cx = rect.x - box.x + rect.w / 2;
  const cy = rect.y - box.y + rect.h / 2;
  return `translate(${(1 - scale) * cx}px, ${(1 - scale) * cy}px) scale(${scale})`;
}

const radiusOf = (element: Element) => parseFloat(getComputedStyle(element).borderTopLeftRadius) || 0;

/** 图片查看器。关闭时先播完缩回卡片的动画再移除。 */
export function ImageViewer({ post, ...props }: ImageViewerProps) {
  return <AnimatePresence>{post && <Viewer key="viewer" post={post} {...props} />}</AnimatePresence>;
}

function Viewer({ post, posts, onClose, onChange, local = false, downloaded, cardOf }: ViewerProps) {
  const [present, safeToRemove] = usePresence();
  const [scope, animate] = useAnimate<HTMLDivElement>();
  const reduced = useReducedMotionConfig() ?? false;
  const stageRef = useRef<HTMLDivElement>(null);
  const morphRef = useRef<HTMLDivElement>(null);
  const placeholderRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointerId: number; x: number; y: number; from: View } | null>(null);
  const opened = useRef(false);
  /** 打开的放大动画还在播：这时关闭只淡出，不飞回卡片。 */
  const morphing = useRef(false);
  const key = postKey(post);
  /** 可用区域（已减去底部工具栏的位置和四周留白）。 */
  const [area, setArea] = useState<Size | null>(null);
  const [view, setView] = useState<View>({ zoom: 1, x: 0, y: 0 });
  /** 工具栏缩放和适应窗口时平滑过渡；滚轮和拖动跟手，不加过渡。 */
  const [smooth, setSmooth] = useState(false);
  /** 每张图的加载结果：加载完是它的像素尺寸，失败是 "failed"。按地址记，翻回看过的图时直接按它适配。 */
  const [loads, setLoads] = useState<Record<string, Size | "failed">>({});
  /** 当前这张帖子里已经能显示的图。翻页后图片元素是新的，要等它加载完（或挂上去就是完整的）才算。 */
  const [live, setLive] = useState<{ key: string; srcs: string[] }>({ key: "", srcs: [] });
  /** Pixiv 作品查到的原图地址，按帖子记；查不到是 null，出错是 false。 */
  const [originals, setOriginals] = useState<Record<string, string | null | false>>({});
  const requested = useRef(new Set<string>());
  /** 已经按窗口适配过的帖子。第一张图（缩小图或原图）加载完时适配一次，之后换成原图不再重置缩放。 */
  const fitted = useRef<string | null>(null);
  // 帖子没有尺寸信息时按缩略图的比例占位；打开时卡片里的缩略图已经加载好了。
  const [thumbSize, setThumbSize] = useState<(Size & { key: string }) | null>(() => {
    const img = cardOf?.(post)?.querySelector("img");
    return img?.naturalWidth ? { key, w: img.naturalWidth, h: img.naturalHeight } : null;
  });

  const index = posts.findIndex((item) => item.source === post.source && item.id === post.id);
  const naturalOf = (src: string | undefined) => {
    const result = src ? loads[src] : undefined;
    return result && result !== "failed" ? result : null;
  };
  const failedOf = (src: string) => loads[src] === "failed";
  const saved = !local && downloaded?.has(key) ? localFileSrc(post) : undefined;
  const fromDisk = !!saved && !failedOf(saved);
  const lookup = !local && !fromDisk && post.source === "pixiv" && !post.fileExt;
  const sources: Sources = fromDisk ? { full: saved } : sourcesOf(post, local, lookup ? originals[key] : post.fileUrl);
  const { preview, full } = sources;
  const thumb = imageSrc(post.thumbUrl);
  const previewSize = naturalOf(preview);
  const fullSize = naturalOf(full);
  const shown = live.key === key ? live.srcs : [];
  const previewShown = !!preview && shown.includes(preview);
  const fullShown = !!full && shown.includes(full);
  /** 缩略图以外的图已经显示出来了。 */
  const ready = previewShown || fullShown;
  const loading = !fullShown && (!!sources.pending || (!!full && !failedOf(full)) || (!!preview && !previewShown && !failedOf(preview)));
  // 按原图的尺寸排版，缩放倍数相对原图；站点没给尺寸时按已经加载出来的最大的那张，都没有时按缩略图的比例占位。
  const size = hasSize(post) ? { w: post.width, h: post.height } : (fullSize ?? previewSize ?? (thumbSize?.key === key ? thumbSize : null));
  const fitZoom = size && area ? fitScale(size, area) : 1;
  const placeholder = area && size ? { w: size.w * fitZoom, h: size.h * fitZoom } : null;
  const readyRef = useRef(ready);
  readyRef.current = ready;
  const fitRef = useRef(fitZoom);
  fitRef.current = fitZoom;

  // 底部提示：加载中转圈；原图拿不到时说明原因，一直显示到关闭或翻页。
  let failed: string | null = null;
  let note: string | null = null;
  if (!loading) {
    if (sources.missing && (ready || !preview)) note = t(MISSING_NOTE[sources.missing]);
    else if (!ready) failed = t("图片加载失败");
    else if (!fullShown) failed = t("原图加载失败，显示的是缩小图");
  }

  // 可用区域随窗口大小变化。
  useLayoutEffect(() => {
    const layer = morphRef.current;
    if (!layer) return;
    const measure = () => {
      const style = getComputedStyle(layer);
      const w = layer.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - FIT_MARGIN * 2;
      const h = layer.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom) - FIT_MARGIN * 2;
      setArea((prev) => (prev?.w === w && prev.h === h ? prev : { w: Math.max(1, w), h: Math.max(1, h) }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(layer);
    return () => observer.disconnect();
  }, []);

  // 打开：遮罩淡入，占位图从卡片的位置放大到适应窗口的位置，工具栏从底部升起。
  // 量到可用区域、占位图有了尺寸之后、第一次绘制之前开始，第一帧就在卡片上。
  useLayoutEffect(() => {
    const root = scope.current;
    const layer = morphRef.current;
    if (opened.current || !area || !root || !layer) return;
    opened.current = true;
    const scrim = root.querySelector<HTMLElement>(".image-viewer-scrim");
    const toolbar = root.querySelector<HTMLElement>(".image-viewer-toolbar");
    const navs = root.querySelectorAll<HTMLElement>(".viewer-nav");
    // 先设好起点再开始：带延迟的动画在延迟期间不生效，不这样会先完整地闪一下。
    [scrim, toolbar, ...navs].forEach((element) => element && (element.style.opacity = "0"));
    if (scrim) animate(scrim, { opacity: [0, 1] }, { duration: 0.22, ease: "linear" });
    if (toolbar) animate(toolbar, { opacity: [0, 1], y: [16, 0] }, { duration: 0.26, delay: 0.1, ease: EASE_OUT });
    navs.forEach((nav) => animate(nav, { opacity: [0, 1] }, { duration: 0.2, delay: 0.12 }));

    const target = placeholder && placeholderRef.current ? rectOf(placeholderRef.current) : null;
    const card = reduced ? null : (cardOf?.(post) ?? null);
    const box = rectOf(layer);
    if (card && target) {
      const from = morphTo(target, rectOf(card), radiusOf(card), box);
      layer.style.transform = from.transform;
      layer.style.clipPath = from.clipPath;
      morphing.current = true;
      animate(layer, { transform: [from.transform, IDENTITY.transform], clipPath: [from.clipPath, IDENTITY.clipPath] }, OPEN).then(() => {
        morphing.current = false;
        layer.style.transform = "";
        layer.style.clipPath = "";
      });
    } else {
      const keyframes = reduced || !target ? { opacity: [0, 1] } : { opacity: [0, 1], transform: [scaleAt(target, 0.96, box), IDENTITY.transform] };
      animate(layer, keyframes, { duration: 0.2, ease: EASE_OUT }).then(() => {
        layer.style.transform = "";
      });
    }
  }, [area]);

  // 关闭：遮罩淡出，图片从现在的位置和大小缩回卡片；卡片看不见时原地淡出。
  useEffect(() => {
    if (present) return;
    const root = scope.current;
    const layer = morphRef.current;
    if (!root || !layer) {
      safeToRemove?.();
      return;
    }
    const scrim = root.querySelector(".image-viewer-scrim");
    const toolbar = root.querySelector(".image-viewer-toolbar");
    const shown = ready ? canvasRef.current : placeholder ? placeholderRef.current : null;
    const card = reduced || morphing.current ? null : (cardOf?.(post) ?? null);
    const box = rectOf(layer);
    const running = [
      ...(scrim ? [animate(scrim, { opacity: 0 }, { duration: 0.24, ease: "linear" })] : []),
      ...(toolbar ? [animate(toolbar, { opacity: 0, y: 16 }, { duration: 0.16, ease: [0.4, 0, 1, 1] })] : []),
      ...Array.from(root.querySelectorAll(".viewer-nav"), (nav) => animate(nav, { opacity: 0 }, { duration: 0.12 })),
    ];
    if (card && shown) {
      const to = morphTo(rectOf(shown), rectOf(card), radiusOf(card), box);
      running.push(animate(layer, { transform: [IDENTITY.transform, to.transform], clipPath: [IDENTITY.clipPath, to.clipPath] }, CLOSE));
    } else if (reduced || !shown || morphing.current) {
      running.push(animate(layer, { opacity: 0 }, FADE_OUT));
    } else {
      running.push(animate(layer, { opacity: 0, transform: [IDENTITY.transform, scaleAt(rectOf(shown), 0.96, box)] }, FADE_OUT));
    }
    void Promise.all(running).then(() => safeToRemove?.());
  }, [present]);

  // Pixiv 的作品先按页查到原图地址。
  useEffect(() => {
    if (!lookup || requested.current.has(key)) return;
    requested.current.add(key);
    originalUrl(post).then(
      (url) => setOriginals((prev) => ({ ...prev, [key]: url })),
      () => setOriginals((prev) => ({ ...prev, [key]: false })),
    );
  }, [key, lookup]);

  // 尺寸已经知道（站点给了，或者这张看过）就在绘制前适配好：翻页时不先按上一张的缩放闪一下。
  // 刚打开时要等量到可用区域。
  useLayoutEffect(() => {
    if (fitted.current === key || !area || !(hasSize(post) || fullSize || previewSize) || !size) return;
    fitted.current = key;
    setSmooth(false);
    setView({ zoom: fitScale(size, area), x: 0, y: 0 });
  }, [key, area]);

  /**
   * 一张图加载完。这个帖子还没适配过窗口就适配；站点没给尺寸的帖子第一次换成原图时，
   * 按两张图的宽度比例调小倍数，屏幕上的大小不变。
   */
  const onLoaded = (src: string, image: HTMLImageElement) => {
    const natural = { w: image.naturalWidth, h: image.naturalHeight };
    if (!natural.w || !natural.h) return;
    const known = naturalOf(src);
    if (!known) setLoads((prev) => ({ ...prev, [src]: natural }));
    setLive((prev) => {
      if (prev.key !== key) return { key, srcs: [src] };
      return prev.srcs.includes(src) ? prev : { key, srcs: [...prev.srcs, src] };
    });
    if (fitted.current !== key && area) {
      fitted.current = key;
      setSmooth(false);
      setView({ zoom: fitScale(hasSize(post) ? { w: post.width, h: post.height } : (fullSize ?? natural), area), x: 0, y: 0 });
    } else if (!hasSize(post) && src === full && !known && previewSize) {
      const k = previewSize.w / natural.w;
      setSmooth(false);
      setView((prev) => ({ ...prev, zoom: prev.zoom * k }));
    }
  };

  // 缓存里的图挂上去就是完整的，load 事件却要晚一拍：绘制前先标记好，翻回看过的图时不先闪一下占位图。
  useLayoutEffect(() => {
    canvasRef.current?.querySelectorAll("img").forEach((image) => {
      const src = image.getAttribute("src");
      if (src && image.complete && image.naturalWidth && !shown.includes(src)) onLoaded(src, image);
    });
  });

  const onFailed = (src: string) => setLoads((prev) => ({ ...prev, [src]: "failed" }));

  // 打开时焦点移进查看器，关闭后回到原来的位置。
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    scope.current?.focus({ preventScroll: true });
    return () => previous?.focus({ preventScroll: true });
  }, []);

  const go = (delta: number) => {
    const next = posts[index + delta];
    if (present && index >= 0 && next) onChange(next);
  };

  const close = () => {
    if (present) onClose();
  };

  const fit = () => {
    if (!size || !area) return;
    setSmooth(true);
    setView({ zoom: fitScale(size, area), x: 0, y: 0 });
  };

  /** 工具栏缩放：以可用区域中心为准，按步长取整。 */
  const step = (direction: 1 | -1) => {
    setSmooth(true);
    setView((prev) => {
      const zoom = clampZoom(Math.round((prev.zoom + direction * ZOOM_STEP) * 4) / 4, fitZoom);
      const k = zoom / prev.zoom;
      return { zoom, x: prev.x * k, y: prev.y * k };
    });
  };

  useEffect(() => {
    if (!present) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
      else if (event.key === "ArrowLeft") go(-1);
      else if (event.key === "ArrowRight") go(1);
      else if (event.key === "0") fit();
      else return;
      event.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // 滚轮缩放，指针下的那一点保持不动。触控板捏合发来的是带 ctrlKey 的小幅滚动，灵敏度调高。
  useEffect(() => {
    const stage = stageRef.current;
    const layer = morphRef.current;
    if (!stage || !layer) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      if (!readyRef.current || morphing.current) return;
      const rect = layer.getBoundingClientRect();
      const bottom = parseFloat(getComputedStyle(layer).paddingBottom);
      const point = { x: event.clientX - rect.left - rect.width / 2, y: event.clientY - rect.top - (rect.height - bottom) / 2 };
      const factor = Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.002));
      setSmooth(false);
      setView((prev) => {
        const zoom = clampZoom(prev.zoom * factor, fitRef.current);
        const k = zoom / prev.zoom;
        return { zoom, x: point.x - k * (point.x - prev.x), y: point.y - k * (point.y - prev.y) };
      });
    };
    stage.addEventListener("wheel", onWheel, { passive: false });
    return () => stage.removeEventListener("wheel", onWheel);
  }, []);

  const pointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, from: view };
    setSmooth(false);
  };
  const pointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const { from } = current;
    setView((prev) => ({ ...prev, x: from.x + event.clientX - current.x, y: from.y + event.clientY - current.y }));
  };
  const pointerUp = () => {
    drag.current = null;
  };

  // 点图片以外的空白处关闭。
  const stageDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget || event.target === morphRef.current) close();
  };

  return (
    <div ref={scope} className="image-viewer" role="dialog" aria-modal="true" aria-label={t("图片查看器")} tabIndex={-1}>
      <div className="image-viewer-scrim" aria-hidden="true" />
      <div ref={stageRef} className="image-viewer-stage" onMouseDown={stageDown}>
        <div ref={morphRef} className="image-viewer-morph">
          {/* 缩小图或原图加载完之前先放大显示缩略图，加载完后淡入盖住它。 */}
          <div
            ref={placeholderRef}
            className="image-viewer-placeholder"
            data-sized={placeholder ? true : undefined}
            data-hidden={ready || undefined}
            style={placeholder ? { width: placeholder.w, height: placeholder.h } : undefined}
          >
            {thumb && (
              <img
                key={thumb}
                src={thumb}
                alt=""
                draggable={false}
                onLoad={(event) => {
                  const img = event.currentTarget;
                  if (img.naturalWidth) setThumbSize({ key, w: img.naturalWidth, h: img.naturalHeight });
                }}
              />
            )}
          </div>
          {(preview || full) && (
            <div
              ref={canvasRef}
              className="image-viewer-canvas"
              data-ready={ready || undefined}
              data-smooth={smooth || undefined}
              style={{ width: size?.w, height: size?.h, transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}
              onPointerDown={pointerDown}
              onPointerMove={pointerMove}
              onPointerUp={pointerUp}
              onPointerCancel={pointerUp}
            >
              {preview && (
                <img
                  key={preview}
                  className="image-viewer-image"
                  data-covered={fullShown || undefined}
                  src={preview}
                  alt={full ? "" : `#${postNumber(post)}`}
                  draggable={false}
                  onLoad={(event) => onLoaded(preview, event.currentTarget)}
                  onError={() => onFailed(preview)}
                />
              )}
              {full && (
                <img
                  key={full}
                  className="image-viewer-image is-full"
                  data-ready={fullShown || undefined}
                  src={full}
                  alt={`#${postNumber(post)}`}
                  draggable={false}
                  onLoad={(event) => onLoaded(full, event.currentTarget)}
                  onError={() => onFailed(full)}
                />
              )}
            </div>
          )}
        </div>
      </div>

      {index > 0 && (
        <button type="button" className="viewer-nav prev" onClick={() => go(-1)} aria-label={t("上一张")} title={t("上一张")}>
          <Icon name="chevronLeft" size={24} />
        </button>
      )}
      {index >= 0 && index < posts.length - 1 && (
        <button type="button" className="viewer-nav next" onClick={() => go(1)} aria-label={t("下一张")} title={t("下一张")}>
          <Icon name="chevronRight" size={24} />
        </button>
      )}

      <LoadingPill
        loading={present && loading}
        label={ready ? t("正在加载原图…") : undefined}
        failed={present ? failed : null}
        note={present ? note : null}
      />

      <div className="image-viewer-toolbar">
        <span className="image-viewer-title">
          #{postNumber(post)} <small>{index >= 0 ? `${index + 1} / ${posts.length}` : ""}</small>
        </span>
        <div className="image-viewer-actions">
          <button type="button" className="viewer-btn" onClick={() => step(-1)} aria-label={t("缩小")} title={t("缩小")}>
            <Icon name="zoomOut" size={17} />
          </button>
          <button type="button" className="viewer-btn" onClick={fit} aria-label={t("适应窗口")} title={t("适应窗口")}>
            <Icon name="fit" size={17} />
          </button>
          <button type="button" className="viewer-zoom" onClick={fit} title={t("适应窗口")}>
            {ready ? `${Math.round(view.zoom * 100)}%` : "—"}
          </button>
          <button type="button" className="viewer-btn" onClick={() => step(1)} aria-label={t("放大")} title={t("放大")}>
            <Icon name="zoomIn" size={17} />
          </button>
          <button type="button" className="viewer-btn" onClick={close} aria-label={t("关闭")} title={t("关闭")}>
            <Icon name="close" size={17} />
          </button>
        </div>
      </div>
    </div>
  );
}
