import { createElement, Fragment, useSyncExternalStore, type ReactNode } from "react";

import en from "../locales/en.json";

/** 界面语言。 */
export type Language = "zh" | "en";
/** 设置里的选项：跟随系统，或固定用某一种。 */
export type LanguageSetting = "system" | Language;

/**
 * 界面文字的键：源码里直接写中文原文，英文界面按原文到 `locales/en.json` 里查。
 * 同一句中文要译成不同英文时，在后面加 `::用途` 区分，中文界面不显示这一段。
 * 键必须在英文表里，漏译会在类型检查时报错。
 */
export type Msg = keyof typeof en;

const EN: Record<string, string> = en;
let current: Language = "zh";
const listeners = new Set<() => void>();

export const language = () => current;

/** 日期、数字格式用的区域。 */
export const locale = () => (current === "en" ? "en-US" : "zh-CN");

export function setLanguage(next: Language) {
  document.documentElement.lang = next === "en" ? "en" : "zh-CN";
  if (next === current) return;
  current = next;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 语言切换时重新渲染。放在根组件里，整个界面都会跟着换，各页面的状态不受影响。 */
export const useLanguage = () => useSyncExternalStore(subscribe, language);

function template(text: Msg): string {
  if (current === "en") return EN[text] ?? text;
  const context = text.indexOf("::");
  return context >= 0 ? text.slice(0, context) : text;
}

/** `{name}` 是占位符；`{n|image|images}` 按 n 是否为 1 选单数或复数，只有英文用得到。 */
const PLACEHOLDER = /\{(\w+)(?:\|([^|}]*)\|([^}]*))?\}/g;

type Value = string | number;

function fill(whole: string, value: Value | undefined, one?: string, other?: string): string {
  if (value === undefined) return whole;
  if (one !== undefined) return String(value) === "1" ? one : (other ?? one);
  return String(value);
}

/** 界面文字，占位符换成 `vars` 里的值。数字要分位显示时先用 formatCount 转好再传。 */
export function t(text: Msg, vars?: Record<string, Value>): string {
  const raw = template(text);
  if (!vars) return raw;
  return raw.replace(PLACEHOLDER, (whole, key: string, one?: string, other?: string) =>
    fill(whole, vars[key], one, other),
  );
}

/**
 * 可收起按钮（样式里的 `.collapsible`）的悬停提示：英文窄窗口下按钮只留图标，靠它看文字；
 * 中文不会收起，不加提示，免得和按钮上的字重复。
 */
export const collapsedTitle = (text: string) => (current === "en" ? text : undefined);

/** 把几句话连成一段：中文直接相连，英文句子之间加空格。 */
export const sentences = (...parts: (string | false | null | undefined)[]) =>
  parts.filter(Boolean).join(current === "en" ? " " : "");

/** 和 `t` 一样，但占位符可以换成元素，例如加粗的数字、路径、输入框。 */
export function tx(text: Msg, vars: Record<string, ReactNode>): ReactNode {
  const raw = template(text);
  const parts: ReactNode[] = [];
  let last = 0;
  for (const match of raw.matchAll(PLACEHOLDER)) {
    const [whole, key, one, other] = match;
    parts.push(raw.slice(last, match.index));
    const value = vars[key];
    const plain = typeof value === "string" || typeof value === "number";
    parts.push(plain ? fill(whole, value, one, other) : (value ?? whole));
    last = match.index + whole.length;
  }
  parts.push(raw.slice(last));
  return createElement(Fragment, null, ...parts);
}
