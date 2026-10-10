import type { PixivCreator, Post, Source } from "../../lib/ipc";

export function mergePixivCreators(previous: PixivCreator[], incoming: PixivCreator[]): PixivCreator[] {
  const merged = new Map(previous.map((creator) => [creator.id, creator]));
  for (const creator of incoming) merged.set(creator.id, creator);
  return [...merged.values()];
}

export const pixivCreatorQuery = (creator: PixivCreator) => `https://www.pixiv.net/users/${creator.id}`;

const canonicalId = (value: string): string | null => /^[0-9]+$/.test(value) ? value.replace(/^0+(?=[0-9])/, "") : null;

export function hasPixivIdCollision(
  sources: Source[],
  query: string,
  creators: PixivCreator[],
  posts: Pick<Post, "source" | "id">[],
): boolean {
  if (sources.length !== 1 || sources[0] !== "pixiv") return false;
  const words = query.trim().split(/\s+/).filter((word) => !/^(rating|order|sort):/.test(word));
  if (words.length !== 1) return false;
  const id = canonicalId(words[0]);
  if (!id || id === "0") return false;
  return creators.some((creator) => canonicalId(creator.id) === id)
    && posts.some((post) => post.source === "pixiv" && (typeof post.id === "string" ? /^[0-9]+$/.test(post.id) : Number.isSafeInteger(post.id) && post.id >= 0)
      && (BigInt(post.id) / 1000n).toString() === id);
}
