import type { Job } from "../../lib/downloads";

const normalize = (value: string) => value.trim().split(/\s+/).join(" ");

export function jobHeading(job: Job): string {
  const title = normalize(job.title);
  if (job.source !== "fanbox" && job.source !== "kemono") return title;
  return title.replace(/^creator:(\S+)(?=\s|$)/, (_, creator: string) =>
    job.source === "fanbox" ? `@${creator}` : creator,
  );
}

/** 标题已经显示的查询范围不重复；额外的分级和排序条件仍留在查询行。 */
export function jobQueryDetails(job: Job): string {
  const query = normalize(job.query ?? "");
  const title = normalize(job.title);
  if (query === title) return "";
  if (title && query.startsWith(`${title} `)) return query.slice(title.length + 1);
  if (job.source === "fanbox" || job.source === "kemono") {
    return query.replace(/^creator:\S+(?:\s+|$)/, "");
  }
  return query;
}

export function jobProgress(job: Job) {
  const done = job.saved + job.skipped + job.failed;
  const denominator = job.total ?? job.discovered;
  return {
    done,
    knownTotal: job.total !== null,
    reading: job.total === null && job.status === "running",
    width: denominator > 0 ? Math.min(100, (done / denominator) * 100) : 0,
  };
}
