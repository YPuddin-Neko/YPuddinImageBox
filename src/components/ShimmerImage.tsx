import { useCallback, useState } from "react";

type LoadState = "loading" | "loaded" | "failed";

interface ShimmerImageProps {
  src: string | undefined;
  alt: string;
  className?: string;
  /** 缩略图用 lazy；详情大图用 eager，选中后立即加载。 */
  loading?: "lazy" | "eager";
  fit?: "cover" | "contain";
}

/**
 * 图片加载完成前显示扫光占位，加载完成后淡入；失败时停止扫光，显示静态占位。
 * 调用方通过 key 在换图时重建组件，避免沿用上一张图的状态。
 */
export function ShimmerImage({ src, alt, className, loading = "lazy", fit = "cover" }: ShimmerImageProps) {
  const [state, setState] = useState<LoadState>(src ? "loading" : "failed");

  // 已在内存缓存里的图可能在事件绑定前就完成加载，挂载时补查一次。
  const probe = useCallback((img: HTMLImageElement | null) => {
    if (img?.complete) setState(img.naturalWidth > 0 ? "loaded" : "failed");
  }, []);

  return (
    <span className={`shimmer-img ${className ?? ""}`} data-state={state} data-fit={fit}>
      {src && (
        <img
          ref={probe}
          src={src}
          alt={alt}
          loading={loading}
          decoding="async"
          draggable={false}
          onLoad={() => setState("loaded")}
          onError={() => setState("failed")}
        />
      )}
    </span>
  );
}
