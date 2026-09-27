// 由 src/theme/themes.json 生成 src/styles/themes.css。
// 每套主题输出一个 [data-theme="id"] 变量块；tag 分类色按深色 / 浅色两组共用。
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const data = JSON.parse(readFileSync(resolve(root, "src/theme/themes.json"), "utf8"));

// JSON 字段名 → CSS 变量名
const TOKEN_VARS = [
  ["bg", "bg"], ["panel", "panel"], ["raised", "raised"], ["raised2", "raised-2"],
  ["line", "line"], ["line2", "line-2"], ["chip", "chip"],
  ["fg1", "fg-1"], ["fg2", "fg-2"], ["fg3", "fg-3"],
  ["accent", "accent"], ["accentFg", "accent-fg"], ["accentSoft", "accent-soft"],
  ["glass", "glass"], ["ok", "ok"], ["warn", "warn"], ["err", "err"], ["shadow", "shadow"],
];
const TAG_VARS = ["artist", "copyright", "character", "general", "meta"];

const lines = ["/* 由 src/theme/themes.json 生成（pnpm themes），不要手改。 */"];
for (const theme of data.themes) {
  const decl = TOKEN_VARS.map(([key, name]) => `--${name}:${theme.tokens[key]}`).join(";");
  lines.push(`[data-theme="${theme.id}"]{color-scheme:${theme.mode};${decl}}`);
}
for (const mode of ["dark", "light"]) {
  const selector = data.themes.filter((t) => t.mode === mode).map((t) => `[data-theme="${t.id}"]`).join(",");
  const colors = data.tagColors[mode];
  const tagDecl = TAG_VARS.map((name) => `--tag-${name}:${colors[name]}`);
  // 与主题无关、只随明暗变化的变量，例如图片加载扫光的高光色。
  const modeDecl = Object.entries(data.modeTokens[mode]).map(([name, value]) => `--${name}:${value}`);
  lines.push(`${selector}{${[...tagDecl, ...modeDecl].join(";")}}`);
}
writeFileSync(resolve(root, "src/styles/themes.css"), `${lines.join("\n")}\n`);
console.log(`themes.css：${data.themes.length} 套主题`);
