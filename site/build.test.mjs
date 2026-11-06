// The site's build, held: the Markdown the guide writes, read as GitHub reads
// it; Pleris coloured without a change to its text; and the whole site, with
// no script, no style the policy did not hash, and no link to nowhere.
//
//   node --test site/build.test.mjs

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { STYLE_HASH, build } from "./build.mjs";
import { highlightPw, inline, renderMarkdown } from "./markdown.mjs";

test("a heading takes GitHub's id, and a repeated one is numbered", () => {
  const { html, headings } = renderMarkdown("# Where `pw check` stops\n\n## A *case*\n\n## A case");
  assert.deepEqual(
    headings.map((h) => h.id),
    ["where-pw-check-stops", "a-case", "a-case-1"],
  );
  assert.match(html, /<h1 id="where-pw-check-stops">Where <code>pw check<\/code> stops<\/h1>/);
});

test("a line's markup: code, links, emphasis and strong; snake_case is not emphasis", () => {
  const html = inline("a `x_y` and **b** *c* _d_ one_per_key [e](f.md#g) <https://h.dev>", {
    link: (u) => `L:${u}`,
  });
  assert.equal(
    html,
    'a <code>x_y</code> and <strong>b</strong> <em>c</em> <em>d</em> one_per_key <a href="L:f.md#g">e</a> <a href="https://h.dev">https://h.dev</a>',
  );
});

test("raw HTML in prose is text, and a comment is dropped", () => {
  const { html } = renderMarkdown("<!-- hidden -->\nA <script>alert(1)</script> & <b>.");
  assert.equal(html, "<p>A &lt;script&gt;alert(1)&lt;/script&gt; &amp; &lt;b&gt;.</p>");
});

test("code keeps its text, a backtick in it too", () => {
  assert.equal(inline("``a ` b``"), "<code>a ` b</code>");
  assert.equal(inline("`<title>`"), "<code>&lt;title&gt;</code>");
  assert.equal(inline("a `*b*` c"), "a <code>*b*</code> c");
});

test("a table: an escaped pipe, alignment as a class, a cell rewritten", () => {
  const { html } = renderMarkdown("| A | B |\n|---|--:|\n| `\\|` | x |", {
    cell: (c, column) => (column === 0 ? `[${c}]` : c),
  });
  assert.match(html, /<th scope="col">A<\/th><th scope="col" class="align-right">B<\/th>/);
  assert.match(html, /<td>\[<code>\|<\/code>\]<\/td><td class="align-right">x<\/td>/);
});

test("lists: ordered, tight, nested, with lines that continue an item", () => {
  const { html } = renderMarkdown("1. one\n   more\n2. two\n   - a\n   - b\n\n- x\n- y");
  assert.equal(
    html,
    "<ol>\n<li>one\nmore</li>\n<li>two\n<ul>\n<li>a</li>\n<li>b</li>\n</ul></li>\n</ol>\n<ul>\n<li>x</li>\n<li>y</li>\n</ul>",
  );
});

test("a fenced block is given its text, its info and the heading it falls under", () => {
  const seen = [];
  renderMarkdown("## Here\n\n```pw fails=PW0001\nmodule a\n```", {
    code: (text, info, context) => {
      seen.push([text, info, context.heading.id]);
      return "";
    },
  });
  assert.deepEqual(seen, [["module a", "pw fails=PW0001", "here"]]);
});

test("Pleris is coloured with spans alone: the text is the program", () => {
  const src = 'module a\n\nfn f(x: Int) -> String !{} {\n    // c\n    "s" <p>{x}</p> {#each xs as x (x)}\n}';
  const html = highlightPw(src);
  const text = html
    .replace(/<\/?span[^>]*>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
  assert.equal(text, src);
  assert.match(html, /<span class="k">module<\/span>/);
  assert.match(html, /<span class="t">Int<\/span>/);
  assert.match(html, /<span class="c">\/\/ c<\/span>/);
  assert.match(html, /<span class="tag">&lt;p<\/span>/);
});

test("the whole site: every page and code, no script, one hashed style, no link to nowhere", (t) => {
  const out = mkdtempSync(join(tmpdir(), "pleris-site-"));
  t.after(() => rmSync(out, { recursive: true, force: true }));
  const { written, catalog } = build(out);
  const pages = ["/", "/404", "/e/", "/e/404", "/guide/", "/guide/why", "/guide/tour", "/guide/tour-server"];
  for (const p of [...pages, "/guide/tour-pages", "/guide/errors", "/guide/tutorial", "/guide/diagnostics"]) {
    assert.ok(written.has(p), `no page at ${p}`);
  }
  assert.ok(catalog.active.length >= 200, `${catalog.active.length} codes`);
  for (const c of [...catalog.active, ...catalog.retired, ...catalog.renumbered]) {
    assert.ok(written.has(`/e/${c.code}`), `no page for ${c.code}`);
  }
  for (const [path, html] of written) {
    assert.doesNotMatch(html, /<script/i, path);
    assert.doesNotMatch(html, /\sstyle="/i, `${path}: a style attribute the policy refuses`);
    assert.match(html, /^<!doctype html>\n<html lang="en">/, path);
    assert.match(html, /<title>[^<]+<\/title>/, path);
    const style = /<style>([\s\S]*?)<\/style>/.exec(html)[1];
    assert.equal(`sha256-${createHash("sha256").update(style).digest("base64")}`, STYLE_HASH, path);
    for (const [, href] of html.matchAll(/href="([^"]+)"/g)) {
      if (/^https?:/.test(href) || href === "/favicon.svg") continue;
      const [target = "", hash] = href.split("#");
      const at = target === "" ? path : target;
      assert.ok(written.has(at), `${path} links to ${href}: no such page`);
      if (hash) assert.ok(written.get(at).includes(` id="${hash}"`), `${path} links to ${href}: no such anchor`);
    }
  }
  const headers = readFileSync(join(out, "_headers"), "utf8");
  assert.ok(headers.includes(`style-src '${STYLE_HASH}'`), headers);
  // The home page's sample is the guide's, with what the compiler prints.
  assert.match(written.get("/"), /error: \[PW0606\]/);
  // A code's page shows the guide's example the compiler refuses with it.
  assert.match(written.get("/e/PW5001"), /Refused: <a href="\/e\/PW5001">PW5001<\/a>/);
});
