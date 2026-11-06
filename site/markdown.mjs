// **The Markdown the guide writes, read into HTML** (site/README.md):
// headings with the ids GitHub gives them, paragraphs, lists, GitHub's
// tables, fenced code with its info string, and in a line: code, links,
// autolinks, emphasis and strong. Nothing else is markup. Raw HTML in prose is
// text, escaped, and a comment is dropped, so a page can carry no script the
// site's policy did not write. `build.test.mjs` holds each construct, and the
// whole guide.
//
// Each block and each piece of a line is read by its own reader, tried in
// order: a reader answers with its HTML and where it ends, or with nothing,
// and the next is tried.

/** Text as HTML: safe in an element's content and in a quoted attribute. */
export function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

/** A heading's text without its inline markup: a link's label, code's text. */
function plainText(md) {
  return md
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/`+([^`]*)`+/g, "$1")
    .replace(/\*\*|\*/g, "")
    .replace(/\\(.)/g, "$1")
    .trim();
}

/** **A heading's id as GitHub writes it**, so that the guide's links to an
 * anchor (`tour-pages.md#where-pw-check-stops`) land: its plain text in lower
 * case, without what is not a letter, a mark, a digit, `_`, `-` or a space,
 * each space a `-`; and a later heading of the same text `-1`, `-2`. */
function slugger() {
  const seen = new Map();
  return (text) => {
    const base = text.toLowerCase().replace(/[^\p{L}\p{M}\p{N}_\- ]/gu, "").replace(/ /g, "-");
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return n === 0 ? base : `${base}-${n}`;
  };
}

const FENCE = /^( {0,3})(`{3,}|~{3,})(.*)$/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const MARKER = /^( *)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const DELIMITER = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const COMMENT = /^\s*<!--/;
const BLANK = /^\s*$/;

/**
 * `src` as HTML. `options.link(url)` gives each link's address (the site
 * rewrites the guide's own); `options.code(text, info, context)` writes each
 * fenced block (`context.heading`: the heading it falls under); and
 * `options.cell(html, column)` may rewrite a table's cell. Returns the HTML,
 * its headings and its title (its first `#` heading's text).
 */
export function renderMarkdown(src, options = {}) {
  const ctx = contextOf(options);
  const html = blocks(src.replace(/\r\n?/g, "\n").split("\n"), ctx);
  const title = ctx.headings.find((h) => h.level === 1)?.text ?? "";
  return { html, headings: ctx.headings, title };
}

function contextOf(options) {
  return {
    link: options.link ?? ((url) => url),
    code: options.code ?? ((text) => `<pre tabindex="0"><code>${escapeHtml(text)}</code></pre>`),
    cell: options.cell ?? ((html) => html),
    slug: slugger(),
    headings: [],
    heading: null,
  };
}

// --- blocks ------------------------------------------------------------------

/** The lines that begin a block, and so end a paragraph. */
const STARTS = [FENCE, HEADING, MARKER, COMMENT];

function startsBlock(line, next) {
  return STARTS.some((re) => re.test(line)) || opensTable(line, next);
}

/** A table: a row, and under it the row of its columns' alignments. */
function opensTable(line, next) {
  return /^\s*\|/.test(line) && next !== undefined && DELIMITER.test(next);
}

/** Each kind of block, in the order a line is read as one: a paragraph is
 * what a line is that is nothing else. A block whose `html` is null is
 * dropped. */
const BLOCKS = [blank, comment, fenced, headingAt, tableAt, listBlock, paragraph];

function blocks(lines, ctx) {
  const out = [];
  for (let i = 0; i < lines.length; ) {
    const block = blockAt(lines, i, ctx);
    if (block.html !== null) out.push(block.html);
    i = block.next;
  }
  return out.join("\n");
}

function blockAt(lines, i, ctx) {
  for (const read of BLOCKS) {
    const block = read(lines, i, ctx);
    if (block) return block;
  }
}

function blank(lines, i) {
  return BLANK.test(lines[i]) ? { html: null, next: i + 1 } : null;
}

/** A comment, dropped: its lines, to the one that closes it. */
function comment(lines, i) {
  if (!COMMENT.test(lines[i])) return null;
  let k = i;
  while (k < lines.length && !lines[k].includes("-->")) k++;
  return { html: null, next: k + 1 };
}

function fenced(lines, i, ctx) {
  const fence = FENCE.exec(lines[i]);
  if (!fence) return null;
  const [, indent, marker, info] = fence;
  const close = new RegExp(`^ {0,3}${marker[0]}{${marker.length},}\\s*$`);
  const body = [];
  let k = i + 1;
  while (k < lines.length && !close.test(lines[k])) {
    const lead = lines[k].search(/\S|$/);
    body.push(lines[k].slice(Math.min(indent.length, lead)));
    k++;
  }
  return { html: ctx.code(body.join("\n"), info.trim(), { heading: ctx.heading }), next: k + 1 };
}

function headingAt(lines, i, ctx) {
  const heading = HEADING.exec(lines[i]);
  if (!heading) return null;
  const level = heading[1].length;
  const text = plainText(heading[2]);
  const id = ctx.slug(text);
  ctx.headings.push({ level, text, id });
  ctx.heading = { level, text, id };
  return { html: `<h${level} id="${id}">${inline(heading[2], ctx)}</h${level}>`, next: i + 1 };
}

function tableAt(lines, i, ctx) {
  if (!opensTable(lines[i], lines[i + 1])) return null;
  let k = i;
  while (k < lines.length && /^\s*\|/.test(lines[k])) k++;
  return { html: table(lines.slice(i, k), ctx), next: k };
}

function listBlock(lines, i, ctx) {
  return MARKER.test(lines[i]) ? listAt(lines, i, ctx) : null;
}

/** A paragraph: its first line, and each after it that is not blank and
 * begins no block. */
function paragraph(lines, i, ctx) {
  let k = i + 1;
  while (k < lines.length && continuesParagraph(lines[k], lines[k + 1])) k++;
  const text = lines.slice(i, k).map((line) => line.trim()).join("\n");
  return { html: `<p>${inline(text, ctx)}</p>`, next: k };
}

function continuesParagraph(line, next) {
  return !BLANK.test(line) && !startsBlock(line, next);
}

// --- tables ------------------------------------------------------------------

/** A row's cells, split at each `|` that is not escaped; `\|` is a `|` in its
 * cell's text, in code too, as GitHub reads a table. */
export function cellsOf(row) {
  return withoutEdgePipes(row.trim())
    .split(/(?<!\\)\|/)
    .map((cell) => cell.replace(/\\\|/g, "|").trim());
}

/** A row without the `|` that opens it and the one that closes it, where it
 * has them; an escaped `\|` closes nothing. */
function withoutEdgePipes(s) {
  const from = s.startsWith("|") ? 1 : 0;
  const to = s.endsWith("|") && !s.endsWith("\\|") ? -1 : s.length;
  return s.slice(from, to);
}

function table(rows, ctx) {
  const head = cellsOf(rows[0]);
  const aligns = cellsOf(rows[1]).map((d) => (d.startsWith(":") && d.endsWith(":") ? "center" : d.endsWith(":") ? "right" : null));
  // A class, not a `style` attribute: the site's policy allows its one
  // stylesheet by hash, and no attribute's style.
  const style = (c) => (aligns[c] ? ` class="align-${aligns[c]}"` : "");
  const th = head.map((h, c) => `<th scope="col"${style(c)}>${inline(h, ctx)}</th>`).join("");
  const body = rows
    .slice(2)
    .map((row) => {
      const cells = cellsOf(row);
      const tds = head.map((_, c) => `<td${style(c)}>${ctx.cell(inline(cells[c] ?? "", ctx), c)}</td>`).join("");
      return `<tr>${tds}</tr>`;
    })
    .join("\n");
  return `<div class="table" tabindex="0"><table>\n<thead><tr>${th}</tr></thead>\n<tbody>\n${body}\n</tbody>\n</table></div>`;
}

// --- lists -------------------------------------------------------------------

/** A list, and each of its items: the lines after its marker, and those
 * indented past it (a blank line inside an item where the next line is). */
function listAt(lines, start, ctx) {
  const first = MARKER.exec(lines[start]);
  const list = { indent: first[1].length, ordered: /\d/.test(first[2]), loose: false };
  const items = [];
  let i = start;
  for (let m = first; sameList(m, list); m = MARKER.exec(lines[i] ?? "")) {
    const item = itemAt(lines, i, m, list);
    items.push(item.body);
    i = afterItem(lines, item.next, list);
  }
  return { html: listHtml(items, first, list, ctx), next: i };
}

/** Whether a marker is one of this list's: at its indent, and numbered as
 * its first is. */
function sameList(m, list) {
  return m !== null && m[1].length === list.indent && /\d/.test(m[2]) === list.ordered;
}

/** An item's lines: the text after its marker, and each line after that
 * belongs to it (`itemLine`). */
function itemAt(lines, i, m, list) {
  const contentAt = m[0].length - m[3].length;
  const body = [m[3]];
  let k = i + 1;
  for (let line = itemLine(lines, k, contentAt, list); line !== null; line = itemLine(lines, k, contentAt, list)) {
    body.push(line);
    k++;
  }
  return { body, next: k };
}

/** Line `k` as its item holds it, from the column its text starts at; or
 * null, where the item has ended. */
function itemLine(lines, k, contentAt, list) {
  if (k >= lines.length) return null;
  if (BLANK.test(lines[k])) return blankInItem(lines, k, list);
  const lead = lines[k].search(/\S/);
  return lead > list.indent ? lines[k].slice(Math.min(lead, contentAt)) : null;
}

/** A blank line inside an item, where the next line that is not blank is
 * indented past the list's marker; the list is then loose. */
function blankInItem(lines, k, list) {
  const j = nextFilled(lines, k);
  if (j >= lines.length || lines[j].search(/\S/) <= list.indent) return null;
  list.loose = true;
  return "";
}

/** The first line at or after `k` that is not blank, or the end. */
function nextFilled(lines, k) {
  let j = k;
  while (j < lines.length && BLANK.test(lines[j])) j++;
  return j;
}

/** Where a list goes on after an item: past the blank lines before its next
 * item, which make it loose; or at the item's end. */
function afterItem(lines, k, list) {
  const j = nextFilled(lines, k);
  if (j === k || !sameList(MARKER.exec(lines[j] ?? ""), list)) return k;
  list.loose = true;
  return j;
}

function listHtml(items, first, list, ctx) {
  const tag = list.ordered ? "ol" : "ul";
  const start = list.ordered ? Number.parseInt(first[2], 10) : 1;
  const html = items.map((body) => `<li>${itemHtml(body, list, ctx)}</li>`).join("\n");
  return `<${tag}${start !== 1 ? ` start="${start}"` : ""}>\n${html}\n</${tag}>`;
}

/** An item's blocks. A tight list's item holds its text, not a paragraph
 * of it. */
function itemHtml(body, list, ctx) {
  const inner = blocks(body, ctx);
  return list.loose ? inner : inner.replace(/^<p>([\s\S]*?)<\/p>/, "$1");
}

// --- a line ------------------------------------------------------------------

const ESCAPABLE = /[\\`*_{}[\]()#+\-.!|<>~]/;

/** Each piece of markup a line holds, in the order a character is read as
 * one; a character that begins none is text. */
const INLINES = [escapeAt, codeSpanAt, linkHere, autolinkAt, strongAt, emphasisAt];

/** One line's markup, its text escaped. */
export function inline(text, ctx = { link: (u) => u }) {
  let out = "";
  for (let i = 0; i < text.length; ) {
    const piece = inlineAt(text, i, ctx);
    out += piece.html;
    i = piece.end;
  }
  return out;
}

function inlineAt(text, i, ctx) {
  for (const read of INLINES) {
    const piece = read(text, i, ctx);
    if (piece) return piece;
  }
  return { html: escapeHtml(text[i]), end: i + 1 };
}

/** A backslash before a character Markdown could read as markup: the
 * character, as text. */
function escapeAt(text, i) {
  return text[i] === "\\" && ESCAPABLE.test(text[i + 1] ?? "") ? { html: escapeHtml(text[i + 1]), end: i + 2 } : null;
}

/** Code: from a run of backticks to the next run of as many. With none, the
 * run is text. */
function codeSpanAt(text, i) {
  if (text[i] !== "`") return null;
  const n = /^`*/.exec(text.slice(i))[0].length;
  const close = closingRun(text, i + n, n);
  return close === -1
    ? { html: "`".repeat(n), end: i + n }
    : { html: `<code>${escapeHtml(codeText(text.slice(i + n, close)))}</code>`, end: close + n };
}

/** Where the next run of exactly `n` backticks from `from` starts, or -1. */
function closingRun(text, from, n) {
  const runs = /`+/g;
  runs.lastIndex = from;
  for (let m = runs.exec(text); m; m = runs.exec(text)) {
    if (m[0].length === n) return m.index;
  }
  return -1;
}

/** Code's text: each line end a space, and one space taken from each end
 * where both have one and the code is not all spaces. */
function codeText(raw) {
  const code = raw.replace(/\n/g, " ");
  return /^ [\s\S]*\S[\s\S]* $/.test(code) ? code.slice(1, -1) : code;
}

function linkHere(text, i, ctx) {
  const link = text[i] === "[" ? linkAt(text, i) : null;
  return link && { html: `<a href="${escapeHtml(ctx.link(link.url))}">${inline(link.label, ctx)}</a>`, end: link.end };
}

function autolinkAt(text, i) {
  const auto = text[i] === "<" ? /^<(https?:\/\/[^\s<>]+)>/.exec(text.slice(i)) : null;
  return auto && { html: `<a href="${escapeHtml(auto[1])}">${escapeHtml(auto[1])}</a>`, end: i + auto[0].length };
}

/** `**strong**`: opened before a character that is not a space, and closed
 * after one. */
function strongAt(text, i, ctx) {
  const close = /^\*\*[^ ]/.test(text.slice(i, i + 3)) ? text.indexOf("**", i + 2) : -1;
  return close > i + 2 && text[close - 1] !== " "
    ? { html: `<strong>${inline(text.slice(i + 2, close), ctx)}</strong>`, end: close + 2 }
    : null;
}

/** `*emphasis*` or `_emphasis_`; a `_` inside a word, as in `snake_case`,
 * opens and closes nothing. */
function emphasisAt(text, i, ctx) {
  const close = opensEmphasis(text, i) ? emphasisClose(text, i) : -1;
  return close > i + 1 ? { html: `<em>${inline(text.slice(i + 1, close), ctx)}</em>`, end: close + 1 } : null;
}

function opensEmphasis(text, i) {
  const c = text[i];
  return "*_".includes(c) && ![undefined, " ", c].includes(text[i + 1]) && flanks(c, text[i - 1]);
}

/** Where the emphasis opened at `i` closes, or -1. Code inside emphasis is
 * read whole: its `*` or `_` closes nothing. */
function emphasisClose(text, i) {
  for (let k = i + 1; k < text.length; k = pastCode(text, k)) {
    if (closesEmphasis(text, k, text[i])) return k;
  }
  return -1;
}

/** The position after `k`: past code, where `k` opens it, and the end where
 * that code does not close. */
function pastCode(text, k) {
  if (text[k] !== "`") return k + 1;
  const end = text.indexOf("`", k + 1);
  return end === -1 ? text.length : end + 1;
}

function closesEmphasis(text, k, c) {
  return text[k] === c && text[k - 1] !== " " && text[k + 1] !== c && flanks(c, text[k + 1]);
}

/** Whether `c` may open or close emphasis beside `neighbour`: a `*` beside
 * anything, a `_` beside no letter or digit. */
function flanks(c, neighbour) {
  return c === "*" || !word(neighbour);
}

function word(c) {
  return c !== undefined && /[\p{L}\p{N}]/u.test(c);
}

// --- links -------------------------------------------------------------------

const BRACKETS = new Map([
  ["[", 1],
  ["]", -1],
]);
const PARENS = new Map([
  ["(", 1],
  [")", -1],
]);

/** `[label](url)` at `at`: its label, address and end, or none. */
function linkAt(text, at) {
  const k = labelEnd(text, at);
  const e = k !== -1 && text[k + 1] === "(" ? urlEnd(text, k + 2) : -1;
  return e === -1 ? null : { label: text.slice(at + 1, k), url: text.slice(k + 2, e), end: e + 1 };
}

/** Where the label opened at `at` closes: its `]`, past escapes, code and
 * the brackets nested in it; or -1. */
function labelEnd(text, at) {
  let depth = 0;
  for (let k = at; k < text.length; k = labelStep(text, k)) {
    depth += BRACKETS.get(text[k]) ?? 0;
    if (depth === 0) return k;
  }
  return -1;
}

/** The position a label is read at after `k`: past an escaped character, or
 * past code; the end, where that code does not close. */
function labelStep(text, k) {
  return text[k] === "\\" ? k + 2 : pastCode(text, k);
}

/** Where the address opened at `from` closes: its `)`, past the parentheses
 * nested in it; or -1, where whitespace or the end comes first. */
function urlEnd(text, from) {
  const at = closingParen(text.slice(from).split(/\s/, 1)[0]);
  return at === -1 ? -1 : from + at;
}

/** Where `s`'s first `)` that closes nothing opened in it is, or -1. */
function closingParen(s) {
  let parens = 0;
  for (let e = 0; e < s.length; e++) {
    parens += PARENS.get(s[e]) ?? 0;
    if (parens < 0) return e;
  }
  return -1;
}

// --- Pleris ------------------------------------------------------------------

const KEYWORDS = new Set(
  (
    "module import type opaque fn view page route query command signal source event predicate layout " +
    "let match if else for in as each return host requires says holds reads emits changes publish " +
    "cache key freshness consistency concurrency timeout on_key_change invalidates_on idempotent_by " +
    "optimistic placement transactions schedule invoked_by not_found_on session user public private " +
    "shared read_your_writes one_per_key cancel true false"
  ).split(" "),
);

/** The class of each of `highlightPw`'s groups, in order: a comment, a
 * string, a block's keyword, a tag, a number, a type. A word, the seventh,
 * is a keyword or nothing. */
const KINDS = [null, "c", "s", "k", "tag", "n", "t"];

/** **Pleris source, coloured**: comments, strings, numbers, keywords, types
 * and constructors, and the tags of a view's markup. Only `<span>`s are
 * added: the text the reader copies is the program as written. */
export function highlightPw(src) {
  const re =
    /(\/\/[^\n]*)|("(?:[^"\\\n]|\\.)*")|(\{[#:/][a-z]+)|(<\/?[a-z][\w-]*)|(\b\d[\d_]*(?:\.\d+)?\b)|(\b[A-Z][A-Za-z0-9_]*\b)|(\b[a-z_][A-Za-z0-9_]*\b)/g;
  let out = "";
  let last = 0;
  for (const m of src.matchAll(re)) {
    out += escapeHtml(src.slice(last, m.index));
    const kind = kindOf(m);
    out += kind ? `<span class="${kind}">${escapeHtml(m[0])}</span>` : escapeHtml(m[0]);
    last = m.index + m[0].length;
  }
  return out + escapeHtml(src.slice(last));
}

function kindOf(m) {
  const group = m.findIndex((g, n) => n > 0 && g);
  return KINDS[group] ?? (KEYWORDS.has(m[0]) ? "k" : null);
}
