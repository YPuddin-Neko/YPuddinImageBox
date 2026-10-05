import { useCallback, useEffect, useRef, useState } from "react";

import { EVENTS, type SavedPayload } from "../../lib/downloads";
import { useTauriEvent } from "../../lib/events";
import type { PostRef } from "../../lib/library";
import { searchRemote, type Rating } from "../../lib/ipc";
import { libraryList } from "../../lib/library";
import { CreatorFeed, type CreatorFeedSnapshot } from "./creatorFeed";

const EMPTY: CreatorFeedSnapshot = { posts: [], owned: new Set(), hasMore: false, errors: [] };

export function useCreatorFeed(active: boolean, creator: string | null, ratings: Rating[], onlineKey: string | null) {
  const key = creator ? JSON.stringify([creator, ratings, onlineKey]) : null;
  const [state, setState] = useState<{ key: string | null; data: CreatorFeedSnapshot; loading: boolean }>({ key: null, data: EMPTY, loading: false });
  const current = useRef<{ key: string; feed: CreatorFeed } | null>(null);

  const read = useCallback(async (entry: { key: string; feed: CreatorFeed }) => {
    setState((prev) => ({ key: entry.key, data: prev.key === entry.key ? prev.data : EMPTY, loading: true }));
    await entry.feed.more((data) => {
      if (current.current === entry) setState({ key: entry.key, data, loading: true });
    });
    if (current.current === entry) setState({ key: entry.key, data: entry.feed.snapshot(), loading: false });
  }, []);

  const refresh = useCallback(() => {
    if (!key || !creator) return;
    if (current.current?.key === key) {
      const entry = current.current;
      setState((prev) => ({ ...prev, loading: true }));
      void entry.feed.reload((data) => {
        if (current.current === entry) setState((prev) => ({ ...prev, data }));
      }).finally(() => {
        if (current.current === entry) setState((prev) => ({ ...prev, loading: false }));
      });
      return;
    }
    const entry = { key, feed: new CreatorFeed(
      (offset) => libraryList({ source: "fanbox", fanboxCreator: creator, tags: "", ratings, sort: "newest", offset, limit: 40 }),
      onlineKey ? (cursor) => searchRemote({ source: "fanbox", tags: `creator:${creator}`, ratings, sort: "newest", cursor }) : null,
    ) };
    current.current = entry;
    void read(entry);
  }, [key, read]);

  useEffect(() => {
    if (active && key) refresh();
    else current.current = null;
    return () => { current.current = null; };
  }, [active, key, refresh]);

  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const refreshLater = () => {
    if (!active || !key) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const entry = current.current;
      if (entry) void entry.feed.refreshLocal((data) => {
        if (current.current === entry) setState((prev) => ({ ...prev, data }));
      });
    }, 500);
  };
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, [key, active]);
  useTauriEvent<SavedPayload>(EVENTS.librarySaved, (post) => {
    if (post.source !== "fanbox") return;
    current.current?.feed.saved(post.postId);
    refreshLater();
  });
  useTauriEvent<PostRef[]>(EVENTS.libraryRemoved, (posts) => {
    const ids = posts.filter((post) => post.source === "fanbox").map((post) => post.postId);
    if (ids.length === 0) return;
    const entry = current.current;
    if (entry) {
      entry.feed.removeSaved(ids);
      setState((prev) => ({ ...prev, data: entry.feed.snapshot() }));
    }
    refreshLater();
  });

  const loadMore = useCallback(() => {
    if (current.current && !state.loading) void read(current.current);
  }, [read, state.loading]);

  return { ...(state.key === key ? state.data : EMPTY), loading: state.key === key && state.loading, refresh, loadMore };
}
