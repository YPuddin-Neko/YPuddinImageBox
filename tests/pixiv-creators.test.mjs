import assert from "node:assert/strict";
import test from "node:test";

import { mergePixivCreators, pixivCreatorQuery } from "../src/features/discover/pixivCreators.ts";

test("creator pagination keeps matching names with different IDs and updates repeated IDs", () => {
  const first = [
    { id: "22675109", name: "花咲ちゆ", avatarUrl: null },
    { id: "2", name: "同名作者", avatarUrl: null },
  ];
  const next = [
    { id: "22675109", name: "花咲ちゆ＠お仕事募集中", avatarUrl: "https://i.pximg.net/avatar.jpg" },
    { id: "3", name: "同名作者", avatarUrl: null },
  ];
  assert.deepEqual(mergePixivCreators(first, next), [next[0], first[1], next[1]]);
  assert.equal(first[0].name, "花咲ちゆ");
});

test("opening an artist uses the stable profile URL instead of a name tag", () => {
  const creator = { id: "22675109", name: "花咲 ちゆ＠お仕事募集中", avatarUrl: null };
  assert.equal(pixivCreatorQuery(creator), "https://www.pixiv.net/users/22675109");
  assert.equal(pixivCreatorQuery({ ...creator, name: "renamed" }), pixivCreatorQuery(creator));
});
