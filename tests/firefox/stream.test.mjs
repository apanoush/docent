import assert from "node:assert/strict";
import test from "node:test";

import { NdjsonParser, StreamError, parseEvent } from "../../plugins/firefox/lib/stream.js";

test("parses one event", () => {
  assert.deepEqual(parseEvent('{"type":"delta","text":"Hello"}'), {
    type: "delta",
    text: "Hello",
  });
});

test("blank lines are ignored", () => {
  assert.equal(parseEvent(""), null);
  assert.equal(parseEvent("   "), null);
});

test("invalid json raises", () => {
  assert.throws(() => parseEvent("not json"), StreamError);
});

test("non-object json raises", () => {
  assert.throws(() => parseEvent('"just a string"'), /expected a JSON object/);
  assert.throws(() => parseEvent("[1,2]"), /expected a JSON object/);
  assert.throws(() => parseEvent("null"), /expected a JSON object/);
});

test("multiple complete lines are returned", () => {
  const parser = new NdjsonParser();
  const events = parser.feed(
    '{"type":"delta","text":"Hello"}\n{"type":"delta","text":" world"}\n',
  );
  assert.deepEqual(events, [
    { type: "delta", text: "Hello" },
    { type: "delta", text: " world" },
  ]);
});

test("a partial line is buffered", () => {
  const parser = new NdjsonParser();
  const payload = '{"type":"delta","text":"Hello"}\n';
  assert.deepEqual(parser.feed(payload.slice(0, -1)), []);
  assert.deepEqual(parser.feed("\n"), [{ type: "delta", text: "Hello" }]);
});

test("flush parses a final line without a newline", () => {
  const parser = new NdjsonParser();
  assert.deepEqual(parser.feed('{"type":"done"}'), []);
  assert.deepEqual(parser.flush(), [{ type: "done" }]);
  assert.deepEqual(parser.flush(), []);
});

test("a surrogate pair split across chunks is restored", () => {
  const payload = '{"type":"delta","text":"\u{1F600}"}\n';
  const index = payload.indexOf("\u{1F600}") + 1;
  const parser = new NdjsonParser();
  assert.deepEqual(parser.feed(payload.slice(0, index)), []);
  assert.deepEqual(parser.feed(payload.slice(index)), [
    { type: "delta", text: "\u{1F600}" },
  ]);
});
