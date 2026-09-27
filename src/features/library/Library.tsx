import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import { errorMessage, postKey, RATING_LABEL, RATINGS, SOURCE_LABEL, type Rating, type Source } from "../../lib/ipc";
import { libraryList, type LocalPost } from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { revealLabel } from "../../lib/platform";
import { Inspector } from "../discover/Inspector";

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
  const [revealError, setRevealError] = useState<string | null>(null);
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
    setRevealError(null);
    try {
      await revealItemInDir(post.path);
    } catch (err) {
      setRevealError(`${revealLabel}失败：${errorMessage(err)}`);
    }
  };

  const posts = listing?.posts ?? [];
  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  const filtered = isFiltered(committed.current);

  return (
    <div className="discover">
      <div className="center">
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
          {revealError && (
            <div className="alert" role="alert">
              <span>{revealError}</span>
              <button type="button" className="btn" onClick={() => setRevealError(null)}>
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
          <PostGrid posts={posts} selected={selected} onSelect={(post) => setSelected(postKey(post))} pageSize={PAGE_SIZE} />
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {listing?.hasMore && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? "正在加载…" : "加载更多"}
            </button>
          )}
        </div>
      </div>
      <Inspector
        key={selectedPost ? postKey(selectedPost) : "none"}
        post={selectedPost}
        emptyText="点一张图查看详情"
        localPath={selectedPost?.path}
        primaryAction={
          selectedPost && (
            <button type="button" className="btn primary" onClick={() => void reveal(selectedPost)}>
              <Icon name="folder" size={15} />
              {revealLabel}
            </button>
          )
        }
      />
    </div>
  );
}
