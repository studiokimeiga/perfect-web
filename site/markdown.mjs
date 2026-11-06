// **The Markdown the guide writes, read into HTML** (site/README.md):
// headings with the ids GitHub gives them, paragraphs, lists, GitHub's
// tables, fenced code with its info string, and in a line: code, links,
// autolinks, emphasis and strong. Nothing else is markup. Raw HTML in prose is
// text, escaped, and a comment is dropped, so a page can carry no script the
// site's policy did not write. `build.test.mjs` holds each construct, and the
// whole guide.

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

/**
 * `src` as HTML. `options.link(url)` gives each link's address (the site
 * rewrites the guide's own); `options.code(text, info, context)` writes each
 * fenced block (`context.heading`: the heading it falls under); and
 * `options.cell(html, column)` may rewrite a table's cell. Returns the HTML,
 * its headings and its title (its first `#` heading's text).
 */
export function renderMarkdown(src, options = {}) {
  const ctx = {
    link: options.link ?? ((url) => url),
    code: options.code ?? ((text) => `<pre tabindex="0"><code>${escapeHtml(text)}</code></pre>`),
    cell: options.cell ?? ((html) => html),
    slug: slugger(),
    headings: [],
    heading: null,
  };
  const html = blocks(src.replace(/\r\n?/g, "\n").split("\n"), ctx);
  const title = ctx.headings.find((h) => h.level === 1)?.text ?? "";
  return { html, headings: ctx.headings, title };
}

function startsBlock(line, next) {
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    MARKER.test(line) ||
    /^\s*<!--/.test(line) ||
    (/^\s*\|/.test(line) && next !== undefined && DELIMITER.test(next))
  );
}

function blocks(lines, ctx) {
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^\s*$/.test(line)) {
      i++;
      continue;
    }
    if (/^\s*<!--/.test(line)) {
      while (i < lines.length && !lines[i].includes("-->")) i++;
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const [, indent, marker, info] = fence;
      const close = new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}\\s*$`);
      const body = [];
      i++;
      while (i < lines.length && !close.test(lines[i])) {
        const lead = lines[i].search(/\S|$/);
        body.push(lines[i].slice(Math.min(indent.length, lead)));
        i++;
      }
      i++;
      out.push(ctx.code(body.join("\n"), info.trim(), { heading: ctx.heading }));
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      const level = heading[1].length;
      const text = plainText(heading[2]);
      const id = ctx.slug(text);
      ctx.headings.push({ level, text, id });
      ctx.heading = { level, text, id };
      out.push(`<h${level} id="${id}">${inline(heading[2], ctx)}</h${level}>`);
      i++;
      continue;
    }
    if (/^\s*\|/.test(line) && i + 1 < lines.length && DELIMITER.test(lines[i + 1])) {
      const rows = [];
      let j = i;
      while (j < lines.length && /^\s*\|/.test(lines[j])) rows.push(lines[j++]);
      out.push(table(rows, ctx));
      i = j;
      continue;
    }
    if (MARKER.test(line)) {
      const list = listAt(lines, i, ctx);
      out.push(list.html);
      i = list.next;
      continue;
    }
    const para = [];
    while (i < lines.length && !/^\s*$/.test(lines[i]) && (para.length === 0 || !startsBlock(lines[i], lines[i + 1]))) {
      para.push(lines[i].trim());
      i++;
    }
    out.push(`<p>${inline(para.join("\n"), ctx)}</p>`);
  }
  return out.join("\n");
}

/** A row's cells, split at each `|` that is not escaped; `\|` is a `|` in its
 * cell's text, in code too, as GitHub reads a table. */
export function cellsOf(row) {
  let s = row.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  const cells = [];
  let cell = "";
  for (let k = 0; k < s.length; k++) {
    if (s[k] === "\\" && s[k + 1] === "|") {
      cell += "|";
      k++;
    } else if (s[k] === "|") {
      cells.push(cell.trim());
      cell = "";
    } else {
      cell += s[k];
    }
  }
  cells.push(cell.trim());
  return cells;
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

/** A list, and each of its items: the lines after its marker, and those
 * indented past it (a blank line inside an item where the next line is). */
function listAt(lines, start, ctx) {
  const first = MARKER.exec(lines[start]);
  const indent = first[1].length;
  const ordered = /\d/.test(first[2]);
  const items = [];
  let loose = false;
  let i = start;
  while (i < lines.length) {
    const m = MARKER.exec(lines[i]);
    if (!m || m[1].length !== indent || /\d/.test(m[2]) !== ordered) break;
    const contentAt = m[0].length - m[3].length;
    const body = [m[3]];
    i++;
    while (i < lines.length) {
      const line = lines[i];
      if (/^\s*$/.test(line)) {
        let j = i + 1;
        while (j < lines.length && /^\s*$/.test(lines[j])) j++;
        if (j < lines.length && lines[j].search(/\S/) > indent) {
          body.push("");
          loose = true;
          i++;
          continue;
        }
        break;
      }
      const lead = line.search(/\S/);
      if (lead <= indent) break;
      body.push(line.slice(Math.min(lead, contentAt)));
      i++;
    }
    items.push(body);
    // A blank line between two items of one list makes it loose.
    let j = i;
    while (j < lines.length && /^\s*$/.test(lines[j])) j++;
    const next = j < lines.length ? MARKER.exec(lines[j]) : null;
    if (j > i && next && next[1].length === indent && /\d/.test(next[2]) === ordered) {
      loose = true;
      i = j;
    }
  }
  const start1 = ordered ? Number.parseInt(first[2], 10) : 1;
  const tag = ordered ? "ol" : "ul";
  const html = items
    .map((body) => {
      let inner = blocks(body, ctx);
      // A tight list's item holds its text, not a paragraph of it.
      if (!loose) inner = inner.replace(/^<p>([\s\S]*?)<\/p>/, "$1");
      return `<li>${inner}</li>`;
    })
    .join("\n");
  return { html: `<${tag}${ordered && start1 !== 1 ? ` start="${start1}"` : ""}>\n${html}\n</${tag}>`, next: i };
}

const ESCAPABLE = /[\\`*_{}[\]()#+\-.!|<>~]/;

/** One line's markup, its text escaped. */
export function inline(text, ctx = { link: (u) => u }) {
  let out = "";
  let i = 0;
  const word = (c) => c !== undefined && /[\p{L}\p{N}]/u.test(c);
  while (i < text.length) {
    const c = text[i];
    if (c === "\\" && i + 1 < text.length && ESCAPABLE.test(text[i + 1])) {
      out += escapeHtml(text[i + 1]);
      i += 2;
      continue;
    }
    if (c === "`") {
      let n = 0;
      while (text[i + n] === "`") n++;
      let close = -1;
      for (let k = i + n; k < text.length; k++) {
        if (text[k] !== "`") continue;
        let run = 0;
        while (text[k + run] === "`") run++;
        if (run === n) {
          close = k;
          break;
        }
        k += run - 1;
      }
      if (close !== -1) {
        let code = text.slice(i + n, close).replace(/\n/g, " ");
        if (code.length > 1 && code.startsWith(" ") && code.endsWith(" ") && code.trim()) code = code.slice(1, -1);
        out += `<code>${escapeHtml(code)}</code>`;
        i = close + n;
        continue;
      }
      out += "`".repeat(n);
      i += n;
      continue;
    }
    if (c === "[") {
      const link = linkAt(text, i);
      if (link) {
        out += `<a href="${escapeHtml(ctx.link(link.url))}">${inline(link.label, ctx)}</a>`;
        i = link.end;
        continue;
      }
    }
    if (c === "<") {
      const auto = /^<(https?:\/\/[^\s<>]+)>/.exec(text.slice(i));
      if (auto) {
        out += `<a href="${escapeHtml(auto[1])}">${escapeHtml(auto[1])}</a>`;
        i += auto[0].length;
        continue;
      }
    }
    if (c === "*" && text[i + 1] === "*" && text[i + 2] && text[i + 2] !== " ") {
      const close = text.indexOf("**", i + 2);
      if (close > i + 2 && text[close - 1] !== " ") {
        out += `<strong>${inline(text.slice(i + 2, close), ctx)}</strong>`;
        i = close + 2;
        continue;
      }
    }
    if ((c === "*" || c === "_") && text[i + 1] && text[i + 1] !== " " && text[i + 1] !== c && (c === "*" || !word(text[i - 1]))) {
      let close = -1;
      for (let k = i + 1; k < text.length; k++) {
        if (text[k] === "`") {
          // Code inside emphasis is read whole: its `*` or `_` closes nothing.
          const end = text.indexOf("`", k + 1);
          if (end === -1) break;
          k = end;
          continue;
        }
        if (text[k] === c && text[k - 1] !== " " && text[k + 1] !== c && (c === "*" || !word(text[k + 1]))) {
          close = k;
          break;
        }
      }
      if (close > i + 1) {
        out += `<em>${inline(text.slice(i + 1, close), ctx)}</em>`;
        i = close + 1;
        continue;
      }
    }
    out += c === "\n" ? "\n" : escapeHtml(c);
    i++;
  }
  return out;
}

/** `[label](url)` at `at`: its label, address and end, or none. */
function linkAt(text, at) {
  let depth = 0;
  let k = at;
  for (; k < text.length; k++) {
    if (text[k] === "\\") {
      k++;
      continue;
    }
    if (text[k] === "`") {
      const end = text.indexOf("`", k + 1);
      if (end === -1) return null;
      k = end;
      continue;
    }
    if (text[k] === "[") depth++;
    if (text[k] === "]" && --depth === 0) break;
  }
  if (k >= text.length || text[k + 1] !== "(") return null;
  let parens = 0;
  let e = k + 2;
  for (; e < text.length; e++) {
    if (text[e] === "(") parens++;
    if (text[e] === ")" && parens-- === 0) break;
    if (/\s/.test(text[e])) return null;
  }
  if (e >= text.length) return null;
  return { label: text.slice(at + 1, k), url: text.slice(k + 2, e), end: e + 1 };
}

const KEYWORDS = new Set(
  (
    "module import type opaque fn view page route query command signal source event predicate layout " +
    "let match if else for in as each return host requires says holds reads emits changes publish " +
    "cache key freshness consistency concurrency timeout on_key_change invalidates_on idempotent_by " +
    "optimistic placement transactions schedule invoked_by not_found_on session user public private " +
    "shared read_your_writes one_per_key cancel true false"
  ).split(" "),
);

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
    const [token] = m;
    const kind = m[1] ? "c" : m[2] ? "s" : m[3] ? "k" : m[4] ? "tag" : m[5] ? "n" : m[6] ? "t" : KEYWORDS.has(token) ? "k" : null;
    out += kind ? `<span class="${kind}">${escapeHtml(token)}</span>` : escapeHtml(token);
    last = m.index + token.length;
  }
  return out + escapeHtml(src.slice(last));
}
