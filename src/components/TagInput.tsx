import { useRef, useState, type KeyboardEvent } from "react";

import { t } from "../lib/i18n";
import { Icon } from "./Icon";

/** 按空白拆成一个个 tag。 */
export const splitTags = (text: string) => text.split(/\s+/).filter(Boolean);

/** tag 里的空白换成下划线（不然会被拆成两个）；从「复制 tag」粘贴过来的词尾逗号去掉。 */
export const cleanTag = (tag: string) => tag.trim().replace(/\s+/g, "_").replace(/,+$/, "");

/** 把一个 tag 加到条件末尾，已经有了就不重复加。 */
export function appendTag(value: string, tag: string): string {
  const clean = cleanTag(tag);
  const tags = splitTags(value);
  return !clean || tags.includes(clean) ? value : [...tags, clean].join(" ");
}

/** 站点的限定条件（user:、order:、rating: 这类）和粘贴进来的链接。 */
const QUALIFIER =
  /^(user|order|sort|rating|creator|tag|id|fav|ordfav|vote|score|date|age|width|height|mpixels|ratio|filesize|filetype|status|pool|source|parent|md5|limit|favcount|favorites|bookmarks):/i;

/** 排除的 tag 和限定条件换个颜色，一眼能和普通 tag 分开。 */
function kindOf(tag: string): "exclude" | "qualifier" | undefined {
  if (tag.length > 1 && tag.startsWith("-")) return "exclude";
  if (/^https?:\/\//i.test(tag) || QUALIFIER.test(tag)) return "qualifier";
  return undefined;
}

interface TagInputProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  label: string;
  /** 刚从详情里点进来（或本来就有）的 tag，闪一下告诉用户它在哪。`at` 每次点击都不同。 */
  flash?: { tag: string; at: number } | null;
}

/**
 * 搜索框里的 tag：每个 tag 一个胶囊，点 × 删除，双击修改；输入时按空格收成胶囊，
 * 空着按退格把最后一个拿回来改。`value` 就是空格分隔的整条条件（正在输入的词也在最后），外面照旧当字符串用。
 */
export function TagInput({ id, value, onChange, placeholder, label, flash }: TagInputProps) {
  const input = useRef<HTMLInputElement>(null);
  /** 正在输入、还没收成胶囊的词，连同它所在的那条条件：条件被外面改了（收藏的搜索、点详情里的 tag）就不再算数。 */
  const [draft, setDraft] = useState({ text: "", value: "" });
  const [editing, setEditing] = useState<{ index: number; text: string } | null>(null);
  /** 和 `editing` 同步：Enter、Esc 之后输入框移除时还会触发一次失焦，靠它避免再提交一次。 */
  const editingRef = useRef(editing);
  const composing = useRef(false);
  const [focused, setFocused] = useState(false);

  const typing = draft.value === value ? draft.text : "";
  const chips = splitTags(typing ? value.slice(0, value.length - typing.length) : value);

  const emit = (nextChips: string[], text = "") => {
    const next = [...nextChips, text].filter(Boolean).join(" ");
    setDraft({ text, value: next });
    onChange(next);
  };

  /** 输入框里的字：带了空格（打了空格、粘贴了几个词）就把前面的收成胶囊，最后一段留着接着输。 */
  const type = (text: string) => {
    if (composing.current) {
      emit(chips, text);
      return;
    }
    const parts = text.split(/\s+/);
    const rest = parts.pop() ?? "";
    emit([...chips, ...parts.map(cleanTag).filter(Boolean)], rest);
  };

  const keyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Backspace" && !typing && chips.length > 0) {
      event.preventDefault();
      emit(chips.slice(0, -1), chips[chips.length - 1]);
    } else if (event.key === "Enter" && typing) {
      // 条件不变，只是把正在输入的词收成胶囊；表单照常提交。
      emit([...chips, cleanTag(typing)]);
    }
  };

  const startEdit = (index: number) => {
    editingRef.current = { index, text: chips[index] };
    setEditing(editingRef.current);
  };

  const finishEdit = (commit: boolean) => {
    const current = editingRef.current;
    if (!current) return;
    editingRef.current = null;
    setEditing(null);
    if (commit) {
      const parts = splitTags(current.text).map(cleanTag).filter(Boolean);
      emit([...chips.slice(0, current.index), ...parts, ...chips.slice(current.index + 1)], typing);
    }
    input.current?.focus();
  };

  const remove = (index: number) => emit([...chips.slice(0, index), ...chips.slice(index + 1)], typing);

  return (
    <div
      className="tag-input"
      role="group"
      aria-label={label}
      onMouseDown={(event) => {
        // 点在胶囊之间的空白处也是去输入。
        if (event.target !== event.currentTarget) return;
        event.preventDefault();
        input.current?.focus();
      }}
    >
      {chips.map((tag, index) =>
        editing?.index === index ? (
          <input
            key={`edit-${index}`}
            className="tag-chip-edit"
            data-kind={kindOf(editing.text)}
            aria-label={t("修改 {tag}", { tag })}
            value={editing.text}
            size={Math.max(2, editing.text.length)}
            autoFocus
            onFocus={(event) => event.currentTarget.select()}
            onChange={(event) => {
              editingRef.current = { index, text: event.target.value };
              setEditing(editingRef.current);
            }}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing) return;
              if (event.key === "Enter") {
                event.preventDefault();
                finishEdit(true);
              } else if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                finishEdit(false);
              }
            }}
            onBlur={() => finishEdit(true)}
            spellCheck={false}
            autoComplete="off"
          />
        ) : (
          <span
            key={flash?.tag === tag ? `${index}-${tag}-${flash.at}` : `${index}-${tag}`}
            className="tag-chip"
            data-kind={kindOf(tag)}
            data-flash={flash?.tag === tag || undefined}
            title={t("双击修改")}
            onDoubleClick={() => startEdit(index)}
          >
            <span className="tag-chip-text">{tag}</span>
            <button
              type="button"
              className="tag-chip-remove"
              aria-label={t("删除 {tag}", { tag })}
              onClick={() => remove(index)}
              onDoubleClick={(event) => event.stopPropagation()}
            >
              <Icon name="close" size={10} />
            </button>
          </span>
        ),
      )}
      {chips.length > 0 && !focused && !typing && !editing && (
        <button
          type="button"
          className="tag-add"
          aria-label={t("添加 tag")}
          title={t("添加 tag")}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => input.current?.focus()}
        >
          <Icon name="plus" size={12} />
        </button>
      )}
      <input
        ref={input}
        id={id}
        className="search-input"
        aria-label={label}
        placeholder={chips.length === 0 ? placeholder : undefined}
        value={typing}
        onChange={(event) => type(event.target.value)}
        onCompositionStart={() => (composing.current = true)}
        onCompositionEnd={(event) => {
          composing.current = false;
          type(event.currentTarget.value);
        }}
        onKeyDown={keyDown}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          if (typing) emit([...chips, cleanTag(typing)]);
        }}
        spellCheck={false}
        autoComplete="off"
      />
    </div>
  );
}
