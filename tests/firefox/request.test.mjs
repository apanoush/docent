import assert from "node:assert/strict";
import test from "node:test";

import { buildRequest, cleanUrl, fileLabel } from "../../plugins/firefox/lib/request.js";

const capture = {
  selection: "a transformer",
  document: "A transformer maps a sequence to a sequence.",
  file: "Paper (https://example.com/paper)",
};

test("builds the request", () => {
  assert.deepEqual(buildRequest(capture), {
    selection: "a transformer",
    document: "A transformer maps a sequence to a sequence.",
    file: "Paper (https://example.com/paper)",
  });
});

test("adds a non-empty question", () => {
  assert.equal(buildRequest(capture, "Why?").question, "Why?");
  assert.equal("question" in buildRequest(capture, "   "), false);
  assert.equal("question" in buildRequest(capture, null), false);
});

test("omits an empty file label", () => {
  assert.equal("file" in buildRequest({ selection: "x", document: "y" }), false);
});

test("cleans page urls", () => {
  assert.equal(
    cleanUrl("https://user:secret@example.com/a/b?token=1#frag"),
    "https://example.com/a/b",
  );
  assert.equal(cleanUrl("https://example.com/path/?q=1"), "https://example.com/path/");
  assert.equal(cleanUrl("file:///home/you/page.html?q=1#x"), "file:///home/you/page.html");
});

test("keeps unparseable urls", () => {
  assert.equal(cleanUrl("not a url"), "not a url");
  assert.equal(cleanUrl(""), "");
});

test("builds the file label", () => {
  assert.equal(
    fileLabel("https://example.com/a?b=1", "Title"),
    "Title (https://example.com/a)",
  );
  assert.equal(fileLabel("", "Title"), "Title");
  assert.equal(fileLabel("https://example.com/a", ""), "https://example.com/a");
  assert.equal(fileLabel("", ""), "");
});
