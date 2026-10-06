import { useEffect, useRef, useState } from "react";

import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { formatCount } from "../../lib/format";
import { t } from "../../lib/i18n";
import { errorMessage } from "../../lib/ipc";
import { libraryDelete, libraryDeletePreview, type LibraryDeletePreview, type LibraryDeleteScope } from "../../lib/library";
import { trashLabel } from "../../lib/platform";

interface Pending {
  title: string;
  preview: LibraryDeletePreview | null;
  phase: "loading" | "choose" | "shared";
  keepFiles: boolean;
}

export function useScopeDelete(onDeleted: () => void) {
  const [pending, setPending] = useState<Pending | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [sharedReady, setSharedReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useToast();
  const alert = useRef<HTMLDivElement>(null);
  const generation = useRef(0);
  const executing = useRef(false);
  const trigger = useRef<HTMLElement | null>(null);
  const shared = pending?.phase === "shared";
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => {
    if (error) alert.current?.scrollIntoView({ block: "nearest" });
  }, [error]);
  useEffect(() => {
    if (pending !== null || deleting || !trigger.current) return;
    const target = trigger.current;
    trigger.current = null;
    const frame = window.requestAnimationFrame(() => {
      if (target.isConnected && !target.matches(":disabled")) target.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [pending, deleting]);
  useEffect(() => {
    setSharedReady(false);
    if (!shared) return;
    const timer = window.setTimeout(() => setSharedReady(true), 600);
    return () => window.clearTimeout(timer);
  }, [shared]);

  const cancel = () => {
    if (executing.current) return;
    generation.current++;
    setPending(null);
  };

  const request = async (scope: LibraryDeleteScope, title: string, button?: HTMLElement) => {
    if (executing.current) return;
    if (!pending) trigger.current = button ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    const id = ++generation.current;
    setError(null);
    setPending({ title, preview: null, phase: "loading", keepFiles: false });
    try {
      const preview = await libraryDeletePreview(scope);
      if (id !== generation.current) return;
      if (preview.posts.length === 0) {
        setPending(null);
        setNotice(t("没有可删除的文件。"));
        onDeleted();
      } else {
        setPending({ title, preview, phase: "choose", keepFiles: false });
      }
    } catch (err) {
      if (id !== generation.current) return;
      setPending(null);
      setError(errorMessage(err));
    }
  };

  const execute = async (target: Pending, keepFiles: boolean) => {
    if (!target.preview || executing.current) return;
    executing.current = true;
    setDeleting(true);
    try {
      const outcome = await libraryDelete(target.preview.posts, keepFiles);
      setPending(null);
      if (outcome.removed.length > 0) {
        setNotice(t(keepFiles ? "已从图库移除 {n} 项" : "已删除 {n} 项", { n: formatCount(outcome.removed.length) }));
      }
      if (outcome.failed.length > 0) {
        setError(t("有 {n} 项没能移到{trash}：{error}", {
          n: formatCount(outcome.failed.length), trash: trashLabel(), error: outcome.failed[0].message,
        }));
      }
      onDeleted();
    } catch (err) {
      setPending(null);
      setError(errorMessage(err));
    } finally {
      executing.current = false;
      setDeleting(false);
    }
  };

  const choose = (keepFiles: boolean) => {
    if (!pending?.preview || executing.current) return;
    if (pending.preview.sharedCount > 0) {
      setSharedReady(false);
      setPending({ ...pending, phase: "shared", keepFiles });
    } else {
      void execute(pending, keepFiles);
    }
  };

  const preview = pending?.preview;
  const errorAlert = error && <div ref={alert} className="alert page-block" role="alert"><span>{error}</span>
    <button type="button" className="btn" onClick={() => setError(null)}>{t("关闭")}</button>
  </div>;
  const content = <>
    <Toast message={notice} />
    <Dialog key={shared ? "shared" : "choose"} open={pending !== null}
      title={shared ? t("同时从其他分组移除？") : t("删除「{name}」中的全部文件？", { name: pending?.title ?? "" })}
      onClose={cancel} initialFocus="last"
      actions={<>
        {preview && (shared ? <button type="button" className="btn danger" disabled={deleting || !sharedReady}
          onClick={(event) => {
            if (pending && sharedReady && event.detail <= 1) void execute(pending, pending.keepFiles);
          }}>
          <Icon name="trash" size={15} />{t(pending?.keepFiles ? "确认移除" : "确认删除")}
        </button> : <>
          <button type="button" className="btn danger" disabled={deleting} onClick={() => choose(false)}>
            <Icon name="trash" size={15} />{t("移到{trash}", { trash: trashLabel() })}
          </button>
          <button type="button" className="btn" disabled={deleting} onClick={() => choose(true)}>{t("只从图库移除")}</button>
        </>)}
        <button type="button" className="btn ghost" disabled={deleting} onClick={cancel}>{t("取消")}</button>
      </>}
    >
      {!preview ? <p className="dialog-copy">{t("正在读取…")}</p> : shared ? <>
        <p className="dialog-copy">{t("其中 {n} 项也会从其他分组移除。", { n: formatCount(preview.sharedCount) })}</p>
        <p className="dialog-copy scope-delete-names">{t("受影响的分组：{names}", { names: preview.otherGroups.join(t("、::list")) })}</p>
        {preview.otherGroupCount > preview.otherGroups.length && <p className="dialog-copy">{t("另有 {n} 个分组。", { n: formatCount(preview.otherGroupCount - preview.otherGroups.length) })}</p>}
        <p className="dialog-copy">{t(pending?.keepFiles
          ? "「{name}」共 {n} 项，保留本地文件。"
          : "「{name}」共 {n} 项，移到{trash}。", {
          name: pending?.title ?? "", n: formatCount(preview.posts.length), trash: trashLabel(),
        })}</p>
      </> : <>
        <p className="dialog-copy">{t("共 {n} 项", { n: formatCount(preview.posts.length) })}</p>
        <p className="dialog-copy">{t("仅从图库移除时，保留本地文件。")}</p>
      </>}
    </Dialog>
  </>;
  return { request, busy: pending !== null || deleting, errorAlert, content };
}
