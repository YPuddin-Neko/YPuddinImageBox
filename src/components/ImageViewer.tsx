import { useCallback, useEffect, useRef, useState, type CSSProperties, type PointerEvent, type WheelEvent } from "react";

import { imageSrc, postNumber, type Post } from "../lib/ipc";
import { t } from "../lib/i18n";
import { Icon } from "./Icon";

interface ImageViewerProps {
  post: Post | null;
  posts: Post[];
  onClose: () => void;
  onChange: (post: Post) => void;
  /** 图库记录优先使用本地 sampleUrl，避免放大时再次访问远程图床。 */
  useSample?: boolean;
}

const MIN_ZOOM = 0.1;
const MAX_ZOOM = 4;
type LoadPhase = "loading" | "leaving" | "failed" | null;

export function ImageViewer({ post, posts, onClose, onChange, useSample = false }: ImageViewerProps) {
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [loadPhase, setLoadPhase] = useState<LoadPhase>(null);
  const drag = useRef<{ pointerId: number; x: number; y: number; startX: number; startY: number } | null>(null);
  const loadTimer = useRef<number | undefined>(undefined);
  const stageRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const index = post ? posts.findIndex((item) => item.source === post.source && item.id === post.id) : -1;
  const src = post ? imageSrc(useSample ? post.sampleUrl ?? post.thumbUrl : post.fileUrl ?? post.sampleUrl ?? post.thumbUrl) : undefined;

  useEffect(() => {
    if (!post) return;
    setZoom(1);
    setOffset({ x: 0, y: 0 });
  }, [post?.source, post?.id]);

  const fitToWindow = useCallback(() => {
    const image = imageRef.current;
    const stage = stageRef.current;
    if (!image?.naturalWidth || !image.naturalHeight || !stage) return;
    const width = Math.max(1, stage.clientWidth - 32);
    const height = Math.max(1, stage.clientHeight - 32);
    const fit = Math.min(width / image.naturalWidth, height / image.naturalHeight);
    setZoom(Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, fit)));
    setOffset({ x: 0, y: 0 });
  }, []);

  const finishLoading = useCallback(
    (failed: boolean) => {
      if (failed) {
        setLoadPhase("failed");
        return;
      }
      fitToWindow();
      setLoadPhase("leaving");
      window.clearTimeout(loadTimer.current);
      loadTimer.current = window.setTimeout(() => setLoadPhase(null), 220);
    },
    [fitToWindow],
  );

  useEffect(() => {
    window.clearTimeout(loadTimer.current);
    const image = imageRef.current;
    if (src && image?.complete && image.currentSrc === src) {
      if (image.naturalWidth > 0) {
        fitToWindow();
        setLoadPhase(null);
      } else {
        setLoadPhase("failed");
      }
    } else {
      setLoadPhase(src ? "loading" : "failed");
    }
    return () => window.clearTimeout(loadTimer.current);
  }, [fitToWindow, src]);

  useEffect(() => {
    if (!post) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key === "ArrowLeft" && index > 0) onChange(posts[index - 1]);
      if (event.key === "ArrowRight" && index >= 0 && index < posts.length - 1) onChange(posts[index + 1]);
      if (event.key === "0") fitToWindow();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fitToWindow, index, onChange, onClose, post, posts]);

  if (!post) return null;
  const changeZoom = (delta: number) => {
    setZoom((current) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.round((current + delta) * 4) / 4)));
  };
  const wheel = (event: WheelEvent<HTMLDivElement>) => {
    event.preventDefault();
    changeZoom(event.deltaY < 0 ? 0.25 : -0.25);
  };
  const pointerDown = (event: PointerEvent<HTMLImageElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, startX: offset.x, startY: offset.y };
  };
  const pointerMove = (event: PointerEvent<HTMLImageElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    setOffset({ x: current.startX + event.clientX - current.x, y: current.startY + event.clientY - current.y });
  };
  const pointerUp = () => {
    drag.current = null;
  };

  return (
    <div className="image-viewer-backdrop" role="dialog" aria-modal="true" aria-label="图片查看器" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="image-viewer">
        <header className="image-viewer-toolbar">
          <span className="image-viewer-title">
            #{postNumber(post)} <small>{index >= 0 ? `${index + 1} / ${posts.length}` : ""}</small>
          </span>
          <div className="image-viewer-actions">
            <button type="button" className="viewer-btn" onClick={() => changeZoom(-0.25)} aria-label="缩小" title="缩小">
              <Icon name="zoomOut" size={17} />
            </button>
            <button type="button" className="viewer-btn" onClick={fitToWindow} aria-label={t("适应窗口")} title={t("适应窗口")}>
              <Icon name="fit" size={17} />
            </button>
            <button type="button" className="viewer-zoom" onClick={fitToWindow} title="适应窗口">
              {Math.round(zoom * 100)}%
            </button>
            <button type="button" className="viewer-btn" onClick={() => changeZoom(0.25)} aria-label="放大" title="放大">
              <Icon name="zoomIn" size={17} />
            </button>
            <button type="button" className="viewer-btn" onClick={onClose} aria-label="关闭" title="关闭">
              <Icon name="close" size={17} />
            </button>
          </div>
        </header>
        <div ref={stageRef} className="image-viewer-stage" onWheel={wheel}>
          {index > 0 && (
            <button type="button" className="viewer-nav prev" onClick={() => onChange(posts[index - 1])} aria-label="上一张">
              <Icon name="chevronLeft" size={24} />
            </button>
          )}
          <div
            className={`image-viewer-canvas${loadPhase === "loading" ? " is-loading" : ""}`}
            style={{
              transform: `translate(${offset.x}px, ${offset.y}px) scale(${zoom})`,
              "--viewer-inverse-zoom": String(1 / zoom),
            } as CSSProperties}
          >
            <img
              ref={imageRef}
              className={`image-viewer-image is-draggable${loadPhase === "loading" ? " is-loading" : ""}`}
              src={src}
              alt={`#${postNumber(post)}`}
              draggable={false}
              onPointerDown={pointerDown}
              onPointerMove={pointerMove}
              onPointerUp={pointerUp}
              onPointerCancel={pointerUp}
              onLoad={() => finishLoading(false)}
              onError={() => finishLoading(true)}
            />
            {loadPhase && (
              <div className={`image-viewer-load${loadPhase === "leaving" ? " is-leaving" : ""}${loadPhase === "failed" ? " is-failed" : ""}`} role="status">
                {loadPhase === "loading" && <span className="image-viewer-spinner" aria-hidden="true" />}
                <span>{loadPhase === "failed" ? t("图片加载失败") : t("正在加载…")}</span>
              </div>
            )}
          </div>
          {index >= 0 && index < posts.length - 1 && (
            <button type="button" className="viewer-nav next" onClick={() => onChange(posts[index + 1])} aria-label="下一张">
              <Icon name="chevronRight" size={24} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
