import assert from "node:assert/strict";
import test from "node:test";
import { FANBOX_NAMING_DEFAULTS, fanboxDownloadExamples } from "../src/features/settings/fanboxDownloadExamples.ts";

const settings = (values = {}) => ({ directory: null, ...FANBOX_NAMING_DEFAULTS, ...values });

test("default naming previews image, cover, and original attachment names", () => {
  assert.deepEqual(fanboxDownloadExamples(settings(), "/Pictures/fanbox"), {
    image: "/Pictures/fanbox/Artist/2026-10-06-October sketches/001.jpg",
    cover: "/Pictures/fanbox/Artist/2026-10-06-October sketches/000.jpg",
    attachment: "/Pictures/fanbox/Artist/2026-10-06-October sketches/source.psd",
  });
});

test("custom folder and filename fields use the selected directory and platform separators", () => {
  const result = fanboxDownloadExamples(settings({ directory: "D:\\FANBOX\\", folderTemplate: "{creator_id}/{postid}", imageTemplate: "{postid}-{index}" }), "/ignored");
  assert.equal(result.image, "D:\\FANBOX\\sample-artist\\12560223\\12560223-001.jpg");
  assert.equal(result.cover, "D:\\FANBOX\\sample-artist\\12560223\\12560223-000.jpg");
  assert.equal(fanboxDownloadExamples(settings({ directory: "/", folderTemplate: "{user}" }), "/ignored").attachment, "/Artist/source.psd");
});

test("incomplete or unsafe templates do not produce a misleading path preview", () => {
  for (const folderTemplate of ["", "{missing}", "{user", "../{user}", "/{user}", "C:/{user}", "a//b", "a/../b", "a\\b", "a\nb", Array(11).fill("a").join("/"), "a".repeat(501)]) {
    assert.equal(fanboxDownloadExamples(settings({ folderTemplate }), "/saved"), null);
  }
  for (const imageTemplate of ["", "{missing}", "{user}/{index}", "{user}\\{index}", ".", "..", "C:filename"]) {
    assert.equal(fanboxDownloadExamples(settings({ imageTemplate }), "/saved"), null);
  }
});

test("previews use the downloader's safe punctuation and reserved filename rules", () => {
  const result = fanboxDownloadExamples(settings({ folderTemplate: "{user}/ sketches: {title}?", imageTemplate: "CON", attachmentTemplate: "{name}~" }), "/Pictures/fanbox");
  assert.equal(result.image, "/Pictures/fanbox/Artist/sketches： October sketches？/_CON.jpg");
  assert.equal(result.attachment, "/Pictures/fanbox/Artist/sketches： October sketches？/source～.psd");
  const long = fanboxDownloadExamples(settings({ imageTemplate: "画".repeat(150) }), "/Pictures/fanbox");
  assert.equal(long.image.split("/").at(-1), `${"画".repeat(60)}.jpg`);
});
