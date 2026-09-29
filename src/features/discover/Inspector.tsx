import { useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { ShimmerImage } from "../../components/ShimmerImage";
import { formatBytes } from "../../lib/format";
import { t, type Msg } from "../../lib/i18n";
import { hasSize, imageSrc, postNumber, ratingLabel, SOURCE_LABEL, type Post, type PostTags } from "../../lib/ipc";
import { PANEL_ENTER } from "../../lib/motion";

const TAG_GROUPS: { key: keyof PostTags; label: Msg }[] = [
  { key: "artist", label: "画师" },
  { key: "copyright", label: "作品" },
  { key: "character", label: "角色" },
  { key: "general", label: "一般" },
  { key: "meta", label: "元" },
];

/** 不带时区的时间按 UTC 读（和 Rust 端一致）；按本地时间读的话，东八区会早一天。 */
const NAIVE_ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?$/;

function formatDate(value: string | null): string {
  if (!value) return "—";
  const date = new Date(NAIVE_ISO.test(value) ? `${value}Z` : value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString().slice(0, 10);
}

interface InspectorProps {
  post: Post | null;
  emptyText?: string;
  /** 图库里的图：显示保存位置。 */
  localPath?: string;
  /** 排在最前、占满一行的主要操作（下载原图、在访达中显示等）。 */
  primaryAction?: ReactNode;
  /** 预览图下方的提醒，例如文件已经不见了。 */
  notice?: string;
}

export function Inspector({ post, emptyText, localPath, primaryAction, notice }: InspectorProps) {
  const [copied, setCopied] = useState<"ok" | "failed" | null>(null);

  if (!post) {
    return (
      <aside className="insp insp-empty" aria-label={t("详情")}>
        <p>{emptyText ?? t("点一张图查看详情")}</p>
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
    <aside className="insp" aria-label={t("详情")}>
      <motion.div className="insp-body" initial={{ opacity: 0, x: 8 }} animate={{ opacity: 1, x: 0 }} transition={PANEL_ENTER}>
        <figure className="insp-figure">
          <div className="insp-pv">
            <ShimmerImage
              src={imageSrc(post.sampleUrl ?? post.thumbUrl)}
              alt={`#${postNumber(post)}`}
              loading="eager"
              fit="contain"
            />
          </div>
          <figcaption className="insp-res">
            {[
              hasSize(post) ? `${post.width} × ${post.height}` : null,
              post.fileExt ? post.fileExt.toUpperCase() : null,
              post.pages ? t("{n} 页", { n: post.pages }) : null,
            ]
              .filter(Boolean)
              .join(" · ") || "—"}
          </figcaption>
        </figure>
        {notice && <p className="insp-notice">{notice}</p>}
        <div className="insp-id">
          <b>#{postNumber(post, { grouped: true })}</b>
          <span className="badge">{SOURCE_LABEL[post.source]}</span>
          {post.rating && <span className="badge">{ratingLabel(post.rating)}</span>}
        </div>
        <dl className="insp-meta">
          <div>
            <dt>{t("大小")}</dt>
            <dd>{formatBytes(post.fileSize)}</dd>
          </div>
          <div>
            <dt>{t("分数")}</dt>
            {/* Pixiv 的作品列表、Kemono 的帖子不给分数。 */}
            <dd>{post.source === "pixiv" || post.source === "kemono" ? "—" : post.score}</dd>
          </div>
          <div>
            <dt>{t("收藏")}</dt>
            <dd>{post.favCount ?? "—"}</dd>
          </div>
          <div>
            <dt>{t("发布")}</dt>
            <dd>{formatDate(post.createdAt)}</dd>
          </div>
          <div className="wide">
            <dt>md5</dt>
            <dd className="mono">{post.md5 ?? "—"}</dd>
          </div>
          {localPath && (
            <div className="wide">
              <dt>{t("保存位置")}</dt>
              <dd className="mono" title={localPath}>
                {localPath}
              </dd>
            </div>
          )}
        </dl>
        <div className="insp-tags">
          {TAG_GROUPS.filter(({ key }) => post.tags[key].length > 0).map(({ key, label }) => (
            <section key={key}>
              <h3>{t(label)}</h3>
              <ul className={`tags tag-${key}`}>
                {post.tags[key].map((tag) => (
                  <li key={tag}>{tag}</li>
                ))}
              </ul>
            </section>
          ))}
        </div>
        <div className="insp-acts">
          {primaryAction && <div className="insp-primary">{primaryAction}</div>}
          <button type="button" className="btn" onClick={() => void openUrl(post.postUrl)}>
            <Icon name="ext" size={15} />
            {t("打开原帖")}
          </button>
          <button type="button" className="btn" onClick={() => void copyTags()} disabled={allTags.length === 0}>
            <Icon name={copied === "ok" ? "check" : "copy"} size={15} />
            {copied === "ok" ? t("已复制") : copied === "failed" ? t("复制失败") : t("复制 tag")}
          </button>
        </div>
      </motion.div>
    </aside>
  );
}
