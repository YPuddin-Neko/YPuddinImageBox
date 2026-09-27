import { Fragment, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { Icon } from "./Icon";

export interface SelectOption<T extends string | number> {
  value: T;
  label: string;
  /** 显示在选项右侧的简短说明。 */
  hint?: string;
}

interface CommonProps {
  /** 触发按钮的样式：search-source（搜索框里）、select（和按钮同高）、filter-select（筛选行）。 */
  className: string;
  id?: string;
  /** 读屏用的名称，例如「排序」。 */
  name: string;
  /** 按钮里值前面的小标题。 */
  label?: string;
  disabled?: boolean;
}

export interface MenuItem {
  key: string;
  label: string;
  hint?: string;
  selected: boolean;
  title?: string;
  /** 这一项下面画分隔线。 */
  divider?: boolean;
}

interface DropdownProps extends CommonProps {
  text: string;
  /** 按钮宽度按其中最宽的文字算，换选项时不跳动。 */
  sizers?: string[];
  items: MenuItem[];
  multiple?: boolean;
  onPick: (key: string) => void;
  /** 换掉按钮里的文字，例如只放一个图标。 */
  trigger?: ReactNode;
  title?: string;
  /** 菜单和按钮左边对齐（默认）还是右边对齐；靠右的按钮用右边对齐。 */
  align?: "start" | "end";
  /** 按钮上额外的状态标记，样式里用。 */
  state?: string;
}

const GAP = 6;
const MARGIN = 8;

/** 移动焦点时不滚动外面的页面（页面一滚动菜单就会收起），只在菜单自己放不下时滚动菜单。 */
function focusOption(menu: HTMLElement, option: HTMLElement | undefined) {
  if (!option) return;
  option.focus({ preventScroll: true });
  const { offsetTop: top, offsetHeight: height } = option;
  if (top < menu.scrollTop || top + height > menu.scrollTop + menu.clientHeight) {
    menu.scrollTop = top - (menu.clientHeight - height) / 2;
  }
}

/**
 * 应用里统一的下拉框。菜单放进浏览器顶层（popover），不会被对话框或滚动区域裁掉，
 * 贴着按钮下方打开，下方放不下时翻到上方。键盘：方向键移动，回车或空格选择，Esc 关闭。
 */
function Dropdown({
  className,
  id,
  name,
  label,
  disabled,
  text,
  sizers,
  items,
  multiple = false,
  onPick,
  trigger: content,
  title,
  align = "start",
  state,
}: DropdownProps) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useLayoutEffect(() => {
    const el = menu.current;
    const anchor = trigger.current;
    if (!open || !el || !anchor) return;
    if ("showPopover" in el) el.showPopover();
    const rect = anchor.getBoundingClientRect();
    el.style.minWidth = `${rect.width}px`;
    const { offsetWidth: width, offsetHeight: height } = el;
    const flip = rect.bottom + GAP + height > window.innerHeight - MARGIN && rect.top - GAP - height >= MARGIN;
    el.style.top = `${flip ? rect.top - GAP - height : rect.bottom + GAP}px`;
    const left = align === "end" ? rect.right - width : rect.left;
    el.style.left = `${Math.max(MARGIN, Math.min(left, window.innerWidth - width - MARGIN))}px`;
    el.dataset.placement = flip ? "above" : "below";
    const options = [...el.querySelectorAll<HTMLElement>('[role="option"]')];
    focusOption(el, options.find((option) => option.getAttribute("aria-selected") === "true") ?? options[0]);
  }, [open, align]);

  // 点到别处、滚动页面、窗口变化或失去焦点时收起。
  useEffect(() => {
    if (!open) return;
    const within = (target: EventTarget | null, ...nodes: (HTMLElement | null)[]) =>
      target instanceof Node && nodes.some((node) => node?.contains(target));
    const onPointerDown = (event: PointerEvent) => {
      if (!within(event.target, menu.current, trigger.current)) setOpen(false);
    };
    const onScroll = (event: Event) => {
      if (!within(event.target, menu.current)) setOpen(false);
    };
    const dismiss = () => setOpen(false);
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
    };
  }, [open]);

  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };

  const pick = (key: string) => {
    onPick(key);
    if (!multiple) close();
  };

  const onTriggerKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setOpen(true);
    }
  };

  const onMenuKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const options = [...el.querySelectorAll<HTMLElement>('[role="option"]')];
    const index = options.indexOf(document.activeElement as HTMLElement);
    const focusAt = (at: number) => focusOption(el, options[(at + options.length) % options.length]);
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focusAt(index + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        focusAt(index - 1);
        break;
      case "Home":
        event.preventDefault();
        focusAt(0);
        break;
      case "End":
        event.preventDefault();
        focusAt(options.length - 1);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        if (index >= 0) pick(options[index].dataset.key ?? "");
        break;
      case "Escape":
        // 在对话框里时只收起菜单，不连对话框一起关掉。
        event.preventDefault();
        event.stopPropagation();
        close();
        break;
      case "Tab":
        // 焦点先回到按钮，再照常移到下一个（或上一个）控件。
        trigger.current?.focus();
        setOpen(false);
        break;
    }
  };

  return (
    <>
      <button
        ref={trigger}
        id={id}
        type="button"
        className={className}
        aria-label={content ? name : `${name}：${text}`}
        title={title}
        data-state={state}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-open={open || undefined}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
        onKeyDown={onTriggerKey}
      >
        {content ?? (
          <>
            {label && <span className="dropdown-label">{label}</span>}
            <span className="dropdown-value">
              {sizers?.map((sizer) => (
                <span key={sizer} className="dropdown-sizer" aria-hidden="true">
                  {sizer}
                </span>
              ))}
              <span>{text}</span>
            </span>
          </>
        )}
      </button>
      {open && (
        <div
          ref={menu}
          id={menuId}
          className="menu"
          popover="manual"
          role="listbox"
          aria-label={name}
          aria-multiselectable={multiple || undefined}
          onKeyDown={onMenuKey}
        >
          {items.map((item) => (
            <Fragment key={item.key}>
              <div
                role="option"
                className="menu-option"
                tabIndex={-1}
                aria-selected={item.selected}
                data-key={item.key}
                title={item.title}
                onClick={() => pick(item.key)}
                onPointerMove={(event) => {
                  if (document.activeElement !== event.currentTarget) event.currentTarget.focus({ preventScroll: true });
                }}
              >
                <span className={multiple ? "menu-box" : "menu-mark"} aria-hidden="true">
                  {item.selected && <Icon name="check" size={multiple ? 12 : 15} />}
                </span>
                <span className="menu-label">{item.label}</span>
                {item.hint && <span className="menu-hint">{item.hint}</span>}
              </div>
              {item.divider && <div className="menu-divider" aria-hidden="true" />}
            </Fragment>
          ))}
        </div>
      )}
    </>
  );
}

/** 图标按钮加菜单：点了菜单项就执行并收起，`selected` 的项前面打勾。 */
export function MenuButton({
  items,
  onPick,
  children,
  title,
  state,
  ...common
}: CommonProps & { items: MenuItem[]; onPick: (key: string) => void; children: ReactNode; title?: string; state?: string }) {
  return (
    <Dropdown {...common} text="" items={items} onPick={onPick} trigger={children} title={title} align="end" state={state} />
  );
}

/** 单选下拉框，选中后自动收起。 */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  ...common
}: CommonProps & { value: T; options: SelectOption<T>[]; onChange: (value: T) => void }) {
  const current = options.find((option) => option.value === value) ?? options[0];
  return (
    <Dropdown
      {...common}
      text={current?.label ?? ""}
      sizers={options.map((option) => option.label)}
      items={options.map((option) => ({
        key: String(option.value),
        label: option.label,
        hint: option.hint,
        selected: option.value === current?.value,
      }))}
      onPick={(key) => {
        const next = options.find((option) => String(option.value) === key);
        if (next && next.value !== value) onChange(next.value);
      }}
    />
  );
}

const ALL_KEY = "__all__";

/**
 * 复选下拉框：第一项「全部」一键全选，至少保留一项。
 * `values` 为空和全选等价，按钮上显示 `allLabel`。
 */
export function MultiSelect<T extends string | number>({
  values,
  options,
  onChange,
  allLabel,
  ...common
}: CommonProps & { values: T[]; options: SelectOption<T>[]; onChange: (values: T[]) => void; allLabel: string }) {
  const chosen = options.filter((option) => values.includes(option.value)).map((option) => option.value);
  const all = chosen.length === 0 || chosen.length === options.length;
  const effective = all ? options.map((option) => option.value) : chosen;
  const text = all ? allLabel : options.filter((option) => chosen.includes(option.value)).map((o) => o.label).join("、");

  const toggle = (key: string) => {
    if (key === ALL_KEY) {
      if (!all) onChange(options.map((option) => option.value));
      return;
    }
    const option = options.find((o) => String(o.value) === key);
    if (!option) return;
    const next = effective.includes(option.value)
      ? effective.filter((value) => value !== option.value)
      : [...effective, option.value];
    if (next.length === 0) return;
    // 按选项本身的顺序排，和勾选先后无关。
    onChange(options.map((o) => o.value).filter((value) => next.includes(value)));
  };

  return (
    <Dropdown
      {...common}
      multiple
      text={text}
      items={[
        { key: ALL_KEY, label: allLabel, selected: all, divider: true },
        ...options.map((option) => {
          const selected = effective.includes(option.value);
          return {
            key: String(option.value),
            label: option.label,
            hint: option.hint,
            selected,
            title: selected && effective.length === 1 ? "至少保留一项" : undefined,
          };
        }),
      ]}
      onPick={toggle}
    />
  );
}
