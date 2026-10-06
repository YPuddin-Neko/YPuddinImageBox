import type { PixivCreator } from "../../lib/ipc";

export function mergePixivCreators(previous: PixivCreator[], incoming: PixivCreator[]): PixivCreator[] {
  const merged = new Map(previous.map((creator) => [creator.id, creator]));
  for (const creator of incoming) merged.set(creator.id, creator);
  return [...merged.values()];
}

export const pixivCreatorQuery = (creator: PixivCreator) => `https://www.pixiv.net/users/${creator.id}`;
