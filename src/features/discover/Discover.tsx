import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import type { View } from "../../components/Rail";
import { MenuButton, MultiSelect, Select } from "../../components/Select";
import { SelectionDock } from "../../components/SelectionDock";
import { Toast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import { hasMod, spaceForButton, useHotkeys } from "../../lib/hotkeys";
import { collapsedTitle, sentences, t, tx } from "../../lib/i18n";
import {
  countRemote,
  errorCode,
  errorMessage,
  goldOnly,
  postKey,
  postNumber,
  ratingOptions,
  RATINGS,
  remoteSortLabel,
  remoteSorts,
  searchRemote,
  searchSites,
  SOURCE_LABEL,
  SOURCE_OPTIONS,
  SOURCES,
  type Post,
  type Rating,
  type RemoteSort,
  type SearchParams,
  type SiteStatus,
  type Source,
} from "../../lib/ipc";
import type { PostRef } from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import {
  sameSearch,
  savedHint,
  savedSearchAdd,
  savedSearchesList,
  savedSearchRemove,
  savedTitle,
  type SavedSearch,
} from "../../lib/saved";
import { intervalOptions, subscriptionCreate, subscriptionPreview, subscriptionTitle } from "../../lib/subscriptions";
import { useDownloads } from "../downloads/context";
import { Inspector } from "./Inspector";

interface Criteria {
  /** 来源里勾选的站点；两个以上时是聚合搜索。 */
  sources: Source[];
  /** 聚合搜索时平台筛选留下的站点（来源里勾选的一部分）；只搜一个站点时不用。 */
  platforms: Source[];
  tags: string;
  ratings: Rating[];
  sort: RemoteSort;
}

interface Results {
  posts: Post[];
  /** 下一页的位置，没有更多时为 null。 */
  next: string | null;
  /** 各站点实际发出的查询、本地筛选的 tag 和出错情况；只搜一个站点时只有一项。 */
  sites: SiteStatus[];
  /** 聚合搜索的结果：卡片上标出每张图来自哪个站点。 */
  combined: boolean;
}

type Count = number | null | "loading" | "failed";

/** 对话框里一个站点的条件。 */
interface SiteCriteria {
  source: Source;
  query: string;
  localFilter: string;
}

/** 「下载全部结果」对话框。聚合搜索时每个平台各建一个下载任务。 */
interface Bulk {
  criteria: Criteria;
  sites: (SiteCriteria & { count: Count })[];
  /** 最多下载前多少张，留空表示不限。 */
  max: string;
}

interface Toast {
  message: string;
  /** 带「查看」按钮时跳到哪个页面。 */
  link?: View;
}

/** 「订阅」对话框。聚合搜索时每个平台各建一个订阅。 */
interface SubscribeDraft {
  criteria: Criteria;
  /** 各平台实际订阅的条件：订阅不带排序，超出 tag 上限时的拆分可能和当前搜索不同。 */
  sites: SiteCriteria[];
  interval: number;
  /** 现在是否也下载已有的图。 */
  existing: boolean;
  max: string;
  busy: boolean;
  error: string | null;
}

const DEFAULT_CRITERIA: Criteria = {
  sources: ["danbooru"],
  platforms: ["danbooru"],
  tags: "",
  ratings: ["general"],
  sort: "newest",
};
/** 与 Rust 端每页条数一致，用于卡片入场错开。 */
const PAGE_SIZE = 40;
/** 来源、分级、平台都是复选，连着勾几项时等停下来再搜，免得每勾一项搜一次。 */
const FILTER_DEBOUNCE_MS = 300;

function countText(count: Count): string {
  if (count === "loading") return t("正在统计…");
  if (count === "failed") return t("暂时无法统计，可以直接开始");
  if (count === null) return t("站点没有给出总数（条件较复杂时会这样），可以直接开始");
  return t("约 {n} 张", { n: formatCount(count) });
}

/** 发给只认一个站点的接口（统计张数、下载全部结果、订阅）的条件。 */
const siteParams = (criteria: Criteria, source: Source): SearchParams => ({
  source,
  tags: criteria.tags,
  ratings: criteria.ratings,
  sort: criteria.sort,
});

/** 来源勾选了两个以上站点：聚合搜索，卡片上标出每张图来自哪个站点。 */
const isCombined = (criteria: Criteria) => criteria.sources.length > 1;

/** 实际要搜的站点：来源勾选了几个时，平台筛选再从里面挑。收藏按这个存，也按这个比较。 */
function searchSources(criteria: Criteria): Source[] {
  if (!isCombined(criteria)) return criteria.sources;
  const picked = criteria.platforms.filter((source) => criteria.sources.includes(source));
  return picked.length > 0 ? picked : criteria.sources;
}

/** 搜一页：只搜一个站点和聚合搜索的结果整理成同一种样子，`owned` 是这一页里已在图库中的帖子。 */
async function searchPage(criteria: Criteria, cursor: string | null): Promise<{ results: Results; owned: string[] }> {
  const sources = searchSources(criteria);
  if (isCombined(criteria)) {
    const page = await searchSites({
      sources,
      tags: criteria.tags,
      ratings: criteria.ratings,
      sort: criteria.sort,
      cursor,
    });
    return {
      results: { posts: page.posts, next: page.next, sites: page.sites, combined: true },
      owned: page.owned.map((post) => postKey({ source: post.source, id: post.postId })),
    };
  }
  const source = sources[0];
  const page = await searchRemote({ ...siteParams(criteria, source), cursor });
  const site: SiteStatus = { source, query: page.query, localFilter: page.localFilter, error: null, retry: false };
  return {
    results: { posts: page.posts, next: page.next, sites: [site], combined: false },
    owned: page.owned.map((id) => postKey({ source, id })),
  };
}

/** 接在已有的结果后面，同一帖子只留一张；同一张图（md5 相同）两个平台都有时只留先出现的那张。 */
function appendNew(prev: Post[], incoming: Post[]): Post[] {
  const keys = new Set(prev.map(postKey));
  const hashes = new Set(prev.flatMap((post) => (post.md5 ? [post.md5.toLowerCase()] : [])));
  const added = incoming.filter((post) => {
    const key = postKey(post);
    const hash = post.md5?.toLowerCase();
    if (keys.has(key) || (hash && hashes.has(hash))) return false;
    keys.add(key);
    if (hash) hashes.add(hash);
    return true;
  });
  return [...prev, ...added];
}

/** 往下翻时更新各站点的情况；这一页没搜的站点（已经翻完，或出错后不再往下翻）保留上一次的。 */
const mergeSites = (prev: SiteStatus[], next: SiteStatus[]) =>
  SOURCES.flatMap((source) => next.find((site) => site.source === source) ?? prev.find((site) => site.source === source) ?? []);

/** 筛选行右侧显示的查询：各站点一样时只写一份，不一样时分站点写。 */
function queryText(sites: SiteStatus[]): string {
  const queries = sites.map((site) => site.query || t("最新帖子"));
  if (new Set(queries).size <= 1) return queries[0] ?? "";
  return sites.map((site, index) => `${SOURCE_LABEL[site.source]} ${queries[index]}`).join(" · ");
}

function queryTitle(sites: SiteStatus[]): string {
  const title = t("实际发给站点的查询");
  if (sites.length < 2) return title;
  const lines = sites.map((site) =>
    t("{site}：{message}", { site: SOURCE_LABEL[site.source], message: site.query || t("最新帖子") }),
  );
  return [title, ...lines].join("\n");
}

/** 站点出错的提示：出错信息里没提到是哪个站点时（例如网络问题）补上站点名。 */
function siteMessage(site: SiteStatus): string {
  const message = site.error?.message ?? "";
  const name = SOURCE_LABEL[site.source];
  return message.includes(name) ? message : t("{site}：{message}", { site: name, message });
}

const isAccountError = (code: string | null | undefined) => code === "credentials_missing" || code === "bad_credentials";
/** 账号出错时去设置页的按钮：Pixiv 是登录，其余站点填用户名和 API Key。 */
const accountAction = (source: Source | undefined) => (source === "pixiv" ? t("登录 Pixiv") : t("填写账号"));

const sitesLabel = (sources: Source[]) => sources.map((source) => SOURCE_LABEL[source]).join(t("、::list"));

/** 对话框里的站点名。按最长的站点名留宽度，几行后面的条件、数量对齐。 */
function SiteName({ source }: { source: Source }) {
  return (
    <span className="dialog-site-name">
      {SOURCES.map((other) => (
        <span key={other} className="dialog-site-sizer" aria-hidden="true">
          {SOURCE_LABEL[other]}
        </span>
      ))}
      <span>{SOURCE_LABEL[source]}</span>
    </span>
  );
}

/** 对话框里一个站点的条件；聚合搜索时每个平台一行，前面写站点名。 */
function SiteCondition({ site, named }: { site: SiteCriteria; named: boolean }) {
  const condition = (
    <>
      <code>{site.query || t("全部帖子")}</code>
      {site.localFilter && (
        <span className="dialog-sub">{tx("，本地筛选 {filter}", { filter: <code>{site.localFilter}</code> })}</span>
      )}
    </>
  );
  if (!named) return condition;
  return (
    <div className="dialog-site">
      <SiteName source={site.source} />
      {condition}
    </div>
  );
}

export function Discover({ active, onNavigate }: { active: boolean; onNavigate: Navigate }) {
  const { addPosts, addQuery } = useDownloads();
  const [sources, setSources] = useState<Source[]>(DEFAULT_CRITERIA.sources);
  const [platforms, setPlatforms] = useState<Source[]>(DEFAULT_CRITERIA.platforms);
  const [tags, setTags] = useState(DEFAULT_CRITERIA.tags);
  const [ratings, setRatings] = useState<Rating[]>(DEFAULT_CRITERIA.ratings);
  const [sort, setSort] = useState<RemoteSort>(DEFAULT_CRITERIA.sort);
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
  const [saved, setSaved] = useState<SavedSearch[]>([]);
  const committed = useRef<Criteria>(DEFAULT_CRITERIA);
  const requestId = useRef(0);
  const filterTimer = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);

  const posts = results?.posts ?? [];
  const { picked, pickedPosts, toggle: togglePick, clear: clearPicks, pickAll } = usePicker(posts);

  /** `cursor` 为 null 表示重新搜第一页。 */
  const run = useCallback(async (criteria: Criteria, cursor: string | null) => {
    const id = ++requestId.current;
    const first = cursor === null;
    // 重新搜第一页时，还没发出的分级、平台改动已经包含在这次的条件里。
    if (first) window.clearTimeout(filterTimer.current);
    committed.current = criteria;
    setLoading(true);
    setError(null);
    try {
      const page = await searchPage(criteria, cursor);
      if (id !== requestId.current) return;
      const next = page.results;
      setResults((prev) => {
        if (first || !prev) return { ...next, posts: appendNew([], next.posts) };
        return { ...next, posts: appendNew(prev.posts, next.posts), sites: mergeSites(prev.sites, next.sites) };
      });
      setOwned((prev) => {
        const ownedNow = new Set(prev);
        page.owned.forEach((key) => ownedNow.add(key));
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
    return () => window.clearTimeout(filterTimer.current);
  }, [run]);

  useEffect(() => {
    savedSearchesList().then(setSaved, () => {});
  }, []);

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

  // 聚合搜索时有站点这一页没加载出来（网络问题），先不自动往下翻，免得一直重试，等用户点「重试」。
  const paused = results?.sites.some((site) => site.error && site.retry) ?? false;

  const loadMore = useCallback(() => {
    if (!results?.next || loading || error || paused) return;
    void run(committed.current, results.next);
  }, [results, loading, error, paused, run]);

  const retrySites = () => {
    if (results?.next) void run(committed.current, results.next);
  };

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

  /** 搜索框和筛选行里现在的条件（输入框里的字还没提交也算）。 */
  const formCriteria = (): Criteria => ({ sources, platforms, tags, ratings, sort });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void run(formCriteria(), null);
  };

  const runLater = (criteria: Criteria) => {
    window.clearTimeout(filterTimer.current);
    filterTimer.current = window.setTimeout(() => void run(criteria, null), FILTER_DEBOUNCE_MS);
  };

  const changeRatings = (next: Rating[]) => {
    setRatings(next);
    runLater({ ...formCriteria(), ratings: next });
  };

  const changePlatforms = (next: Source[]) => {
    setPlatforms(next);
    runLater({ ...formCriteria(), platforms: next });
  };

  const changeSort = (next: RemoteSort) => {
    setSort(next);
    void run({ ...formCriteria(), sort: next }, null);
  };

  // 换来源时平台筛选回到全部；新选的站点不支持当前排序就回到默认顺序（几个站点一起搜时只能用都支持的排序）。
  const changeSources = (next: Source[]) => {
    const nextSort = remoteSorts(next).some((option) => option.value === sort) ? sort : "newest";
    setSources(next);
    setPlatforms(next);
    setSort(nextSort);
    runLater({ ...formCriteria(), sources: next, platforms: next, sort: nextSort });
  };

  const selectedPost = posts.find((post) => postKey(post) === selected) ?? null;
  /** 来源勾选了两个以上站点（按搜索框里现在的勾选，不等搜索结果）。 */
  const combined = sources.length > 1;
  const firstLoad = loading && !results;
  /** 当前结果对应的收藏（按已经搜过的条件算，不看输入框里还没提交的字）。 */
  const currentSaved =
    saved.find((item) => sameSearch(item, { ...committed.current, sources: searchSources(committed.current) })) ?? null;
  const localFilter = results?.sites.find((site) => site.localFilter)?.localFilter ?? "";

  const toggleSaved = async () => {
    try {
      if (currentSaved) {
        setSaved(await savedSearchRemove(currentSaved.id));
        setToast({ message: t("已取消收藏「{title}」", { title: savedTitle(currentSaved) }) });
      } else {
        const criteria = committed.current;
        setSaved(
          await savedSearchAdd({
            sources: searchSources(criteria),
            tags: criteria.tags,
            ratings: criteria.ratings,
            sort: criteria.sort,
          }),
        );
        setToast({ message: t("已收藏「{title}」", { title: criteria.tags.trim() || t("全部帖子") }) });
      }
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
  };

  const applySaved = (item: SavedSearch) => {
    const criteria: Criteria = {
      sources: item.sources,
      platforms: item.sources,
      tags: item.tags,
      ratings: item.ratings.length ? item.ratings : RATINGS,
      sort: item.sort,
    };
    setSources(criteria.sources);
    setPlatforms(criteria.platforms);
    setTags(criteria.tags);
    setRatings(criteria.ratings);
    setSort(criteria.sort);
    void run(criteria, null);
  };

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
    if (await enqueue(pickedPosts, t("已加入下载队列：{n} 张", { n: formatCount(pickedPosts.length) }))) clearPicks();
  };

  /** 能接着搜的站点：出错后不再往下翻的（例如没填账号）不下载、不订阅。 */
  const healthySites = () => (results?.sites ?? []).filter((site) => !site.error || site.retry);

  const openBulk = () => {
    const criteria = committed.current;
    const sites = healthySites();
    setBulk({
      criteria,
      sites: sites.map((site) => ({ source: site.source, query: site.query, localFilter: site.localFilter, count: "loading" })),
      max: "",
    });
    sites.forEach(({ source }) => {
      const settle = (count: Count) =>
        setBulk((current) =>
          current && current.criteria === criteria
            ? { ...current, sites: current.sites.map((site) => (site.source === source ? { ...site, count } : site)) }
            : current,
        );
      countRemote(siteParams(criteria, source)).then(settle, () => settle("failed"));
    });
  };

  const confirmBulk = async () => {
    if (!bulk) return;
    const max = Number.parseInt(bulk.max, 10);
    const limit = Number.isFinite(max) && max > 0 ? max : null;
    setBulk(null);
    try {
      for (const site of bulk.sites) await addQuery(siteParams(bulk.criteria, site.source), limit);
      const title =
        isCombined(bulk.criteria)
          ? `${bulk.criteria.tags.trim() || t("全部帖子")} · ${sitesLabel(bulk.sites.map((site) => site.source))}`
          : bulk.sites[0]?.query || t("全部帖子");
      setToast({ message: t("已加入下载队列：{query}", { query: title }), link: "downloads" });
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
  };

  // 订阅不带排序，发给站点的条件以 Rust 端算出的为准。
  const openSubscribe = async () => {
    const criteria = committed.current;
    const sources = healthySites().map((site) => site.source);
    try {
      const previews = await Promise.all(sources.map((source) => subscriptionPreview(siteParams(criteria, source))));
      setSubscribing({
        criteria,
        sites: sources.map((source, index) => ({ source, ...previews[index] })),
        interval: 360,
        existing: false,
        max: "",
        busy: false,
        error: null,
      });
    } catch (err) {
      setToast({ message: errorMessage(err) });
    }
  };

  const confirmSubscribe = async () => {
    if (!subscribing) return;
    const draft = subscribing;
    setSubscribing({ ...draft, busy: true, error: null });
    const max = Number.parseInt(draft.max, 10);
    const limit = draft.existing && Number.isFinite(max) && max > 0 ? max : null;
    const done: Source[] = [];
    try {
      let title = "";
      for (const site of draft.sites) {
        const sub = await subscriptionCreate(siteParams(draft.criteria, site.source), draft.interval, draft.existing, limit);
        done.push(site.source);
        title = subscriptionTitle(sub);
      }
      setSubscribing(null);
      const label = isCombined(draft.criteria) ? `${title} · ${sitesLabel(done)}` : title;
      setToast({ message: t("已订阅「{title}」", { title: label }), link: "subscriptions" });
    } catch (err) {
      // 已经订阅好的平台留着，再点「订阅」时只订剩下的。
      setSubscribing(
        (current) =>
          current && {
            ...current,
            sites: current.sites.filter((site) => !done.includes(site.source)),
            busy: false,
            error: sentences(done.length > 0 && t("{sites} 已订阅。", { sites: sitesLabel(done) }), errorMessage(err)),
          },
      );
    }
  };

  const selectedKey = selectedPost ? postKey(selectedPost) : null;

  // 列表快捷键：←/→ 上一张、下一张，空格勾选，⌘/Ctrl + A 全选，Esc 取消勾选，⌘/Ctrl + D 下载。
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
  ) : null;

  return (
    <div className="discover">
      <div className="center" data-picking={picked.size > 0 || undefined}>
        <div className="topbar" data-tauri-drag-region>
          <form className="search" onSubmit={submit} role="search">
            <MultiSelect
              id="search-source"
              className="search-source"
              name={t("来源")}
              allLabel={t("全部平台")}
              values={sources}
              options={SOURCE_OPTIONS}
              sizers={[t("全部平台"), ...SOURCE_OPTIONS.map((option) => option.label)]}
              onChange={changeSources}
            />
            <input
              id="search-tags"
              className="search-input"
              aria-label={t("搜索 tag")}
              placeholder={
                // Pixiv 还能看画师的全部作品；几个站点一起搜时 user: 对别的站点另有意思，不提示。
                sources.length === 1 && sources[0] === "pixiv"
                  ? t("输入 tag、user:画师 ID，或粘贴画师、作品链接")
                  : t("输入 tag，空格分隔，例如 scenery sky")
              }
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
            <MenuButton
              className="search-bookmark"
              name={t("收藏的搜索")}
              title={currentSaved ? t("已收藏这个搜索") : t("收藏这个搜索")}
              state={currentSaved ? "saved" : undefined}
              disabled={!results}
              items={[
                {
                  key: "toggle",
                  label: currentSaved ? t("取消收藏这个搜索") : t("收藏这个搜索"),
                  selected: false,
                  divider: saved.length > 0,
                },
                ...saved.map((item) => ({
                  key: String(item.id),
                  label: savedTitle(item),
                  hint: savedHint(item),
                  selected: item.id === currentSaved?.id,
                })),
              ]}
              onPick={(key) => {
                if (key === "toggle") void toggleSaved();
                else {
                  const item = saved.find((entry) => String(entry.id) === key);
                  if (item) applySaved(item);
                }
              }}
            >
              <Icon name="bookmark" size={17} />
            </MenuButton>
            <button type="submit" className="search-go" aria-label={t("搜索")}>
              <Icon name="search" size={17} />
            </button>
          </form>
        </div>
        <div className="filters">
          <MultiSelect
            className="filter-select"
            name={t("平台")}
            label={t("平台")}
            allLabel={t("全部")}
            values={combined ? platforms : sources}
            options={combined ? SOURCE_OPTIONS.filter((option) => sources.includes(option.value)) : SOURCE_OPTIONS}
            onChange={changePlatforms}
            disabled={!combined}
            title={combined ? undefined : t("来源勾选两个以上平台（聚合搜索）时才能按平台筛选")}
          />
          <MultiSelect
            className="filter-select"
            name={t("分级")}
            label={t("分级")}
            allLabel={t("全部")}
            values={ratings}
            options={ratingOptions()}
            onChange={changeRatings}
          />
          <Select
            className="filter-select"
            name={t("排序")}
            label={t("排序")}
            value={sort}
            options={remoteSorts(sources)}
            onChange={changeSort}
          />
          {results && (
            <span className="query" title={queryTitle(results.sites)}>
              {queryText(results.sites)}
            </span>
          )}
          <div className="filters-end">
            {localFilter && (
              <span
                className="local-filter"
                title={t("站点一次能搜的 tag 数有限，「{filter}」在本地逐页筛选，加载会慢一些。Gold 以上等级的账号能直接搜更多 tag。", {
                  filter: localFilter,
                })}
              >
                {t("本地筛选 {filter}", { filter: localFilter })}
              </span>
            )}
            <span className="count">{t("{n} 张", { n: formatCount(posts.length) })}</span>
            <button
              type="button"
              className="btn sm collapsible"
              title={collapsedTitle(t("订阅"))}
              onClick={() => void openSubscribe()}
              disabled={!results || loading}
            >
              <Icon name="bell" size={14} />
              <span className="btn-text">{t("订阅")}</span>
            </button>
            <button
              type="button"
              className="btn sm collapsible"
              title={collapsedTitle(t("下载全部结果"))}
              onClick={openBulk}
              disabled={posts.length === 0 || loading}
            >
              <Icon name="download" size={14} />
              <span className="btn-text">{t("下载全部结果")}</span>
            </button>
          </div>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              {isAccountError(error.code) ? (
                <button type="button" className="btn" onClick={() => onNavigate("settings", "accounts")}>
                  <Icon name="user" size={15} />
                  {/* 几个站点全都出错时报的是排在最前的那个站点的错。 */}
                  {accountAction(SOURCES.find((source) => searchSources(committed.current).includes(source)))}
                </button>
              ) : (
                <button type="button" className="btn" onClick={() => void run(committed.current, error.cursor)}>
                  <Icon name="retry" size={15} />
                  {t("重试")}
                </button>
              )}
            </div>
          )}
          {results?.sites
            .filter((site) => site.error)
            .map((site) => (
              <div key={site.source} className="alert" role="alert">
                <span>{siteMessage(site)}</span>
                {isAccountError(site.error?.code) ? (
                  <button type="button" className="btn" onClick={() => onNavigate("settings", "accounts")}>
                    <Icon name="user" size={15} />
                    {accountAction(site.source)}
                  </button>
                ) : (
                  site.retry && (
                    <button type="button" className="btn" onClick={retrySites} disabled={loading}>
                      <Icon name="retry" size={15} />
                      {t("重试")}
                    </button>
                  )
                )}
              </div>
            ))}
          {firstLoad && <p className="hint">{t("正在加载…")}</p>}
          {results && posts.length === 0 && !results.next && !loading && !error && (
            <p className="hint">
              {committed.current.sort === "popular"
                ? t("没有找到符合条件的图片。「近期热门」只包含最近两天上传的图，可以换个排序再试。")
                : t("没有找到符合条件的图片。可以减少 tag 或放宽分级再试。")}
            </p>
          )}
          <PostGrid
            posts={posts}
            selected={selectedKey}
            onSelect={selectCard}
            pageSize={PAGE_SIZE}
            owned={owned}
            picked={picked}
            onPick={togglePick}
            showSource={results?.combined}
          />
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {results?.next && !error && !paused && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading}>
              {loading ? t("正在加载…") : t("加载更多")}
            </button>
          )}
        </div>

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
                  const target = toast.link;
                  setToast(null);
                  if (target) onNavigate(target);
                }}
              >
                {t("查看")}
              </button>
            )
          }
        />
      </div>

      <Inspector key={selectedKey ?? "none"} post={selectedPost} primaryAction={primaryAction} />

      <Dialog
        open={bulk !== null}
        title={t("下载全部搜索结果？")}
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
              <dt>{t("条件")}</dt>
              <dd>
                {bulk.sites.map((site) => (
                  <SiteCondition key={site.source} site={site} named={isCombined(bulk.criteria)} />
                ))}
              </dd>
              <dt>{t("数量")}</dt>
              <dd>
                {isCombined(bulk.criteria)
                  ? bulk.sites.map((site) => (
                      <div key={site.source} className="dialog-site">
                        <SiteName source={site.source} />
                        {countText(site.count)}
                      </div>
                    ))
                  : countText(bulk.sites[0]?.count ?? null)}
              </dd>
              <dt>
                <label htmlFor="bulk-max">{t("上限")}</label>
              </dt>
              <dd className="dialog-field">
                <input
                  id="bulk-max"
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
                <span>
                  {bulk.criteria.sort === "newest"
                    ? t("张，留空表示全部下载")
                    : t("张，按「{sort}」取排在前面的，留空表示全部下载", { sort: remoteSortLabel(bulk.criteria.sort) })}
                </span>
              </dd>
            </dl>
            <p className="dialog-note">
              {sentences(
                bulk.sites.length > 1 && t("每个平台各建一个下载任务，上限对每个平台分别计算。"),
                t("已在图库里的图会自动跳过。下载在后台进行，可以随时在「下载」里暂停或取消。"),
              )}
            </p>
          </>
        )}
      </Dialog>

      <Dialog
        open={subscribing !== null}
        title={t("订阅这个搜索条件？")}
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
              {subscribing?.busy ? t("正在订阅…") : t("订阅")}
            </button>
            <button type="button" className="btn ghost" onClick={() => setSubscribing(null)}>
              {t("取消")}
            </button>
          </>
        }
      >
        {subscribing && (
          <>
            <dl className="dialog-paths">
              <dt>{t("条件")}</dt>
              <dd>
                {subscribing.sites.map((site) => (
                  <SiteCondition key={site.source} site={site} named={isCombined(subscribing.criteria)} />
                ))}
              </dd>
              <dt>
                <label htmlFor="subscribe-interval">{t("检查")}</label>
              </dt>
              <dd className="dialog-field">
                <Select
                  id="subscribe-interval"
                  className="select"
                  name={t("检查间隔")}
                  value={subscribing.interval}
                  options={intervalOptions()}
                  onChange={(interval) => setSubscribing((current) => current && { ...current, interval })}
                />
              </dd>
              <dt>{t("已有的图")}</dt>
              <dd className="dialog-choices">
                <label className="choice">
                  <input
                    type="radio"
                    name="subscribe-existing"
                    checked={!subscribing.existing}
                    onChange={() => setSubscribing((current) => current && { ...current, existing: false })}
                  />
                  {t("不下载，只下载以后的新图")}
                </label>
                <label className="choice">
                  <input
                    type="radio"
                    name="subscribe-existing"
                    checked={subscribing.existing}
                    onChange={() => setSubscribing((current) => current && { ...current, existing: true })}
                  />
                  {tx("现在也下载，最多{input}张", {
                    input: (
                      <input
                        className="field-input"
                        type="number"
                        min={1}
                        step={1}
                        inputMode="numeric"
                        placeholder={t("不限")}
                        aria-label={t("最多下载多少张已有的图")}
                        value={subscribing.max}
                        onFocus={() => setSubscribing((current) => current && { ...current, existing: true })}
                        onChange={(event) => {
                          const max = event.target.value;
                          setSubscribing((current) => current && { ...current, max, existing: true });
                        }}
                      />
                    ),
                  })}
                </label>
              </dd>
            </dl>
            {subscribing.error && <p className="form-error">{subscribing.error}</p>}
            <p className="dialog-note">
              {sentences(
                t("以后按设定的间隔检查，有新图就自动下载。"),
                subscribing.sites.length > 1 && t("每个平台各建一个订阅。"),
                subscribing.criteria.sort !== "newest" &&
                  t("订阅按上传先后找新图，不使用「{sort}」排序。", { sort: remoteSortLabel(subscribing.criteria.sort) }),
                t("关掉窗口后会在后台继续，可以在「设置 → 通用」里修改。"),
              )}
            </p>
          </>
        )}
      </Dialog>

    </div>
  );
}
