import assert from "node:assert/strict";
import test from "node:test";

import { hasPixivIdCollision, mergePixivCreators, pixivCreatorQuery } from "../src/features/discover/pixivCreators.ts";

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

const collisionArtist = { id: "22675109", name: "花咲ちゆ＠お仕事募集中", avatarUrl: null };
const collisionWork = { source: "pixiv", id: 22675109000 };

test("a Pixiv numeric ID collision requires the matching artist and work", () => {
  const collision = (creators, posts) => hasPixivIdCollision(["pixiv"], "22675109", creators, posts);
  assert.equal(collision([collisionArtist], [collisionWork]), true);
  assert.equal(collision([collisionArtist], []), false);
  assert.equal(collision([], [collisionWork]), false);
  assert.equal(collision([{ ...collisionArtist, id: "2" }], [collisionWork]), false);
  assert.equal(collision([collisionArtist], [{ ...collisionWork, id: 22675110000 }]), false);
  assert.equal(collision([collisionArtist], [{ ...collisionWork, source: "danbooru" }]), false);
});

test("numeric ID collision accepts filters, leading zeros, and a later page of the matching work", () => {
  assert.equal(hasPixivIdCollision(
    ["pixiv"], " rating:general 00022675109 order:date sort:id:asc ",
    [{ ...collisionArtist, id: "00022675109" }], [{ ...collisionWork, id: 22675109007 }],
  ), true);
  assert.equal(hasPixivIdCollision(["pixiv"], "\t22675109\n", [collisionArtist], [collisionWork]), true);
  assert.equal(hasPixivIdCollision(["pixiv"], "22675109", [collisionArtist], [{ ...collisionWork, id: 22675109999 }]), true);
});

test("the collision filter stays hidden for explicit targets, names, and other query forms", () => {
  for (const query of [
    "", "rating:general", "花咲ちゆ＠お仕事募集中", "user:22675109", "id:22675109",
    "https://www.pixiv.net/users/22675109", "https://www.pixiv.net/artworks/22675109",
    "22675109 scenery", "22675109 22675109", "-22675109", "+22675109", "22675109.0", "2.2675109e7",
  ]) {
    assert.equal(hasPixivIdCollision(["pixiv"], query, [collisionArtist], [collisionWork]), false, query);
  }
  for (const sources of [[], ["danbooru"], ["pixiv", "danbooru"], ["pixiv", "pixiv"]]) {
    assert.equal(hasPixivIdCollision(sources, "22675109", [collisionArtist], [collisionWork]), false);
  }
});

test("invalid or imprecise post IDs cannot create an apparent collision", () => {
  for (const id of [NaN, Infinity, -22675109000, 22675109000.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(hasPixivIdCollision(["pixiv"], "22675109", [collisionArtist], [{ ...collisionWork, id }]), false);
  }
  assert.equal(hasPixivIdCollision(["pixiv"], "0", [{ ...collisionArtist, id: "0" }], [{ ...collisionWork, id: 0 }]), false);
  assert.equal(hasPixivIdCollision(["pixiv"], "22675109", [{ ...collisionArtist, id: "22675109x" }], [collisionWork]), false);
  assert.equal(hasPixivIdCollision(["pixiv"], "9007199254740993", [{ ...collisionArtist, id: "9007199254740993" }], [{ ...collisionWork, id: 9007199254740993000 }]), false);
});
