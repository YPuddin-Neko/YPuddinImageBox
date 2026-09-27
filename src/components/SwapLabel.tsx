/**
 * 按钮文字在几种之间切换时（例如「暂停 / 恢复」），按最宽的那种定宽，换了文字按钮不变宽窄，
 * 上下几行的按钮保持对齐。英文的几种说法长短不一，中文一般一样长。
 */
export function SwapLabel({ labels, active }: { labels: string[]; active: number }) {
  return (
    <span className="swap-label">
      {labels.map((label, index) => (
        <span key={label} data-hidden={index !== active || undefined} aria-hidden={index !== active || undefined}>
          {label}
        </span>
      ))}
    </span>
  );
}
