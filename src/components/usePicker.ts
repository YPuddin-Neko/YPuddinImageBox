import { useCallback, useRef, useState, type MouseEvent } from "react";

import { postKey, type Post } from "../lib/ipc";

/**
 * 瀑布流多选：点勾选框切换；按住 Shift 时把上次勾选的到这一张之间全部选上。
 * `posts` 是当前列表，用来算 Shift 范围和全选。
 */
export function usePicker<T extends Post>(posts: T[]) {
  const [picked, setPicked] = useState<Set<string>>(() => new Set());
  const anchor = useRef<string | null>(null);

  const toggle = (post: T, event: MouseEvent) => {
    const key = postKey(post);
    const from = anchor.current;
    setPicked((prev) => {
      const next = new Set(prev);
      const keys = posts.map(postKey);
      const [start, end] = [from ? keys.indexOf(from) : -1, keys.indexOf(key)];
      if (event.shiftKey && start >= 0 && end >= 0) {
        keys.slice(Math.min(start, end), Math.max(start, end) + 1).forEach((k) => next.add(k));
      } else if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
    anchor.current = key;
  };

  const clear = useCallback(() => {
    setPicked(new Set());
    anchor.current = null;
  }, []);

  const pickAll = () => setPicked(new Set(posts.map(postKey)));

  /** 去掉已经不在列表里的（例如刚删掉的）。 */
  const forget = useCallback((keys: Iterable<string>) => {
    setPicked((prev) => {
      const next = new Set(prev);
      for (const key of keys) next.delete(key);
      return next.size === prev.size ? prev : next;
    });
  }, []);

  const pickedPosts = posts.filter((post) => picked.has(postKey(post)));
  return { picked, pickedPosts, toggle, clear, pickAll, forget };
}
