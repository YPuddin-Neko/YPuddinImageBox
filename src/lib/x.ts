import { invoke } from "@tauri-apps/api/core";

import type { Post } from "./ipc";

export interface XPostsPayload {
  posts: Post[];
}

export const xCaptureOpen = (username: string) => invoke<void>("x_capture_open", { username });
export const xCaptureClose = () => invoke<void>("x_capture_close");
