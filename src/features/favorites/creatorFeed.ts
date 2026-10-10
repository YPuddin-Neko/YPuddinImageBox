import type { Post, PostId } from "../../lib/ipc";
import type { LocalPost } from "../../lib/library";

export const localRecord = (post: Post): LocalPost | null => "path" in post ? post as LocalPost : null;

const articleId = (post: Post) => post.postUrl?.match(/\/posts\/(\d+)(?:[/?#]|$)/)?.[1];
function assetKey(post: Post): string | null {
  if (post.source !== "fanbox" || !post.fileUrl) return null;
  try {
    const page = new URL(post.postUrl);
    const file = new URL(post.fileUrl);
    const article = articleId(post);
    if (!article || page.protocol !== "https:" || !(page.hostname === "fanbox.cc" || page.hostname.endsWith(".fanbox.cc"))
      || !["https:", "http:"].includes(file.protocol)) return null;
    return `${article}:${file.href}`;
  } catch { return null; }
}
const compareIds = (a: PostId, b: PostId) => {
  const left = String(a).replace(/^0+(?=\d)/, "");
  const right = String(b).replace(/^0+(?=\d)/, "");
  return left.length - right.length || left.localeCompare(right);
};

function resourceIndex(post: Post, article: string): number | null {
  if (post.downloadIndex != null) return post.downloadIndex;
  // 早期记录使用稿件号乘 1000 加资源位置；稳定资源编号不包含位置。
  const legacy = BigInt(post.id);
  if (legacy < 2n ** 52n && legacy / 1000n === BigInt(article)) {
    const index = Number(legacy % 1000n);
    const cover = post.fileUrl?.startsWith(`https://pixiv.pximg.net/fanbox/public/images/post/${article}/cover/`);
    return index === 999 && cover ? 0 : index + 1;
  }
  return null;
}

function comparePosts(a: Post, b: Post): number {
  const left = articleId(a);
  const right = articleId(b);
  if (left && right) {
    if (left !== right) return compareIds(right, left);
    const aIndex = resourceIndex(a, left);
    const bIndex = resourceIndex(b, right);
    return aIndex !== null && bIndex !== null ? aIndex - bIndex : 0;
  }
  const aTime = Date.parse(a.createdAt ?? "");
  const bTime = Date.parse(b.createdAt ?? "");
  if (Number.isFinite(aTime) && Number.isFinite(bTime) && aTime !== bTime) return bTime - aTime;
  return compareIds(b.id, a.id);
}

/** 同一资源优先读本地；本地文件缺失时仍可显示当前账号能访问的远程预览。 */
export function mergeCreatorPosts(remote: Post[], local: LocalPost[]): Post[] {
  const posts = new Map<string, Post>();
  const assets = new Map<string, string>();
  for (const post of remote) {
    const key = String(post.id);
    const asset = assetKey(post);
    const previous = asset && assets.get(asset);
    if (previous) posts.delete(previous);
    posts.set(key, post);
    if (asset) assets.set(asset, key);
  }
  for (const post of local) {
    const key = String(post.id);
    const asset = assetKey(post);
    const previous = asset && assets.get(asset);
    const online = posts.get(key) ?? (previous ? posts.get(previous) : undefined);
    if (previous && previous !== key) posts.delete(previous);
    posts.set(key, post.missing && online ? { ...post, ...online, id: post.id }
      : online ? { ...post, downloadIndex: online.downloadIndex ?? post.downloadIndex } : post);
    if (asset) assets.set(asset, key);
  }
  return [...posts.values()].sort(comparePosts);
}

interface LocalPage { posts: LocalPost[]; hasMore: boolean }
interface RemotePage { posts: Post[]; next: string | null; owned: PostId[] }
export interface CreatorFeedSnapshot {
  posts: Post[];
  owned: Set<string>;
  hasMore: boolean;
  errors: unknown[];
}

/** 两个来源独立翻页和报错，远程失权或断网不会清掉已经读取的本地记录。 */
export class CreatorFeed {
  private local: LocalPost[] = [];
  private remote: Post[] = [];
  private owned = new Set<string>();
  private removed = new Set<string>();
  private removedAssets = new Map<string, string>();
  private offset = 0;
  private cursor: string | null = null;
  private localMore = true;
  private remoteMore: boolean;
  private localError: unknown;
  private remoteError: unknown;
  private busy = false;
  private refreshPending = false;

  private readLocal: (offset: number) => Promise<LocalPage>;
  private readRemote: ((cursor: string | null) => Promise<RemotePage>) | null;

  constructor(
    readLocal: (offset: number) => Promise<LocalPage>,
    readRemote: ((cursor: string | null) => Promise<RemotePage>) | null,
  ) {
    this.readLocal = readLocal;
    this.readRemote = readRemote;
    this.remoteMore = readRemote !== null;
  }

  snapshot(): CreatorFeedSnapshot {
    const local = this.local.filter((post) => !this.removed.has(String(post.id)));
    const posts = mergeCreatorPosts(this.remote, local);
    const visible = new Map(posts.map((post) => [String(post.id), String(post.id)]));
    const assets = new Map(posts.flatMap((post) => {
      const asset = assetKey(post);
      return asset ? [[asset, String(post.id)] as const] : [];
    }));
    const owned = new Set<string>();
    for (const post of this.remote) {
      const id = String(post.id);
      const asset = assetKey(post);
      const key = visible.get(id) ?? (asset ? assets.get(asset) : undefined);
      if (key && this.owned.has(id) && !this.removed.has(id) && (!asset || !this.removedAssets.has(asset))) owned.add(key);
    }
    local.forEach((post) => post.missing ? owned.delete(String(post.id)) : owned.add(String(post.id)));
    return {
      posts, owned,
      hasMore: this.localMore || this.remoteMore,
      errors: [this.localError, this.remoteError].filter((error) => error !== undefined),
    };
  }

  async more(changed: (snapshot: CreatorFeedSnapshot) => void): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      await Promise.all([
        this.localMore ? this.readLocal(this.offset).then((page) => {
          this.local.push(...page.posts);
          this.offset += page.posts.length;
          this.localMore = page.hasMore && page.posts.length > 0;
        }).catch((error: unknown) => {
          this.localError = error;
          this.localMore = false;
        }).then(() => changed(this.snapshot())) : Promise.resolve(),
        this.remoteMore && this.readRemote ? this.readRemote(this.cursor).then((page) => {
          this.remote.push(...page.posts);
          page.owned.forEach((id) => this.owned.add(String(id)));
          this.remoteMore = page.next !== null && page.next !== this.cursor;
          this.cursor = page.next;
        }).catch((error: unknown) => {
          this.remoteError = error;
          this.remoteMore = false;
        }).then(() => changed(this.snapshot())) : Promise.resolve(),
      ]);
    } finally {
      this.busy = false;
      if (this.refreshPending) {
        this.refreshPending = false;
        await this.refreshLocal(changed);
      }
    }
  }

  removeSaved(ids: PostId[]): void {
    const removed = new Set(ids.map(String));
    const assets = new Set(this.local.filter((post) => removed.has(String(post.id))).map(assetKey).filter((key) => key !== null));
    this.local.forEach((post) => {
      const asset = assetKey(post);
      if (asset && removed.has(String(post.id))) this.removedAssets.set(asset, String(post.id));
    });
    this.remote.forEach((post) => {
      const asset = assetKey(post);
      if (asset && assets.has(asset)) removed.add(String(post.id));
    });
    removed.forEach((id) => { this.owned.delete(id); this.removed.add(id); });
    this.local = this.local.filter((post) => !removed.has(String(post.id)));
  }

  saved(id: PostId): void {
    const key = String(id);
    this.removed.delete(key);
    const post = [...this.remote, ...this.local].find((post) => String(post.id) === key);
    const currentAsset = post && assetKey(post);
    for (const [asset, removedId] of this.removedAssets) {
      if (removedId === key || asset === currentAsset) this.removedAssets.delete(asset);
    }
  }

  async refreshLocal(changed: (snapshot: CreatorFeedSnapshot) => void): Promise<void> {
    if (this.busy) { this.refreshPending = true; return; }
    this.busy = true;
    try {
      const target = Math.max(40, this.offset);
      const posts: LocalPost[] = [];
      let more = true;
      while (more && posts.length < target) {
        const page = await this.readLocal(posts.length);
        posts.push(...page.posts);
        more = page.hasMore && page.posts.length > 0;
      }
      this.local = posts;
      this.offset = posts.length;
      this.localMore = more;
      this.localError = undefined;
    } catch (error) { this.localError = error; }
    finally {
      this.busy = false;
      changed(this.snapshot());
      if (this.refreshPending) {
        this.refreshPending = false;
        await this.refreshLocal(changed);
      }
    }
  }

  async reload(changed: (snapshot: CreatorFeedSnapshot) => void): Promise<void> {
    if (this.busy) return;
    await this.refreshLocal(changed);
    if (!this.readRemote) return;
    this.busy = true;
    try {
      const page = await this.readRemote(null);
      this.remote = page.posts;
      this.cursor = page.next;
      this.remoteMore = page.next !== null;
      this.owned = new Set(page.owned.map(String));
      this.remoteError = undefined;
    } catch (error) { this.remoteError = error; }
    finally {
      this.busy = false;
      changed(this.snapshot());
      if (this.refreshPending) {
        this.refreshPending = false;
        await this.refreshLocal(changed);
      }
    }
  }

}
