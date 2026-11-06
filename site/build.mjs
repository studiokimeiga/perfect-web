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
  { name: "README", path: "/guide/", short: "Start here" },
  { name: "why", path: "/guide/why", short: "Why Pleris" },
  { name: "tour", path: "/guide/tour", short: "The language" },
  { name: "tour-server", path: "/guide/tour-server", short: "Data on the server" },
  { name: "tour-pages", path: "/guide/tour-pages", short: "Pages" },
  { name: "errors", path: "/guide/errors", short: "Errors and diagnostics" },
  { name: "tutorial", path: "/guide/tutorial", short: "Tutorial: a guestbook" },
  { name: "diagnostics", path: "/guide/diagnostics", short: "The diagnostic codes" },
];

/** The file a path is served from (`html_handling`'s `auto-trailing-slash`). */
const fileOf = (path) => (path.endsWith("/") ? `${path}index.html` : `${path}.html`).slice(1);

/** A link the guide writes, as the site serves it: a page of the guide is the
 * site's, anything else in the repository is GitHub's. */
function guideLink(url) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith("//") || url.startsWith("#")) return url;
  const [path, hash] = url.split("#");
  const anchor = hash ? `#${hash}` : "";
  const page = GUIDE.find((p) => path === `${p.name}.md` || path === `./${p.name}.md`);
  if (page) return page.path + anchor;
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
.hero figure.code pre{font-size:.74rem}
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

/** A page's first paragraph, as plain text for its description. */
function summaryOf(html, fallback) {
  const p = /<p>([\s\S]*?)<\/p>/.exec(html)?.[1] ?? fallback;
  const text = p
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > 158 ? `${text.slice(0, 155).replace(/\s+\S*$/, "")}…` : text;
}

// --- the guide --------------------------------------------------------------

/** Each code block the guide refuses with a code, by code: the program, the
 * compiler's words where the page shows them, and where it stands. */
const examples = new Map();

function guidePage(entry, index) {
  const md = readFileSync(join(ROOT, "docs/guide", `${entry.name}.md`), "utf8");
  let previous = null;
  const code = (text, info, context) => {
    const [lang = "", ...rest] = info.split(/\s+/).filter(Boolean);
    const attrs = Object.fromEntries(rest.filter((r) => r.includes("=")).map((r) => r.split("=", 2)));
    const flags = rest.filter((r) => !r.includes("="));
    const fails = attrs.fails ? attrs.fails.split(",") : [];
    const where = { page: entry, heading: context.heading };
    const output = lang === "text" && flags.includes("pw-check");
    if (fails.length) {
      previous = { text, file: attrs.file ?? null, fails, where, output: null };
      for (const c of fails) {
        if (!examples.has(c)) examples.set(c, []);
        examples.get(c).push(previous);
      }
    } else if (output && previous && previous.where.heading === context.heading) {
      previous.output = text;
      previous = null;
    } else {
      previous = null;
    }
    return codeFigure(text, { lang, file: attrs.file, fails, gap: attrs.gap, output });
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

/** A block of code: Pleris coloured, the compiler's output as it prints it,
 * and above it what the guide says of it. */
function codeFigure(text, { lang, file, fails = [], gap, output }) {
  const caption = [];
  if (file) caption.push(`<span>${escapeHtml(file)}</span>`);
  if (output) caption.push("<span>pw check</span>");
  if (fails.length) {
    caption.push(`<span class="refused">Refused: ${fails.map((c) => `<a href="/e/${c}">${c}</a>`).join(", ")}</span>`);
  }
  if (gap) caption.push(`<span>A gap: <code>pw ${escapeHtml(gap)}</code> passes this today</span>`);
  const body = lang === "pw" ? highlightPw(text) : escapeHtml(text);
  return `<figure class="code${output ? " output" : ""}">${caption.length ? `<figcaption>${caption.join("")}</figcaption>` : ""}<pre tabindex="0"><code${lang ? ` class="language-${escapeHtml(lang)}"` : ""}>${body}</code></pre></figure>`;
}

// --- the codes --------------------------------------------------------------

/** The diagnostic codes, from the guide's list of them. */
function codes() {
  const lines = readFileSync(join(ROOT, "docs/guide/diagnostics.md"), "utf8").split("\n");
  const active = [];
  const retired = [];
  const renumbered = [];
  let section = null;
  for (let i = 0; i < lines.length; i++) {
    const heading = /^## (.+)$/.exec(lines[i]);
    if (heading) {
      section = heading[1].trim();
      continue;
    }
    if (!lines[i].startsWith("|") || !/^\|[\s:|-]+\|?\s*$/.test(lines[i + 1] ?? "")) continue;
    const head = cellsOf(lines[i]);
    for (i += 2; i < lines.length && lines[i].startsWith("|"); i++) {
      const cells = cellsOf(lines[i]);
      const row = Object.fromEntries(head.map((h, k) => [h, cells[k] ?? ""]));
      if (section === "Retired codes") retired.push({ code: row.Code, why: row.Why });
      else if (section === "Old numbers") renumbered.push({ code: row["Old number"], now: row.Now, symbol: row.Symbol });
      else active.push({ code: row.Code, symbol: row.Symbol.replace(/`/g, ""), rule: row["What the program must hold to"], adr: row.ADR, section });
    }
    i--;
  }
  return { active, retired, renumbered };
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
  const adrs = entry.adr && entry.adr !== "—" ? `<h2>The decisions behind it</h2><p>${md(entry.adr)}</p>` : "";
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

/** What the compiler refuses: the guide's own six (docs/guide/why.md). */
const REFUSALS = [
  ["A user's data in a cache everyone shares", "A query says whose data it returns, and a session's data cannot sit in a shared cache.", "PW5001"],
  ["Rendering that reaches the database", "A view declares that it has no effects, and the compiler checks the claim through every call it makes.", "PW0401"],
  ["A function that says it is pure and is not", "An effect row is a claim: a helper that reads the clock makes every caller that claims purity wrong.", "PW0400"],
  ["A case forgotten", "Every match covers every case, and a misspelt case is a case the type lacks, never a catch-all.", "PW0305"],
  ["A link to nowhere", "Every internal link is matched against the program's routes.", "PW5009"],
  ["A form control nobody can name", "A control a screen reader cannot announce is refused, as are a duplicate id and a page with no title.", "PW5014"],
];

/** The head-to-head's record (ADR-0392's instrument), from PR #76's evidence. */
const EVIDENCE = `${REPO}/blob/perf/first-press-20261106/docs/evidence/E14/first-press-2026-11-06/README.md`;
const BENCH = [
  ["TodoMVC: the first todo added, to the frame that shows it (Slow 4G)", "31 ms", "33 ms", "34 ms", "React", "Svelte"],
  ["A store: ready to use on a first visit (Slow 4G)", "1,381 ms", "2,067 ms", "1,749 ms", "Next.js", "SvelteKit"],
  ["A store: ready to use on the next visit (Slow 4G)", "626 ms", "660 ms", "638 ms", "Next.js", "SvelteKit"],
  ["A store: a press with 19 other tabs of it open", "21 ms", "43 ms", "44 ms", "Next.js", "SvelteKit"],
  ["A store: server CPU for each request, under load", "0.20 ms", "5.12 ms", "1.17 ms", "Next.js", "SvelteKit"],
];

function home(codeCount) {
  const sample = (examples.get("PW0606") ?? []).find((ex) => ex.output);
  if (!sample) throw new Error("the guide's PW0606 example and its output: the home page shows them");
  const cards = REFUSALS.map(
    ([title, text, code]) =>
      `<li class="card"><h3>${escapeHtml(title)}</h3><p>${escapeHtml(text)}</p><a class="code-link" href="/e/${code}">${code}</a></li>`,
  ).join("\n");
  const rows = BENCH.map(
    ([what, pleris, a, b, an, bn]) =>
      `<tr><td>${escapeHtml(what)}</td><td class="best">${pleris}</td><td>${a} <small>${an}</small></td><td>${b} <small>${bn}</small></td></tr>`,
  ).join("\n");
  const body = `<section class="hero">
<div>
<span class="pill">A research project · not released yet</span>
<h1>Web apps whose compiler refuses whole classes of bugs.</h1>
<p class="lead">Pleris is a language for web apps, with a compiler and a server for it. You write the data, the server's reads and writes, and the pages in one language, and the compiler refuses programs with bugs that TypeScript, React, Next.js and Svelte let through to production.</p>
<div class="actions">
<a class="button primary" href="/guide/">Read the guide</a>
<a class="button" href="/guide/why">Why Pleris</a>
<a class="button" href="${REPO}">GitHub</a>
</div>
</div>
<div>
${codeFigure(sample.text, { lang: "pw", file: "price.pw" })}
${codeFigure(sample.output, { lang: "text", output: true })}
</div>
</section>

<section class="section">
<h2>What the compiler refuses</h2>
<p class="sub">Each refusal has a code, a one-sentence rule and the decision behind it: ${codeCount} codes, from the compiler's own registry.</p>
<ul class="cards">
${cards}
</ul>
<p><a href="/e/">Every error code</a> · <a href="/guide/why">What it refuses, and what it does not do yet</a></p>
</section>

<section class="section">
<h2>Fast where a reader feels it</h2>
<p class="sub">The same apps in Pleris, React, Svelte, Next.js and SvelteKit, measured by the project's own instrument: Chromium on one development PC, medians of five runs; Slow 4G is Lighthouse's profile of a slow phone network.</p>
<div class="table" tabindex="0"><table class="bench"><thead><tr><th scope="col">Measured</th><th scope="col">Pleris</th><th scope="col">React or Next.js</th><th scope="col">Svelte or SvelteKit</th></tr></thead><tbody>
${rows}
</tbody></table></div>
<p class="note">Where it still loses: the first download (TodoMVC sends 142 KB, Svelte 21 KB), Svelte's first visit by about 120 ms, and latency under load against SvelteKit, whose server keeps connections open. These numbers come from a branch still under review (PR #76), and field performance is not claimed. <a href="${EVIDENCE}">The runs, the commands and every row, losses first.</a></p>
</section>

<section class="section two">
<div>
<h2>Every example compiles</h2>
<p>Each example in the guide is a whole program, and a test runs every one through the real compiler: one meant to compile must build, one meant to fail must fail with exactly the codes it names, and where a page shows the compiler's output, the compiler must print exactly that.</p>
<p><a href="/guide/tutorial">Build a guestbook with sign-in</a>, end to end.</p>
</div>
<div>
<h2>What it does not do yet</h2>
<ul>
<li>An app's storage is written in Rust, behind host bindings.</li>
<li>Sign-in is a development provider, and there is one server.</li>
<li>No <code>pw new</code>, no project file and no editor support.</li>
<li>Some checks have gaps, each shown in the guide.</li>
</ul>
<p><a href="/guide/why#what-it-does-not-do-yet">The whole list, honestly.</a></p>
</div>
</section>`;
  return {
    path: "/",
    title: "Pleris: web apps whose compiler refuses whole classes of bugs",
    description:
      "Pleris is a language for web apps, with a compiler and a server for it. The compiler refuses programs with bugs that TypeScript, React, Next.js and Svelte let through.",
    body,
  };
}

function notFound(inCodes) {
  return {
    path: inCodes ? "/e/404" : "/404",
    title: inCodes ? "No such code" : "Not found",
    description: "There is no page here.",
    body: `<article class="prose">
<h1>${inCodes ? "No such code" : "Not found"}</h1>
<p class="lead">${inCodes ? "The compiler prints no code with this number." : "There is no page at this address."}</p>
<p>${inCodes ? '<a href="/e/">Every code</a>, with its rule.' : '<a href="/">Pleris</a> · <a href="/guide/">The guide</a> · <a href="/e/">Error codes</a>'}</p>
</article>`,
  };
}

// --- the policy and the rest -------------------------------------------------

/** What each response says beside the page (`_headers`): the pages' policy
 * allows their one style, by its hash, the icon from this origin, and
 * nothing else; no page may be framed or framed into another's. */
function headers() {
  return `/*
  Content-Security-Policy: default-src 'none'; style-src '${STYLE_HASH}'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
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
  const pages = GUIDE.map((entry, i) => guidePage(entry, i));
  const catalog = codes();
  pages.push(codesIndex(catalog));
  for (const c of catalog.active) pages.push(codePage(c));
  for (const c of catalog.retired) pages.push(retiredPage(c));
  for (const c of catalog.renumbered) pages.push(renumberedPage(c));
  pages.push(home(catalog.active.length), notFound(false), notFound(true));
  const written = new Map();
  for (const p of pages) {
    const file = join(out, fileOf(p.path));
    mkdirSync(dirname(file), { recursive: true });
    const html = page(p);
    writeFileSync(file, html);
    written.set(p.path, html);
  }
  for (const f of readdirSync(join(SITE, "static"))) copyFileSync(join(SITE, "static", f), join(out, f));
  writeFileSync(join(out, "_headers"), headers());
  const listed = pages.filter((p) => !p.path.endsWith("404")).map((p) => `<url><loc>${ORIGIN}${p.path}</loc></url>`);
  writeFileSync(
    join(out, "sitemap.xml"),
    `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${listed.join("\n")}\n</urlset>\n`,
  );
  return { written, catalog };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const out = process.argv[2] ?? join(SITE, "dist");
  const { written, catalog } = build(out);
  console.log(`pleris.dev: ${written.size} pages (${catalog.active.length} codes) in ${out}`);
}
