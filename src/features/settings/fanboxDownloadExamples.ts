import type { FanboxDownloadSettings } from "../../lib/settings";

export const FANBOX_NAMING_DEFAULTS = {
  folderTemplate: "{user}/{date}-{title}",
  imageTemplate: "{index}",
  attachmentTemplate: "{name}",
};

const SAMPLE: Record<string, string> = {
  user: "Artist",
  creator_id: "sample-artist",
  date: "2026-10-06",
  title: "October sketches",
  postid: "12560223",
  index: "001",
  name: "source",
};

const CONTROL = /\p{Cc}/u;

function validTemplate(template: string, folder: boolean): boolean {
  const parts = template.split("/");
  return !!template.trim() && [...template].length <= 500
    && !template.includes("\\") && !CONTROL.test(template)
    && parts.length <= (folder ? 10 : 1)
    && parts.every((part) => !["", ".", ".."].includes(part.trim()) && !/^[a-z]:/i.test(part.trim()));
}

/** 与下载器的文件名处理保持一致，避免示例与实际保存路径不同。 */
function safeName(name: string): string {
  let result = "";
  let length = 0;
  const encoder = new TextEncoder();
  for (let char of name) {
    if (CONTROL.test(char)) continue;
    if (/[\\/:?"<>*|~]/.test(char)) char = String.fromCodePoint(char.codePointAt(0)! + 0xfee0);
    length += encoder.encode(char).length;
    if (length > 180) break;
    result += char;
  }
  result = result.trim().replace(/^\./, "．").replace(/\.$/, "．");
  if (!result) return "_";
  const stem = result.split(".")[0].toUpperCase();
  return /^(?:CON|PRN|AUX|NUL|COM\d|LPT\d)$/.test(stem) ? `_${result}` : result;
}

function render(template: string, values: Record<string, string>): string | null {
  const rendered = template.replace(/\{([^{}]+)\}/g, (token, name: string) =>
    values[name] === undefined ? token : safeName(values[name]));
  return /[{}]/.test(rendered) ? null : safeName(rendered.replace(/^[-_ ]+|[-_ ]+$/g, ""));
}

/** 示例只使用固定作品信息，不读取账号或磁盘内容。 */
export function fanboxDownloadExamples(settings: FanboxDownloadSettings, defaultDirectory: string) {
  if (!validTemplate(settings.folderTemplate, true)
    || !validTemplate(settings.imageTemplate, false)
    || !validTemplate(settings.attachmentTemplate, false)) return null;
  const folder = settings.folderTemplate.split("/").map((part) => render(part, SAMPLE));
  const image = render(settings.imageTemplate, SAMPLE);
  const cover = render(settings.imageTemplate, { ...SAMPLE, index: "000", name: "cover" });
  const attachment = render(settings.attachmentTemplate, SAMPLE);
  if (folder.some((part) => part === null) || image === null || cover === null || attachment === null) return null;
  const directory = settings.directory ?? defaultDirectory;
  const separator = /^(?:[a-z]:\\|\\\\)/i.test(directory) ? "\\" : "/";
  const root = directory.replace(/[\\/]+$/, "");
  const prefix = [root, ...folder].join(separator);
  return {
    image: `${prefix}${separator}${image}.jpg`,
    cover: `${prefix}${separator}${cover}.jpg`,
    attachment: `${prefix}${separator}${attachment}.psd`,
  };
}
