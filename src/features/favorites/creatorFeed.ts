import type { Post } from "../../lib/ipc";
import type { LocalPost } from "../../lib/library";

export const localRecord = (post: Post): LocalPost | null => "path" in post ? post as LocalPost : null;

/** 同一资源优先读本地；本地文件缺失时仍可显示当前账号能访问的远程预览。 */
export function mergeCreatorPosts(remote: Post[], local: LocalPost[]): Post[] {
  const posts = new Map(remote.map((post) => [post.id, post]));
  for (const post of local) {
    const online = posts.get(post.id);
    posts.set(post.id, post.missing && online ? { ...post, ...online } : post);
  }
  return [...posts.values()].sort((a, b) => b.id - a.id);
}

interface LocalPage { posts: LocalPost[]; hasMore: boolean }
interface RemotePage { posts: Post[]; next: string | null; owned: number[] }
export interface CreatorFeedSnapshot {
  posts: Post[];
  owned: Set<number>;
  hasMore: boolean;
  errors: unknown[];
}

/** 两个来源独立翻页和报错，远程失权或断网不会清掉已经读取的本地记录。 */
export class CreatorFeed {
  private local: LocalPost[] = [];
  private remote: Post[] = [];
  private owned = new Set<number>();
  private removed = new Set<number>();
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
    const owned = new Set([...this.owned].filter((id) => !this.removed.has(id)));
    const local = this.local.filter((post) => !this.removed.has(post.id));
    local.forEach((post) => post.missing ? owned.delete(post.id) : owned.add(post.id));
    return {
      posts: mergeCreatorPosts(this.remote, local), owned,
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
          page.owned.forEach((id) => this.owned.add(id));
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

  removeSaved(ids: number[]): void {
    const removed = new Set(ids);
    ids.forEach((id) => { this.owned.delete(id); this.removed.add(id); });
    this.local = this.local.filter((post) => !removed.has(post.id));
  }

  saved(id: number): void { this.removed.delete(id); }

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
      this.owned = new Set(page.owned);
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
