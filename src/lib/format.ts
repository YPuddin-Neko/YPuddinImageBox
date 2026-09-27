import { locale } from "./i18n";

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/** 任务时间：今天只显示时分，其余加上月日（中文「9月27日」，英文「Sep 27」）。 */
export function formatTime(ms: number): string {
  const date = new Date(ms);
  const time = date.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const today = new Date();
  if (date.toDateString() === today.toDateString()) return time;
  return `${date.toLocaleDateString(locale(), { month: "short", day: "numeric" })} ${time}`;
}

export const formatCount = (value: number) => value.toLocaleString(locale());
