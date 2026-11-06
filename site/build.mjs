#!/usr/bin/env node
// **pleris.dev, built from the repository** (site/README.md): the guide
// (docs/guide/*.md), a page for each diagnostic code (docs/guide/diagnostics.md,
// which scripts/guide_diagnostics.py writes from the compiler's registry) and
// the home page, into static files that Cloudflare serves as they are. No
// script runs in a reader's browser, and the build has no dependency.
//
//   node site/build.mjs [out]      out: site/dist

import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { cellsOf, escapeHtml, highlightPw, inline, renderMarkdown } from "./markdown.mjs";

const SITE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(SITE, "..");
const ORIGIN = "https://pleris.dev";
const REPO = "https://github.com/studiokimeiga/perfect-web";
const BLOB = `${REPO}/blob/master`;
const TREE = `${REPO}/tree/master`;

/** The guide's pages, in its own reading order (docs/guide/README.md). */
const GUIDE = [
  { name: "README", path: "/guide/", short: "Start here", about: "what the guide covers, and the order to read it in" },
  { name: "why", path: "/guide/why", short: "Why Pleris", about: "what the compiler refuses that other frameworks let through, and what Pleris does not do yet" },
  { name: "tour", path: "/guide/tour", short: "The language", about: "modules, types, records, sum types, functions, effects, results and money" },
  {
    name: "tour-server",
    path: "/guide/tour-server",
    short: "Data on the server",
    about: "host bindings, sources, queries and live updates, commands, predicates, privacy and time",
  },
  {
    name: "tour-pages",
    path: "/guide/tour-pages",
    short: "Pages",
    about: "routes and query parameters, markup, signals and handlers, forms, layouts and navigation",
  },
  { name: "errors", path: "/guide/errors", short: "Errors and diagnostics", about: "reading what the compiler says, and the first mistakes a TypeScript developer makes" },
  { name: "tutorial", path: "/guide/tutorial", short: "Tutorial: a guestbook", about: "one small app with sign-in, built end to end and run on the development server" },
  { name: "diagnostics", path: "/guide/diagnostics", short: "The diagnostic codes", about: "every code, its rule and its ADR" },
];

/** The file a path is served from (`html_handling`'s `auto-trailing-slash`). */
const fileOf = (path) => (path.endsWith("/") ? `${path}index.html` : `${path}.html`).slice(1);

/** A link that leaves the guide's own files: to another scheme, another
 * host, or a place on its own page. */
const ABSOLUTE = /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i;

/** A link the guide writes, as the site serves it: a page of the guide is the
 * site's, anything else in the repository is GitHub's. */
function guideLink(url) {
  if (ABSOLUTE.test(url)) return url;
  const [path, hash] = url.split("#");
  const anchor = hash ? `#${hash}` : "";
  const page = guidePageAt(path);
  return page ? page.path + anchor : repositoryLink(path, anchor);
}

/** The page of the guide a relative link names, if it names one. */
function guidePageAt(path) {
  return GUIDE.find((p) => path === `${p.name}.md` || path === `./${p.name}.md`);
}

function repositoryLink(path, anchor) {
  const target = posix.normalize(posix.join("docs/guide", path));
  return `${path.endsWith("/") ? TREE : BLOB}/${target}${anchor}`;
}

// --- the stylesheet, one for every page, written into each (no request) ----

const CSS = `
:root{color-scheme:light dark;--bg:#fbfbf9;--fg:#18191d;--muted:#575d6b;--line:#e3e3de;--soft:#f3f3ef;--accent:#0a7560;--accent-fg:#fff;--code:#f2f2ee;--k:#6d28d9;--t:#0a7560;--s:#a24a00;--c:#6b7280;--n:#b5175a;--tag:#1d5fd1;--max:74rem}
@media (prefers-color-scheme:dark){:root{--bg:#0e1014;--fg:#e9eaee;--muted:#a4a9b6;--line:#262a33;--soft:#151920;--accent:#3fd7ae;--accent-fg:#052a20;--code:#141820;--k:#c3a6ff;--t:#5fe1bd;--s:#f4b45c;--c:#8d95a6;--n:#ff8db8;--tag:#7fb4ff}}
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%;text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.65 system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif}
a{color:var(--accent);text-underline-offset:.18em}
a:hover{text-decoration-thickness:2px}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:3px}
.skip{position:absolute;left:-9999px}
.skip:focus{left:1rem;top:1rem;z-index:9;background:var(--bg);padding:.5rem .9rem;border:1px solid var(--line)}
.top{border-bottom:1px solid var(--line)}
.top nav{max-width:var(--max);margin:0 auto;padding:.85rem 1.25rem;display:flex;gap:1.4rem;align-items:center;flex-wrap:wrap}
.brand{display:flex;align-items:center;gap:.5rem;margin-right:auto;font-weight:700;font-size:1.1rem;color:var(--fg);text-decoration:none;letter-spacing:-.01em}
.brand svg{width:1.45rem;height:1.45rem}
.top nav a:not(.brand){color:var(--muted);text-decoration:none}
.top nav a:not(.brand):hover,.top nav a[aria-current]{color:var(--fg)}
main{max-width:var(--max);margin:0 auto;padding:2.5rem 1.25rem 4rem}
h1,h2,h3{line-height:1.25;letter-spacing:-.015em}
h2{margin-top:2.6rem}
code,pre,kbd{font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace}
:not(pre)>code{font-size:.88em;background:var(--code);padding:.1em .35em;border-radius:5px;overflow-wrap:anywhere}
pre{margin:0;background:var(--code);padding:1rem 1.1rem;overflow-x:auto;font-size:.84rem;line-height:1.55;border-radius:0 0 10px 10px}
figure.code{margin:1.4rem 0;border:1px solid var(--line);border-radius:10px;overflow:hidden;background:var(--code)}
figure.code>pre{border-radius:0}
figure.code figcaption{display:flex;flex-wrap:wrap;gap:.4rem 1rem;padding:.45rem 1.1rem;border-bottom:1px solid var(--line);font-size:.8rem;color:var(--muted);background:var(--soft)}
figure.code figcaption .refused{color:var(--n)}
figure.code figcaption .refused a{color:inherit}
figure.output pre{color:var(--fg)}
.table{overflow-x:auto;margin:1.2rem 0;border:1px solid var(--line);border-radius:10px}
table{border-collapse:collapse;width:100%;font-size:.92rem}
th,td{padding:.55rem .75rem;text-align:left;vertical-align:top;border-bottom:1px solid var(--line)}
tbody tr:last-child td{border-bottom:0}
th{background:var(--soft);font-weight:600}
.align-right{text-align:right}.align-center{text-align:center}
.k{color:var(--k)}.t{color:var(--t)}.s{color:var(--s)}.c{color:var(--c);font-style:italic}.n{color:var(--n)}.tag{color:var(--tag)}
.brand rect{fill:var(--accent)}.brand path{stroke:var(--bg)}
.doc{display:grid;grid-template-columns:14.5rem minmax(0,1fr);gap:3rem}
.doc>nav{position:sticky;top:1.5rem;align-self:start;font-size:.94rem}
.doc>nav ol{list-style:none;margin:0;padding:0}
.doc>nav li{margin:.1rem 0}
.doc>nav a{display:block;padding:.3rem .6rem;border-radius:6px;color:var(--muted);text-decoration:none}
.doc>nav a:hover{color:var(--fg);background:var(--soft)}
.doc>nav a[aria-current]{color:var(--fg);background:var(--soft);font-weight:600}
.prose{max-width:47rem;min-width:0}
.prose h1{font-size:2.3rem;margin-top:0}
.prose li{margin:.25rem 0}
.pager{display:flex;justify-content:space-between;gap:1rem;margin-top:3.5rem;padding-top:1.25rem;border-top:1px solid var(--line)}
.pager a{text-decoration:none}
.pager .next{text-align:right}
.prose.wide{max-width:none}
.pager small{display:block;color:var(--muted)}
.edit{margin-top:1rem;font-size:.9rem;color:var(--muted)}
@media (max-width:56rem){.doc{grid-template-columns:minmax(0,1fr);gap:1.5rem}.doc>nav{position:static}.doc>nav ol{display:flex;flex-wrap:wrap;gap:.2rem}}
.hero{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1.05fr);gap:3rem;align-items:center;padding:1rem 0 1.5rem}
@media (max-width:62rem){.hero{grid-template-columns:minmax(0,1fr)}}
.pill{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:.2rem .75rem;font-size:.85rem;color:var(--muted);background:var(--soft)}
.hero h1{font-size:clamp(2.2rem,4.6vw,3.4rem);line-height:1.06;letter-spacing:-.035em;margin:.9rem 0}
.lead{font-size:1.15rem;color:var(--muted);margin:0}
.actions{display:flex;flex-wrap:wrap;gap:.75rem;margin-top:1.6rem}
.button{display:inline-block;padding:.65rem 1.15rem;border-radius:9px;border:1px solid var(--line);color:var(--fg);text-decoration:none;font-weight:600}
.button:hover{background:var(--soft)}
.button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg)}
.button.primary:hover{filter:brightness(1.08)}
.section{margin-top:4rem}
.section>h2{font-size:1.75rem;margin:0 0 .4rem}
.section>p.sub{color:var(--muted);margin:0 0 1.5rem;max-width:46rem}
.cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(16.5rem,1fr));gap:1rem;list-style:none;padding:0;margin:0}
.card{border:1px solid var(--line);border-radius:12px;padding:1.1rem 1.2rem;background:var(--soft)}
.card h3{font-size:1.02rem;margin:0 0 .35rem}
.card p{margin:0;color:var(--muted);font-size:.95rem}
.card .code-link{display:inline-block;margin-top:.6rem;font:600 .82rem ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
.bench td:not(:first-child),.bench th:not(:first-child){text-align:right;white-space:nowrap}
.bench td.best{font-weight:700;color:var(--accent)}
.bench td:first-child{min-width:13rem}
.note{color:var(--muted);font-size:.92rem;max-width:52rem}
.two{display:grid;grid-template-columns:repeat(auto-fit,minmax(20rem,1fr));gap:2rem}
.crumbs{color:var(--muted);font-size:.92rem;margin:0 0 .6rem}
.rule{font-size:1.25rem;line-height:1.5;margin:.6rem 0 1.4rem;max-width:46rem}
.symbol{margin:-.4rem 0 0;color:var(--muted)}
.codes td:first-child{white-space:nowrap}
.codes code{overflow-wrap:normal;white-space:nowrap}
.prose h1 code{font-size:inherit;background:none;padding:0}
.stats{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:1rem;list-style:none;padding:0;margin:0}
.stats li{border:1px solid var(--line);border-radius:12px;padding:1.1rem 1.2rem;background:var(--soft)}
.stats b{display:block;font-size:2.3rem;line-height:1.05;letter-spacing:-.03em;color:var(--accent)}
.stats span{display:block;margin-top:.45rem;color:var(--muted);font-size:.92rem;line-height:1.45}
.versus{display:grid;grid-template-columns:minmax(0,1fr);gap:.6rem 1.2rem;margin:0 0 2.8rem}
@media (min-width:72rem){.versus{grid-template-columns:minmax(0,2fr) minmax(0,3fr)}}
.versus h3{grid-column:1/-1;font-size:1.22rem;margin:0 0 .15rem}
.versus figure.code{margin:0}
.versus pre{font-size:.78rem;white-space:pre-wrap;overflow-wrap:anywhere}
.said code{display:block}
.said .ln{display:flex}
.said .gutter{white-space:pre;flex:none}
.said .txt{white-space:pre-wrap;overflow-wrap:anywhere;min-width:0}
.versus p{margin:.55rem 0 0;font-size:.92rem;color:var(--muted)}
figure.code figcaption .shipped{color:var(--s)}
.section h3{font-size:1.15rem;margin:1.8rem 0 .2rem}
.points{padding-left:1.2rem;max-width:50rem}
.points li{margin:.5rem 0}
.foot{border-top:1px solid var(--line);color:var(--muted);font-size:.9rem}
.foot>div{max-width:var(--max);margin:0 auto;padding:1.5rem 1.25rem 2.5rem;display:flex;flex-wrap:wrap;gap:.6rem 1.5rem;justify-content:space-between}
.foot a{color:inherit}
@media (prefers-reduced-motion:no-preference){a,.button{transition:color .15s,background-color .15s,filter .15s}}
`
  .trim()
  .replace(/\n/g, "");

/** The style's hash, by which the pages' policy allows it and nothing else. */
export const STYLE_HASH = `sha256-${createHash("sha256").update(CSS).digest("base64")}`;

// Coloured by the stylesheet (`.brand rect`, `.brand path`): an attribute's
// `var()` is no colour a browser reads.
const MARK = `<svg viewBox="0 0 32 32" aria-hidden="true" focusable="false"><rect width="32" height="32" rx="8"/><path d="M11.5 24V8.5h6a5 5 0 0 1 0 10h-6" fill="none" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

/** A page: its head, the site's header and footer, and `body`. */
function page({ path, title, description, body }) {
  const full = path === "/" ? title : `${title} · Pleris`;
  const current = (prefix) => (path.startsWith(prefix) ? ' aria-current="page"' : "");
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escapeHtml(full)}</title>
<meta name="description" content="${escapeHtml(description)}">
<link rel="canonical" href="${ORIGIN}${path}">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<meta name="theme-color" content="#fbfbf9" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#0e1014" media="(prefers-color-scheme: dark)">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Pleris">
<meta property="og:title" content="${escapeHtml(full)}">
<meta property="og:description" content="${escapeHtml(description)}">
<meta property="og:url" content="${ORIGIN}${path}">
<style>${CSS}</style>
</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
<header class="top"><nav aria-label="Site">
<a class="brand" href="/">${MARK}Pleris</a>
<a href="/guide/"${current("/guide/")}>Guide</a>
<a href="/e/"${current("/e/")}>Error codes</a>
<a href="${REPO}">GitHub</a>
</nav></header>
<main id="main">
${body}
</main>
<footer class="foot"><div>
<span>Pleris is a research project, MIT or Apache-2.0 licensed. This site is static HTML: no script, no cookie, no tracking.</span>
<span><a href="/guide/">Guide</a> · <a href="/e/">Error codes</a> · <a href="${REPO}">GitHub</a></span>
</div></footer>
</body>
</html>
`;
}

/** The five entities `escapeHtml` writes, and the characters they are. */
const ENTITIES = new Map([
  ["&amp;", "&"],
  ["&lt;", "<"],
  ["&gt;", ">"],
  ["&quot;", '"'],
  ["&#39;", "'"],
]);

/** A page's first paragraph, as plain text for its description: its tags
 * dropped, then its entities read in one pass, so that no `&` one of them
 * gives starts another. `page` escapes the text again where it writes it. */
export function summaryOf(html, fallback) {
  const p = /<p>([\s\S]*?)<\/p>/.exec(html)?.[1] ?? fallback;
  const text = withoutTags(p)
    .replace(/&(?:amp|lt|gt|quot|#39);/g, (entity) => ENTITIES.get(entity))
    .replace(/\s+/g, " ")
    .trim();
  return text.length > 158 ? `${text.slice(0, 155).replace(/\s+\S*$/, "")}…` : text;
}

/** `html` with its tags dropped, again until no tag is left. */
function withoutTags(html) {
  let text = html;
  let was;
  do {
    was = text;
    text = text.replace(/<[^>]+>/g, "");
  } while (text !== was);
  return text;
}

// --- the guide --------------------------------------------------------------

/** Each code block the guide refuses with a code, by code: the program, the
 * compiler's words where the page shows them, and where it stands. */
const examples = new Map();

function guidePage(entry, index) {
  const md = readFileSync(join(ROOT, "docs/guide", `${entry.name}.md`), "utf8");
  const kept = { previous: null };
  const code = (text, info, context) => {
    const block = blockInfo(info);
    keep(kept, text, block, { page: entry, heading: context.heading });
    return codeFigure(text, block);
  };
  const diagnostics = entry.name === "diagnostics";
  const { html, title } = renderMarkdown(md, {
    link: guideLink,
    code,
    // The list of codes links each to its page.
    cell: (cell, column) => (diagnostics && column === 0 && /^PW\d{4}$/.test(cell) ? `<a href="/e/${cell}">${cell}</a>` : cell),
  });
  const nav = GUIDE.map(
    (p) => `<li><a href="${p.path}"${p === entry ? ' aria-current="page"' : ""}>${escapeHtml(p.short)}</a></li>`,
  ).join("");
  const prev = GUIDE[index - 1];
  const next = GUIDE[index + 1];
  const pager = `<nav class="pager" aria-label="The guide, in order">${
    prev ? `<a href="${prev.path}"><small>Previous</small>${escapeHtml(prev.short)}</a>` : "<span></span>"
  }${next ? `<a class="next" href="${next.path}"><small>Next</small>${escapeHtml(next.short)}</a>` : ""}</nav>`;
  const body = `<div class="doc">
<nav aria-label="The guide"><ol>${nav}</ol></nav>
<article class="prose">
${html}
${pager}
<p class="edit">This page is <a href="${BLOB}/docs/guide/${entry.name}.md">docs/guide/${entry.name}.md</a>, and a test compiles every example on it.</p>
</article>
</div>`;
  return { path: entry.path, title, description: summaryOf(html, title), body };
}

/** A fenced block's info string, read: its language, its file, the codes the
 * compiler refuses it with, its gap, and whether it is `pw check`'s output. */
function blockInfo(info) {
  const [lang = "", ...rest] = info.split(/\s+/).filter(Boolean);
  const attrs = Object.fromEntries(rest.filter((r) => r.includes("=")).map((r) => r.split("=", 2)));
  const fails = attrs.fails ? attrs.fails.split(",") : [];
  return { lang, file: attrs.file, fails, gap: attrs.gap, output: lang === "text" && rest.includes("pw-check") };
}

/** Each example the guide refuses with a code, kept by code, and with it
 * `pw check`'s output where the next block, under the same heading, is it. */
function keep(kept, text, block, where) {
  if (block.fails.length) keepExample(kept, text, block, where);
  else keepOutput(kept, text, block, where);
}

function keepExample(kept, text, block, where) {
  kept.previous = { text, file: block.file ?? null, fails: block.fails, where, output: null };
  for (const c of block.fails) {
    if (!examples.has(c)) examples.set(c, []);
    examples.get(c).push(kept.previous);
  }
}

function keepOutput(kept, text, block, where) {
  if (block.output && kept.previous?.where.heading === where.heading) kept.previous.output = text;
  kept.previous = null;
}

/** A block of code: Pleris coloured, the compiler's output as it prints it,
 * and above it what the guide says of it. */
function codeFigure(text, block) {
  return `<figure class="code${block.output ? " output" : ""}">${captionOf(block)}<pre tabindex="0">${codeOf(text, block.lang)}</pre></figure>`;
}

/** What the guide says of a block, each where it says it: its file, that it
 * is `pw check`'s output, the codes it is refused with, and its gap. */
const CAPTION = [
  (b) => b.file && `<span>${escapeHtml(b.file)}</span>`,
  (b) => b.output && "<span>pw check</span>",
  (b) => b.fails?.length > 0 && `<span class="refused">Refused: ${b.fails.map((c) => `<a href="/e/${c}">${c}</a>`).join(", ")}</span>`,
  (b) => b.gap && `<span>A gap: <code>pw ${escapeHtml(b.gap)}</code> passes this today</span>`,
];

function captionOf(block) {
  const said = CAPTION.map((part) => part(block)).filter(Boolean);
  return said.length ? `<figcaption>${said.join("")}</figcaption>` : "";
}

function codeOf(text, lang) {
  const body = lang === "pw" ? highlightPw(text) : escapeHtml(text);
  return `<code${lang ? ` class="language-${escapeHtml(lang)}"` : ""}>${body}</code>`;
}

// --- the codes --------------------------------------------------------------

/** The diagnostic codes, from the guide's list of them. */
function codes() {
  const lines = readFileSync(join(ROOT, "docs/guide/diagnostics.md"), "utf8").split("\n");
  const catalog = { active: [], retired: [], renumbered: [] };
  for (const { section, rows } of tablesIn(lines)) {
    for (const row of rows) catalogue(catalog, section, row);
  }
  return catalog;
}

/** Each table in a Markdown file's lines, with the `##` section it is under:
 * its rows, each a record by its head's cells. */
function tablesIn(lines) {
  const tables = [];
  let section = null;
  for (let i = 0; i < lines.length; i++) {
    section = sectionOf(lines[i], section);
    if (!tableStarts(lines, i)) continue;
    const end = rowsEnd(lines, i + 2);
    tables.push({ section, rows: recordsOf(lines.slice(i, end)) });
    i = end - 1;
  }
  return tables;
}

/** The section a `##` heading begins, or the one a line is already in. */
function sectionOf(line, section) {
  return /^## (.+)$/.exec(line)?.[1].trim() ?? section;
}

function tableStarts(lines, i) {
  return lines[i].startsWith("|") && /^\|[\s:|-]+\|?\s*$/.test(lines[i + 1] ?? "");
}

function rowsEnd(lines, from) {
  let end = from;
  while (end < lines.length && lines[end].startsWith("|")) end++;
  return end;
}

/** A table's rows after its head and alignments, each by its head's cells. */
function recordsOf(rows) {
  const head = cellsOf(rows[0]);
  return rows.slice(2).map((row) => {
    const cells = cellsOf(row);
    return Object.fromEntries(head.map((h, k) => [h, cells[k] ?? ""]));
  });
}

function catalogue(catalog, section, row) {
  if (section === "Retired codes") catalog.retired.push({ code: row.Code, why: row.Why });
  else if (section === "Old numbers") catalog.renumbered.push({ code: row["Old number"], now: row.Now, symbol: row.Symbol });
  else catalog.active.push({ code: row.Code, symbol: row.Symbol.replace(/`/g, ""), rule: row["What the program must hold to"], adr: row.ADR, section });
}

const md = (text) => inline(text, { link: guideLink });

function codePage(entry) {
  const shown = examples.get(entry.code) ?? [];
  const example = shown
    .map((ex) => {
      const from = ex.where.heading
        ? `<a href="${ex.where.page.path}#${ex.where.heading.id}">${escapeHtml(ex.where.heading.text)}</a>, in ${escapeHtml(ex.where.page.short)}`
        : `<a href="${ex.where.page.path}">${escapeHtml(ex.where.page.short)}</a>`;
      const out = ex.output ? codeFigure(ex.output, { lang: "text", output: true }) : "";
      return `${codeFigure(ex.text, { lang: "pw", file: ex.file, fails: ex.fails })}${out}<p class="note">From ${from}. A test holds the guide's examples to the compiler.</p>`;
    })
    .join("\n");
  const adrs = decisionsOf(entry);
  const body = `<article class="prose">
<p class="crumbs"><a href="/e/">Error codes</a> · ${escapeHtml(entry.section)}</p>
<h1><code>${entry.code}</code></h1>
<p class="symbol"><code>${escapeHtml(entry.symbol)}</code></p>
<p class="rule">The program must hold to this: ${md(entry.rule)}.</p>
${example ? `<h2>A program the compiler refuses with it</h2>\n${example}` : ""}
${adrs}
<h2>Reading the diagnostic</h2>
<p><code>pw check</code> prints the code in brackets, <code>[${entry.code}]</code>, with the sentence above and the place in your program. <a href="/guide/errors">Errors and diagnostics</a> explains how to read one, and <a href="/e/">every code</a> is listed with its rule.</p>
</article>`;
  return {
    path: `/e/${entry.code}`,
    title: `${entry.code}: ${entry.symbol}`,
    description: `${entry.code} (${entry.symbol}): the program must hold to this: ${summaryOf(`<p>${md(entry.rule)}</p>`, entry.rule)}.`,
    body,
  };
}

function decisionsOf(entry) {
  return entry.adr && entry.adr !== "—" ? `<h2>The decisions behind it</h2><p>${md(entry.adr)}</p>` : "";
}

function retiredPage(entry) {
  return {
    path: `/e/${entry.code}`,
    title: `${entry.code}: retired`,
    description: `${entry.code} is retired: the compiler no longer prints it, and its number is never reused.`,
    body: `<article class="prose">
<p class="crumbs"><a href="/e/">Error codes</a> · Retired</p>
<h1><code>${entry.code}</code></h1>
<p class="rule">Retired: the compiler no longer prints it, and its number is never reused.</p>
<p>${md(entry.why)}</p>
</article>`,
  };
}

function renumberedPage(entry) {
  return {
    path: `/e/${entry.code}`,
    title: `${entry.code}: now ${entry.now}`,
    description: `${entry.code} is an old number for ${entry.now} (${entry.symbol.replace(/`/g, "")}).`,
    body: `<article class="prose">
<p class="crumbs"><a href="/e/">Error codes</a> · Old numbers</p>
<h1><code>${entry.code}</code></h1>
<p class="rule">An old number, now <a href="/e/${entry.now}"><code>${entry.now}</code></a> (${md(entry.symbol)}).</p>
<p>The corpus's fixtures wrote it before one invariant got one code. The compiler never prints it.</p>
</article>`,
  };
}

function codesIndex({ active, retired, renumbered }) {
  const sections = [...new Set(active.map((c) => c.section))];
  const tables = sections
    .map((s) => {
      const rows = active
        .filter((c) => c.section === s)
        .map((c) => `<tr><td><a href="/e/${c.code}"><code>${c.code}</code></a></td><td><code>${escapeHtml(c.symbol)}</code></td><td>${md(c.rule)}</td></tr>`)
        .join("\n");
      const id = s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
      return `<h2 id="${id}">${escapeHtml(s)}</h2>
<div class="table" tabindex="0"><table class="codes"><thead><tr><th scope="col">Code</th><th scope="col">Symbol</th><th scope="col">What the program must hold to</th></tr></thead><tbody>
${rows}
</tbody></table></div>`;
    })
    .join("\n");
  const retiredRows = retired.map((r) => `<tr><td><a href="/e/${r.code}"><code>${r.code}</code></a></td><td>${md(r.why)}</td></tr>`).join("\n");
  const oldRows = renumbered
    .map((r) => `<tr><td><a href="/e/${r.code}"><code>${r.code}</code></a></td><td><a href="/e/${r.now}"><code>${r.now}</code></a></td><td>${md(r.symbol)}</td></tr>`)
    .join("\n");
  return {
    path: "/e/",
    title: "Error codes",
    description: `Every code the Pleris compiler prints (${active.length} of them), each with the rule a program must hold to.`,
    body: `<article class="prose wide">
<h1>Error codes</h1>
<p class="lead">Every diagnostic <code>pw check</code> prints carries a code, such as <code>[PW0606]</code>, and says in one sentence what the program must hold to. These are all ${active.length} of them, from the compiler's registry. A number is never reused.</p>
<p class="note">Find one by its address: <code>pleris.dev/e/PW0606</code>. <a href="/guide/errors">Errors and diagnostics</a> explains how to read a diagnostic.</p>
${tables}
<h2 id="retired">Retired codes</h2>
<p>No longer printed, and never reused.</p>
<div class="table" tabindex="0"><table class="codes"><thead><tr><th scope="col">Code</th><th scope="col">Why</th></tr></thead><tbody>
${retiredRows}
</tbody></table></div>
<h2 id="old-numbers">Old numbers</h2>
<p>Numbers the corpus's fixtures wrote before one invariant got one code. The compiler never prints them.</p>
<div class="table" tabindex="0"><table class="codes"><thead><tr><th scope="col">Old number</th><th scope="col">Now</th><th scope="col">Symbol</th></tr></thead><tbody>
${oldRows}
</tbody></table></div>
</article>`,
  };
}

// --- the home page ----------------------------------------------------------

/** The head-to-head's record (ADR-0392's instrument), from PR #76's evidence. */
const EVIDENCE = `${REPO}/blob/perf/first-press-20261106/docs/evidence/E14/first-press-2026-11-06/README.md`;

/** How the checks are checked: `python3 scripts/mutation_anchors.py` counts
 * the mutants, and the build counts the scripts, so that a script added or
 * taken away stops the build until this says so. */
const MUTANTS = { mutants: 4974, scripts: 324 };

/**
 * **The same mistake, in each.** Its code in Next.js or React, which
 * compiles and ships it; and the guide's example of it, refused, as
 * `pw check` prints it (`examples`: the guide's test holds what it prints).
 */
const MISTAKES = [
  {
    title: "One reader's data, served to the next",
    code: "PW5001",
    them: "Next.js",
    file: "app/api/cart/route.ts",
    source: `export async function GET() {
  const cart = await getCart(await currentUser());
  return Response.json(cart, {
    headers: { "Cache-Control": "public, max-age=60" },
  });
}`,
    ships: "A shared cache now serves one reader's cart to whoever asks next.",
  },
  {
    title: "A request sent twice, an order placed twice",
    code: "PW0338",
    them: "Next.js",
    file: "app/actions.ts",
    source: `"use server";

export async function placeOrder(cart: Cart) {
  await db.order.create({ data: { items: cart.items } });
}`,
    ships: "A double click, or a request a network retries, places a second order.",
  },
  {
    title: "A button for something its reader may not do",
    code: "PW5048",
    them: "Next.js",
    file: "app/actions.ts",
    source: `"use server";

export async function deletePost(id: string) {
  await db.post.delete({ where: { id } });
}`,
    ships: "Who may delete is a check each action has to remember. This one forgot, and anyone can call it.",
  },
  {
    title: "A secret, written into the page",
    code: "PW5003",
    them: "Next.js",
    file: "app/checkout/page.tsx",
    source: `export default function Checkout() {
  const key = process.env.PAYMENTS_KEY!;
  // PayButton is a client component.
  return <PayButton apiKey={key} />;
}`,
    ships: "The key is in the page every visitor of it downloads.",
  },
  {
    title: "An old answer, shown for a new question",
    code: "PW5309",
    them: "React",
    file: "Search.tsx",
    source: `useEffect(() => {
  fetch(\`/api/search?q=\${q}\`)
    .then((r) => r.json())
    .then(setResults);
}, [q]);`,
    ships: "Type quickly, and the answer to an earlier word can arrive last, and stay.",
  },
];

/** More of what the compiler refuses, each in the registry's own words. */
const MORE = [
  ["Outside data, trusted as typed", "PW0601"],
  ["An error, silently dropped", "PW0618"],
  ["A missing value, used anyway", "PW0600"],
  ["A reader's own data, shown stale", "PW0102"],
  ["A shared cache, keyed without its tenant", "PW5004"],
  ["A page's rendering that reaches the database", "PW0401"],
  ["A function that says it is pure and is not", "PW0400"],
  ["A case forgotten", "PW0305"],
  ["A link to nowhere", "PW5009"],
  ["A form control nobody can name", "PW5014"],
];

/** The head-to-head, each row's best marked, whichever framework it is. */
const SPEED = [
  {
    title: "TodoMVC, against TodoMVC's own React and Svelte apps",
    them: ["React", "Svelte"],
    rows: [
      ["Ready to use, first visit (Slow 4G)", ["1,417 ms", "1,671 ms", "1,295 ms"], [2]],
      ["Ready to use, the visit after (Slow 4G)", ["645 ms", "1,189 ms", "1,168 ms"], [0]],
      ["First paint (Slow 4G)", ["1,260 ms", "1,200 ms", "1,196 ms"], [2]],
      ["The first todo added, to its frame (Slow 4G)", ["31 ms", "33 ms", "34 ms"], [0]],
      ["The first toggle, to its frame (Slow 4G)", ["22 ms", "24 ms", "19 ms"], [2]],
      ["A filter, to its frame (Slow 4G)", ["15 ms", "17 ms", "29 ms"], [0]],
      ["Downloaded on a first visit", ["142 KB", "70 KB", "21 KB"], [2]],
    ],
  },
  {
    title: "A store, against the same store in Next.js and SvelteKit",
    them: ["Next.js", "SvelteKit"],
    rows: [
      ["Ready to use, first visit (Slow 4G)", ["1,381 ms", "2,067 ms", "1,749 ms"], [0]],
      ["Ready to use, the visit after (Slow 4G)", ["626 ms", "660 ms", "638 ms"], [0]],
      ["An item added, to its frame (Slow 4G)", ["13 ms", "638 ms", "1,179 ms"], [0]],
      ["A press, with 19 other tabs of it open", ["21 ms", "43 ms", "44 ms"], [0]],
      ["Server CPU for each request, under load", ["0.20 ms", "5.12 ms", "1.17 ms"], [0]],
      ["Latency under load, median and 99th percentile", ["1.09 / 2.88 ms", "3.57 / 6.57 ms", "0.81 / 2.37 ms"], [2]],
      ["Downloaded on a first visit", ["46 KB", "140 KB", "35 KB"], [2]],
      ["Requests a minute from an open, idle page", ["29.7", "0", "0"], [1, 2]],
    ],
  },
];

// **What `pw check` prints, laid out to read in a narrow box.** Its words and
// their order are the compiler's. A label it writes after its markers, which
// would run past `COLUMNS`, goes on the line under them, as far left as it must
// to fit and never left of a span still open. Every other line keeps its lead
// (the gutter, `error: [PW…] `, `= note: `), and its text wraps under itself,
// not at the box's edge.

const COLUMNS = 80;
/** A line of markers under source, and the label after them. */
const MARKED = /^(\s*\|[ |_/\\^-]*[-^]) (\S.*)$/;
/** What a line's text wraps under: its gutter, or what its kind begins with. */
const LEAD = /^(?:(?:error|warning): \[PW\d+\] |\s*= \w+: |\s*\d+ \| ?)/;

function laidOut(output) {
  return output
    .trimEnd()
    .split("\n")
    .flatMap((line) => {
      const marked = MARKED.exec(line);
      return marked ? markedLines(line, marked) : [plainLine(line)];
    });
}

function plainLine(line) {
  const lead = LEAD.exec(line)?.[0] ?? "";
  return { lead, text: line.slice(lead.length) };
}

function markedLines(line, [, markers, label]) {
  if (line.length <= COLUMNS) return [{ lead: `${markers} `, text: label }];
  const bars = openBars(markers);
  const at = Math.max(bars.length + 1, Math.min(markers.search(/[-^_]/), COLUMNS - label.length));
  return [
    { lead: markers, text: "" },
    { lead: bars.padEnd(at), text: label },
  ];
}

/** The gutter, and each bar of a span still open, before a line's markers:
 * a bar that turns into `_` is a span ending there. */
function openBars(markers) {
  return markers
    .replace(/\|(?=_)/g, " ")
    .slice(0, markers.search(/[-^_]/))
    .trimEnd();
}

function lineHtml({ lead, text }) {
  return `<span class="ln"><span class="gutter">${escapeHtml(lead)}</span><span class="txt">${escapeHtml(text)}</span></span>`;
}

function shipped(m) {
  return `<figure class="code"><figcaption><span>${m.them}</span><span>${escapeHtml(m.file)}</span><span class="shipped">Compiles, and ships</span></figcaption><pre tabindex="0"><code class="language-ts">${escapeHtml(m.source)}</code></pre></figure>`;
}

function refused(m) {
  const example = (examples.get(m.code) ?? []).find((ex) => ex.output);
  if (!example) throw new Error(`the guide's ${m.code} example and its output: the home page shows them`);
  return `<figure class="code output"><figcaption><span>Pleris</span><span>pw check</span><span class="refused">Refused: <a href="/e/${m.code}">${m.code}</a></span></figcaption><pre class="said" tabindex="0"><code>${laidOut(example.output).map(lineHtml).join("")}</code></pre></figure>`;
}

function mistake(m) {
  return `<article class="versus">
<h3>${escapeHtml(m.title)}</h3>
<div>${shipped(m)}<p>${escapeHtml(m.ships)}</p></div>
<div>${refused(m)}<p><a href="/e/${m.code}">The whole program, and why it is refused</a></p></div>
</article>`;
}

function ruleOf(catalog, code) {
  const entry = catalog.active.find((c) => c.code === code);
  if (!entry) throw new Error(`the home page names ${code}, which the registry does not hold`);
  return entry.rule;
}

function moreCard(catalog, [title, code]) {
  return `<li class="card"><h3>${escapeHtml(title)}</h3><p>${md(ruleOf(catalog, code))}</p><a class="code-link" href="/e/${code}">${code}</a></li>`;
}

function benchRow([what, values, best]) {
  const cells = values.map((v, i) => `<td${best.includes(i) ? ' class="best"' : ""}>${v}</td>`).join("");
  return `<tr><td>${escapeHtml(what)}</td>${cells}</tr>`;
}

function benchTable(t) {
  const head = ["Pleris", ...t.them].map((name) => `<th scope="col">${name}</th>`).join("");
  return `<h3>${escapeHtml(t.title)}</h3>
<div class="table" tabindex="0"><table class="bench"><thead><tr><th scope="col">Measured</th>${head}</tr></thead><tbody>
${t.rows.map(benchRow).join("\n")}
</tbody></table></div>`;
}

/** The scripts that check the checks, counted as the build finds them. */
function mutationScripts() {
  const found = readdirSync(join(ROOT, "scripts")).filter((f) => f.endsWith("_mutations.py")).length;
  if (found !== MUTANTS.scripts) {
    throw new Error(`${found} mutation scripts, and the home page says ${MUTANTS.scripts}: count the mutants again`);
  }
  return found;
}

function home(catalog) {
  const scripts = mutationScripts();
  const stats = [
    ["25×", "less server CPU for each request than Next.js, serving the same store"],
    ["13 ms", "from a press to its frame on a slow phone network, where Next.js took 638 ms"],
    [String(catalog.active.length), "rules every program is held to, each with a code and a page"],
    [MUTANTS.mutants.toLocaleString("en-US"), "bugs planted in Pleris on purpose, each of which a test must catch"],
  ]
    .map(([n, what]) => `<li><b>${n}</b><span>${escapeHtml(what)}</span></li>`)
    .join("\n");
  const body = `<section class="hero">
<div>
<span class="pill">A research project · not released yet</span>
<h1>Building the perfect web framework.</h1>
<p class="lead">Pleris is a language for web apps, with a compiler and a server for it: the data, the server's reads and writes, and the pages, in one language. Its compiler refuses the mistakes Next.js, SvelteKit, React and Svelte let through, and its runtime is measured against all four.</p>
<div class="actions">
<a class="button primary" href="#mistakes">The same mistake, twice</a>
<a class="button" href="/guide/">Read the guide</a>
<a class="button" href="${REPO}">GitHub</a>
</div>
</div>
<ul class="stats">
${stats}
</ul>
</section>

<section class="section" id="mistakes">
<h2>The same mistake, twice</h2>
<p class="sub">Each of these compiles in Next.js or React, and ships. In Pleris the compiler refuses it, in its own words. Each Pleris program is the guide's, which a test runs through the compiler.</p>
${MISTAKES.map(mistake).join("\n")}
</section>

<section class="section" id="rules">
<h2>And whole classes more</h2>
<p class="sub">${catalog.active.length} rules every program is held to, each with a code, its rule in one sentence, and a page.</p>
<ul class="cards">
${MORE.map((m) => moreCard(catalog, m)).join("\n")}
</ul>
<p><a href="/e/">Every rule and its code</a></p>
</section>

<section class="section" id="speed">
<h2>Measured against all four</h2>
<p class="sub">The same apps, measured by the project's own instrument: Chromium on one development PC, medians of five runs. Slow 4G is Lighthouse's profile of a slow phone network. The best in each row is marked, whichever framework it is.</p>
${SPEED.map(benchTable).join("\n")}
<p class="note">Where another framework is marked, Pleris is behind, and that row is work in progress. These numbers come from a branch still under review (PR #76), and field performance is not claimed. <a href="${EVIDENCE}">Every run, and the command that makes it.</a></p>
</section>

<section class="section" id="secure">
<h2>Secure without configuring it</h2>
<ul class="points">
<li><strong>A page runs only what its build wrote.</strong> Each page carries a content security policy generated from its build (<a href="${BLOB}/docs/DECISIONS/ADR-0333-what-a-page-may-run-is-generated-from-its-build.md">ADR-0333</a>).</li>
<li><strong>Data says whose it is</strong>, everyone's, a session's or a user's, and the label follows it through every call, branch and return (<a href="/e/PW5001">PW5001</a>, <a href="/e/PW5006">PW5006</a>).</li>
<li><strong>A session's handle never reaches the browser</strong>, and nothing the browser sends can claim one (<a href="/e/PW5040">PW5040</a>, <a href="/e/PW5039">PW5039</a>).</li>
<li><strong>A command says who may run it</strong>, the server asks on every call, and no page shows its button to anyone else (<a href="/e/PW5048">PW5048</a>).</li>
</ul>
</section>

<section class="section two" id="checked">
<div>
<h2>Each check, checked</h2>
<p>A test that cannot fail checks nothing. So each of Pleris's checks is broken on purpose: ${scripts} scripts plant ${MUTANTS.mutants.toLocaleString("en-US")} bugs in its own source, one at a time, and each must make a test fail. The guide's examples are compiled by a test too, so what this site says the compiler prints is what it prints.</p>
<p><a href="${TREE}/scripts">The scripts</a></p>
</div>
<div>
<h2>Made for agents</h2>
<p>An AI writes Pleris from its guide and its compiler. Every refusal has a code and a page an agent can read, such as <a href="/e/PW0338">pleris.dev/e/PW0338</a>, and <a href="/llms.txt">llms.txt</a> gives it the whole guide as Markdown.</p>
</div>
</section>

<section class="section" id="status">
<h2>Not finished</h2>
<p class="sub">Pleris is a research project, and is not released yet. Before you build on it:</p>
<ul class="points">
<li>An app's storage is written in Rust, behind host bindings.</li>
<li>Sign-in is a development provider, and there is one server.</li>
<li>There is no static or edge build: an app runs on its own server.</li>
<li>There is no <code>pw new</code>, no project file and no editor support.</li>
<li>Some checks have gaps, and the guide shows each.</li>
</ul>
<p>Each of these, and each row above that another framework wins, is work in progress. <a href="/guide/why#what-it-does-not-do-yet">The whole list.</a></p>
</section>`;
  return {
    path: "/",
    title: "Pleris: building the perfect web framework",
    description:
      "Pleris is a language for web apps whose compiler refuses the mistakes Next.js, SvelteKit, React and Svelte let through: one reader's data served to another, an order placed twice, a secret in the page. Measured against all four.",
    body,
  };
}

/** The two pages for an address with nothing at it: the site's, and the
 * codes'. */
const NOT_FOUND = {
  site: {
    path: "/404",
    title: "Not found",
    lead: "There is no page at this address.",
    links: '<a href="/">Pleris</a> · <a href="/guide/">The guide</a> · <a href="/e/">Error codes</a>',
  },
  codes: {
    path: "/e/404",
    title: "No such code",
    lead: "The compiler prints no code with this number.",
    links: '<a href="/e/">Every code</a>, with its rule.',
  },
};

function notFound(inCodes) {
  const { path, title, lead, links } = inCodes ? NOT_FOUND.codes : NOT_FOUND.site;
  return {
    path,
    title,
    description: "There is no page here.",
    body: `<article class="prose">
<h1>${title}</h1>
<p class="lead">${lead}</p>
<p>${links}</p>
</article>`,
  };
}

// --- the policy and the rest -------------------------------------------------

/** **The guide's Markdown, for an agent** (`llms.txt`): each page as it is
 * in the repository, its links made whole, so that a reader with no
 * browser follows them. */
function markdownCopy(md) {
  return md.replace(/\]\(([^)\s]+)\)/g, (_, url) => `](${copyLink(url)})`);
}

/** A link of the guide's Markdown, made whole: to a page of the guide, that
 * page's Markdown here; to anything else, where `guideLink` sends it. */
function copyLink(url) {
  const [path, hash] = url.split("#");
  const page = ABSOLUTE.test(url) ? null : guidePageAt(path);
  return page ? `${ORIGIN}/guide/${page.name}.md${hash ? `#${hash}` : ""}` : guideLink(url);
}

/** **What an agent reads first** (llmstxt.org): what Pleris is, and where
 * the guide and the codes are, as Markdown. */
function llms(codeCount) {
  const pages = GUIDE.map((p) => `- [${p.short}](${ORIGIN}/guide/${p.name}.md): ${p.about}`).join("\n");
  return `# Pleris

> Pleris is a language for web apps, with a compiler and a server for it. You write the data, the server's reads and writes, and the pages in one language, and the compiler refuses programs with whole classes of bugs that TypeScript, React, Next.js and Svelte let through. A research project, not released yet.

Every example in the guide is a whole program that a test compiles. Every diagnostic the compiler prints carries a code, such as \`[PW0606]\`, and ${ORIGIN}/e/PW0606 explains it: ${codeCount} codes, from the compiler's registry.

## The guide

${pages}

## Optional

- [Error codes](${ORIGIN}/e/): one page for each code, with its rule
- [Source](${REPO}): the compiler, the server, the decisions (ADRs) and their evidence
`;
}

/** What each response says beside the page (`_headers`): the pages' policy
 * allows their one style, by its hash, the icon from this origin, and
 * nothing else; no page may be framed or framed into another's. A page runs
 * no script, so `connect-src 'self'` gives none a way out. It lets a
 * browser's own tools, such as an audit, read the site's `robots.txt` and
 * `llms.txt` through the page. */
function headers() {
  return `/*
  Content-Security-Policy: default-src 'none'; style-src '${STYLE_HASH}'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
  X-Content-Type-Options: nosniff
  Referrer-Policy: strict-origin-when-cross-origin
  Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=(), usb=()
  Cross-Origin-Opener-Policy: same-origin
  Cross-Origin-Resource-Policy: same-origin
  Strict-Transport-Security: max-age=63072000; includeSubDomains; preload

/favicon.svg
  Cache-Control: public, max-age=86400
`;
}

/** Writes the site into `out`. Returns what it wrote, by path. */
export function build(out = join(SITE, "dist")) {
  examples.clear();
  rmSync(out, { recursive: true, force: true });
  // The guide first: its pages keep the examples a code's page shows.
  const pages = GUIDE.map((entry, i) => guidePage(entry, i));
  const catalog = codes();
  pages.push(...codePages(catalog), home(catalog), notFound(false), notFound(true));
  const written = writePages(out, pages);
  writeFiles(out, pages, catalog);
  return { written, catalog };
}

function codePages(catalog) {
  return [
    codesIndex(catalog),
    ...catalog.active.map((c) => codePage(c)),
    ...catalog.retired.map((c) => retiredPage(c)),
    ...catalog.renumbered.map((c) => renumberedPage(c)),
  ];
}

/** Each page, written to the file it is served from. Returns each page's
 * HTML, by path. */
function writePages(out, pages) {
  const written = new Map();
  for (const p of pages) {
    const file = join(out, fileOf(p.path));
    mkdirSync(dirname(file), { recursive: true });
    const html = page(p);
    writeFileSync(file, html);
    written.set(p.path, html);
  }
  return written;
}

/** The rest: the static files, the guide's Markdown, `llms.txt`, the
 * policy and the sitemap. */
function writeFiles(out, pages, catalog) {
  for (const f of readdirSync(join(SITE, "static"))) copyFileSync(join(SITE, "static", f), join(out, f));
  for (const p of GUIDE) {
    writeFileSync(join(out, "guide", `${p.name}.md`), markdownCopy(readFileSync(join(ROOT, "docs/guide", `${p.name}.md`), "utf8")));
  }
  writeFileSync(join(out, "llms.txt"), llms(catalog.active.length));
  writeFileSync(join(out, "_headers"), headers());
  writeFileSync(join(out, "sitemap.xml"), sitemap(pages));
}

function sitemap(pages) {
  const listed = pages.filter((p) => !p.path.endsWith("404")).map((p) => `<url><loc>${ORIGIN}${p.path}</loc></url>`);
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${listed.join("\n")}\n</urlset>\n`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const out = process.argv[2] ?? join(SITE, "dist");
  const { written, catalog } = build(out);
  console.log(`pleris.dev: ${written.size} pages (${catalog.active.length} codes) in ${out}`);
}
