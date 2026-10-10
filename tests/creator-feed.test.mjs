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
  assert.deepEqual([...feed.snapshot().owned], ["4", "3"]);
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
  assert.deepEqual([...feed.snapshot().owned], ["2"]);
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
  assert.equal(feed.snapshot().owned.has("1"), false);
  assert.equal(feed.snapshot().posts[0].path, undefined);
  records = [local(1)];
  feed.saved(1);
  await feed.refreshLocal(() => {});
  assert.equal(feed.snapshot().owned.has("1"), true);
});

test("creator posts sort by article then cover and resource order, independent of stable IDs", () => {
  const resource = (id, article, downloadIndex) => ({ ...remote(id), postUrl: `https://www.fanbox.cc/@artist/posts/${article}`, downloadIndex });
  const older = resource("9223372036854775807", "42", 1);
  const cover = resource("5000000000000009", "43", 0);
  const first = resource("9000000000000005", "43", 1);
  const second = resource("5000000000000002", "43", 2);
  const third = resource("9223372036854775806", "43", 3);
  const merged = mergeCreatorPosts([older, third, second, cover, first], [{ ...local(first.id), postUrl: first.postUrl, downloadIndex: 3 }]);
  assert.deepEqual(merged.map((post) => post.id), [cover.id, first.id, second.id, third.id, older.id]);
  assert.equal(merged[1].path, `/saved/${first.id}`);
  const legacy = [43999, 43002, 43001, 43000].map((id) => ({
    ...resource(id, "43", undefined),
    fileUrl: id === 43999 ? "https://pixiv.pximg.net/fanbox/public/images/post/43/cover/abc.jpg" : null,
  }));
  assert.deepEqual(mergeCreatorPosts([], legacy).map((post) => post.id), [43999, 43000, 43001, 43002]);
});

test("number and string resource identities share one record and ownership", async () => {
  const feed = new CreatorFeed(async () => ({ posts: [local("2")], hasMore: false }),
    async () => ({ posts: [remote(2), remote("9223372036854775807")], next: null, owned: [2] }));
  await feed.more(() => {});
  assert.equal(feed.snapshot().posts.length, 2);
  assert.equal(feed.snapshot().posts.find((post) => String(post.id) === "2").path, "/saved/2");
  feed.removeSaved([2]);
  assert.equal(feed.snapshot().owned.has("2"), false);
  assert.equal(feed.snapshot().posts.find((post) => String(post.id) === "2").path, undefined);
  feed.saved("2");
  await feed.refreshLocal(() => {});
  assert.equal(feed.snapshot().owned.has("2"), true);
});

test("legacy local IDs merge with stable remote IDs and deletion clears their ownership alias", async () => {
  const online = { ...remote("9223372036854775807"), postUrl: "https://www.fanbox.cc/@artist/posts/42", downloadIndex: 2, fileUrl: "https://downloads.fanbox.cc/images/post/42/asset.png" };
  const saved = { ...local(42000), postUrl: online.postUrl, downloadIndex: 1, fileUrl: online.fileUrl };
  let records = [saved];
  const feed = new CreatorFeed(async () => ({ posts: records, hasMore: false }),
    async () => ({ posts: [online], owned: [online.id], next: null }));
  await feed.more(() => {});
  assert.equal(feed.snapshot().posts.length, 1);
  assert.equal(feed.snapshot().posts[0].id, 42000);
  assert.equal(feed.snapshot().posts[0].path, saved.path);
  assert.equal(feed.snapshot().posts[0].downloadIndex, 2);
  assert.deepEqual([...feed.snapshot().owned], ["42000"]);
  assert.equal(mergeCreatorPosts([online], [{ ...saved, missing: true }])[0].id, 42000);
  feed.removeSaved(["42000"]);
  records = [];
  await feed.refreshLocal(() => {});
  assert.equal(feed.snapshot().posts.length, 1);
  assert.equal(feed.snapshot().posts[0].id, online.id);
  assert.equal(feed.snapshot().posts[0].fileUrl, online.fileUrl);
  assert.equal(feed.snapshot().posts[0].path, undefined);
  assert.deepEqual([...feed.snapshot().owned], []);
  records = [{ ...online, path: "/saved/new", missing: false }];
  feed.saved(online.id);
  await feed.refreshLocal(() => {});
  assert.deepEqual([...feed.snapshot().owned], [online.id]);
});

test("missing or unrelated resource URLs never merge different IDs", () => {
  const online = { ...remote("9223372036854775807"), postUrl: "https://www.fanbox.cc/@artist/posts/42", fileUrl: "https://downloads.fanbox.cc/images/post/42/asset.png" };
  for (const overrides of [{ fileUrl: null }, { fileUrl: "" }, { fileUrl: "not a URL" }, { postUrl: "https://www.fanbox.cc/@artist/posts/43" }]) {
    assert.equal(mergeCreatorPosts([online], [{ ...local(42000), ...online, id: 42000, ...overrides }]).length, 2);
  }
});

test("a late stable-ID response cannot restore ownership of a deleted legacy resource", async () => {
  const online = { ...remote("9223372036854775807"), postUrl: "https://www.fanbox.cc/@artist/posts/42", fileUrl: "https://downloads.fanbox.cc/images/post/42/asset.png" };
  let records = [{ ...online, id: 42000, path: "/saved/legacy", missing: false }];
  let finish;
  const feed = new CreatorFeed(async () => ({ posts: records, hasMore: false }), async () => {
    await new Promise((resolve) => { finish = resolve; });
    return { posts: [online], owned: [online.id], next: null };
  });
  const loading = feed.more(() => {});
  await new Promise((resolve) => setImmediate(resolve));
  feed.removeSaved([42000]);
  records = [];
  finish();
  await loading;
  assert.equal(feed.snapshot().posts.length, 1);
  assert.equal(feed.snapshot().posts[0].fileUrl, online.fileUrl);
  assert.deepEqual([...feed.snapshot().owned], []);
});


test("legacy thousandth body resources stay after the body instead of becoming a cover", () => {
  const resources = [43999, 43000, 43001].map((id) => ({ ...local(id),
    postUrl: "https://www.fanbox.cc/@artist/posts/43", fileUrl: `https://downloads.fanbox.cc/images/${id}.jpg`,
  }));
  assert.deepEqual(mergeCreatorPosts([], resources).map((post) => post.id), [43000, 43001, 43999]);
});
