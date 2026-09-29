import type { ReactNode } from "react";

const PATHS = {
  compass: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="m15.5 8.5-2 5-5 2 2-5z" />
    </>
  ),
  gear: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.5-2-3.4-2.3 1a7.4 7.4 0 0 0-2.6-1.5l-.4-2.6h-4l-.4 2.6a7.4 7.4 0 0 0-2.6 1.5l-2.3-1-2 3.4 2 1.5a7.6 7.6 0 0 0 0 3l-2 1.5 2 3.4 2.3-1a7.4 7.4 0 0 0 2.6 1.5l.4 2.6h4l.4-2.6a7.4 7.4 0 0 0 2.6-1.5l2.3 1 2-3.4z" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4 4" />
    </>
  ),
  ext: <path d="M14 4.5h5.5V10M19.5 4.5 11 13M18 14v4.5a1 1 0 0 1-1 1H5.5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1H10" />,
  copy: (
    <>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
      <path d="M15.5 5.5h-9a1 1 0 0 0-1 1v9" />
    </>
  ),
  check: <path d="m5 12.5 4.5 4.5L19 7.5" />,
  pages: (
    <>
      <rect x="4.5" y="8" width="11" height="11.5" rx="1.5" />
      <path d="M8.5 4.5h9.5a1.5 1.5 0 0 1 1.5 1.5v10" />
    </>
  ),
  retry: <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4" />,
  folder: <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4.2l2 2H19a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z" />,
  library: (
    <>
      <rect x="7" y="7" width="13.5" height="13" rx="2" />
      <path d="M3.5 16.5V5.5a2 2 0 0 1 2-2h11M20.5 16.5l-3.8-3.8-7.2 7.3" />
      <circle cx="12" cy="11.5" r="1.4" />
    </>
  ),
  download: <path d="M12 4.5v10.5M7.5 11 12 15.5l4.5-4.5M5 19.5h14" />,
  pause: <path d="M9 6.5v11M15 6.5v11" />,
  play: <path d="M8.5 5.8v12.4l9.5-6.2z" />,
  close: <path d="m6.5 6.5 11 11M17.5 6.5l-11 11" />,
  zoomIn: (
    <>
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M15 15 20 20M10.5 8v5M8 10.5h5" />
    </>
  ),
  zoomOut: (
    <>
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M15 15 20 20M8 10.5h5" />
    </>
  ),
  chevronLeft: <path d="m14.5 5.5-6.5 6.5 6.5 6.5" />,
  chevronRight: <path d="m9.5 5.5 6.5 6.5-6.5 6.5" />,
  fit: (
    <>
      <path d="M5 9V5h4M15 5h4v4M19 15v4h-4M9 19H5v-4" />
    </>
  ),
  trash: <path d="M5 7h14M10 7V5h4v2M7 7l1 12.5h8L17 7" />,
  bell: <path d="M6.5 16.5V11a5.5 5.5 0 0 1 11 0v5.5l1.5 2h-14zM10 20.5a2.2 2.2 0 0 0 4 0" />,
  bookmark: <path d="M7 4.5h10a.5.5 0 0 1 .5.5v14.5L12 16l-5.5 3.5V5a.5.5 0 0 1 .5-.5z" />,
  back: <path d="M14.5 5.5 8 12l6.5 6.5" />,
  grid: <path d="M5 5h5.5v5.5H5zM13.5 5H19v5.5h-5.5zM5 13.5h5.5V19H5zM13.5 13.5H19V19h-5.5z" />,
  user: (
    <>
      <circle cx="12" cy="8.5" r="3.8" />
      <path d="M4.8 19.5c1.3-3.4 4-5.1 7.2-5.1s5.9 1.7 7.2 5.1" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17M12 3.5c2.3 2.4 3.4 5.2 3.4 8.5s-1.1 6.1-3.4 8.5c-2.3-2.4-3.4-5.2-3.4-8.5s1.1-6.1 3.4-8.5z" />
    </>
  ),
  palette: (
    <>
      <path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.1 0 1.7-.8 1.7-1.7 0-.9-.7-1.4-.7-2.3 0-.9.7-1.7 1.7-1.7h1.8a4 4 0 0 0 4-4c0-4.1-3.8-7.3-8.5-7.3z" />
      <circle cx="7.8" cy="11.3" r="1.1" />
      <circle cx="10.8" cy="7.8" r="1.1" />
      <circle cx="15.2" cy="8.4" r="1.1" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg className="icon" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      {PATHS[name]}
    </svg>
  );
}
