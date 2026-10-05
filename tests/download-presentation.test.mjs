import assert from "node:assert/strict";
import test from "node:test";

import { jobHeading, jobProgress, jobQueryDetails } from "../src/features/downloads/presentation.ts";

const job = (overrides = {}) => ({
  id: 1,
  kind: "query",
  source: "fanbox",
  title: "creator:artist",
  query: "creator:artist",
  localFilter: null,
  maxPosts: null,
  status: "running",
  total: null,
  discovered: 0,
  saved: 0,
  skipped: 0,
  failed: 0,
  error: null,
  createdAt: 0,
  updatedAt: 0,
  ...overrides,
});

test("unknown totals keep reading until pagination supplies the final resource count", () => {
  assert.deepEqual(jobProgress(job()), { done: 0, knownTotal: false, reading: true, width: 0 });
  const firstPage = job({ discovered: 20, saved: 10, skipped: 3, failed: 2 });
  assert.deepEqual(jobProgress(firstPage), { done: 15, knownTotal: false, reading: true, width: 75 });
  assert.equal(jobProgress({ ...firstPage, discovered: 40 }).width, 37.5);
  const pageProcessed = job({ discovered: 20, saved: 15, skipped: 3, failed: 2 });
  assert.deepEqual(jobProgress(pageProcessed), { done: 20, knownTotal: false, reading: true, width: 100 });
  const finished = { ...pageProcessed, total: 20, status: "done" };
  assert.deepEqual(jobProgress(finished), { done: 20, knownTotal: true, reading: false, width: 100 });
});

test("paused, canceled, failed, and queued jobs retain counts without claiming to read the list", () => {
  for (const status of ["paused", "canceled", "failed", "queued"]) {
    assert.deepEqual(jobProgress(job({ status, discovered: 20, saved: 8, skipped: 1, failed: 1 })), {
      done: 10, knownTotal: false, reading: false, width: 50,
    });
  }
});

test("retry puts failed resources back into the unprocessed portion", () => {
  const completed = job({ total: 10, discovered: 10, status: "done", saved: 7, skipped: 1, failed: 2 });
  assert.equal(jobProgress(completed).width, 100);
  assert.deepEqual(jobProgress({ ...completed, status: "queued", failed: 0 }), {
    done: 8, knownTotal: true, reading: false, width: 80,
  });
});

test("known counts, limits, and empty terminal pages do not depend on source", () => {
  for (const source of ["fanbox", "danbooru", "gelbooru", "x"]) {
    const base = job({ source, discovered: 30, saved: 20, skipped: 5, failed: 5 });
    assert.deepEqual(jobProgress(base), { done: 30, knownTotal: false, reading: true, width: 100 });
    assert.equal(jobProgress({ ...base, total: 100 }).width, 30);
    assert.deepEqual(jobProgress({ ...base, maxPosts: 30, total: 30, status: "done" }), {
      done: 30, knownTotal: true, reading: false, width: 100,
    });
    const empty = jobProgress(job({ source, total: 0, status: "done" }));
    assert.equal(empty.width, 0);
    assert.equal(empty.knownTotal, true);
    assert.equal(empty.reading, false);
  }
});

test("legacy creator titles become readable and identical query lines disappear", () => {
  assert.equal(jobHeading(job()), "@artist");
  assert.equal(jobQueryDetails(job()), "");
  assert.equal(jobHeading(job({ title: "Creator Name" })), "Creator Name");
  assert.equal(jobQueryDetails(job({ title: "Creator Name" })), "");
  assert.equal(jobHeading(job({ source: "kemono", title: "creator:patreon/123" })), "patreon/123");
  assert.equal(jobQueryDetails(job({ source: "danbooru", title: "cat", query: "cat" })), "");
});

test("query compaction retains rating and ordering constraints", () => {
  assert.equal(jobQueryDetails(job({ query: "creator:artist rating:general" })), "rating:general");
  assert.equal(jobQueryDetails(job({ title: "Creator Name", query: "creator:artist rating:general" })), "rating:general");
  assert.equal(jobQueryDetails(job({ source: "danbooru", title: "cat", query: "cat rating:safe order:score" })), "rating:safe order:score");
  assert.equal(jobQueryDetails(job({ source: "danbooru", title: "cat", query: "catgirl rating:safe" })), "catgirl rating:safe");
  assert.equal(jobHeading(job({ title: "creator:artist rating:general" })), "@artist rating:general");
  assert.equal(jobQueryDetails(job({ source: "danbooru", title: "All posts", query: "" })), "");
});
