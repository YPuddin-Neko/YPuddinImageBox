import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import type { View } from "../../components/Rail";
import { SelectionDock } from "../../components/SelectionDock";
import { Toast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import {
  countRemote,
  errorCode,
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
import type { PostRef } from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { INTERVALS, subscriptionCreate, subscriptionTitle } from "../../lib/subscriptions";
import { useDownloads } from "../downloads/context";
import { Inspector } from "./Inspector";

interface Criteria {
  source: Source;
  tags: string;
  ratings: Rating[];
}

interface Results {
  posts: Post[];
  /** 下一页的位置，没有更多时为 null。 */
  next: string | null;
  query: string;
  /** 超出 tag 上限、在本地筛选的 tag。 */
  localFilter: string;
}

/** 「下载全部结果」对话框。 */
interface Bulk {
  criteria: Criteria;
  query: string;
  localFilter: string;
  count: number | null | "loading" | "failed";
  /** 最多下载前多少张，留空表示不限。 */
  max: string;
}

interface Toast {
  message: string;
  /** 带「查看」按钮时跳到哪个页面。 */
  link?: View;
}

/** 「订阅」对话框。 */
interface SubscribeDraft {
  criteria: Criteria;
  query: string;
  localFilter: string;
  interval: number;
  /** 现在是否也下载已有的图。 */
  existing: boolean;
  max: string;
  busy: boolean;
  error: string | null;
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

export function Discover({ active, onNavigate }: { active: boolean; onNavigate: Navigate }) {
  const { addPosts, addQuery } = useDownloads();
  const [source, setSource] = useState<Source>(DEFAULT_CRITERIA.source);
  const [tags, setTags] = useState(DEFAULT_CRITERIA.tags);
  const [ratings, setRatings] = useState<Rating[]>(DEFAULT_CRITERIA.ratings);
  const [results, setResults] = useState<Results | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  /** 记下失败的是哪一页，重试时重跑这一页；code 用来判断是不是账号问题。 */
  const [error, setError] = useState<{ message: string; code: string | null; cursor: string | null } | null>(null);
  /** 已在图库中的帖子。只增不减：下载完成的事件也会加进来。 */
  const [owned, setOwned] = useState<Set<string>>(() => new Set());
  /** 这次打开软件后加入过下载队列、还没下载完的帖子。 */
  const [queued, setQueued] = useState<Set<string>>(() => new Set());
  const [bulk, setBulk] = useState<Bulk | null>(null);
  const [subscribing, setSubscribing] = useState<SubscribeDraft | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [busy, setBusy] = useState(false);
  const committed = useRef<Criteria>(DEFAULT_CRITERIA);
  const requestId = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);

  const posts = results?.posts ?? [];
  const { picked, pickedPosts, toggle: togglePick, clear: clearPicks, pickAll } = usePicker(posts);

  /** `cursor` 为 null 表示重新搜第一页。 */
  const run = useCallback(async (criteria: Criteria, cursor: string | null) => {
    const id = ++requestId.current;
    const first = cursor === null;
    committed.current = criteria;
    setLoading(true);
    setError(null);
    try {
      const next = await searchRemote({ ...criteria, cursor });
      if (id !== requestId.current) return;
      setResults((prev) => {
        const page = { posts: next.posts, next: next.next, query: next.query, localFilter: next.localFilter };
        if (first || !prev) return page;
        const seen = new Set(prev.posts.map(postKey));
        return { ...page, posts: [...prev.posts, ...next.posts.filter((p) => !seen.has(postKey(p)))] };
      });
      setOwned((prev) => {
        const ownedNow = new Set(prev);
        next.owned.forEach((postId) => ownedNow.add(postKey({ source: criteria.source, id: postId })));
        return ownedNow;
      });
      if (first) {
        setSelected(next.posts[0] ? postKey(next.posts[0]) : null);
        clearPicks();
      }
    } catch (err) {
      if (id === requestId.current) setError({ message: errorMessage(err), code: errorCode(err), cursor });
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, [clearPicks]);

  useEffect(() => {
    void run(DEFAULT_CRITERIA, null);
  }, [run]);

  useTauriEvent<PostRef[]>(EVENTS.libraryRemoved, (removed) =>
    setOwned((prev) => {
      const next = new Set(prev);
      removed.forEach((post) => next.delete(postKey({ source: post.source, id: post.postId })));
      return next;
    }),
  );

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
    if (!results?.next || loading || error) return;
    void run(committed.current, results.next);
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
    void run({ source, tags, ratings }, null);
  };

  const toggleRating = (rating: Rating) => {
    const next = ratings.includes(rating) ? ratings.filter((r) => r !== rating) : [...ratings, rating];
    setRatings(next);
    void run({ source, tags, ratings: next }, null);
  };

  const changeSource = (next: Source) => {
    setSource(next);
    void run({ source: next, tags, ratings }, null);
  };

  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  const firstLoad = loading && !results;

  // 点卡片看详情；按住 ⌘ / Ctrl / Shift 点卡片等同于点勾选框。
  const selectCard = (post: Post, event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) togglePick(post, event);
    else setSelected(postKey(post));
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
      setToast({ message, link: "downloads" });
      return true;
    } catch (err) {
      setToast({ message: errorMessage(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const downloadPicked = async () => {
    if (await enqueue(pickedPosts, `已加入下载队列：${pickedPosts.length} 张`)) clearPicks();
  };

  const openBulk = () => {
    const criteria = committed.current;
    setBulk({ criteria, query: results?.query ?? "", localFilter: results?.localFilter ?? "", count: "loading", max: "" });
    const settle = (count: Bulk["count"]) =>
      setBulk((current) => (current && current.criteria === criteria ? { ...current, count } : current));
    countRemote(criteria).then(settle, () => settle("failed"));
  };

  const confirmBulk = async () => {
    if (!bulk) return;
    const max = Number.parseInt(bulk.max, 10);
    setBulk(null);
    try {
      await addQuery(bulk.criteria, Number.isFinite(max) && max > 0 ? max : null);
      setToast({ message: `已加入下载队列：${bulk.query || "全部帖子"}`, link: "downloads" });
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
  };

  const openSubscribe = () => {
    setSubscribing({
      criteria: committed.current,
      query: results?.query ?? "",
      localFilter: results?.localFilter ?? "",
      interval: 360,
      existing: false,
      max: "",
      busy: false,
      error: null,
    });
  };

  const confirmSubscribe = async () => {
    if (!subscribing) return;
    const draft = subscribing;
    setSubscribing({ ...draft, busy: true, error: null });
    const max = Number.parseInt(draft.max, 10);
    try {
      const sub = await subscriptionCreate(
        draft.criteria,
        draft.interval,
        draft.existing,
        draft.existing && Number.isFinite(max) && max > 0 ? max : null,
      );
      setSubscribing(null);
      setToast({ message: `已订阅「${subscriptionTitle(sub)}」`, link: "subscriptions" });
    } catch (err) {
      setSubscribing((current) => current && { ...current, busy: false, error: errorMessage(err) });
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
          {results?.localFilter && (
            <span
              className="local-filter"
              title={`站点一次能搜的 tag 数有限，「${results.localFilter}」在本地逐页筛选，加载会慢一些。登录后能直接搜更多 tag。`}
            >
              本地筛选 {results.localFilter}
            </span>
          )}
          <span className="count">{posts.length} 张</span>
          <button type="button" className="btn sm" onClick={openSubscribe} disabled={!results || loading}>
            <Icon name="bell" size={14} />
            订阅
          </button>
          <button type="button" className="btn sm" onClick={openBulk} disabled={posts.length === 0 || loading}>
            <Icon name="download" size={14} />
            下载全部结果
          </button>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              {error.code === "credentials_missing" || error.code === "bad_credentials" ? (
                <button type="button" className="btn" onClick={() => onNavigate("settings", "accounts")}>
                  <Icon name="user" size={15} />
                  填写账号
                </button>
              ) : (
                <button type="button" className="btn" onClick={() => void run(committed.current, error.cursor)}>
                  <Icon name="retry" size={15} />
                  重试
                </button>
              )}
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
          {results?.next && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? "正在加载…" : "加载更多"}
            </button>
          )}
        </div>

        <SelectionDock count={picked.size} total={posts.length} onPickAll={pickAll} onClear={clearPicks}>
          <button type="button" className="btn primary" onClick={() => void downloadPicked()} disabled={busy}>
            <Icon name="download" size={15} />
            下载
          </button>
        </SelectionDock>

        <Toast
          message={toast?.message ?? null}
          action={
            toast?.link && (
              <button
                type="button"
                className="link"
                onClick={() => {
                  const target = toast.link;
                  setToast(null);
                  if (target) onNavigate(target);
                }}
              >
                查看
              </button>
            )
          }
        />
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
                {bulk.localFilter && <span className="dialog-sub">，本地筛选 <code>{bulk.localFilter}</code></span>}
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

      <Dialog
        open={subscribing !== null}
        title="订阅这个搜索条件？"
        onClose={() => setSubscribing(null)}
        actions={
          <>
            <button
              type="button"
              className="btn primary"
              onClick={() => void confirmSubscribe()}
              disabled={subscribing?.busy}
            >
              <Icon name="bell" size={15} />
              {subscribing?.busy ? "正在订阅…" : "订阅"}
            </button>
            <button type="button" className="btn ghost" onClick={() => setSubscribing(null)}>
              取消
            </button>
          </>
        }
      >
        {subscribing && (
          <>
            <dl className="dialog-paths">
              <dt>条件</dt>
              <dd>
                <code>{subscribing.query || "全部帖子"}</code>
                {subscribing.localFilter && (
                  <span className="dialog-sub">，本地筛选 <code>{subscribing.localFilter}</code></span>
                )}
              </dd>
              <dt>
                <label htmlFor="subscribe-interval">检查</label>
              </dt>
              <dd className="dialog-field">
                <select
                  id="subscribe-interval"
                  className="select"
                  value={subscribing.interval}
                  onChange={(event) => {
                    const interval = Number(event.target.value);
                    setSubscribing((current) => current && { ...current, interval });
                  }}
                >
                  {INTERVALS.map((option) => (
                    <option key={option.minutes} value={option.minutes}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </dd>
              <dt>已有的图</dt>
              <dd className="dialog-choices">
                <label className="choice">
                  <input
                    type="radio"
                    name="subscribe-existing"
                    checked={!subscribing.existing}
                    onChange={() => setSubscribing((current) => current && { ...current, existing: false })}
                  />
                  不下载，只下载以后的新图
                </label>
                <label className="choice">
                  <input
                    type="radio"
                    name="subscribe-existing"
                    checked={subscribing.existing}
                    onChange={() => setSubscribing((current) => current && { ...current, existing: true })}
                  />
                  现在也下载，最多
                  <input
                    className="field-input"
                    type="number"
                    min={1}
                    step={1}
                    inputMode="numeric"
                    placeholder="不限"
                    aria-label="最多下载多少张已有的图"
                    value={subscribing.max}
                    onFocus={() => setSubscribing((current) => current && { ...current, existing: true })}
                    onChange={(event) => {
                      const max = event.target.value;
                      setSubscribing((current) => current && { ...current, max, existing: true });
                    }}
                  />
                  张
                </label>
              </dd>
            </dl>
            {subscribing.error && <p className="form-error">{subscribing.error}</p>}
            <p className="dialog-note">
              以后按设定的间隔检查，有新图就自动下载。关掉窗口后会在后台继续，可以在「设置 → 通用」里修改。
            </p>
          </>
        )}
      </Dialog>

    </div>
  );
}
