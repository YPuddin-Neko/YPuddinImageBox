import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import { SelectionDock } from "../../components/SelectionDock";
import { Toast, useToast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import { errorMessage, postKey, RATING_LABEL, RATINGS, SOURCE_LABEL, type Rating, type Source } from "../../lib/ipc";
import { libraryDelete, libraryList, type LocalPost, type PostRef } from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { revealLabel, trashLabel } from "../../lib/platform";
import { Inspector } from "../discover/Inspector";
import { useDownloads } from "../downloads/DownloadsProvider";

interface Filter {
  source: Source | "all";
  tags: string;
  ratings: Rating[];
}

interface Listing {
  posts: LocalPost[];
  total: number;
  hasMore: boolean;
}

const PAGE_SIZE = 60;
const EMPTY_FILTER: Filter = { source: "all", tags: "", ratings: [] };

const isFiltered = (filter: Filter) =>
  filter.source !== "all" || filter.tags.trim() !== "" || (filter.ratings.length > 0 && filter.ratings.length < RATINGS.length);

export function Library({ active, onNavigate }: { active: boolean; onNavigate: Navigate }) {
  const [source, setSource] = useState<Filter["source"]>("all");
  const [tags, setTags] = useState("");
  const [ratings, setRatings] = useState<Rating[]>([]);
  const [listing, setListing] = useState<Listing | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<{ message: string; offset: number } | null>(null);
  /** 上次刷新之后新下载的张数。 */
  const [fresh, setFresh] = useState(0);
  /** 操作失败的提示（在访达中显示、删除、重新下载）。 */
  const [actionError, setActionError] = useState<string | null>(null);
  /** 等待确认删除的图。 */
  const [deleting, setDeleting] = useState<LocalPost[] | null>(null);
  const [busy, setBusy] = useState(false);
  /** 这次打开软件后点过「重新下载」的图。 */
  const [requeued, setRequeued] = useState<Set<string>>(() => new Set());
  const [notice, setNotice] = useToast();
  const { addPosts } = useDownloads();
  const committed = useRef<Filter>(EMPTY_FILTER);
  const requestId = useRef(0);
  const refreshTimer = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);

  const load = useCallback(async (filter: Filter, offset: number) => {
    const id = ++requestId.current;
    committed.current = filter;
    setLoading(true);
    setError(null);
    try {
      const page = await libraryList({
        source: filter.source === "all" ? null : filter.source,
        tags: filter.tags,
        ratings: filter.ratings,
        offset,
        limit: PAGE_SIZE,
      });
      if (id !== requestId.current) return;
      setListing((prev) => {
        if (offset === 0 || !prev) return { posts: page.posts, total: page.total, hasMore: page.hasMore };
        const seen = new Set(prev.posts.map(postKey));
        return {
          posts: [...prev.posts, ...page.posts.filter((post) => !seen.has(postKey(post)))],
          total: page.total,
          hasMore: page.hasMore,
        };
      });
      if (offset === 0) {
        setFresh(0);
        setSelected((current) =>
          page.posts.some((post) => postKey(post) === current) ? current : page.posts[0] ? postKey(page.posts[0]) : null,
        );
      }
    } catch (err) {
      if (id === requestId.current) setError({ message: errorMessage(err), offset });
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(EMPTY_FILTER, 0);
    return () => window.clearTimeout(refreshTimer.current);
  }, [load]);

  useTauriEvent<SavedPayload>(EVENTS.librarySaved, () => setFresh((count) => count + 1));

  const posts = listing?.posts ?? [];
  const { picked, pickedPosts, toggle: togglePick, clear: clearPicks, pickAll, forget } = usePicker(posts);

  // 删掉的图直接从列表里拿掉，不必重新加载。
  useTauriEvent<PostRef[]>(EVENTS.libraryRemoved, (removed) => {
    const gone = new Set(removed.map((post) => postKey({ source: post.source, id: post.postId })));
    setListing((prev) =>
      prev && {
        ...prev,
        posts: prev.posts.filter((post) => !gone.has(postKey(post))),
        total: Math.max(0, prev.total - gone.size),
      },
    );
    forget(gone);
  });

  // 有新下载的图时：视图在前台、还停在第一页附近就自动刷新（下载进行中最多每 1.2 秒一次）；
  // 已经往下翻了很多就只提示，免得列表突然跳动。视图不在前台时等切回来再刷新。
  const deep = (listing?.posts.length ?? 0) > PAGE_SIZE;
  useEffect(() => {
    if (fresh === 0 || !active || deep || refreshTimer.current) return;
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = 0;
      void load(committed.current, 0);
    }, 1200);
  }, [fresh, active, deep, load]);

  const loadMore = useCallback(() => {
    if (!listing?.hasMore || loading || error) return;
    void load(committed.current, listing.posts.length);
  }, [listing, loading, error, load]);

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
    void load({ source, tags, ratings }, 0);
  };

  const toggleRating = (rating: Rating) => {
    const next = ratings.includes(rating) ? ratings.filter((r) => r !== rating) : [...ratings, rating];
    setRatings(next);
    void load({ source, tags, ratings: next }, 0);
  };

  const changeSource = (next: Filter["source"]) => {
    setSource(next);
    void load({ source: next, tags, ratings }, 0);
  };

  const reveal = async (post: LocalPost) => {
    setActionError(null);
    try {
      await revealItemInDir(post.path);
    } catch (err) {
      setActionError(`${revealLabel}失败：${errorMessage(err)}`);
    }
  };

  // 文件不见了：按原来的帖子重新下载，存好后图库自动刷新。
  const redownload = async (post: LocalPost) => {
    setActionError(null);
    try {
      await addPosts([post]);
      setRequeued((prev) => new Set(prev).add(postKey(post)));
      setNotice(`已加入下载队列：#${post.id}`);
    } catch (err) {
      setActionError(errorMessage(err));
    }
  };

  const confirmDelete = async (keepFiles: boolean) => {
    if (!deleting) return;
    const list = deleting;
    setDeleting(null);
    setBusy(true);
    setActionError(null);
    try {
      const outcome = await libraryDelete(
        list.map((post) => ({ source: post.source, postId: post.id })),
        keepFiles,
      );
      if (outcome.removed.length > 0) {
        setNotice(keepFiles ? `已从图库移除 ${outcome.removed.length} 张` : `已删除 ${outcome.removed.length} 张`);
      }
      if (outcome.failed.length > 0) {
        setActionError(`有 ${outcome.failed.length} 张没能移到${trashLabel}：${outcome.failed[0].message}`);
      }
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  // 点卡片看详情；按住 ⌘ / Ctrl / Shift 点卡片等同于点勾选框。
  const selectCard = (post: LocalPost, event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) togglePick(post, event);
    else setSelected(postKey(post));
  };

  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  const filtered = isFiltered(committed.current);
  const missing = new Set(posts.filter((post) => post.missing).map(postKey));

  return (
    <div className="discover">
      <div className="center" data-picking={picked.size > 0 || undefined}>
        <div className="topbar" data-tauri-drag-region>
          <form className="search" onSubmit={submit} role="search">
            <select
              className="search-source"
              aria-label="来源"
              value={source}
              onChange={(event) => changeSource(event.target.value as Filter["source"])}
            >
              <option value="all">全部来源</option>
              {(Object.keys(SOURCE_LABEL) as Source[]).map((key) => (
                <option key={key} value={key}>
                  {SOURCE_LABEL[key]}
                </option>
              ))}
            </select>
            <input
              className="search-input"
              aria-label="按 tag 筛选"
              placeholder="按 tag 筛选图库，空格分隔，-tag 表示排除"
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <button type="submit" className="search-go" aria-label="筛选">
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
          {fresh > 0 && deep && (
            <button type="button" className="btn sm" onClick={() => void load(committed.current, 0)}>
              <Icon name="retry" size={14} />
              有 {formatCount(fresh)} 张新下载的图
            </button>
          )}
          <span className="count">共 {formatCount(listing?.total ?? 0)} 张</span>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              <button type="button" className="btn" onClick={() => void load(committed.current, error.offset)}>
                <Icon name="retry" size={15} />
                重试
              </button>
            </div>
          )}
          {actionError && (
            <div className="alert" role="alert">
              <span>{actionError}</span>
              <button type="button" className="btn" onClick={() => setActionError(null)}>
                关闭
              </button>
            </div>
          )}
          {!listing && loading && <p className="hint">正在加载…</p>}
          {listing && posts.length === 0 && !loading && !error && (
            filtered ? (
              <p className="hint">没有符合条件的图片。可以减少 tag 或放宽分级再试。</p>
            ) : (
              <div className="empty">
                <p className="empty-title">图库里还没有图片</p>
                <p>在「发现」里下载的图片会出现在这里。</p>
                <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
                  <Icon name="compass" size={15} />
                  去发现
                </button>
              </div>
            )
          )}
          <PostGrid
            posts={posts}
            selected={selected}
            onSelect={selectCard}
            pageSize={PAGE_SIZE}
            missing={missing}
            picked={picked}
            onPick={togglePick}
          />
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {listing?.hasMore && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? "正在加载…" : "加载更多"}
            </button>
          )}
        </div>

        <SelectionDock count={picked.size} total={posts.length} onPickAll={pickAll} onClear={clearPicks}>
          <button type="button" className="btn danger" onClick={() => setDeleting(pickedPosts)} disabled={busy}>
            <Icon name="trash" size={15} />
            删除
          </button>
        </SelectionDock>
        <Toast message={notice} />
      </div>
      <Inspector
        key={selectedPost ? postKey(selectedPost) : "none"}
        post={selectedPost}
        emptyText="点一张图查看详情"
        localPath={selectedPost?.path}
        notice={selectedPost?.missing ? "文件不在记录的位置，可能已被移动或删除。可以重新下载到图片位置。" : undefined}
        primaryAction={
          selectedPost && (
            <>
              {!selectedPost.missing ? (
                <button type="button" className="btn primary" onClick={() => void reveal(selectedPost)}>
                  <Icon name="folder" size={15} />
                  {revealLabel}
                </button>
              ) : requeued.has(postKey(selectedPost)) ? (
                <button type="button" className="btn" disabled>
                  <Icon name="check" size={15} />
                  已加入下载队列
                </button>
              ) : (
                <button type="button" className="btn primary" onClick={() => void redownload(selectedPost)}>
                  <Icon name="download" size={15} />
                  重新下载
                </button>
              )}
              <button
                type="button"
                className="btn danger icon-only"
                aria-label="删除这张图"
                title="删除这张图"
                onClick={() => setDeleting([selectedPost])}
                disabled={busy}
              >
                <Icon name="trash" size={15} />
              </button>
            </>
          )
        }
      />

      <Dialog
        open={deleting !== null}
        title={deleting && deleting.length > 1 ? `删除选中的 ${formatCount(deleting.length)} 张图？` : "删除这张图？"}
        onClose={() => setDeleting(null)}
        initialFocus="last"
        actions={
          <>
            <button type="button" className="btn danger" onClick={() => void confirmDelete(false)}>
              <Icon name="trash" size={15} />
              移到{trashLabel}
            </button>
            <button type="button" className="btn" onClick={() => void confirmDelete(true)}>
              只从图库移除
            </button>
            <button type="button" className="btn ghost" onClick={() => setDeleting(null)}>
              取消
            </button>
          </>
        }
      >
        <ul className="dialog-options">
          <li>
            <b>移到{trashLabel}</b>：图片文件移到{trashLabel}，还能从那里找回；图库记录和缩略图一起删除。
          </li>
          <li>
            <b>只从图库移除</b>：文件留在原处，只删除图库记录。
          </li>
        </ul>
      </Dialog>
    </div>
  );
}
