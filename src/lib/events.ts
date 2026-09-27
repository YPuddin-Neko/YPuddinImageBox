import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/** 订阅 Rust 端推送的事件，组件卸载时自动退订。handler 可以每次渲染都换，不会重复订阅。 */
export function useTauriEvent<T>(name: string, handler: (payload: T) => void) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<T>(name, (event) => latest.current(event.payload)).then(
      (stop) => {
        if (disposed) stop();
        else unlisten = stop;
      },
      () => {},
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [name]);
}
