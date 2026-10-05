import assert from "node:assert/strict";
import test from "node:test";
import { CreatorFeed, mergeCreatorPosts } from "../src/features/favorites/creatorFeed.ts";

const remote = (id) => ({ id, source: "fanbox", sampleUrl: `https://example.invalid/${id}`, title: `post ${id}` });
const local = (id, missing = false) => ({ ...remote(id), path: `/saved/${id}`, sampleUrl: `local/file/fanbox/${id}`, missing });
const offline = { code: "network", message: "offline" };

test("merges by resource identity, keeps order and prefers saved originals", () => {
  const merged = mergeCreatorPosts([remote(4), remote(2)], [local(2), local(3)]);
  assert.deepEqual(merged.map((p) => p.id), [4, 3, 2]);
  assert.equal(merged[2].sampleUrl, "local/file/fanbox/2");
  assert.equal(mergeCreatorPosts([remote(2)], [local(2, true)])[0].sampleUrl, "https://example.invalid/2");
});

test("offline pagination retains saved images and attachments across pages", async () => {
  const calls = [];
  const feed = new CreatorFeed(async (offset) => {
    calls.push(offset);
    return { posts: offset === 0 ? [local(4), { ...local(3), fileExt: "zip", fileName: "archive.zip" }] : [local(2, true)], hasMore: offset === 0 };
  }, async () => { throw offline; });
  await feed.more(() => {});
  assert.deepEqual(feed.snapshot().posts.map((p) => p.id), [4, 3]);
  assert.equal(feed.snapshot().hasMore, true);
  await feed.more(() => {});
  assert.deepEqual(calls, [0, 2]);
  assert.deepEqual([...feed.snapshot().owned], [4, 3]);
  assert.equal(feed.snapshot().hasMore, false);
  assert.deepEqual(feed.snapshot().errors, [offline]);
});

test("empty restricted remote page continues to the next cursor", async () => {
  const cursors = [];
  const feed = new CreatorFeed(async () => ({ posts: [local(3)], hasMore: false }), async (cursor) => {
    cursors.push(cursor);
    return cursor === null ? { posts: [], owned: [], next: "page-2" } : { posts: [remote(4), remote(3)], owned: [3], next: null };
  });
  await feed.more(() => {});
  await feed.more(() => {});
  assert.deepEqual(cursors, [null, "page-2"]);
  assert.deepEqual(feed.snapshot().posts.map((p) => p.id), [4, 3]);
  assert.equal(feed.snapshot().posts[1].sampleUrl, "local/file/fanbox/3");
});

test("saved-only mode never needs a remote loader", async () => {
  const feed = new CreatorFeed(async () => ({ posts: [local(1)], hasMore: false }), null);
  await feed.more(() => {});
  assert.equal(feed.snapshot().posts.length, 1);
  assert.equal(feed.snapshot().errors.length, 0);
});

test("library changes retain the loaded window and do not fetch remote again", async () => {
  let records = Array.from({ length: 85 }, (_, i) => local(100 - i));
  let remoteCalls = 0;
  const feed = new CreatorFeed(async (offset) => ({ posts: records.slice(offset, offset + 40), hasMore: offset + 40 < records.length }), async () => {
    remoteCalls++;
    return { posts: [remote(101)], owned: [], next: null };
  });
  await feed.more(() => {});
  await feed.more(() => {});
  records = records.filter((p) => p.id !== 99);
  await feed.refreshLocal(() => {});
  assert.equal(feed.snapshot().posts.length, 81);
  assert.equal(feed.snapshot().posts.some((p) => p.id === 99), false);
  assert.equal(remoteCalls, 1);
  assert.equal(feed.snapshot().hasMore, true);
});

test("failed refresh never clears previously loaded local records", async () => {
  let fail = false;
  const feed = new CreatorFeed(async () => {
    if (fail) throw offline;
    return { posts: [local(3)], hasMore: false };
  }, async () => { throw offline; });
  await feed.more(() => {});
  fail = true;
  const frames = [];
  await feed.reload((snapshot) => frames.push(snapshot.posts.length));
  assert.deepEqual(frames, [1, 1]);
});

test("a library event during remote refresh is applied after that refresh", async () => {
  let records = [local(1)];
  let complete;
  let calls = 0;
  const feed = new CreatorFeed(async () => ({ posts: records, hasMore: false }), async () => {
    if (calls++ > 0) await new Promise((resolve) => { complete = resolve; });
    return { posts: [], next: null, owned: [] };
  });
  await feed.more(() => {});
  const reload = feed.reload(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  records = [local(2), local(1)];
  await feed.refreshLocal(() => {});
  complete();
  await reload;
  assert.deepEqual(feed.snapshot().posts.map((post) => post.id), [2, 1]);
});

test("deleting a saved resource clears remote ownership without hiding its remote post", async () => {
  let records = [local(1)];
  const feed = new CreatorFeed(async () => ({ posts: records, hasMore: false }), async () => ({ posts: [remote(1), remote(2)], next: null, owned: [1, 2] }));
  await feed.more(() => {});
  records = [];
  feed.removeSaved([1]);
  await feed.refreshLocal(() => {});
  assert.deepEqual(feed.snapshot().posts.map((post) => post.id), [2, 1]);
  assert.deepEqual([...feed.snapshot().owned], [2]);
  assert.equal(feed.snapshot().posts[1].path, undefined);
});

test("an in-flight remote result cannot restore ownership after deletion", async () => {
  let complete;
  let records = [local(1)];
  const feed = new CreatorFeed(async () => ({ posts: records, hasMore: false }), async () => {
    await new Promise((resolve) => { complete = resolve; });
    return { posts: [remote(1)], next: null, owned: [1] };
  });
  const loading = feed.more(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  feed.removeSaved([1]);
  records = [];
  complete();
  await loading;
  assert.equal(feed.snapshot().owned.has(1), false);
  assert.equal(feed.snapshot().posts[0].path, undefined);
  records = [local(1)];
  feed.saved(1);
  await feed.refreshLocal(() => {});
  assert.equal(feed.snapshot().owned.has(1), true);
});
