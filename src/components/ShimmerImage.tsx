import { useCallback, useRef, useState } from "react";

type LoadState = "loading" | "loaded" | "failed";

interface ShimmerImageProps {
  src: string | undefined;
  alt: string;
  className?: string;
  /** 缩略图用 lazy；详情大图用 eager，选中后立即加载。 */
  loading?: "lazy" | "eager";
  fit?: "cover" | "contain";
  onNaturalSize?: (width: number, height: number) => void;
}

/**
 * 图片加载完成前显示扫光占位，加载完成后淡入；失败时停止扫光，显示静态占位。
 * 调用方通过 key 在换图时重建组件，避免沿用上一张图的状态。
 */
export function ShimmerImage({ src, alt, className, loading = "lazy", fit = "cover", onNaturalSize }: ShimmerImageProps) {
  const [state, setState] = useState<LoadState>(src ? "loading" : "failed");
  /** 挂载时图已经在缓存里（例如瀑布流里滚出去又滚回来的卡片）：直接显示，不再淡入一次。 */
  const [instant, setInstant] = useState(false);
  const naturalSize = useRef(onNaturalSize);
  naturalSize.current = onNaturalSize;

  // 已在内存缓存里的图可能在事件绑定前就完成加载，挂载时补查一次。
  const probe = useCallback((img: HTMLImageElement | null) => {
    if (!img?.complete) return;
    setInstant(true);
    setState(img.naturalWidth > 0 ? "loaded" : "failed");
    if (img.naturalWidth > 0 && img.naturalHeight > 0) naturalSize.current?.(img.naturalWidth, img.naturalHeight);
  }, []);

  return (
    <span
      className={`shimmer-img ${className ?? ""}`}
      data-state={state}
      data-fit={fit}
      data-instant={instant || undefined}
    >
      {src && (
        <img
          ref={probe}
          src={src}
          alt={alt}
          loading={loading}
          decoding="async"
          draggable={false}
          onLoad={(event) => {
            setState("loaded");
            const img = event.currentTarget;
            naturalSize.current?.(img.naturalWidth, img.naturalHeight);
          }}
          onError={() => setState("failed")}
        />
      )}
    </span>
  );
}
