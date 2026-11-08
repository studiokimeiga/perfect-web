// The site's build, held: the Markdown the guide writes, read as GitHub reads
// it; Pleris coloured without a change to its text; and the whole site, with
// no script, no style the policy did not hash, and no link to nowhere.
//
//   node --test site/build.test.mjs

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { SPEED, STYLE_HASH, build, checked, measured, speed, standing, summaryOf } from "./build.mjs";
import { escapeHtml, highlightPw, inline, renderMarkdown } from "./markdown.mjs";

/** The head-to-head's record, as the site reads it. The tests of what the
 * pages show from it take it as the build does (`checked`), with links of
 * their own, so they need no git. */
const RECORD = JSON.parse(readFileSync(new URL("../docs/evidence/E14/head-to-head.json", import.meta.url), "utf8"));
const shown = () => ({ ...checked(structuredClone(RECORD)), link: "https://example.test/record", commands: "https://example.test/commands" });
const counted = (verdict) => RECORD.rows.filter((r) => r.verdict === verdict).length;

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

test("a page's description is its first paragraph's text, each entity read once", () => {
  // `&amp;lt;` is the text `&lt;`: read twice, it would be `<`.
  const html = "<h1>T</h1>\n<p>A <code>&amp;lt;b&amp;gt;</code> tag &amp; <em>more</em>.</p>";
  assert.equal(summaryOf(html, "T"), "A &lt;b&gt; tag & more.");
  assert.equal(summaryOf("<h1>T</h1>", "the title"), "the title");
});

test("the whole site: every page and code, no script, one hashed style, no link to nowhere", (t) => {
  const out = mkdtempSync(join(tmpdir(), "pleris-site-"));
  t.after(() => rmSync(out, { recursive: true, force: true }));
  const { written, catalog } = build(out);
  assertEveryPage(written, catalog);
  for (const [path, html] of written) assertPage(path, html, written, out);
  const headers = readFileSync(join(out, "_headers"), "utf8");
  assert.ok(headers.includes(`style-src '${STYLE_HASH}'`), headers);
  assertForAgents(out);
  // The home page shows each mistake as the compiler refuses it, in the
  // words the guide's test holds it to, with the help that repairs it.
  const homePage = written.get("/");
  for (const code of ["PW5001", "PW5003", "PW5309"]) {
    assert.match(homePage, new RegExp(`error: \\[${code}\\]`), code);
  }
  assert.equal((homePage.match(/= help: /g) ?? []).length, 3, "one help line a refusal");
  // The hero states where Pleris stands over the whole record, and the
  // record's every row is on /measured (the tests below hold both to it).
  assert.ok(homePage.includes(standing(shown())), "the home page states no standing, or another");
  assert.ok(written.get("/measured").includes("<h1>Every row measured</h1>"));
  // A code's page shows the guide's example the compiler refuses with it.
  assert.match(written.get("/e/PW5001"), /Refused: <a href="\/e\/PW5001">PW5001<\/a>/);
});

// --- what the pages show from the head-to-head's record --------------------
//
// These read the record and the pages' HTML, and judge each row's marks
// themselves: a page that shows a row otherwise than the instrument judged
// it fails them. None builds the whole site.

test("the standing the home page states is the whole record's: rows ahead, and rows behind", () => {
  assert.equal(
    standing(shown()),
    `Of the ${RECORD.rows.length} rows its head-to-head measures, it is ahead in ${counted("win")} and behind in ${counted("loss")}: those ${counted("loss")} rows are the work.`,
  );
});

test("the home page's tables are the record's rows, under the page's names, each marked as the instrument judged it", () => {
  const tables = tablesOf(speed(shown()));
  assert.equal(tables.length, SPEED.length);
  SPEED.forEach((t, i) => {
    const chosen = t.rows.map(([label, what]) => [label, RECORD.rows.find((r) => r.app === t.app && r.what === what)]);
    for (const [label, r] of chosen) assert.ok(r, `${t.app}: ${label}: no such row in the record`);
    assertTable(tables[i], chosen);
  });
});

test("/measured shows every row of the record, in the instrument's words, each marked as it judged it", () => {
  const tables = tablesOf(measured(shown()).body);
  const apps = SPEED.map((t) => t.app);
  assert.deepEqual([...new Set(RECORD.rows.map((r) => r.app))].sort(), [...apps].sort());
  assert.equal(tables.length, apps.length);
  apps.forEach((app, i) => assertTable(tables[i], RECORD.rows.filter((r) => r.app === app).map((r) => [r.what, r])));
  assert.equal(tables.reduce((n, table) => n + table.rows.length, 0), RECORD.rows.length);
});

test("a record the instrument refused, one measured on a changed tree, or a loss that names no stack, is shown on no page", () => {
  assert.doesNotThrow(() => checked(structuredClone(RECORD)));
  assert.throws(() => checked({ ...structuredClone(RECORD), problems: ["a cell of one run"] }), /refused its run: a cell of one run/);
  assert.throws(() => checked({ ...structuredClone(RECORD), dirty: "uncommitted changes" }), /measured on a tree changed from/);
  const loss = { ...structuredClone(RECORD.rows[0]), verdict: "loss", better: [] };
  assert.throws(() => checked({ ...structuredClone(RECORD), rows: [loss] }), /a loss that names no stack/);
});

/** A page's tables: each one's head, and its rows' cells, each cell's text
 * and whether it is marked. */
function tablesOf(html) {
  const tables = html.matchAll(/<table class="bench"><thead><tr>(.*?)<\/tr><\/thead><tbody>\n([\s\S]*?)\n<\/tbody>/g);
  return [...tables].map(([, head, body]) => ({
    head: [...head.matchAll(/<th scope="col">(.*?)<\/th>/g)].map(([, h]) => h),
    rows: body.split("\n").map((tr) => [...tr.matchAll(/<td( class="best")?>(.*?)<\/td>/g)].map(([, best, text]) => ({ text, marked: Boolean(best) }))),
  }));
}

/** A table holds these rows of the record (`[label, row]`): a column for each
 * stack they name, in order, then who is ahead; each value as the record
 * shows it, marked where the instrument found its stack ahead. */
function assertTable(table, rows) {
  const stacks = [...new Set(rows.flatMap(([, r]) => r.values.map((v) => v.label)))];
  assert.deepEqual(table.head, ["Measured", ...stacks, "Ahead"]);
  const expected = rows.map(([label, r]) => {
    const ahead = r.verdict === "win" ? ["Pleris"] : r.verdict === "loss" ? r.better : [];
    return [
      { text: escapeHtml(label), marked: false },
      ...stacks.map((s) => ({ text: escapeHtml(r.values.find((v) => v.label === s)?.shown ?? "–"), marked: ahead.includes(s) })),
      { text: ahead.length ? escapeHtml(ahead.join(", ")) : "None", marked: false },
    ];
  });
  assert.deepEqual(table.rows, expected);
}

/** Each page of the guide, the site's own, and one for every code. */
function assertEveryPage(written, catalog) {
  const pages = ["/", "/404", "/measured", "/e/", "/e/404", "/guide/", "/guide/why", "/guide/tour", "/guide/tour-server"];
  for (const p of [...pages, "/guide/tour-pages", "/guide/errors", "/guide/tutorial", "/guide/diagnostics"]) {
    assert.ok(written.has(p), `no page at ${p}`);
  }
  assert.ok(catalog.active.length >= 200, `${catalog.active.length} codes`);
  for (const c of [...catalog.active, ...catalog.retired, ...catalog.renumbered]) {
    assert.ok(written.has(`/e/${c.code}`), `no page for ${c.code}`);
  }
}

/** A page: no script, no style attribute, the one hashed style, a title,
 * and no link to nowhere. */
function assertPage(path, html, written, out) {
  assert.doesNotMatch(html, /<script/i, path);
  assert.doesNotMatch(html, /\sstyle="/i, `${path}: a style attribute the policy refuses`);
  assert.match(html, /^<!doctype html>\n<html lang="en">/, path);
  assert.match(html, /<title>[^<]+<\/title>/, path);
  const style = /<style>([\s\S]*?)<\/style>/.exec(html)[1];
  assert.equal(`sha256-${createHash("sha256").update(style).digest("base64")}`, STYLE_HASH, path);
  for (const [, href] of html.matchAll(/href="([^"]+)"/g)) assertLink(path, href, written, out);
}

const external = (href) => /^https?:/.test(href) || href === "/favicon.svg";

/** A link to another site, or to a file the build wrote that is no page,
 * such as `llms.txt`. */
const elsewhere = (href, out) => external(href) || /\.(?:txt|md|xml)$/.test(href) && existsSync(join(out, href));

/** A link to another site, to a file the build wrote, or to a page written
 * here and the anchor it names. */
function assertLink(path, href, written, out) {
  if (elsewhere(href, out)) return;
  const [target, hash] = href.split("#");
  const at = target || path;
  assert.ok(written.has(at), `${path} links to ${href}: no such page`);
  assert.ok(!hash || written.get(at).includes(` id="${hash}"`), `${path} links to ${href}: no such anchor`);
}

/** What an agent reads: llms.txt, a title first, and each page it names
 * written, as the guide's Markdown with its links made whole. */
function assertForAgents(out) {
  const llms = readFileSync(join(out, "llms.txt"), "utf8");
  assert.match(llms, /^# Pleris\n/);
  for (const [, url] of llms.matchAll(/\]\((https:\/\/pleris\.dev\/[^)]+)\)/g)) {
    const path = new URL(url).pathname;
    const file = path.endsWith("/") ? `${path}index.html` : path;
    assert.ok(existsSync(join(out, file)), `llms.txt names ${url}: not written`);
  }
  const why = readFileSync(join(out, "guide/why.md"), "utf8");
  assert.doesNotMatch(why, /\]\((?![a-z]+:|#)/, "a relative link left in the guide's Markdown");
  assert.match(why, /\]\(https:\/\/pleris\.dev\/guide\/tour\.md\)/);
}
