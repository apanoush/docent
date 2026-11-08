import assert from "node:assert/strict";
import test from "node:test";

import { render } from "../../plugins/firefox/lib/markdown.js";

test("blank input returns an empty fragment", () => {
  assert.equal(render(""), "");
  assert.equal(render(" \n\t\n"), "");
});

test("paragraphs and soft wraps", () => {
  assert.equal(render("one\ntwo\n\nthree"), "<p>one two</p>\n<p>three</p>");
});

test("headings", () => {
  assert.equal(render("# One"), "<h1>One</h1>");
  assert.equal(render("###### Six"), "<h6>Six</h6>");
  assert.equal(render("####### seven"), "<p>####### seven</p>");
});

test("inline formatting", () => {
  assert.equal(
    render("**bold** *ital* ~~gone~~ `code`"),
    "<p><strong>bold</strong> <em>ital</em> <del>gone</del> <code>code</code></p>",
  );
});

test("nested inline formatting", () => {
  assert.equal(
    render("**bold with `code`**"),
    "<p><strong>bold with <code>code</code></strong></p>",
  );
});

test("links open in a new tab", () => {
  assert.equal(
    render("[docs](https://example.com/a?b=1&c=2)"),
    '<p><a href="https://example.com/a?b=1&amp;c=2" target="_blank" rel="noopener noreferrer">docs</a></p>',
  );
});

test("link labels are rendered inline", () => {
  const result = render("[**bold**](https://example.com)");
  assert.match(result, /<strong>bold<\/strong>/);
  assert.match(result, /<a href="https:\/\/example\.com"/);
});

test("mailto links are allowed", () => {
  assert.match(render("[mail](mailto:a@example.com)"), /href="mailto:a@example\.com"/);
});

for (const url of [
  "javascript:alert(1)",
  "data:text/html,x",
  "/relative/path",
  "//example.com/x",
]) {
  test(`unsafe link renders as text: ${url}`, () => {
    const result = render(`[x](${url})`);
    assert.doesNotMatch(result, /<a /);
    assert.match(result, /x \(/);
  });
}

test("malformed urls render as text", () => {
  const result = render("[x](http://[::1)");
  assert.doesNotMatch(result, /<a /);
  assert.match(result, /x \(/);
});

test("link urls cannot break out of the attribute", () => {
  const result = render('[x](https://example.com/"onmouseover="alert)');
  assert.doesNotMatch(result, /onmouseover="/);
  assert.match(result, /&quot;onmouseover=&quot;/);
});

test("images render as alt text", () => {
  assert.equal(render("![diagram](https://example.com/x.png)"), "<p>diagram</p>");
});

test("raw html is escaped", () => {
  const result = render('<script>alert("x")</script>');
  assert.doesNotMatch(result, /<script>/);
  assert.match(result, /&lt;script&gt;/);
});

test("code is escaped", () => {
  assert.equal(render("`<b>`"), "<p><code>&lt;b&gt;</code></p>");
});

test("fenced code block", () => {
  assert.equal(
    render("```python\nprint('<x>')\n```"),
    '<pre><code class="language-python">print(\'&lt;x&gt;\')</code></pre>',
  );
});

test("unclosed fence renders to the end", () => {
  assert.equal(render("```\nx"), "<pre><code>x</code></pre>");
});

test("unordered list", () => {
  assert.equal(render("- one\n- two"), "<ul><li>one</li><li>two</li></ul>");
});

test("ordered list", () => {
  assert.equal(render("1. one\n2. two"), "<ol><li>one</li><li>two</li></ol>");
});

test("blank line ends a list", () => {
  assert.equal(render("- one\n\ntext"), "<ul><li>one</li></ul>\n<p>text</p>");
});

test("blockquote renders inner blocks", () => {
  assert.equal(
    render("> ## Title\n> text"),
    "<blockquote><h2>Title</h2>\n<p>text</p></blockquote>",
  );
});

test("text after a blockquote is a paragraph", () => {
  assert.equal(
    render("> quote\nafter"),
    "<blockquote><p>quote</p></blockquote>\n<p>after</p>",
  );
});

test("horizontal rules", () => {
  assert.equal(render("---"), "<hr>");
  assert.equal(render("* * *"), "<hr>");
});

test("table with alignment", () => {
  const result = render("| Name | Count |\n|:-----|------:|\n| a | 1 |\n| b | 2 |");
  assert.match(result, /<th style="text-align: left">Name<\/th>/);
  assert.match(result, /<th style="text-align: right">Count<\/th>/);
  assert.match(result, /<td style="text-align: left">a<\/td>/);
  assert.match(result, /<td style="text-align: right">1<\/td>/);
  assert.match(result, /<tbody><tr>/);
});

test("table without outer pipes", () => {
  const result = render("a | b\n--- | ---\n1 | 2");
  assert.match(result, /<table>/);
  assert.match(result, /<th>a<\/th>/);
  assert.match(result, /<td>1<\/td>/);
});

test("table cells render inline markdown", () => {
  const result = render("| **a** | `b` |\n|---|---|");
  assert.match(result, /<strong>a<\/strong>/);
  assert.match(result, /<code>b<\/code>/);
});

test("table cell escaped pipe", () => {
  assert.match(render("| a | b |\n|---|---|\n| x \\| y | z |"), /x \| y/);
});

test("pipe text without a separator stays a paragraph", () => {
  assert.equal(render("a | b"), "<p>a | b</p>");
  assert.equal(render("a | b\n---"), "<p>a | b</p>\n<hr>");
});

test("table stops at a blank line", () => {
  const result = render("| a | b |\n|---|---|\n| 1 | 2 |\n\nafter");
  assert.match(result, /<td>1<\/td>/);
  assert.match(result, /<p>after<\/p>/);
  assert.doesNotMatch(result, /after<\/td>/);
});

test("table column counts must match", () => {
  assert.doesNotMatch(render("| a | b | c |\n|---|---|"), /<table>/);
});

test("table needs at least two columns", () => {
  assert.doesNotMatch(render("| a |\n|---|"), /<table>/);
});

test("invalid separator cells are not a table", () => {
  assert.doesNotMatch(render("| a | b |\n|---| nope |"), /<table>/);
});

test("centered table column", () => {
  const result = render("| a | b |\n|:---:|:---:|\n| 1 | 2 |");
  assert.match(result, /<th style="text-align: center">a<\/th>/);
  assert.match(result, /<td style="text-align: center">1<\/td>/);
});
