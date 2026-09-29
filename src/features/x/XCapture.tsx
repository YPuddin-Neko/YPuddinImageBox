import { useState, type FormEvent, type MouseEvent } from "react";

import { Icon } from "../../components/Icon";
import { PostGrid } from "../../components/PostGrid";
import { SelectionDock } from "../../components/SelectionDock";
import { Toast, useToast } from "../../components/Toast";
import { usePicker } from "../../components/usePicker";
import { useTauriEvent } from "../../lib/events";
import { formatCount } from "../../lib/format";
import { errorMessage, postKey, type Post } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { xCaptureClose, xCaptureOpen, type XPostsPayload } from "../../lib/x";
import { useDownloads } from "../downloads/context";

function appendPosts(previous: Post[], incoming: Post[]) {
  const known = new Set(previous.map(postKey));
  return [...previous, ...incoming.filter((post) => !known.has(postKey(post)))];
}

export function XCapture({ active }: { active: boolean }) {
  const { addPosts } = useDownloads();
  const [username, setUsername] = useState("");
  const [posts, setPosts] = useState<Post[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [opened, setOpened] = useState(false);
  const [notice, setNotice] = useToast();
  const { picked, pickedPosts, toggle: togglePick, clear: clearPicks, pickAll } = usePicker(posts);

  useTauriEvent<XPostsPayload>("x-posts", ({ posts: incoming, kind }) => {
    // 喜欢和书签是收藏页打开的。
    if (kind !== "media") return;
    setPosts((previous) => {
      const next = appendPosts(previous, incoming);
      if (!selected && next[0]) setSelected(postKey(next[0]));
      return next;
    });
  });

  const start = async (event: FormEvent) => {
    event.preventDefault();
    try {
      setPosts([]);
      clearPicks();
      setSelected(null);
      const result = await xCaptureOpen(username);
      setOpened(true);
      setNotice(
        result.proxyFallback
          ? t("采集窗口未能使用当前代理，已回退直连")
          : t("X 媒体窗口已打开，页面滚动时会自动收集图片"),
      );
    } catch (error) {
      setNotice(errorMessage(error));
    }
  };

  const close = async () => {
    try {
      await xCaptureClose();
      setOpened(false);
    } catch (error) {
      setNotice(errorMessage(error));
    }
  };

  const download = async () => {
    setBusy(true);
    try {
      await addPosts(pickedPosts);
      setNotice(t("已加入下载队列：{n} 张", { n: formatCount(pickedPosts.length) }));
      clearPicks();
    } catch (error) {
      setNotice(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const select = (post: Post, event: MouseEvent) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) togglePick(post, event);
    else setSelected(postKey(post));
  };

  return (
    <div className="page x-capture-page" data-view-active={active || undefined} data-picking={picked.size > 0 || undefined}>
      <header className="page-head" data-tauri-drag-region>
        <div className="page-title">
          <h1>{t("X 媒体采集")}</h1>
          <p>{t("输入用户名后，在 X 的媒体页面里收集图片")}</p>
        </div>
      </header>

      <form className="x-capture-bar page-block" onSubmit={(event) => void start(event)}>
        <label className="field x-capture-field">
          <span>{t("X 用户名")}</span>
          <input
            className="field-input"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            placeholder={t("例如 artist 或 @artist")}
            spellCheck={false}
            autoCapitalize="off"
          />
        </label>
        <button type="submit" className="btn primary" disabled={!username.trim()}>
          <Icon name="globe" size={15} />
          {t("开始采集")}
        </button>
        {opened && (
          <button type="button" className="btn ghost" onClick={() => void close()}>
            {t("关闭采集窗口")}
          </button>
        )}
      </form>

      <p className="x-capture-help page-block">
        {t("采集窗口会使用自己的登录状态，登录 X 后打开用户的 Media 页面；只收集图片，视频暂不加入图库。")}
      </p>

      {posts.length > 0 ? (
        <div className="scroll">
          <div className="x-capture-count">{t("已收集 {n} 张图片", { n: formatCount(posts.length) })}</div>
          <PostGrid
            posts={posts}
            selected={selected}
            onSelect={select}
            pageSize={40}
            picked={picked}
            onPick={togglePick}
            showSource
          />
          <SelectionDock
            count={picked.size}
            total={posts.length}
            onPickAll={pickAll}
            onClear={clearPicks}
          >
            <button type="button" className="btn primary" onClick={() => void download()} disabled={busy}>
              <Icon name="download" size={15} />
              {busy ? t("正在加入…") : t("下载选中")}
            </button>
          </SelectionDock>
        </div>
      ) : (
        <div className="empty page-block">
          <p className="empty-title">{t("还没有收集到图片")}</p>
          <p>{t("开始采集后，X 页面加载到的图片会出现在这里。")}</p>
        </div>
      )}
      <Toast message={notice} />
    </div>
  );
}
