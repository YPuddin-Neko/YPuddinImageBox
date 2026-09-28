import type { CSSProperties } from "react";

import { imageSrc, postKey } from "../lib/ipc";
import type { Cover } from "../lib/library";
import { ShimmerImage } from "./ShimmerImage";

interface FanStackProps {
  covers: Cover[];
  /** 最多展开几张。 */
  max: number;
  /** 没有封面时画几张虚线空卡。 */
  placeholders?: number;
}

/** 像一手扑克牌那样往右扇开的相片：最新的一张立在最前面，其余的按下载先后依次转开、压在后面。 */
export function FanStack({ covers, max, placeholders = 3 }: FanStackProps) {
  const shown = covers.slice(0, max);
  const count = shown.length || placeholders;
  return (
    <span className="fan" aria-hidden="true">
      {shown.length > 0
        ? shown.map((cover, index) => (
            <span
              key={postKey({ source: cover.source, id: cover.postId })}
              className="fan-card"
              data-front={index === 0 || undefined}
              style={{ "--k": index, zIndex: count - index } as CSSProperties}
            >
              <ShimmerImage src={imageSrc(cover.thumbUrl)} alt="" />
            </span>
          ))
        : Array.from({ length: placeholders }, (_, index) => (
            <span
              key={index}
              className="fan-card"
              data-empty
              style={{ "--k": index, zIndex: count - index } as CSSProperties}
            />
          ))}
    </span>
  );
}
