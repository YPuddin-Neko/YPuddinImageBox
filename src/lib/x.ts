import { invoke } from "@tauri-apps/api/core";

import type { Post } from "./ipc";

/** 采集窗口打开的页面：用户的媒体，或自己的喜欢、书签（收藏页用）。 */
export type XTarget = "media" | "likes" | "bookmarks";

export interface XPostsPayload {
  posts: Post[];
  /** 这批图来自哪种页面。媒体归采集页，喜欢和书签归收藏页。 */
  kind: XTarget;
}

/** 书签页只有自己能看，不用填用户名。 */
export const xCaptureOpen = (username: string, target: XTarget = "media") =>
  invoke<void>("x_capture_open", { username, target });
export const xCaptureClose = () => invoke<void>("x_capture_close");
