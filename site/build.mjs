#!/usr/bin/env node
// **pleris.dev, built from the repository** (site/README.md): the guide
// (docs/guide/*.md), a page for each diagnostic code (docs/guide/diagnostics.md,
// which scripts/guide_diagnostics.py writes from the compiler's registry) and
// the home page, into static files that Cloudflare serves as they are. No
// script runs in a reader's browser, and the build has no dependency.
//
//   node site/build.mjs [out]      out: site/dist

import { execFileSync } from "node:child_process";
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
.hero{max-width:50rem;padding:1rem 0 1.5rem}
.pill{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:.2rem .75rem;font-size:.85rem;color:var(--muted);background:var(--soft)}
.hero h1{font-size:clamp(2.2rem,4.6vw,3.4rem);line-height:1.06;letter-spacing:-.035em;margin:.9rem 0}
.lead{font-size:1.15rem;color:var(--muted);margin:0}
.lead+.lead{margin-top:1rem}
.actions{display:flex;flex-wrap:wrap;gap:.75rem;margin-top:1.6rem}
.button{display:inline-block;padding:.65rem 1.15rem;border-radius:9px;border:1px solid var(--line);color:var(--fg);text-decoration:none;font-weight:600}
.button:hover{background:var(--soft)}
.button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg)}
.button.primary:hover{filter:brightness(1.08)}
.section{margin-top:4rem}
.section>h2{font-size:1.75rem;margin:0 0 .4rem}
.section>p.sub{color:var(--muted);margin:0 0 1.5rem;max-width:46rem}
.bench td:not(:first-child),.bench th:not(:first-child){text-align:right;white-space:nowrap}
.bench td.best{font-weight:700;color:var(--accent)}
.bench td:first-child{min-width:12rem}
@media (max-width:40rem){.bench{font-size:.82rem}.bench th,.bench td{padding:.45rem .4rem}.bench td:first-child{min-width:0}}
.note{color:var(--muted);font-size:.92rem;max-width:52rem}
.invite{font-size:1.1rem;margin-top:1.4rem}
.crumbs{color:var(--muted);font-size:.92rem;margin:0 0 .6rem}
.rule{font-size:1.25rem;line-height:1.5;margin:.6rem 0 1.4rem;max-width:46rem}
.symbol{margin:-.4rem 0 0;color:var(--muted)}
.codes td:first-child{white-space:nowrap}
.codes code{overflow-wrap:normal;white-space:nowrap}
.prose h1 code{font-size:inherit;background:none;padding:0}
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
<span>Pleris is a research project, MIT or Apache-2.0 licensed. This site is static HTML, with no script and no cookie.</span>
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
<p>It named the invariant <code>${entry.now}</code> names. One invariant has one code, so the compiler prints <code>${entry.now}</code>, and this number is never given to another.</p>
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


/**
 * **The same mistake, in each.** Its code in Next.js or React, which
 * compiles and ships it; and the guide's example of it, refused, as
 * `pw check` prints it (`examples`: the guide's test holds what it prints).
 *
 * Each pair is one Pleris refuses as written. Two left on 2026-11-06, when a
 * review read them against the repository: a double click is two presses,
 * which Pleris runs twice as the store's contract asks (ADR-0121), and a
 * command that requires nothing compiles (ADR-0307).
 */
const MISTAKES = [
  {
    title: "One reader's cart, served to the next",
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
    title: "A secret, written into the page",
    code: "PW5003",
    them: "Next.js",
    file: "app/checkout/page.tsx",
    source: `export default function Checkout() {
  const key = process.env.PAYMENTS_KEY!;
  // PayButton is a client component.
  return <PayButton apiKey={key} />;
}`,
    ships: "The key is in the page every visitor downloads.",
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

/**
 * **The head-to-head's record** (ADR-0392's instrument, `just
 * e14-head-to-head`): each row Pleris is in, its values, and the
 * instrument's verdict on it. The home page's tables and `/measured` show
 * its rows as it wrote them (ADR-0408). Until train-15 the home page held a
 * copy, typed by hand from an exploratory run, which nothing held to the
 * record.
 */
const RECORD = "docs/evidence/E14/head-to-head.json";

/** The record, if the site may show it (`checked`), with links to it and to
 * the commands that wrote it, at the commit that holds it. A record changed
 * since that commit stops the build: its link would show other numbers. */
function record() {
  const rec = checked(JSON.parse(readFileSync(join(ROOT, RECORD), "utf8")));
  const git = (...args) => execFileSync("git", args, { cwd: ROOT, encoding: "utf8" }).trim();
  if (git("status", "--porcelain", "--", RECORD)) {
    throw new Error(`${RECORD} is not as it was committed, and the pages link to the committed record: commit it first`);
  }
  const at = `${REPO}/blob/${git("log", "-1", "--format=%H", "--", RECORD)}`;
  return { ...rec, link: `${at}/${RECORD}`, commands: `${at}/just/head-to-head.just` };
}

/** **A record the site may show.** The build stops on a run the instrument's
 * rules refused, in which nothing is a measurement; on one measured on a tree
 * changed from its commit; and on a verdict no page can show (`aheadIn`). */
export function checked(rec) {
  if (rec.problems.length) throw new Error(`${RECORD}: the instrument refused its run: ${rec.problems[0]}`);
  if (rec.dirty) throw new Error(`${RECORD}: measured on a tree changed from ${rec.commit}`);
  for (const r of rec.rows) aheadIn(r);
  return rec;
}

/** A row of the record the site cannot show: the build stops, naming it. */
function refuse(r, why) {
  throw new Error(`${RECORD}: ${why}, on ${r.app}: ${r.what}`);
}

/** **Who the instrument found ahead in a row**, by its verdict: Pleris,
 * ahead of every other stack; each stack ahead of Pleris; or none, when
 * neither holds. A loss that names no stack stops the build. */
const AHEAD = new Map([
  ["win", () => ["Pleris"]],
  ["loss", (r) => (r.better.length ? r.better : refuse(r, "a loss that names no stack ahead of Pleris"))],
  ["neither", () => []],
]);

/** Who is ahead in a row; a verdict the site does not know stops the build. */
function aheadIn(r) {
  const ahead = AHEAD.get(r.verdict) ?? (() => refuse(r, `a verdict the site does not know, ${r.verdict}`));
  return ahead(r);
}

/**
 * **The home page's rows**: the record's, by app and by what the instrument
 * calls each, under the page's own names. A row the record does not hold
 * stops the build, so a row the instrument renames or drops is never shown
 * from an old copy. `/measured` shows every row, these among them.
 *
 * The store's add is not here: Pleris's store shows an add before the server
 * answers, and ADR-0120's contract left that out of all three, so the Next.js
 * and SvelteKit stores wait for the server.
 */
export const SPEED = [
  {
    app: "todomvc",
    title: "TodoMVC, against TodoMVC's own React and Svelte apps",
    rows: [
      ["TodoMVC's specification: tests passed in Chromium, Firefox and WebKit", "TodoMVC's specification, tests passed in Chromium, Firefox and WebKit together (more is better)"],
      ["TodoMVC's own Cypress suite: tests passed", "TodoMVC's official Cypress suite, tests passed (more is better)"],
      ["Ready to use, first visit, Slow 4G", "interactive, ms, median (chromium, cold, slow-4g)"],
      ["Ready to use, the next visit, Slow 4G", "interactive, ms, median (chromium, warm, slow-4g)"],
      ["First paint, Slow 4G", "first contentful paint, ms, median (chromium, cold, slow-4g)"],
      ["A todo added, to its frame, Slow 4G", 'interaction "add" to its frame, ms, median (chromium, cold, slow-4g)'],
      ["A todo toggled, to its frame, Slow 4G", 'interaction "toggle" to its frame, ms, median (chromium, cold, slow-4g)'],
      ["A filter, to its frame, Slow 4G", 'interaction "filter" to its frame, ms, median (chromium, cold, slow-4g)'],
      ["Bytes downloaded on a first visit", "first load, bytes received as each host sent them (Chromium, cold)"],
      ["Requests on a first visit", "first load, requests (Chromium, cold)"],
    ],
  },
  {
    app: "store",
    title: "A store, against the same store in Next.js and SvelteKit",
    rows: [
      ["Bug classes it lets ship, of five tested", "the museum's bug classes the stack lets ship (fewer is better)"],
      ["Twelve changes, each written unsafely: how many its own checker catches", "the twelve changes: unsafe versions the stack's own checker refuses or warns of (more is better)"],
      ["Twelve changes: lines changed in all", "the twelve changes: lines changed in all"],
      ["Lines of the app, with the server code it needs in another language", "source the developer writes: app lines plus the code it needs in its server in another language"],
      ["Ready to use, first visit, Slow 4G", "interactive, ms, median (chromium, cold, slow-4g)"],
      ["Ready to use, the next visit, Slow 4G", "interactive, ms, median (chromium, warm, slow-4g)"],
      ["Bytes downloaded on a first visit", "first load, bytes received as each host sent them (Chromium, cold)"],
      ["Server CPU a request, under load, ms", "server CPU per request, ms, median"],
      ["Server memory at its peak, under load, MiB", "server resident memory at its peak under load, MiB, median"],
      ["Latency under load, median, ms", "latency under load, p50, ms, median"],
      ["Requests a minute from an idle page", "requests an idle visitor's page makes a minute (20 visitors, a browser context each), median"],
      ["Server CPU for 20 idle pages, ms a second", "server CPU, ms a second, for 20 idle visitors, median"],
    ],
  },
];

/** **Where Pleris stands**, over the whole record, not the rows a page
 * chose: the rows it is ahead in, and those another stack is ahead in. */
export function standing(rec) {
  const count = (verdict) => rec.rows.filter((r) => r.verdict === verdict).length;
  return `Of the ${rec.rows.length} rows its head-to-head measures, it is ahead in ${count("win")} and behind in ${count("loss")}: those ${count("loss")} rows are the work.`;
}

// **What `pw check` says, laid out to read in a narrow box.** The home page
// shows its `error:` and `= help:` lines, in the compiler's words. Each keeps
// its lead (`error: [PW…] `, `= help: `), and its text wraps under itself, not
// at the box's edge. The whole output, markers and all, is on the code's page.

/** What a line's text wraps under: what its kind begins with. */
const LEAD = /^(?:(?:error|warning): \[PW\d+\] |= \w+: )/;

function plainLine(line) {
  const lead = LEAD.exec(line)?.[0] ?? "";
  return { lead, text: line.slice(lead.length) };
}

function lineHtml({ lead, text }) {
  return `<span class="ln"><span class="gutter">${escapeHtml(lead)}</span><span class="txt">${escapeHtml(text)}</span></span>`;
}

function shipped(m) {
  return `<figure class="code"><figcaption><span>${m.them}</span><span>${escapeHtml(m.file)}</span><span class="shipped">Compiles, and ships</span></figcaption><pre tabindex="0"><code class="language-ts">${escapeHtml(m.source)}</code></pre></figure>`;
}

/** What `pw check` says is wrong and how to repair it: its `error:` line and
 * its `= help:` lines, the rest on the code's page. */
function verdictLines(output) {
  const said = output.split("\n").filter((l) => /^(?:error|warning): |^\s*= help: /.test(l));
  if (!said.some((l) => l.includes("= help: "))) throw new Error(`no help in what pw check printed:\n${output}`);
  return said.map((l) => plainLine(l.trimStart()));
}

function refused(m) {
  const example = (examples.get(m.code) ?? []).find((ex) => ex.output);
  if (!example) throw new Error(`the guide's ${m.code} example and its output: the home page shows them`);
  return `<figure class="code output"><figcaption><span>Pleris</span><span>pw check</span><span class="refused">Refused: <a href="/e/${m.code}">${m.code}</a></span></figcaption><pre class="said" tabindex="0"><code>${verdictLines(example.output).map(lineHtml).join("")}</code></pre></figure>`;
}

function mistake(m) {
  return `<article class="versus">
<h3>${escapeHtml(m.title)}</h3>
<div>${shipped(m)}<p>${escapeHtml(m.ships)}</p></div>
<div>${refused(m)}<p><a href="/e/${m.code}">The whole program, and why it is refused</a></p></div>
</article>`;
}

/** The record's row for an app and what the instrument calls it: exactly
 * one, or the build stops. */
function recordRow(rec, app, what) {
  const found = rec.rows.filter((r) => r.app === app && r.what === what);
  if (found.length !== 1) throw new Error(`${RECORD} holds ${found.length} rows for ${app}: ${what}`);
  return found[0];
}

/** A row: its values, each stack the instrument found ahead marked, and who
 * that is, in words, for a reader who sees no mark. */
function verdictRow(label, r, stacks) {
  const ahead = aheadIn(r);
  const cells = stacks.map((s) => {
    const v = r.values.find((x) => x.label === s);
    return `<td${ahead.includes(s) ? ' class="best"' : ""}>${v ? escapeHtml(v.shown) : "–"}</td>`;
  });
  return `<tr><td>${escapeHtml(label)}</td>${cells.join("")}<td>${ahead.length ? escapeHtml(ahead.join(", ")) : "None"}</td></tr>`;
}

/** A table of the record's rows (`[label, row]`): a column for each stack,
 * in the order the record names them, and a last one for who is ahead. */
function verdictTable(rows) {
  const stacks = [...new Set(rows.flatMap(([, r]) => r.values.map((v) => v.label)))];
  const head = [...stacks, "Ahead"].map((name) => `<th scope="col">${escapeHtml(name)}</th>`).join("");
  return `<div class="table" tabindex="0"><table class="bench"><thead><tr><th scope="col">Measured</th>${head}</tr></thead><tbody>
${rows.map(([label, r]) => verdictRow(label, r, stacks)).join("\n")}
</tbody></table></div>`;
}

function benchTable(t, rec) {
  return `<h3>${escapeHtml(t.title)}</h3>
${verdictTable(t.rows.map(([label, what]) => [label, recordRow(rec, t.app, what)]))}`;
}

/** Where a record's numbers come from: the commit measured, the day, the
 * machine, and the record and its commands at the commit that holds them. */
function provenance(rec) {
  return `Measured at <a href="${REPO}/commit/${rec.commit}">${rec.commit.slice(0, 8)}</a> on ${rec.finished.slice(0, 10)}, on one PC (${escapeHtml(rec.machine)}): <a href="${rec.link}">the record</a>, and <a href="${rec.commands}">the commands that wrote it</a>.`;
}

/** **The home page's measured section**: what its tables are, the tables,
 * and where their numbers come from. */
export function speed(rec) {
  return `<p class="sub">Two apps against their rivals, by the project's own instrument, on one PC. A time is Chromium's, the median of ${rec.runs} runs; a server's number is the median of ${rec.serverRuns}. Slow 4G is Lighthouse's slow-phone profile, which also slows the CPU four times. A marked number is the instrument's verdict, and the last column names it: Pleris, ahead of both; each framework ahead of Pleris; or none, when neither holds.</p>
${SPEED.map((t) => benchTable(t, rec)).join("\n")}
<p class="note">${provenance(rec)} Field performance is not claimed. An idle Pleris page asks for its live updates, which the other two stores do not have. The store's add is left out: Pleris's store shows it before the server answers, and the other two, as their shared contract wrote them, wait for the server. <a href="/measured">Every row the instrument measures</a>, in three browsers.</p>`;
}

/** **How the checks are checked**, counted when the site is built: the
 * mutants `python3 scripts/mutation_anchors.py` reads, each matching its
 * anchor exactly once, and the scripts that hold them. An anchor that does
 * not match stops the build, so the page never states a count that does not
 * hold. Until train-14 both counts were written here by hand, and only the
 * scripts' was checked: a mutant added to a script changed nothing. */
function mutationCounts() {
  const out = execFileSync("python3", [join(ROOT, "scripts/mutation_anchors.py")], { cwd: ROOT, encoding: "utf8" });
  const said = /mutation anchors: (\d+) mutants in (\d+) scripts, each matching once/.exec(out);
  if (!said) throw new Error(`mutation_anchors.py said nothing the build can read:
${out}`);
  const [mutants, scripts] = [Number(said[1]), Number(said[2])];
  const found = readdirSync(join(ROOT, "scripts")).filter((f) => f.endsWith("_mutations.py")).length;
  if (scripts !== found) throw new Error(`mutation_anchors.py read ${scripts} scripts, and scripts/ holds ${found}`);
  return { mutants, scripts };
}

/**
 * **The home page**, opening with the goal and its test. "Perfect" is a claim
 * only with a test that could fail it, so the page states one, and then shows
 * where Pleris stands on it: the bugs it refuses, and the rows it leads and
 * trails, by the instrument's own verdicts.
 */
function home(catalog, rec) {
  const { mutants, scripts } = mutationCounts();
  const body = `<section class="hero">
<span class="pill">A research project · not released yet</span>
<h1>Building the perfect web framework.</h1>
<p class="lead">Pleris is a language for web apps: their pages, and the server code those pages call. You declare what a program means, such as whose data it holds and how fresh an answer must be, and the compiler refuses it until that holds.</p>
<p class="lead">Perfect has a test: every bug a compiler can rule out is refused before it ships, and no other framework wins a measured row. Pleris refuses each bug below. ${standing(rec)}</p>
<div class="actions">
<a class="button primary" href="#refused">What it refuses</a>
<a class="button" href="/guide/">Read the guide</a>
<a class="button" href="${REPO}">GitHub</a>
</div>
</section>

<section class="section" id="refused">
<h2>It ships in Next.js and React. It does not compile in Pleris.</h2>
<p class="sub">Each refusal is the guide's own example, which a test runs through the compiler.</p>
${MISTAKES.map(mistake).join("\n")}
<p>Three of the ${catalog.active.length} codes the compiler refuses a program with, each with its rule and a page: <a href="/e/">every code</a>.</p>
</section>

<section class="section" id="speed">
<h2>Measured, losses included</h2>
${speed(rec)}
</section>

<section class="section" id="checked">
<h2>How we know</h2>
<p>A test that cannot fail checks nothing, so Pleris's tests are tested too: ${scripts} scripts plant ${mutants.toLocaleString("en-US")} bugs in its own source, one at a time, and each must make a test fail. Every refusal on this site is the guide's example, compiled by a test. Every code has a page an agent can read, and <a href="/llms.txt">llms.txt</a> gives it the whole guide. <a href="${TREE}/scripts">The scripts</a></p>
</section>

<section class="section" id="status">
<h2>Not finished</h2>
<p class="sub">Pleris is not released. Before you build on it:</p>
<ul class="points">
<li>An app's storage is written in Rust, behind host bindings.</li>
<li>Sign-in is a development provider, and there is one server.</li>
<li>There is no static or edge build: an app runs on its own server.</li>
<li>There is no <code>pw new</code>, no project file and no editor support.</li>
<li>Some checks have gaps: <a href="/guide/why#what-it-does-not-do-yet">the guide lists them</a>.</li>
</ul>
<p class="invite">If this is the standard you want to build to, <a href="/guide/">start with the guide</a>, or follow the work on <a href="${REPO}">GitHub</a>.</p>
</section>`;
  return {
    path: "/",
    title: "Pleris: building the perfect web framework",
    description:
      "Pleris is a language for web apps whose compiler refuses whole classes of bugs before they ship: one reader's data served to another, a secret in the page, an old answer shown for a new question. Measured against Next.js, SvelteKit, React and Svelte, losses included.",
    body,
  };
}

/** **Every row the head-to-head measures** (`/measured`): each app's rows
 * in the instrument's own words, with its verdict on each. The home page's
 * tables are chosen from these. A row of an app the site does not name
 * stops the build. */
export function measured(rec) {
  const count = (verdict) => rec.rows.filter((r) => r.verdict === verdict).length;
  const stray = rec.rows.find((r) => !SPEED.some((t) => t.app === r.app));
  if (stray) throw new Error(`${RECORD}: a row of an app the site does not name, ${stray.app}`);
  const tables = SPEED.map(
    (t) => `<h2 id="${t.app}">${escapeHtml(t.title)}</h2>
${verdictTable(rec.rows.filter((r) => r.app === t.app).map((r) => [r.what, r]))}`,
  );
  const shaped = rec.network ? ` The rows marked slow-4g are shaped as ${escapeHtml(rec.network)}.` : "";
  return {
    path: "/measured",
    title: "Every row measured",
    description: `Every row of Pleris's head-to-head against React, Svelte, Next.js and SvelteKit, with the instrument's verdict on each: ${count("win")} rows where Pleris is ahead, ${count("loss")} where another stack is.`,
    body: `<article class="prose wide">
<h1>Every row measured</h1>
<p class="lead">Each row of the head-to-head that Pleris is in, in the instrument's words, with its verdict: Pleris ahead of every other stack in ${count("win")}, another stack ahead of Pleris in ${count("loss")}, and neither in ${count("neither")}.</p>
<p>${provenance(rec)} A time is the median of ${rec.runs} runs, and a server's number the median of ${rec.serverRuns}.${shaped} A marked number is the instrument's verdict, and the last column names who is ahead. A row near the line between two verdicts can fall either side of it from one run to the next.</p>
${tables.join("\n")}
</article>`,
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
  const rec = record();
  pages.push(...codePages(catalog), home(catalog, rec), measured(rec), notFound(false), notFound(true));
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
