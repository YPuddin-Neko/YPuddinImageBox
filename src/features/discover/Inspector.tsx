import { useState } from "react";
import { motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { ShimmerImage } from "../../components/ShimmerImage";
import { PANEL_ENTER } from "../../lib/motion";
import { imageSrc, RATING_LABEL, SOURCE_LABEL, type Post, type PostTags } from "../../lib/ipc";

const TAG_GROUPS: { key: keyof PostTags; label: string }[] = [
  { key: "artist", label: "画师" },
  { key: "copyright", label: "作品" },
  { key: "character", label: "角色" },
  { key: "general", label: "一般" },
  { key: "meta", label: "元" },
];

function formatBytes(bytes: number | null): string {
  if (bytes == null) return "—";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatDate(value: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString().slice(0, 10);
}

export function Inspector({ post }: { post: Post | null }) {
  const [copied, setCopied] = useState<"ok" | "failed" | null>(null);

  if (!post) {
    return (
      <aside className="insp insp-empty" aria-label="详情">
        <p>点一张图查看详情</p>
      </aside>
    );
  }

  const allTags = TAG_GROUPS.flatMap(({ key }) => post.tags[key]);
  const copyTags = async () => {
    try {
      await navigator.clipboard.writeText(allTags.join(", "));
      setCopied("ok");
    } catch {
      setCopied("failed");
    }
    window.setTimeout(() => setCopied(null), 1600);
  };

  return (
    <aside className="insp" aria-label="详情">
      <motion.div className="insp-body" initial={{ opacity: 0, x: 8 }} animate={{ opacity: 1, x: 0 }} transition={PANEL_ENTER}>
      <div className="insp-pv">
        <ShimmerImage src={imageSrc(post.sampleUrl ?? post.thumbUrl)} alt={`#${post.id}`} loading="eager" fit="contain" />
        <span className="insp-res">
          {post.width} × {post.height}
          {post.fileExt ? ` · ${post.fileExt.toUpperCase()}` : ""}
        </span>
      </div>
      <div className="insp-id">
        <b>#{post.id.toLocaleString("en-US")}</b>
        <span className="badge">{SOURCE_LABEL[post.source]}</span>
        {post.rating && <span className="badge">{RATING_LABEL[post.rating]}</span>}
      </div>
      <dl className="insp-meta">
        <div>
          <dt>大小</dt>
          <dd>{formatBytes(post.fileSize)}</dd>
        </div>
        <div>
          <dt>分数</dt>
          <dd>{post.score}</dd>
        </div>
        <div>
          <dt>收藏</dt>
          <dd>{post.favCount ?? "—"}</dd>
        </div>
        <div>
          <dt>发布</dt>
          <dd>{formatDate(post.createdAt)}</dd>
        </div>
        <div className="wide">
          <dt>md5</dt>
          <dd className="mono">{post.md5 ?? "—"}</dd>
        </div>
      </dl>
      <div className="insp-tags">
        {TAG_GROUPS.filter(({ key }) => post.tags[key].length > 0).map(({ key, label }) => (
          <section key={key}>
            <h3>{label}</h3>
            <ul className={`tags tag-${key}`}>
              {post.tags[key].map((tag) => (
                <li key={tag}>{tag}</li>
              ))}
            </ul>
          </section>
        ))}
      </div>
      <div className="insp-acts">
        <button type="button" className="btn" onClick={() => void openUrl(post.postUrl)}>
          <Icon name="ext" size={15} />
          打开原帖
        </button>
        <button type="button" className="btn" onClick={() => void copyTags()} disabled={allTags.length === 0}>
          <Icon name={copied === "ok" ? "check" : "copy"} size={15} />
          {copied === "ok" ? "已复制" : copied === "failed" ? "复制失败" : "复制 tag"}
        </button>
      </div>
      </motion.div>
    </aside>
  );
}
