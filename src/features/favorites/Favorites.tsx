import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { ImageViewer } from "../../components/ImageViewer";
import { LoadingPill } from "../../components/LoadingPill";
import { PostGrid, visibleCard } from "../../components/PostGrid";
import { MultiSelect, Select } from "../../components/Select";
import { SelectionDock } from "../../components/SelectionDock";
import { Toast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import {
  FAVORITE_MODES,
  FAVORITE_SITES,
  favoritesQuery,
  kemonoFavoriteCreators,
  RATED_FAVORITES,
  signedIn,
  type FavoriteCreator,
  type FavoriteMode,
  type FavoriteSite,
} from "../../lib/favorites";
import { formatCount } from "../../lib/format";
import { hasMod, spaceForButton, useHotkeys } from "../../lib/hotkeys";
import { collapsedTitle, t } from "../../lib/i18n";
import {
  countRemote,
  errorCode,
  errorMessage,
  goldOnly,
  postKey,
  postNumber,
  ratingOptions,
  RATINGS,
  searchRemote,
  SOURCE_LABEL,
  type Post,
  type Rating,
  type SearchParams,
} from "../../lib/ipc";
import type { PostRef } from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { accountsInfo, type AccountsInfo } from "../../lib/settings";
import { xCaptureOpen, type XPostsPayload } from "../../lib/x";
import { useDownloads } from "../downloads/context";
import { Inspector } from "../discover/Inspector";

/** 与 Rust 端每页条数一致，用于卡片入场错开。 */
const PAGE_SIZE = 40;
/** 记住上次看的站点和 X 用户名（只是方便，读不到时用默认值）。 */
const SITE_KEY = "imagebox:favorites-site";
const X_HANDLE_KEY = "imagebox:x-handle";

const SITE_OPTIONS = FAVORITE_SITES.map((site) => ({ value: site, label: SOURCE_LABEL[site] }));
const DEFAULT_MODES: Partial<Record<FavoriteSite, FavoriteMode>> = { pixiv: "public", kemono: "posts", x: "likes" };

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // 存不下只是下次不记得，不影响使用。
  }
}

const storedSite = (): FavoriteSite => FAVORITE_SITES.find((site) => site === stored(SITE_KEY)) ?? "danbooru";

/** 接在已有的结果后面，同一帖子只留一张。 */
function appendNew(prev: Post[], incoming: Post[]): Post[] {
  const keys = new Set(prev.map(postKey));
  return [...prev, ...incoming.filter((post) => !keys.has(postKey(post)))];
}

/** 没填账号时，去「设置 → 账号」要做什么。 */
function signInText(site: FavoriteSite): string {
  if (site === "pixiv" || site === "kemono") return t("在「设置 → 账号」里登录 {site}。", { site: SOURCE_LABEL[site] });
  if (site === "yandere") return t("在「设置 → 账号」里填写 Yande.re 用户名。");
  if (site === "gelbooru") return t("在「设置 → 账号」里填写 User ID 和 API Key。");
  return t("在「设置 → 账号」里填写用户名和 API Key。");
}

type Count = number | null | "loading" | "failed";

function countText(count: Count, site: FavoriteSite): string {
  if (count === "loading") return t("正在统计…");
  if (count === "failed") return t("暂时无法统计，可以直接开始");
  if (count === null) return t("站点没有给出总数（条件较复杂时会这样），可以直接开始");
  if (site === "kemono") return t("约 {n} 个帖子", { n: formatCount(count) });
  return t("约 {n} 张", { n: formatCount(count) });
}

export function Favorites({ active, onNavigate }: { active: boolean; onNavigate: Navigate }) {
  const { addPosts, addQuery } = useDownloads();
  const [site, setSite] = useState<FavoriteSite>(storedSite);
  const [modes, setModes] = useState(DEFAULT_MODES);
  const [ratings, setRatings] = useState<Rating[]>([...RATINGS]);
  const [info, setInfo] = useState<AccountsInfo | null>(null);
  const [results, setResults] = useState<{ posts: Post[]; next: string | null } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<{ message: string; code: string | null } | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [viewerPost, setViewerPost] = useState<Post | null>(null);
  /** 已在图库中的帖子。只增不减：下载完成的事件也会加进来。 */
  const [owned, setOwned] = useState<Set<string>>(() => new Set());
  /** 这次打开软件后加入过下载队列、还没下载完的帖子。 */
  const [queued, setQueued] = useState<Set<string>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<{ message: string; link?: boolean } | null>(null);
  const [bulk, setBulk] = useState<{ params: SearchParams; count: Count; max: string } | null>(null);
  /** Kemono 收藏的作者；点开一个作者时看他的帖子。 */
  const [creators, setCreators] = useState<FavoriteCreator[] | null>(null);
  const [creator, setCreator] = useState<FavoriteCreator | null>(null);
  /** X 采集窗口收集到的喜欢和书签。 */
  const [xPosts, setXPosts] = useState<{ likes: Post[]; bookmarks: Post[] }>({ likes: [], bookmarks: [] });
  const [xHandle, setXHandle] = useState(() => stored(X_HANDLE_KEY) ?? "");
  const requestId = useRef(0);
  /** 已经加载过的条件，回到这一页时不再重新加载。 */
  const loadedKey = useRef<string | null>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const center = useRef<HTMLDivElement>(null);

  const mode = modes[site] ?? "posts";
  const account = info?.accounts.find((item) => item.source === site);
  const isX = site === "x";
  const xKind = mode === "bookmarks" ? "bookmarks" : "likes";
  const showCreators = site === "kemono" && mode === "creators" && !creator;
  const rated = RATED_FAVORITES.includes(site);
  const query = creator
    ? `creator:${creator.service}/${creator.id}`
    : showCreators
      ? null
      : favoritesQuery(site, account, mode);
  const params: SearchParams | null = query
    ? { source: site, tags: query, ratings: rated && ratings.length < RATINGS.length ? ratings : [], sort: "newest" }
    : null;
  const paramsKey = params ? JSON.stringify(params) : null;
  const needsAccount = !isX && info !== null && !signedIn(account);
  const posts = isX ? xPosts[xKind] : showCreators ? [] : (results?.posts ?? []);
  const { picked, pickedPosts, toggle: togglePick, clear: clearPicks, pickAll } = usePicker(posts);

  // 每次回到这一页都重新读一遍账号：可能刚在设置里登录或退出。
  useEffect(() => {
    if (!active) return;
    accountsInfo().then(setInfo, (err) => setError({ message: errorMessage(err), code: null }));
  }, [active]);

  const load = useCallback(
    async (search: SearchParams, cursor: string | null) => {
      const id = ++requestId.current;
      setLoading(true);
      setError(null);
      try {
        const page = await searchRemote({ ...search, cursor });
        if (id !== requestId.current) return;
        setResults((prev) => ({ posts: cursor && prev ? appendNew(prev.posts, page.posts) : page.posts, next: page.next }));
        setOwned((prev) => {
          const next = new Set(prev);
          page.owned.forEach((postId) => next.add(postKey({ source: search.source, id: postId })));
          return next;
        });
        if (!cursor) {
          setSelected(page.posts[0] ? postKey(page.posts[0]) : null);
          clearPicks();
        }
      } catch (err) {
        if (id === requestId.current) setError({ message: errorMessage(err), code: errorCode(err) });
      } finally {
        if (id === requestId.current) setLoading(false);
      }
    },
    [clearPicks],
  );

  // 换站点、换类型、换分级时重新加载第一页；只在这一页打开时加载，启动软件时不去访问各个站点。
  useEffect(() => {
    if (!paramsKey || !params) {
      requestId.current++;
      loadedKey.current = null;
      setResults(null);
      setLoading(false);
      return;
    }
    if (!active || loadedKey.current === paramsKey) return;
    loadedKey.current = paramsKey;
    setResults(null);
    void load(params, null);
    // params 由 paramsKey 决定。
  }, [active, paramsKey, load]);

  // Kemono 收藏的作者：切到「作者」时读一次，刷新时重读。
  const loadCreators = useCallback(async () => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);
    try {
      const list = await kemonoFavoriteCreators();
      if (id === requestId.current) setCreators(list);
    } catch (err) {
      if (id === requestId.current) setError({ message: errorMessage(err), code: errorCode(err) });
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (active && showCreators && signedIn(account) && creators === null) void loadCreators();
  }, [active, showCreators, account, creators, loadCreators]);

  const loadMore = useCallback(() => {
    if (isX || !params || !results?.next || loading || error) return;
    void load(params, results.next);
    // 同上，params 由 paramsKey 决定。
  }, [isX, paramsKey, results, loading, error, load]);

  useEffect(() => {
    const target = sentinel.current;
    if (!target || !active) return;
    const observer = new IntersectionObserver((entries) => entries.some((entry) => entry.isIntersecting) && loadMore(), {
      rootMargin: "600px 0px",
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [loadMore, active]);

  useTauriEvent<XPostsPayload>("x-posts", ({ posts: incoming, kind }) => {
    if (kind !== "likes" && kind !== "bookmarks") return;
    setXPosts((prev) => ({ ...prev, [kind]: appendNew(prev[kind], incoming) }));
  });

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

  const changeSite = (next: FavoriteSite) => {
    setSite(next);
    setCreator(null);
    setError(null);
    remember(SITE_KEY, next);
  };

  const changeMode = (next: FavoriteMode) => {
    setModes((prev) => ({ ...prev, [site]: next }));
    setCreator(null);
    setError(null);
  };

  const refresh = () => {
    if (showCreators) {
      setCreators(null);
      return;
    }
    if (!params) return;
    loadedKey.current = paramsKey;
    setResults(null);
    void load(params, null);
  };

  const openX = async (event?: FormEvent) => {
    event?.preventDefault();
    try {
      await xCaptureOpen(xHandle, xKind);
      if (xKind === "likes") remember(X_HANDLE_KEY, xHandle.trim());
      setToast({ message: t("采集窗口已打开，页面滚动时会自动收集图片") });
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
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
      setToast({ message: errorMessage(err) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const downloadPicked = async () => {
    if (await enqueue(pickedPosts, t("已加入下载队列：{n} 张", { n: formatCount(pickedPosts.length) }))) clearPicks();
  };

  const openBulk = () => {
    if (!params) return;
    setBulk({ params, count: "loading", max: "" });
    countRemote(params).then(
      (count) => setBulk((current) => current && { ...current, count }),
      () => setBulk((current) => current && { ...current, count: "failed" }),
    );
  };

  const confirmBulk = async () => {
    if (!bulk) return;
    const max = Number(bulk.max);
    const limit = Number.isFinite(max) && max > 0 ? Math.floor(max) : null;
    setBulk(null);
    try {
      await addQuery(bulk.params, limit);
      setToast({ message: t("已加入下载队列"), link: true });
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
  };

  const selectCard = (post: Post, event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) togglePick(post, event);
    else setSelected(postKey(post));
  };

  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  const selectedKey = selectedPost ? postKey(selectedPost) : null;

  // 和发现页一样的列表快捷键：←/→ 上一张、下一张，空格勾选，⌘/Ctrl + A 全选，Esc 取消勾选，⌘/Ctrl + D 下载。
  useHotkeys(active, (event) => {
    const key = event.key.toLowerCase();
    if ((event.key === "ArrowRight" || event.key === "ArrowLeft") && !hasMod(event)) {
      const index = posts.findIndex((post) => postKey(post) === selectedKey);
      const next = posts[Math.min(posts.length - 1, Math.max(0, index + (event.key === "ArrowRight" ? 1 : -1)))];
      if (next) setSelected(postKey(next));
      return true;
    }
    if (event.key === " " && !spaceForButton(event) && selectedPost) {
      togglePick(selectedPost, event);
      return true;
    }
    if (hasMod(event) && key === "a") {
      pickAll();
      return true;
    }
    if (event.key === "Escape" && picked.size > 0) {
      clearPicks();
      return true;
    }
    if (hasMod(event) && key === "d") {
      if (picked.size > 0) {
        if (!busy) void downloadPicked();
      } else if (selectedPost?.fileUrl && selectedKey && !owned.has(selectedKey) && !queued.has(selectedKey) && !busy) {
        void enqueue([selectedPost], t("已加入下载队列：#{id}", { id: postNumber(selectedPost) }));
      }
      return true;
    }
    return false;
  });

  const primaryAction = selectedPost ? (
    owned.has(postKey(selectedPost)) ? (
      <button type="button" className="btn" disabled>
        <Icon name="check" size={15} />
        {t("已在图库中")}
      </button>
    ) : queued.has(postKey(selectedPost)) ? (
      <button type="button" className="btn" disabled>
        <Icon name="check" size={15} />
        {t("已加入下载队列")}
      </button>
    ) : selectedPost.fileUrl ? (
      <button
        type="button"
        className="btn primary"
        disabled={busy}
        onClick={() => void enqueue([selectedPost], t("已加入下载队列：#{id}", { id: postNumber(selectedPost) }))}
      >
        <Icon name="download" size={15} />
        {t("下载原图")}
      </button>
    ) : (
      <button
        type="button"
        className="btn"
        disabled
        title={
          goldOnly(selectedPost)
            ? t("带受限 tag 的图，Danbooru 只对 Gold 及以上等级的账号开放原图。")
            : t("站点没有开放这张图的原图，画师被封禁或图片已下架时会这样。")
        }
      >
        {goldOnly(selectedPost) ? t("原图需要 Gold 账号") : t("站点没有开放原图")}
      </button>
    )
  ) : undefined;

  const modeOptions = FAVORITE_MODES[site]?.map((option) => ({ value: option.value, label: t(option.label) }));
  const who = creator
    ? creator.name
    : isX
      ? null
      : signedIn(account)
        ? t("{name} 的收藏", { name: account?.name ?? "" })
        : t("还没有登录 {site}", { site: SOURCE_LABEL[site] });

  return (
    <div className="discover">
      <div ref={center} className="center" data-picking={picked.size > 0 || undefined}>
        <div className="topbar" data-tauri-drag-region>
          <form className="search" onSubmit={(event) => (isX ? void openX(event) : event.preventDefault())}>
            <Select
              id="favorites-site"
              className="search-source"
              name={t("平台")}
              value={site}
              options={SITE_OPTIONS}
              onChange={changeSite}
            />
            {isX ? (
              <input
                className="search-input"
                aria-label={t("X 用户名")}
                placeholder={xKind === "likes" ? t("你的 X 用户名，看喜欢要填") : t("书签不用填用户名")}
                value={xHandle}
                onChange={(event) => setXHandle(event.target.value)}
                disabled={xKind === "bookmarks"}
                spellCheck={false}
                autoComplete="off"
              />
            ) : (
              <span className="favorites-who" data-muted={!signedIn(account) || undefined}>
                {who}
              </span>
            )}
            {isX ? (
              <button
                type="submit"
                className="search-go"
                aria-label={t("打开采集窗口")}
                title={t("打开采集窗口")}
                disabled={xKind === "likes" && !xHandle.trim()}
              >
                <Icon name="globe" size={17} />
              </button>
            ) : (
              <button
                type="button"
                className="search-go"
                aria-label={t("刷新")}
                title={t("刷新")}
                onClick={refresh}
                disabled={needsAccount || loading}
              >
                <Icon name="retry" size={17} />
              </button>
            )}
          </form>
        </div>
        <div className="filters">
          {creator ? (
            <button type="button" className="btn sm" onClick={() => setCreator(null)}>
              <Icon name="back" size={14} />
              {t("收藏的作者")}
            </button>
          ) : (
            modeOptions && (
              <Select
                className="filter-select"
                name={t("类型")}
                label={t("类型")}
                value={mode}
                options={modeOptions}
                onChange={changeMode}
              />
            )
          )}
          {rated && (
            <MultiSelect
              className="filter-select"
              name={t("分级")}
              label={t("分级")}
              allLabel={t("全部")}
              values={ratings}
              options={ratingOptions()}
              onChange={setRatings}
            />
          )}
          <div className="filters-end">
            <span className="count">
              {showCreators
                ? t("{n} 位作者", { n: formatCount(creators?.length ?? 0) })
                : t("{n} 张", { n: formatCount(posts.length) })}
            </span>
            {!isX && !showCreators && (
              <button
                type="button"
                className="btn sm collapsible"
                title={collapsedTitle(creator ? t("下载这位作者的全部帖子") : t("下载全部收藏"))}
                onClick={openBulk}
                disabled={!params || posts.length === 0 || loading}
              >
                <Icon name="download" size={14} />
                <span className="btn-text">{creator ? t("下载这位作者的全部帖子") : t("下载全部收藏")}</span>
              </button>
            )}
          </div>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              {error.code === "credentials_missing" || error.code === "bad_credentials" ? (
                <button type="button" className="btn" onClick={() => onNavigate("settings", "accounts")}>
                  <Icon name="user" size={15} />
                  {t("去账号设置")}
                </button>
              ) : (
                <button type="button" className="btn" onClick={refresh}>
                  <Icon name="retry" size={15} />
                  {t("重试")}
                </button>
              )}
            </div>
          )}

          {needsAccount ? (
            <div className="empty">
              <p className="empty-title">{t("登录 {site} 后，这里会显示你的收藏", { site: SOURCE_LABEL[site] })}</p>
              <p>{signInText(site)}</p>
              <button type="button" className="btn primary" onClick={() => onNavigate("settings", "accounts")}>
                <Icon name="user" size={15} />
                {t("去账号设置")}
              </button>
            </div>
          ) : isX && posts.length === 0 ? (
            <div className="empty">
              <p className="empty-title">{xKind === "likes" ? t("在采集窗口里打开你的喜欢") : t("在采集窗口里打开你的书签")}</p>
              <p>{t("窗口里登录 X 后，页面滚动时会自动收集图片，视频暂不收集。")}</p>
              <p>{t("X 从 2024 年起只能看自己的喜欢。")}</p>
              <button type="button" className="btn primary" onClick={() => void openX()} disabled={xKind === "likes" && !xHandle.trim()}>
                <Icon name="globe" size={15} />
                {t("打开采集窗口")}
              </button>
            </div>
          ) : showCreators ? (
            creators && creators.length === 0 && !loading ? (
              <p className="hint">{t("还没有收藏作者。")}</p>
            ) : (
              <ul className="creator-list">
                {creators?.map((item) => (
                  <li key={`${item.service}/${item.id}`}>
                    <button type="button" className="creator-card" onClick={() => setCreator(item)}>
                      <span className="creator-name">{item.name || `${item.service}/${item.id}`}</span>
                      <span className="creator-meta">
                        {item.service}
                        {item.updated ? ` · ${t("更新于 {date}", { date: item.updated.slice(0, 10) })}` : ""}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )
          ) : (
            results &&
            posts.length === 0 &&
            !results.next &&
            !loading &&
            !error && (
              <p className="hint">
                {site === "danbooru"
                  ? t("没有找到收藏。收藏设为私密时，要填写这个账号的 API Key 才能看到。")
                  : t("还没有收藏。")}
              </p>
            )
          )}

          {!needsAccount && !showCreators && (
            <PostGrid
              posts={posts}
              selected={selectedKey}
              onSelect={selectCard}
              onView={(post) => {
                setSelected(postKey(post));
                setViewerPost(post);
              }}
              pageSize={PAGE_SIZE}
              owned={owned}
              picked={picked}
              onPick={togglePick}
            />
          )}
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {!isX && !showCreators && results?.next && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading} data-busy={loading || undefined}>
              {t("加载更多")}
            </button>
          )}
        </div>

        <LoadingPill loading={loading} />

        <SelectionDock count={picked.size} total={posts.length} onPickAll={pickAll} onClear={clearPicks}>
          <button type="button" className="btn primary" onClick={() => void downloadPicked()} disabled={busy}>
            <Icon name="download" size={15} />
            {t("下载")}
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
                  setToast(null);
                  onNavigate("downloads");
                }}
              >
                {t("查看")}
              </button>
            )
          }
        />
      </div>

      <Inspector key={selectedKey ?? "none"} post={selectedPost} primaryAction={primaryAction} />

      <ImageViewer
        post={viewerPost}
        posts={posts}
        useSample
        onClose={() => setViewerPost(null)}
        cardOf={(post) => visibleCard(center.current, post)}
        onChange={(post) => {
          setSelected(postKey(post));
          setViewerPost(post);
        }}
      />

      <Dialog
        open={bulk !== null}
        title={creator ? t("下载这位作者的全部帖子？") : t("下载全部收藏？")}
        onClose={() => setBulk(null)}
        actions={
          <>
            <button type="button" className="btn primary" onClick={() => void confirmBulk()}>
              <Icon name="download" size={15} />
              {t("开始下载")}
            </button>
            <button type="button" className="btn ghost" onClick={() => setBulk(null)}>
              {t("取消")}
            </button>
          </>
        }
      >
        {bulk && (
          <>
            <dl className="dialog-paths">
              <dt>{t("来源")}</dt>
              <dd>{who ? `${SOURCE_LABEL[site]} · ${who}` : SOURCE_LABEL[site]}</dd>
              <dt>{t("数量")}</dt>
              <dd>{countText(bulk.count, site)}</dd>
              <dt>
                <label htmlFor="favorites-bulk-max">{t("上限")}</label>
              </dt>
              <dd className="dialog-field">
                <input
                  id="favorites-bulk-max"
                  className="field-input"
                  type="number"
                  min={1}
                  step={1}
                  inputMode="numeric"
                  placeholder={t("不限")}
                  value={bulk.max}
                  onChange={(event) => {
                    const max = event.target.value;
                    setBulk((current) => current && { ...current, max });
                  }}
                />
                <span>{t("张，留空表示全部下载")}</span>
              </dd>
            </dl>
            <p className="dialog-note">
              {t("已在图库里的图会自动跳过。下载在后台进行，可以随时在「下载」里暂停或取消。")}
            </p>
          </>
        )}
      </Dialog>
    </div>
  );
}
