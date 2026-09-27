import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { AnimatePresence, motion } from "motion/react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import type { View } from "../../components/Rail";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import {
  countRemote,
  errorMessage,
  postKey,
  RATING_LABEL,
  RATINGS,
  searchRemote,
  SOURCE_LABEL,
  type Post,
  type Rating,
  type Source,
} from "../../lib/ipc";
import { PANEL_ENTER } from "../../lib/motion";
import { useDownloads } from "../downloads/DownloadsProvider";
import { Inspector } from "./Inspector";

interface Criteria {
  source: Source;
  tags: string;
  ratings: Rating[];
}

interface Results {
  posts: Post[];
  page: number;
  hasMore: boolean;
  query: string;
}

/** 「下载全部结果」对话框。 */
interface Bulk {
  criteria: Criteria;
  query: string;
  count: number | null | "loading" | "failed";
  /** 最多下载前多少张，留空表示不限。 */
  max: string;
}

interface Toast {
  message: string;
  /** 带「查看」按钮，跳到下载页。 */
  link: boolean;
}

const DEFAULT_CRITERIA: Criteria = { source: "danbooru", tags: "", ratings: ["general"] };
/** 与 Rust 端每页条数一致，用于卡片入场错开。 */
const PAGE_SIZE = 40;

function countText(count: Bulk["count"]): string {
  if (count === "loading") return "正在统计…";
  if (count === "failed") return "暂时无法统计，可以直接开始";
  if (count === null) return "站点没有给出总数（条件较复杂时会这样），可以直接开始";
  return `约 ${formatCount(count)} 张`;
}

export function Discover({ active, onNavigate }: { active: boolean; onNavigate: (view: View) => void }) {
  const { addPosts, addQuery } = useDownloads();
  const [source, setSource] = useState<Source>(DEFAULT_CRITERIA.source);
  const [tags, setTags] = useState(DEFAULT_CRITERIA.tags);
  const [ratings, setRatings] = useState<Rating[]>(DEFAULT_CRITERIA.ratings);
  const [results, setResults] = useState<Results | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  /** 记下失败的是哪一页，重试时重跑这一页。 */
  const [error, setError] = useState<{ message: string; page: number } | null>(null);
  /** 已在图库中的帖子。只增不减：下载完成的事件也会加进来。 */
  const [owned, setOwned] = useState<Set<string>>(() => new Set());
  /** 这次打开软件后加入过下载队列、还没下载完的帖子。 */
  const [queued, setQueued] = useState<Set<string>>(() => new Set());
  const [picked, setPicked] = useState<Set<string>>(() => new Set());
  const [bulk, setBulk] = useState<Bulk | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [busy, setBusy] = useState(false);
  const committed = useRef<Criteria>(DEFAULT_CRITERIA);
  const requestId = useRef(0);
  const lastPicked = useRef<string | null>(null);
  const sentinel = useRef<HTMLDivElement>(null);

  const run = useCallback(async (criteria: Criteria, page: number) => {
    const id = ++requestId.current;
    committed.current = criteria;
    setLoading(true);
    setError(null);
    try {
      const next = await searchRemote({ ...criteria, page });
      if (id !== requestId.current) return;
      setResults((prev) => {
        if (page === 1 || !prev) return next;
        const seen = new Set(prev.posts.map(postKey));
        return { ...next, posts: [...prev.posts, ...next.posts.filter((p) => !seen.has(postKey(p)))] };
      });
      setOwned((prev) => {
        const ownedNow = new Set(prev);
        next.owned.forEach((postId) => ownedNow.add(postKey({ source: criteria.source, id: postId })));
        return ownedNow;
      });
      if (page === 1) {
        setSelected(next.posts[0] ? postKey(next.posts[0]) : null);
        setPicked(new Set());
        lastPicked.current = null;
      }
    } catch (err) {
      if (id === requestId.current) setError({ message: errorMessage(err), page });
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void run(DEFAULT_CRITERIA, 1);
  }, [run]);

  useTauriEvent<SavedPayload>(EVENTS.librarySaved, (saved) => {
    const key = postKey({ source: saved.source, id: saved.postId });
    setOwned((prev) => new Set(prev).add(key));
    setQueued((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  });

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const loadMore = useCallback(() => {
    if (!results?.hasMore || loading || error) return;
    void run(committed.current, results.page + 1);
  }, [results, loading, error, run]);

  useEffect(() => {
    const target = sentinel.current;
    if (!target || !active) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { rootMargin: "600px 0px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [loadMore, active]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void run({ source, tags, ratings }, 1);
  };

  const toggleRating = (rating: Rating) => {
    const next = ratings.includes(rating) ? ratings.filter((r) => r !== rating) : [...ratings, rating];
    setRatings(next);
    void run({ source, tags, ratings: next }, 1);
  };

  const changeSource = (next: Source) => {
    setSource(next);
    void run({ source: next, tags, ratings }, 1);
  };

  const posts = results?.posts ?? [];
  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  const firstLoad = loading && !results;

  // 勾选：单击勾选框切换；按住 Shift 时把上次勾选的到这一张之间全部选上。
  const togglePick = (post: Post, event: MouseEvent) => {
    const key = postKey(post);
    const anchor = lastPicked.current;
    setPicked((prev) => {
      const next = new Set(prev);
      const keys = posts.map(postKey);
      const [from, to] = [anchor ? keys.indexOf(anchor) : -1, keys.indexOf(key)];
      if (event.shiftKey && from >= 0 && to >= 0) {
        keys.slice(Math.min(from, to), Math.max(from, to) + 1).forEach((k) => next.add(k));
      } else if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
    lastPicked.current = key;
  };

  // 点卡片看详情；按住 ⌘ / Ctrl / Shift 点卡片等同于点勾选框。
  const selectCard = (post: Post, event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) togglePick(post, event);
    else setSelected(postKey(post));
  };

  const clearPicks = () => {
    setPicked(new Set());
    lastPicked.current = null;
  };

  const enqueue = async (list: Post[], message: string) => {
    setBusy(true);
    try {
      await addPosts(list);
      setQueued((prev) => {
        const next = new Set(prev);
        list.forEach((post) => next.add(postKey(post)));
        return next;
      });
      setToast({ message, link: true });
      return true;
    } catch (err) {
      setToast({ message: errorMessage(err), link: false });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const downloadPicked = async () => {
    const list = posts.filter((post) => picked.has(postKey(post)));
    if (await enqueue(list, `已加入下载队列：${list.length} 张`)) clearPicks();
  };

  const openBulk = () => {
    const criteria = committed.current;
    setBulk({ criteria, query: results?.query ?? "", count: "loading", max: "" });
    const settle = (count: Bulk["count"]) =>
      setBulk((current) => (current && current.criteria === criteria ? { ...current, count } : current));
    countRemote({ ...criteria, page: 1 }).then(settle, () => settle("failed"));
  };

  const confirmBulk = async () => {
    if (!bulk) return;
    const max = Number.parseInt(bulk.max, 10);
    setBulk(null);
    try {
      await addQuery({ ...bulk.criteria, page: 1 }, Number.isFinite(max) && max > 0 ? max : null);
      setToast({ message: `已加入下载队列：${bulk.query || "全部帖子"}`, link: true });
    } catch (err) {
      setToast({ message: errorMessage(err), link: false });
    }
  };

  const selectedKey = selectedPost ? postKey(selectedPost) : null;
  const primaryAction = selectedPost ? (
    owned.has(postKey(selectedPost)) ? (
      <button type="button" className="btn" disabled>
        <Icon name="check" size={15} />
        已在图库中
      </button>
    ) : queued.has(postKey(selectedPost)) ? (
      <button type="button" className="btn" disabled>
        <Icon name="check" size={15} />
        已加入下载队列
      </button>
    ) : selectedPost.fileUrl ? (
      <button
        type="button"
        className="btn primary"
        disabled={busy}
        onClick={() => void enqueue([selectedPost], `已加入下载队列：#${selectedPost.id}`)}
      >
        <Icon name="download" size={15} />
        下载原图
      </button>
    ) : (
      <button type="button" className="btn" disabled title="站点对未登录用户隐藏了这张图的原图">
        原图需要登录后才能下载
      </button>
    )
  ) : null;

  return (
    <div className="discover">
      <div className="center" data-picking={picked.size > 0 || undefined}>
        <div className="topbar" data-tauri-drag-region>
          <form className="search" onSubmit={submit} role="search">
            <select
              id="search-source"
              className="search-source"
              aria-label="来源"
              value={source}
              onChange={(event) => changeSource(event.target.value as Source)}
            >
              {(Object.keys(SOURCE_LABEL) as Source[]).map((key) => (
                <option key={key} value={key}>
                  {SOURCE_LABEL[key]}
                </option>
              ))}
            </select>
            <input
              id="search-tags"
              className="search-input"
              aria-label="搜索 tag"
              placeholder="输入 tag，空格分隔，例如 scenery sky"
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button type="submit" className="search-go" aria-label="搜索">
              <Icon name="search" size={17} />
            </button>
          </form>
        </div>
        <div className="filters">
          <div className="chips" role="group" aria-label="分级">
            {RATINGS.map((rating) => (
              <button
                key={rating}
                type="button"
                className="chip"
                aria-pressed={ratings.includes(rating)}
                onClick={() => toggleRating(rating)}
              >
                {RATING_LABEL[rating]}
              </button>
            ))}
          </div>
          <span className="filters-space" />
          {results && (
            <span className="query" title="实际发给站点的查询">
              {results.query || "最新帖子"}
            </span>
          )}
          <span className="count">{posts.length} 张</span>
          <button type="button" className="btn sm" onClick={openBulk} disabled={posts.length === 0 || loading}>
            <Icon name="download" size={14} />
            下载全部结果
          </button>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              <button type="button" className="btn" onClick={() => void run(committed.current, error.page)}>
                <Icon name="retry" size={15} />
                重试
              </button>
            </div>
          )}
          {firstLoad && <p className="hint">正在加载…</p>}
          {results && posts.length === 0 && !loading && !error && (
            <p className="hint">没有找到符合条件的图片。可以减少 tag 或放宽分级再试。</p>
          )}
          <PostGrid
            posts={posts}
            selected={selectedKey}
            onSelect={selectCard}
            pageSize={PAGE_SIZE}
            owned={owned}
            picked={picked}
            onPick={togglePick}
          />
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {results?.hasMore && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? "正在加载…" : "加载更多"}
            </button>
          )}
        </div>

        <AnimatePresence>
          {picked.size > 0 && (
            <motion.div
              className="dock"
              role="toolbar"
              aria-label="已选的图片"
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 12 }}
              transition={PANEL_ENTER}
            >
              <span className="dock-count">已选 {formatCount(picked.size)} 张</span>
              <button
                type="button"
                className="btn ghost"
                onClick={() => setPicked(new Set(posts.map(postKey)))}
                disabled={picked.size === posts.length}
              >
                全选已加载的 {formatCount(posts.length)} 张
              </button>
              <button type="button" className="btn ghost" onClick={clearPicks}>
                取消选择
              </button>
              <button type="button" className="btn primary" onClick={() => void downloadPicked()} disabled={busy}>
                <Icon name="download" size={15} />
                下载
              </button>
            </motion.div>
          )}
        </AnimatePresence>

        <AnimatePresence>
          {toast && (
            <motion.div
              className="toast"
              role="status"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 10 }}
              transition={{ duration: 0.2 }}
            >
              <span>{toast.message}</span>
              {toast.link && (
                <button
                  type="button"
                  className="link"
                  onClick={() => {
                    setToast(null);
                    onNavigate("downloads");
                  }}
                >
                  查看
                </button>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <Inspector key={selectedKey ?? "none"} post={selectedPost} primaryAction={primaryAction} />

      <Dialog
        open={bulk !== null}
        title="下载全部搜索结果？"
        onClose={() => setBulk(null)}
        actions={
          <>
            <button type="button" className="btn primary" onClick={() => void confirmBulk()}>
              <Icon name="download" size={15} />
              开始下载
            </button>
            <button type="button" className="btn ghost" onClick={() => setBulk(null)}>
              取消
            </button>
          </>
        }
      >
        {bulk && (
          <>
            <dl className="dialog-paths">
              <dt>条件</dt>
              <dd>
                <code>{bulk.query || "全部帖子"}</code>
              </dd>
              <dt>数量</dt>
              <dd>{countText(bulk.count)}</dd>
              <dt>
                <label htmlFor="bulk-max">上限</label>
              </dt>
              <dd className="dialog-field">
                <input
                  id="bulk-max"
                  className="field-input"
                  type="number"
                  min={1}
                  step={1}
                  inputMode="numeric"
                  placeholder="不限"
                  value={bulk.max}
                  onChange={(event) => {
                    const max = event.target.value;
                    setBulk((current) => current && { ...current, max });
                  }}
                />
                <span>张，留空表示全部下载</span>
              </dd>
            </dl>
            <p className="dialog-note">已在图库里的图会自动跳过。下载在后台进行，可以随时在「下载」里暂停或取消。</p>
          </>
        )}
      </Dialog>

    </div>
  );
}
