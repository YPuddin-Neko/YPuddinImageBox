import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Windows 上不用系统标题栏：右上角自己画最小化、最大化 / 还原、关闭，尺寸和系统的一样（46 × 32）；
 * 顶部一条拖拽区可以拖动窗口，双击最大化。关闭按钮照常走「关闭窗口时」的设置。
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    const sync = () => {
      win.isMaximized().then(setMaximized, () => {});
    };
    sync();
    const stop = win.onResized(sync);
    return () => {
      stop.then((unlisten) => unlisten(), () => {});
    };
  }, []);

  const win = getCurrentWindow();
  return (
    <>
      <div className="titlebar-drag" data-tauri-drag-region />
      <div className="window-controls">
        <button type="button" className="window-control" aria-label="最小化" onClick={() => void win.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0 5h10" />
          </svg>
        </button>
        <button
          type="button"
          className="window-control"
          aria-label={maximized ? "还原" : "最大化"}
          onClick={() => void win.toggleMaximize()}
        >
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M2.5 2.5V.5h7v7h-2M.5 2.5h7v7h-7z" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M.5.5h9v9h-9z" />
            </svg>
          )}
        </button>
        <button type="button" className="window-control close" aria-label="关闭" onClick={() => void win.close()}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="m0 0 10 10M10 0 0 10" />
          </svg>
        </button>
      </div>
    </>
  );
}
