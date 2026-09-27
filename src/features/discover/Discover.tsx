import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { motion } from "motion/react";

import { Icon } from "../../components/Icon";
import { ShimmerImage } from "../../components/ShimmerImage";
import {
  errorMessage,
  imageSrc,
  RATING_LABEL,
  RATINGS,
  searchRemote,
  SOURCE_LABEL,
  type Post,
  type Rating,
  type Source,
} from "../../lib/ipc";
import { cardEnter } from "../../lib/motion";
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

const DEFAULT_CRITERIA: Criteria = { source: "danbooru", tags: "", ratings: ["general"] };
/** 与 Rust 端每页条数一致，用于卡片入场错开。 */
const PAGE_SIZE = 40;

/** 极端长宽比的图在瀑布流里按限定比例占位，图片本身居中裁切。 */
function cardRatio(post: Post): string {
  const ratio = Math.min(2.4, Math.max(0.42, post.width / post.height));
  return String(ratio);
}

const postKey = (post: Post) => `${post.source}-${post.id}`;

export function Discover({ active }: { active: boolean }) {
  const [source, setSource] = useState<Source>(DEFAULT_CRITERIA.source);
  const [tags, setTags] = useState(DEFAULT_CRITERIA.tags);
  const [ratings, setRatings] = useState<Rating[]>(DEFAULT_CRITERIA.ratings);
  const [results, setResults] = useState<Results | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  /** 记下失败的是哪一页，重试时重跑这一页。 */
  const [error, setError] = useState<{ message: string; page: number } | null>(null);
  const committed = useRef<Criteria>(DEFAULT_CRITERIA);
  const requestId = useRef(0);
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
      if (page === 1) setSelected(next.posts[0] ? postKey(next.posts[0]) : null);
    } catch (err) {
      if (id === requestId.current) setError({ message: errorMessage(err), page });
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void run(DEFAULT_CRITERIA, 1);
  }, [run]);

  const loadMore = useCallback(() => {
    if (!results?.hasMore || loading || error) return;
    void run(committed.current, results.page + 1);
  }, [results, loading, error, run]);

  useEffect(() => {
    const target = sentinel.current;
    if (!target || !active) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) loadMore();
    }, { rootMargin: "600px 0px" });
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

  return (
    <div className="discover">
      <div className="center">
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
          <div className="grid">
            {posts.map((post, index) => {
              const key = postKey(post);
              return (
                <motion.button
                  key={key}
                  type="button"
                  className="card"
                  style={{ aspectRatio: cardRatio(post) }}
                  aria-pressed={key === selected}
                  aria-label={`#${post.id}，${post.width} × ${post.height}`}
                  onClick={() => setSelected(key)}
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={cardEnter(index, PAGE_SIZE)}
                  whileTap={{ scale: 0.985 }}
                >
                  <ShimmerImage src={imageSrc(post.thumbUrl)} alt="" />
                </motion.button>
              );
            })}
          </div>
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {results?.hasMore && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? "正在加载…" : "加载更多"}
            </button>
          )}
        </div>
      </div>
      <Inspector key={selectedPost ? postKey(selectedPost) : "none"} post={selectedPost} />
    </div>
  );
}
