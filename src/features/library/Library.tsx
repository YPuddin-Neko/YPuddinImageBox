import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { FanStack } from "../../components/FanStack";
import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { Select } from "../../components/Select";
import { EVENTS } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import { formatCount, formatTime } from "../../lib/format";
import { t } from "../../lib/i18n";
import { errorMessage, SOURCE_LABEL, type Source } from "../../lib/ipc";
import {
  groupKinds,
  groupSorts,
  groupTotal,
  libraryFolders,
  libraryImport,
  libraryGroups,
  type Folder,
  type Group,
  type GroupKind,
  type GroupSort,
} from "../../lib/library";
import type { Navigate } from "../../lib/nav";
import { LibraryGrid, type GridScope } from "./LibraryGrid";

/** 图库的三层：来源文件夹 → 文件夹里的分组 → 一组里的图。 */
type Place =
  | { level: "folders" }
  | { level: "groups"; source: Source }
  | { level: "grid"; scope: GridScope; parent: Place };

/** 文件夹卡片展开 5 张，分组卡片 4 张。 */
const FOLDER_COVERS = 5;
const GROUP_COVERS = 4;
const GROUP_PAGE = 60;
/** 下载进行中每存一张图都会通知，攒一会儿再刷新封面。 */
const REFRESH_DELAY_MS = 1500;

/** 有新下载或删除时，过一会儿重新读取。 */
function useLibraryChanges(refresh: () => void) {
  const timer = useRef(0);
  const latest = useRef(refresh);
  latest.current = refresh;
  const schedule = useCallback(() => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => latest.current(), REFRESH_DELAY_MS);
  }, []);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useTauriEvent(EVENTS.librarySaved, schedule);
  useTauriEvent(EVENTS.libraryRemoved, schedule);
}

export function Library({ active, onNavigate }: { active: boolean; onNavigate: Navigate }) {
  const [place, setPlace] = useState<Place>({ level: "folders" });
  // 分组方式按文件夹记住，排序各文件夹共用。
  const [kinds, setKinds] = useState<Record<Source, GroupKind>>({
    danbooru: "artist",
    gelbooru: "general",
    yandere: "general",
    pixiv: "artist",
    x: "artist",
    custom: "artist",
  });
  const [sort, setSort] = useState<GroupSort>("recent");

  if (place.level === "grid") {
    const { scope, parent } = place;
    return (
      <LibraryGrid
        key={`${scope.source ?? "all"}:${scope.tag ?? ""}`}
        active={active}
        onNavigate={onNavigate}
        scope={scope}
        onBack={() => setPlace(parent)}
      />
    );
  }

  const openGrid = (scope: GridScope) => setPlace({ level: "grid", scope, parent: place });
  return place.level === "folders" ? (
    <FolderShelf
      onOpen={(source) => setPlace({ level: "groups", source })}
      onAll={() => openGrid({ source: null, tag: null, title: t("全部图片") })}
      onNavigate={onNavigate}
    />
  ) : (
    <GroupShelf
      key={place.source}
      source={place.source}
      kind={kinds[place.source]}
      sort={sort}
      onKind={(kind) => setKinds((prev) => ({ ...prev, [place.source]: kind }))}
      onSort={setSort}
      onBack={() => setPlace({ level: "folders" })}
      onOpen={openGrid}
      onNavigate={onNavigate}
    />
  );
}

function EmptyLibrary({ onNavigate, title }: { onNavigate: Navigate; title: string }) {
  return (
    <div className="empty page-block">
      <p className="empty-title">{title}</p>
      <p>{t("在「发现」里下载的图片会出现在这里。")}</p>
      <button type="button" className="btn primary" onClick={() => onNavigate("discover")}>
        <Icon name="compass" size={15} />
        {t("去发现")}
      </button>
    </div>
  );
}

/** 首页：各来源文件夹和自定义导入占位，封面是各自最近下载的几张。 */
function FolderShelf({
  onOpen,
  onAll,
  onNavigate,
}: {
  onOpen: (source: Source) => void;
  onAll: () => void;
  onNavigate: Navigate;
}) {
  const [folders, setFolders] = useState<Folder[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();
  const [pendingImport, setPendingImport] = useState<string[] | null>(null);
  const [classifyImport, setClassifyImport] = useState(true);

  const load = useCallback(() => {
    libraryFolders().then(
      (next) => {
        setFolders(next);
        setError(null);
      },
      (err) => setError(errorMessage(err)),
    );
  }, []);

  useEffect(load, [load]);
  useLibraryChanges(load);

  const total = folders?.reduce((sum, folder) => sum + folder.count, 0) ?? 0;
  const importImages = async () => {
    try {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [{ name: t("图片文件"), extensions: ["jpg", "jpeg", "png", "gif", "webp"] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      setPendingImport(paths);
      return;
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const confirmImport = async () => {
    if (!pendingImport) return;
    try {
      const result = await libraryImport(pendingImport, classifyImport);
      setPendingImport(null);
      setNotice(
        result.skipped > 0
          ? t("已导入 {n} 张，跳过 {skipped} 个文件", { n: formatCount(result.imported), skipped: formatCount(result.skipped) })
          : t("已导入 {n} 张图片", { n: formatCount(result.imported) }),
      );
      load();
    } catch (err) {
      setError(errorMessage(err));
    }
  };
  return (
    <div className="page library-page">
      <header className="page-head library-page-head" data-tauri-drag-region>
        <div className="page-title">
          <h1>{t("图库")}</h1>
          <p>{folders ? t("{n} 张图，按来源分成文件夹，封面是最近下载的几张。", { n: formatCount(total) }) : " "}</p>
        </div>
        <button type="button" className="btn ghost" onClick={onAll} disabled={total === 0}>
          <Icon name="grid" size={15} />
          {t("全部图片")}
        </button>
      </header>

      {error && (
        <div className="alert page-block" role="alert">
          <span>{error}</span>
          <button type="button" className="btn" onClick={load}>
            <Icon name="retry" size={15} />
            {t("重试")}
          </button>
        </div>
      )}

      {folders && (
        <div className="shelf library-shelf" data-size="lg">
          {folders.map((folder) => {
            const content = (
              <>
                <FanStack covers={folder.covers} max={FOLDER_COVERS} />
                <span className="stack-meta">
                  <span className="stack-title">{SOURCE_LABEL[folder.source]}</span>
                  <span className="stack-count">{t("{n} 张", { n: formatCount(folder.count) })}</span>
                </span>
                <span className="stack-sub">
                  {folder.latestAt === null
                    ? folder.source === "custom" ? t("还没有导入的图片") : t("还没有下载的图片")
                    : t("最近下载 {time}", { time: formatTime(folder.latestAt) })}
                </span>
              </>
            );
            if (folder.source === "custom") {
              return (
                <div key={folder.source} className="stack-card stack-card-custom">
                  {content}
                  <div className="stack-custom-actions">
                    {folder.count > 0 && <button type="button" className="btn ghost" onClick={() => onOpen(folder.source)}>{t("查看图片")}</button>}
                    <button type="button" className="btn" onClick={() => void importImages()}>
                      <Icon name="folder" size={15} />
                      {t("导入图片")}
                    </button>
                  </div>
                </div>
              );
            }
            return (
              <button key={folder.source} type="button" className="stack-card" onClick={() => onOpen(folder.source)} disabled={folder.count === 0}>
                {content}
              </button>
            );
          })}
        </div>
      )}

      {folders && total === 0 && !error && <EmptyLibrary onNavigate={onNavigate} title={t("图库里还没有图片")} />}
      <Toast message={notice} />
      <Dialog
        open={pendingImport !== null}
        title={t("导入图片")}
        onClose={() => setPendingImport(null)}
        initialFocus="last"
        actions={
          <>
            <button type="button" className="btn ghost" onClick={() => setPendingImport(null)}>
              {t("取消")}
            </button>
            <button type="button" className="btn primary" onClick={() => void confirmImport()}>
              {t("开始导入")}
            </button>
          </>
        }
      >
        <p className="dialog-copy">{t("可以按所选图片原来的上一级文件夹创建画师或分类，也可以全部放在自定义导入根目录。")}</p>
        <div className="options" role="radiogroup" aria-label={t("导入分类方式")}>
          <label className="option" data-checked={classifyImport || undefined}>
            <input type="radio" name="import-classify" checked={classifyImport} onChange={() => setClassifyImport(true)} />
            <span className="option-title">{t("按原文件夹分类")}</span>
            <span className="option-desc">{t("上一级文件夹作为画师或分类 ID")}</span>
          </label>
          <label className="option" data-checked={!classifyImport || undefined}>
            <input type="radio" name="import-classify" checked={!classifyImport} onChange={() => setClassifyImport(false)} />
            <span className="option-title">{t("不创建分类")}</span>
            <span className="option-desc">{t("所有图片直接放在自定义导入目录")}</span>
          </label>
        </div>
      </Dialog>
    </div>
  );
}

/** 一个来源文件夹里：第一张是「全部」，后面按画师（作品、角色、tag）分组。 */
function GroupShelf({
  source,
  kind,
  sort,
  onKind,
  onSort,
  onBack,
  onOpen,
  onNavigate,
}: {
  source: Source;
  kind: GroupKind;
  sort: GroupSort;
  onKind: (kind: GroupKind) => void;
  onSort: (sort: GroupSort) => void;
  onBack: () => void;
  onOpen: (scope: GridScope) => void;
  onNavigate: Navigate;
}) {
  const [folder, setFolder] = useState<Folder | null>(null);
  const [groups, setGroups] = useState<Group[] | null>(null);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);
  const sentinel = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (offset: number) => {
      const id = ++requestId.current;
      setLoading(true);
      setError(null);
      try {
        const [folders, page] = await Promise.all([
          offset === 0 ? libraryFolders() : Promise.resolve(null),
          libraryGroups({ source, kind, sort, offset, limit: GROUP_PAGE }),
        ]);
        if (id !== requestId.current) return;
        if (folders) setFolder(folders.find((item) => item.source === source) ?? null);
        setGroups((prev) => (offset === 0 || !prev ? page.groups : [...prev, ...page.groups]));
        setTotal(page.total);
        setHasMore(page.hasMore);
      } catch (err) {
        if (id === requestId.current) setError(errorMessage(err));
      } finally {
        if (id === requestId.current) setLoading(false);
      }
    },
    [source, kind, sort],
  );

  useEffect(() => {
    void load(0);
  }, [load]);
  useLibraryChanges(() => void load(0));

  const count = groups?.length ?? 0;
  useEffect(() => {
    const target = sentinel.current;
    if (!target || !hasMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting) && !loading) void load(count);
      },
      { rootMargin: "600px 0px" },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [hasMore, loading, count, load]);

  const name = SOURCE_LABEL[source];
  const kindOptions = groupKinds(source);
  return (
    <div className="page">
      <header className="page-head" data-tauri-drag-region>
        <div className="page-title">
          <h1 className="crumbs">
            <button type="button" className="crumb" onClick={onBack}>
              {t("图库")}
            </button>
            <span className="crumb-sep" aria-hidden="true">
              /
            </span>
            <span>{name}</span>
          </h1>
          <p>
            {folder
              ? `${t("{n} 张", { n: formatCount(folder.count) })} · ${groupTotal(kind, total)}`
              : " "}
          </p>
        </div>
        <div className="page-actions">
          {kindOptions.length > 1 && (
            <Select
              className="select"
              name={t("分组")}
              label={t("分组")}
              value={kind}
              options={kindOptions}
              onChange={onKind}
            />
          )}
          <Select
            className="select"
            name={t("排序")}
            label={t("排序")}
            value={sort}
            options={groupSorts()}
            onChange={onSort}
          />
        </div>
      </header>

      {error && (
        <div className="alert page-block" role="alert">
          <span>{error}</span>
          <button type="button" className="btn" onClick={() => void load(0)}>
            <Icon name="retry" size={15} />
            {t("重试")}
          </button>
        </div>
      )}

      {folder && groups && (
        <div className="shelf page-block" data-size="md" data-loading={loading || undefined}>
          <button type="button" className="stack-card" onClick={() => onOpen({ source, tag: null, title: name })}>
            <FanStack covers={folder.covers} max={GROUP_COVERS} />
            <span className="stack-meta">
              <span className="stack-title">{t("全部")}</span>
              <span className="stack-count">{formatCount(folder.count)}</span>
            </span>
          </button>
          {groups.map((group) => (
            <button
              key={group.name}
              type="button"
              className="stack-card"
              title={group.name}
              onClick={() => onOpen({ source, tag: group.name, title: group.name })}
            >
              <FanStack covers={group.covers} max={GROUP_COVERS} />
              <span className="stack-meta">
                <span className="stack-title">{group.name}</span>
                <span className="stack-count">{formatCount(group.count)}</span>
              </span>
            </button>
          ))}
        </div>
      )}

      {folder?.count === 0 && !error && <EmptyLibrary onNavigate={onNavigate} title={t("这个文件夹里还没有图片")} />}
      {folder && folder.count > 0 && groups?.length === 0 && !loading && (
        <p className="hint page-block">{t("这些图都没有这类 tag，可以换一种分组方式。")}</p>
      )}
      {!groups && loading && <p className="hint page-block">{t("正在加载…")}</p>}
      <div ref={sentinel} className="sentinel" aria-hidden="true" />
    </div>
  );
}
