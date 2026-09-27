import type { MouseEvent } from "react";
import { motion } from "motion/react";

import { imageSrc, postKey, type Post } from "../lib/ipc";
import { cardEnter } from "../lib/motion";
import { Icon } from "./Icon";
import { ShimmerImage } from "./ShimmerImage";

/** 极端长宽比的图在瀑布流里按限定比例占位，图片本身居中裁切。 */
function cardRatio(post: Post): string {
  return String(Math.min(2.4, Math.max(0.42, post.width / post.height)));
}

interface PostGridProps<T extends Post> {
  posts: T[];
  selected: string | null;
  onSelect: (post: T, event: MouseEvent) => void;
  /** 与每页条数一致，用于卡片入场错开。 */
  pageSize: number;
  /** 已在图库中的帖子，卡片右上角标「已下载」。 */
  owned?: ReadonlySet<string>;
  /** 图库里文件已经不在的帖子，右上角标「文件缺失」。 */
  missing?: ReadonlySet<string>;
  /** 多选时已勾选的帖子；不传 `onPick` 就不显示勾选框。 */
  picked?: ReadonlySet<string>;
  onPick?: (post: T, event: MouseEvent) => void;
}

/** 瀑布流卡片：点卡片看详情，左上角勾选框多选，右上角角标表示已下载。 */
export function PostGrid<T extends Post>({
  posts,
  selected,
  onSelect,
  pageSize,
  owned,
  missing,
  picked,
  onPick,
}: PostGridProps<T>) {
  const picking = (picked?.size ?? 0) > 0;
  return (
    <div className="grid" data-picking={picking || undefined}>
      {posts.map((post, index) => {
        const key = postKey(post);
        const isPicked = picked?.has(key) ?? false;
        return (
          <motion.div
            key={key}
            className="card"
            style={{ aspectRatio: cardRatio(post) }}
            data-selected={key === selected || undefined}
            data-picked={isPicked || undefined}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            transition={cardEnter(index, pageSize)}
            whileTap={{ scale: 0.985 }}
          >
            <button
              type="button"
              className="card-hit"
              aria-pressed={key === selected}
              aria-label={`#${post.id}，${post.width} × ${post.height}`}
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
                aria-label={`选择 #${post.id}`}
                onClick={(event) => onPick(post, event)}
              >
                <Icon name="check" size={13} />
              </button>
            )}
            {missing?.has(key) ? (
              <span className="card-owned is-missing">文件缺失</span>
            ) : (
              owned?.has(key) && (
                <span className="card-owned">
                  <Icon name="check" size={11} />
                  已下载
                </span>
              )
            )}
          </motion.div>
        );
      })}
    </div>
  );
}
