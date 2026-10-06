import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { Dialog } from "../../components/Dialog";
import { ImageViewer } from "../../components/ImageViewer";
import { Icon } from "../../components/Icon";
import { LoadingPill } from "../../components/LoadingPill";
import { PostGrid, visibleCard } from "../../components/PostGrid";
import { MultiSelect, Select } from "../../components/Select";
import { SelectionDock } from "../../components/SelectionDock";
import { appendTag, cleanTag, TagInput } from "../../components/TagInput";
import { Toast, useToast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import { hasMod, spaceForButton, useHotkeys } from "../../lib/hotkeys";
import { collapsedTitle, t } from "../../lib/i18n";
import { errorMessage, isFanboxFile, postKey, postNumber, ratingOptions, RATINGS, type Rating, type Source } from "../../lib/ipc";
import {
  libraryDelete,
  libraryList,
  libraryOpenFile,
  librarySorts,
  type LibrarySort,
  type LocalPost,
  type PostRef,
} from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { revealLabel, trashLabel } from "../../lib/platform";
import { Inspector } from "../discover/Inspector";
import { useDownloads } from "../downloads/context";

interface Filter {
  tags: string;
  ratings: Rating[];
  sort: LibrarySort;
}

/** 从文件夹点进来看的范围：全部图片、一个来源，或者来源里的一组（同一个画师、作品等）。 */
export interface GridScope {
  source: Source | null;
  /** 这一组对应的 tag；看整个文件夹时为 null。 */
  tag: string | null;
  /** 返回按钮上显示的名字。 */
  title: string;
}

interface Listing {
  posts: LocalPost[];
  total: number;
  hasMore: boolean;
}

const PAGE_SIZE = 60;
const EMPTY_FILTER: Filter = { tags: "", ratings: [], sort: "downloaded" };

const isFiltered = (filter: Filter) =>
  filter.tags.trim() !== "" || (filter.ratings.length > 0 && filter.ratings.length < RATINGS.length);

/** 文件夹里的图：瀑布流、详情、多选删除。按 tag 筛选时和所在的分组一起算。 */
export function LibraryGrid({
  active,
  onNavigate,
  scope,
  onBack,
}: {
  active: boolean;
  onNavigate: Navigate;
  scope: GridScope;
  onBack: () => void;
}) {
  const [tags, setTags] = useState("");
  /** 刚从详情里点进筛选框的 tag，胶囊闪一下。 */
  const [flash, setFlash] = useState<{ tag: string; at: number } | null>(null);
  const [ratings, setRatings] = useState<Rating[]>([]);
  const [sort, setSort] = useState<LibrarySort>(EMPTY_FILTER.sort);
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
  const [viewerPost, setViewerPost] = useState<LocalPost | null>(null);
  const [notice, setNotice] = useToast();
  const { addPosts } = useDownloads();
  const committed = useRef<Filter>(EMPTY_FILTER);
  const requestId = useRef(0);
  const refreshTimer = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);
  const center = useRef<HTMLDivElement>(null);

  const load = useCallback(async (filter: Filter, offset: number) => {
    const id = ++requestId.current;
    committed.current = filter;
    setLoading(true);
    setError(null);
    try {
      const page = await libraryList({
        source: scope.source,
        tags: [scope.tag, filter.tags.trim()].filter(Boolean).join(" "),
        ratings: filter.ratings,
        sort: filter.sort,
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
  }, [scope.source, scope.tag]);

  useEffect(() => {
    void load(EMPTY_FILTER, 0);
    return () => window.clearTimeout(refreshTimer.current);
  }, [load]);

  useTauriEvent<SavedPayload>(EVENTS.librarySaved, () => setFresh((count) => count + 1));

  const posts = listing?.posts ?? [];
  const fileUnits = scope.source === "fanbox" || scope.source === null;
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
    void load({ tags, ratings, sort }, 0);
  };

  const changeRatings = (next: Rating[]) => {
    setRatings(next);
    void load({ tags, ratings: next, sort }, 0);
  };

  const changeSort = (next: LibrarySort) => {
    setSort(next);
    void load({ tags, ratings, sort: next }, 0);
  };

  const reveal = async (post: LocalPost) => {
    setActionError(null);
    try {
      await revealItemInDir(post.path);
    } catch (err) {
      setActionError(t("{action}失败：{error}", { action: revealLabel(), error: errorMessage(err) }));
    }
  };

  const openFile = async (post: LocalPost) => {
    setActionError(null);
    try {
      await libraryOpenFile({ source: post.source, postId: post.id });
    } catch (err) {
      setActionError(t("{action}失败：{error}", { action: t("打开文件"), error: errorMessage(err) }));
    }
  };

  // 文件不见了：按原来的帖子重新下载，存好后图库自动刷新。
  const redownload = async (post: LocalPost) => {
    setActionError(null);
    try {
      await addPosts([post]);
      setRequeued((prev) => new Set(prev).add(postKey(post)));
      setNotice(t("已加入下载队列：#{id}", { id: postNumber(post) }));
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
        const n = formatCount(outcome.removed.length);
        setNotice(keepFiles ? t(fileUnits ? "已从图库移除 {n} 项" : "已从图库移除 {n} 张", { n }) : t(fileUnits ? "已删除 {n} 项" : "已删除 {n} 张", { n }));
      }
      if (outcome.failed.length > 0) {
        setActionError(
          t(fileUnits ? "有 {n} 项没能移到{trash}：{error}" : "有 {n} 张没能移到{trash}：{error}", {
            n: formatCount(outcome.failed.length),
            trash: trashLabel(),
            error: outcome.failed[0].message,
          }),
        );
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

  // 列表快捷键：←/→ 上一张、下一张，空格勾选，⌘/Ctrl + A 全选，Esc 取消勾选，Delete 删除。
  useHotkeys(active, (event) => {
    if ((event.key === "ArrowRight" || event.key === "ArrowLeft") && !hasMod(event)) {
      const index = posts.findIndex((post) => postKey(post) === selected);
      const next = posts[Math.min(posts.length - 1, Math.max(0, index + (event.key === "ArrowRight" ? 1 : -1)))];
      if (next) setSelected(postKey(next));
      return true;
    }
    if (event.key === " " && !spaceForButton(event) && selectedPost) {
      togglePick(selectedPost, event);
      return true;
    }
    if (hasMod(event) && event.key.toLowerCase() === "a") {
      pickAll();
      return true;
    }
    if (event.key === "Escape" && picked.size > 0) {
      clearPicks();
      return true;
    }
    if ((event.key === "Delete" || event.key === "Backspace") && !busy) {
      const list = picked.size > 0 ? pickedPosts : selectedPost ? [selectedPost] : [];
      if (list.length > 0) setDeleting(list);
      return true;
    }
    return false;
  });
  const missing = new Set(posts.filter((post) => post.missing).map(postKey));

  return (
    <div className="discover">
      <div ref={center} className="center" data-picking={picked.size > 0 || undefined}>
        <div className="topbar" data-tauri-drag-region>
          <form className="search" onSubmit={submit} role="search">
            <button type="button" className="search-back" onClick={onBack} title={t("返回上一层")}>
              <Icon name="back" size={15} />
              <span>{scope.title}</span>
            </button>
            <TagInput
              label={t("按 tag 筛选")}
              placeholder={t("在这里按 tag 筛选，空格分隔，-tag 表示排除")}
              value={tags}
              onChange={setTags}
              flash={flash}
            />
            <button type="submit" className="search-go" aria-label={t("筛选")}>
              <Icon name="search" size={17} />
            </button>
          </form>
        </div>
        <div className="filters">
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
            options={librarySorts()}
            onChange={changeSort}
          />
          <div className="filters-end">
            {fresh > 0 && deep && (
              <button
                type="button"
                className="btn sm collapsible"
                title={collapsedTitle(t(fileUnits ? "有 {n} 项新下载的文件" : "有 {n} 张新下载的图", { n: formatCount(fresh) }))}
                onClick={() => void load(committed.current, 0)}
              >
                <Icon name="retry" size={14} />
                <span className="btn-text">{t(fileUnits ? "有 {n} 项新下载的文件" : "有 {n} 张新下载的图", { n: formatCount(fresh) })}</span>
              </button>
            )}
            <span className="count">{t(fileUnits ? "共 {n} 项" : "共 {n} 张", { n: formatCount(listing?.total ?? 0) })}</span>
          </div>
        </div>
        <div className="scroll">
          {error && (
            <div className="alert" role="alert">
              <span>{error.message}</span>
              <button type="button" className="btn" onClick={() => void load(committed.current, error.offset)}>
                <Icon name="retry" size={15} />
                {t("重试")}
              </button>
            </div>
          )}
          {actionError && (
            <div className="alert" role="alert">
              <span>{actionError}</span>
              <button type="button" className="btn" onClick={() => setActionError(null)}>
                {t("关闭")}
              </button>
            </div>
          )}
          {listing && posts.length === 0 && !loading && !error && (
            filtered ? (
              <p className="hint">{t(fileUnits ? "没有符合条件的文件。可以减少 tag 或放宽分级再试。" : "没有符合条件的图片。可以减少 tag 或放宽分级再试。")}</p>
            ) : (
              <div className="empty">
                <p className="empty-title">{t(fileUnits ? "图库里还没有文件" : "图库里还没有图片")}</p>
                <p>{t(fileUnits ? "在「发现」里下载的图片和附件会出现在这里。" : "在「发现」里下载的图片会出现在这里。")}</p>
                <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
                  <Icon name="compass" size={15} />
                  {t("去发现")}
                </button>
              </div>
            )
          )}
          <PostGrid
            posts={posts}
            selected={selected}
            onSelect={selectCard}
            onView={(post) => {
              if (isFanboxFile(post)) return;
              setSelected(postKey(post));
              setViewerPost(post);
            }}
            pageSize={PAGE_SIZE}
            missing={missing}
            picked={picked}
            onPick={togglePick}
            showSource={scope.source === null}
          />
          <div ref={sentinel} className="sentinel" aria-hidden="true" />
          {listing?.hasMore && !error && (
            <button type="button" className="btn more" onClick={loadMore} disabled={loading} data-busy={loading || undefined}>
              {t("加载更多")}
            </button>
          )}
        </div>

        <LoadingPill loading={loading} />

        <SelectionDock unit={fileUnits ? "items" : "images"} count={picked.size} total={posts.length} onPickAll={pickAll} onClear={clearPicks}>
          <button type="button" className="btn danger" onClick={() => setDeleting(pickedPosts)} disabled={busy}>
            <Icon name="trash" size={15} />
            {t("删除")}
          </button>
        </SelectionDock>
        <Toast message={notice} />
      </div>
      <Inspector
        key={selectedPost ? postKey(selectedPost) : "none"}
        post={selectedPost}
        onTag={(tag) => {
          setTags((prev) => appendTag(prev, tag));
          setFlash({ tag: cleanTag(tag), at: Date.now() });
        }}
        localPath={selectedPost?.path}
        notice={selectedPost?.missing ? t(isFanboxFile(selectedPost) ? "文件不在记录的位置，可能已被移动或删除。可以重新下载。" : "文件不在记录的位置，可能已被移动或删除。可以重新下载到图片位置。") : undefined}
        primaryAction={
          selectedPost && (
            <>
              {!selectedPost.missing ? (<>
                {isFanboxFile(selectedPost) && <button type="button" className="btn primary" onClick={() => void openFile(selectedPost)}>
                  <Icon name="file" size={15} />
                  {t("打开文件")}
                </button>}
                <button type="button" className={`btn${isFanboxFile(selectedPost) ? " icon-only" : " primary"}`} title={revealLabel()} aria-label={revealLabel()} onClick={() => void reveal(selectedPost)}>
                  <Icon name="folder" size={15} />
                  {!isFanboxFile(selectedPost) && revealLabel()}
                </button></>
              ) : requeued.has(postKey(selectedPost)) ? (
                <button type="button" className="btn" disabled>
                  <Icon name="check" size={15} />
                  {t("已加入下载队列")}
                </button>
              ) : (
                <button type="button" className="btn primary" onClick={() => void redownload(selectedPost)}>
                  <Icon name="download" size={15} />
                  {t("重新下载")}
                </button>
              )}
              <button
                type="button"
                className="btn danger icon-only"
                aria-label={t(isFanboxFile(selectedPost) ? "删除这个文件" : "删除这张图")}
                title={t(isFanboxFile(selectedPost) ? "删除这个文件" : "删除这张图")}
                onClick={() => setDeleting([selectedPost])}
                disabled={busy}
              >
                <Icon name="trash" size={15} />
              </button>
            </>
          )
        }
      />

      <ImageViewer
        post={viewerPost}
        posts={posts}
        local
        onClose={() => setViewerPost(null)}
        cardOf={(post) => visibleCard(center.current, post)}
        onChange={(post) => {
          setSelected(postKey(post));
          setViewerPost(post as LocalPost);
        }}
      />

      <Dialog
        open={deleting !== null}
        title={
          deleting && deleting.length > 1
            ? t(deleting.some(isFanboxFile) ? "删除选中的 {n} 个文件？" : "删除选中的 {n} 张图？", { n: formatCount(deleting.length) })
            : t(deleting?.some(isFanboxFile) ? "删除这个文件？" : "删除这张图？")
        }
        onClose={() => setDeleting(null)}
        initialFocus="last"
        actions={
          <>
            <button type="button" className="btn danger" onClick={() => void confirmDelete(false)}>
              <Icon name="trash" size={15} />
              {t("移到{trash}", { trash: trashLabel() })}
            </button>
            <button type="button" className="btn" onClick={() => void confirmDelete(true)}>
              {t("只从图库移除")}
            </button>
            <button type="button" className="btn ghost" onClick={() => setDeleting(null)}>
              {t("取消")}
            </button>
          </>
        }
      >
        <p className="dialog-copy">{t("仅从图库移除时，保留本地文件。")}</p>
      </Dialog>
    </div>
  );
}
