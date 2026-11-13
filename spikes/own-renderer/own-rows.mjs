// **A page's own lists** (ADR-0437): the code the packaging writes for each
// list a device's page keeps whose rows a plan makes (ADR-0427): its rows
// kept by its key and what they read, each row made with its holes' values
// read as fields and escaped as their contexts say, and its new rows' holes
// reached by steps written for the row's markup. It is the general
// renderer's work for that list (`rowsKept`, `holdsStill`, `plannedRowOf`,
// `rowsPlanned`'s walk) partially evaluated over the page's template IR, and
// it calls the renderer's own helpers (`field`, `shown`, `missing`,
// `escAttribute`, `holdsStill`, ...), so it is spliced into the runtime's
// source where the runtime holds a place for it, before the runtime is
// minified: `OWN_ROWS` inside the renderer, `OWN_WALKS` beside `planOf`.
//
// What it does not write it refuses by name, and the runtime keeps the
// general code for that list. `renderer-agreement.test.mjs` holds what it
// writes to the general code, row by row; `optimize-assets.test.mjs` holds
// what it writes and refuses, and the packaging around it.

/** A part a row's plan sets, as the runtime's `planned` reads it. The
 * packaging reads it here too (`optimize-assets.mjs`), to know a build
 * needs plans. */
export const HOLES = (v) =>
  (v.part === "text" && v.context === "text") ||
  (v.part === "attribute" && v.context !== "content") ||
  v.part === "interpolated_attribute" ||
  v.part === "boolean_attribute" ||
  v.part === "event";

/** Whether a list's rows can be made from one parsed row (`planned`). */
export const planned = (list) => list.body.every((c) => c.chunk === "static" || HOLES(c.value));

/** Where the runtime holds a place for what this writes. */
export const OWN_ROWS = "const OWN_ROWS = null;";
export const OWN_WALKS = "const OWN_WALKS = null;";

/** **The lines the runtime holds only for a page's own code**, each run
 * between `// own rows:` and `// :own rows`: a build that writes none holds
 * none of them, and is minified from the runtime as it was before there was
 * any. Folded away by their flag, they would still move esbuild's names,
 * which it gives by what the whole of a source holds, and a page with no
 * list of its own would be sent other bytes than before. */
export const OWN_LINES = /^[ \t]*\/\/ own rows:.*\n[\s\S]*?^[ \t]*\/\/ :own rows\n/gm;

/** **The lines the runtime holds only for a row that is its own anchor**
 * (ADR-0438), each run between `// element rows:` and `// :element rows`: a
 * build none of whose lists' rows is so, and a server's runtime, hold none
 * of them, for the reason `OWN_LINES` gives. */
export const ELEMENT_LINES = /^[ \t]*\/\/ element rows:.*\n[\s\S]*?^[ \t]*\/\/ :element rows\n/gm;

/** **What the runtime's general code is written as where a page's own code
 * keeps a list**, each its text in the runtime and what it becomes: a
 * planned row's scope made when something reads it (`scopeOf`, which the
 * own code writes), each hole reached by the walk its plan holds where the
 * plan holds one (`planOf`), and the general making of a planned row, with
 * what asks whether a list is planned, gone where every list is the page's
 * own. Each is found once in the runtime, or nothing is written. */
export const OWN_EDITS = [
  ["const plannedRowOf = !PW_PLANS ? null : ", "const plannedRowOf = !PW_PLANS || !PW_UNOWNED ? null : "],
  ["const plannable = !PW_PLANS ? null : ", "const plannable = !PW_PLANS || !PW_UNOWNED ? null : "],
  ["const planned = !PW_PLANS ? null : ", "const planned = !PW_PLANS || !PW_UNOWNED ? null : "],
  ["rowText(row.of, { ...row.env, computedValues: null }, row.others, row.key)", "rowText(row.of, { ...scopeOf(row), computedValues: null }, row.others, row.key)"],
  ["return capturedText(parts[h], row.env);", "return capturedText(parts[h], scopeOf(row));"],
  ["    const holes = new Array(plan.paths.length);\n", "    const holes = plan.walk ? plan.walk(clone) : new Array(plan.paths.length);\n"],
  ["      let n = clone;\n      const path = plan.paths[h];\n", "      let n = plan.walk ? holes[h] : clone;\n      const path = plan.walk ? NO_STEPS : plan.paths[h];\n"],
];

const q = (s) => JSON.stringify(s);

/** **Each keyed list a block of `chunks` keeps** (`rowsKept`): at the
 * block's top, in no keyed list's row, each with the names a block around
 * it binds (`around`). An instance's lists are its view's, in a frame. */
export function keptLists(chunks, around = [], out = []) {
  for (const c of chunks) {
    if (c.chunk !== "dynamic") continue;
    const p = c.value;
    if (p.part === "each" && p.key !== null && p.key !== undefined) out.push({ list: p, around });
    else if (p.part === "each") keptLists(p.body, [...around, p.binding], out);
    else if (p.part === "conditional") {
      keptLists(p.then, around, out);
      keptLists(p.otherwise, around, out);
    } else if (p.part === "match") {
      for (const arm of p.arms) keptLists(arm.body, [...around, ...(arm.binding != null ? [arm.binding] : []), ...(arm.fields ?? [])], out);
    } else if (p.part === "stream") keptLists(p.placeholder, around, out);
  }
  return out;
}

/** The part numbered `id` of `chunks`, wherever it is. */
function partOf(chunks, id) {
  for (const c of chunks) {
    if (c.chunk !== "dynamic") continue;
    const p = c.value;
    if (p.id === id) return p;
    for (const inner of [p.then, p.otherwise, p.body, p.placeholder, ...(p.arms ?? []).map((a) => a.body)]) {
      const found = inner && partOf(inner, id);
      if (found) return found;
    }
  }
  return null;
}

/** **What a page's lists are, for its own code**: each list its blocks
 * keep, with what the page holds by name (its signals and what it computes,
 * as `renderBlock` gives the renderer them) and what each of its rows
 * computes (ADR-0424), by the name its item holds it under. */
export function pageLists(path, chunks, plan) {
  const blocks = (plan.live ?? []).filter((l) => ["conditional", "match", "instance", "each"].includes(l.kind));
  const names = new Set([...(plan.signals ?? []).map((s) => s.name), ...(plan.live ?? []).filter((l) => l.derived).map((l) => `#${path}~${l.part}`)]);
  const computes = new Map();
  for (const row of plan.browser_rows ?? []) {
    const at = row.path.indexOf(".");
    if (at > 0) computes.set(row.path.slice(at + 1), row.inputs ?? []);
  }
  const lists = new Map();
  for (const block of blocks) {
    const part = partOf(chunks, block.part);
    if (!part) continue;
    for (const { list, around } of keptLists([{ chunk: "dynamic", value: part }])) {
      const was = lists.get(list.id);
      lists.set(list.id, { template: path, list, around: was ? [...new Set([...was.around, ...around])] : around, names, computes });
    }
  }
  return [...lists.values()];
}

/** **A refusal**: what is not written, and the general code keeps it. */
class Refused extends Error {}
const refuse = (why) => {
  throw new Refused(why);
};

/** The helpers what is written calls, each written once where used. */
const HELPERS = {
  // The two the runtime calls where it keeps a page's own lists: a row's
  // scope, as `plannedRowOf` makes it, made when something reads it (its
  // text, a press's captures); and each function a row computes with.
  scopeOf: `const scopeOf = (row) =>
    (row.env ??= { ...row.e, reads: row.reads, ...row.computing, names: { name: row.of.binding, value: row.item, up: row.names }, frames: [...row.e.frames, { scope: row.of.id, instance: row.token }] });`,
  ownFunctions: "const ownFunctions = new Map();",
  NO_READS: "const NO_READS = new Map();",
  textAt: `const textAt = (v, path) => {
    const s = v === NONE ? null : shown(v);
    if (s === null) throw missing(path);
    return s;
  };`,
  needed: `const needed = (v, path) => {
    if (v === NONE) throw missing(path);
    return v;
  };`,
  pageAt: `const pageAt = (e, reads, name) => {
    if (!e.page.has(name)) return NONE;
    const v = e.page.get(name);
    reads.set(name, v);
    return opened(v);
  };`,
  fieldsOf: `const fieldsOf = (value, { segments, tails }, from) => {
    for (let rest = from; rest < segments.length; rest++) {
      if (!isRecord(value)) return NONE;
      const all = field(value, tails[rest]);
      if (all !== NONE) return all;
      value = field(value, segments[rest]);
      if (value === NONE) return NONE;
    }
    return value;
  };`,
  stillUnread: `const stillUnread = (old, item, e, p) => (old.item === item ? computedHold(old, item, e, p) : holdsStill(old, item, e, p));`,
};

/** **How one list's paths are read** where its row is made: each an
 * expression of `item`, `e`, `reads` and `computing` with `read`'s value, or
 * a refusal. `uses` notes each helper an expression calls; `reaches` whether
 * one reads the page (`page`) or a value the row computes (`computed`). */
function reader({ list, names, computes }, uses, reaches) {
  const B = list.binding;
  /** The longest head of `segments` the page holds by name, and where its
   * rest starts: of a path whose first name is not the row's, so that no
   * head of it is the row's name, which `bound` finds first. */
  const pageHead = (segments) => {
    for (let cut = segments.length - 1; cut >= 1; cut--) {
      const head = segments.slice(0, cut).join(".");
      if (names.has(head)) return [head, cut];
    }
    return null;
  };
  /** Each tail of `segments` from `from` on that a row computes: `read`
   * asks for one before a field of that name. */
  const computedTail = (segments, from) => {
    for (let rest = from; rest < segments.length; rest++) if (computes.has(segments.slice(rest).join("."))) return rest;
    return -1;
  };
  /** One path's expression, or a refusal. */
  const readPath = (path) => {
    if (path === B) return "item";
    // The whole path a name the page holds, as `bound` finds it first.
    if (names.has(path)) {
      reaches.page = true;
      uses.add("pageAt");
      return `pageAt(e, reads, ${q(path)})`;
    }
    const segments = path.split(".");
    if (segments[0] === B) {
      const at = computedTail(segments, 1);
      if (at === 1) {
        // **A value the row computes** (ADR-0424): the item's own, which a
        // host's first render set; else its function's, given its inputs;
        // else its path's fields, as no function was given.
        const name = segments.slice(1).join(".");
        const inputs = computes.get(name).map((input) => {
          if (input === B) return "item";
          if (names.has(input)) {
            uses.add("needed").add("pageAt");
            return `needed(pageAt(e, computing.computedReads, ${q(input)}), ${q(input)})`;
          }
          return refuse(`what its row computes for ${path} reads ${input}, which is neither its row nor a value of the page`);
        });
        if (computedTail(segments, 2) >= 0) refuse(`${path} reads a value its row computes inside another`);
        reaches.computed = true;
        // With no function given (its module not loaded yet, or none for
        // it), its path's fields are read, as the general code reads them
        // where it holds no function (the two are given each together): of
        // a name of one field, which the item was just found not to hold,
        // none.
        let fields = "NONE";
        if (segments.length > 2) {
          uses.add("fieldsOf");
          fields = `fieldsOf(item, cutsOf(${q(path)}), 1)`;
        }
        return `(!isRecord(item) ? NONE : (v = field(item, ${q(name)})) !== NONE ? v : (f = ownFunctions.get(${q(name)})) ? ((v = f(${inputs.join(", ")})), computing.computedValues.push([${q(name)}, v]), v) : ${fields})`;
      }
      if (at > 1) refuse(`${path} reads a value its row computes inside another`);
      if (segments.length === 2) return `(isRecord(item) ? field(item, ${q(segments[1])}) : NONE)`;
      uses.add("fieldsOf");
      return `fieldsOf(item, cutsOf(${q(path)}), 1)`;
    }
    const head = pageHead(segments);
    if (!head) return "NONE";
    if (computedTail(segments, head[1]) >= 0) refuse(`${path} reads a value a row computes inside a value of the page`);
    reaches.page = true;
    uses.add("pageAt").add("fieldsOf");
    return `fieldsOf(pageAt(e, reads, ${q(head[0])}), cutsOf(${q(path)}), ${head[1]})`;
  };
  return readPath;
}

/** Each context's escaping, as `escaped` chooses it. */
const ESCAPE = { text: "escText", attribute: "escAttribute", url: "escUrl", style: "escStyle", raw_html: "" };

/** A planned list's holes in its plan's order, as `holesOf` has them. */
export function holesOfList(body) {
  const out = [];
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (c.chunk === "static") continue;
    if (c.value.part !== "event") {
      out.push(c.value);
      continue;
    }
    const run = [c.value];
    while (body[i + 1]?.value?.part === "event" && body[i + 1].value.owner === c.value.owner) run.push(body[++i].value);
    out.push(run);
  }
  return out;
}

/** **What one list's own code is**, an object of its key, its hold and its
 * making, or a refusal. */
function listSource(entry, uses) {
  const { list, around, names } = entry;
  const B = list.binding;
  if (around.length > 0) refuse(`it is inside a block that binds ${around.join(", ")}`);
  // A path's first name is the row's where it is the binding: one holding a
  // dot would be cut apart.
  if (B === "" || B.includes(".")) refuse(`its row is bound as ${B}`);
  // A path from the row is read from the row only where the page holds no
  // value by that path, or by a head of it, which `bound` would find first.
  for (const name of names) if (name.startsWith(`${B}.`)) refuse(`the page holds a value as ${name}, a path from its row`);
  const reaches = { page: false, computed: false };
  const readOf = reader(entry, uses, reaches);
  const lines = [];
  const holes = [];
  const raws = [];
  /** Hole `h`: its value read once, as `plannedRowOf` writes it into the
   * row's text, and as it is set where the row is made from its nodes. */
  const writeHole = (hole, h) => {
    if (Array.isArray(hole)) {
      holes.push('""');
      raws.push("undefined");
      return;
    }
    if (hole.part === "text") {
      uses.add("textAt");
      lines.push(`const h${h} = textAt(${readOf(hole.value)}, ${q(hole.value)});`);
      holes.push(`h${h}`);
      raws.push(`h${h}`);
    } else if (hole.part === "boolean_attribute") {
      uses.add("needed");
      lines.push(`const h${h} = holds(needed(${readOf(hole.value)}, ${q(hole.value)}));`);
      holes.push(`h${h} ? ${q(hole.name)} : ""`);
      raws.push(`h${h} ? [${q(hole.name)}, ""] : null`);
    } else if (hole.part === "attribute") {
      const escape = ESCAPE[hole.context];
      if (escape === undefined) refuse(`its attribute ${hole.name} is in a context it does not escape: ${hole.context}`);
      uses.add("textAt");
      lines.push(`const h${h} = ${escape}(textAt(${readOf(hole.value)}, ${q(hole.value)}));`);
      holes.push(`\`${hole.name}="\${h${h}}"\``);
      raws.push(`[${q(hole.name)}, h${h}]`);
    } else if (hole.part === "interpolated_attribute") {
      const escape = { url: "escUrlComponent", attribute: "escAttribute" }[hole.context];
      if (!escape) refuse(`its attribute ${hole.name} interpolates in a context it does not escape: ${hole.context}`);
      uses.add("textAt");
      const pieces = hole.segments.map((s) => (s.segment === "static" ? q(s.value) : `${escape}(textAt(${readOf(s.value)}, ${q(s.value)}))`));
      lines.push(`const h${h} = ${pieces.length ? pieces.join(" + ") : '""'};`);
      holes.push(`\`${hole.name}="\${h${h}}"\``);
      raws.push(`[${q(hole.name)}, h${h}]`);
    } else {
      refuse(`it holds a part no plan sets: ${hole.part}`);
    }
  };
  holesOfList(list.body).forEach(writeHole);
  // What a row's handlers capture is read as one is pressed, from the row's
  // scope: a row whose captures read the page notes it in a map of its own.
  const runs = holesOfList(list.body).filter(Array.isArray).flat();
  const capturing = runs.some((v) => v.captures?.length > 0);
  const reachesPage = runs.some((v) =>
    (v.captures ?? []).some((path) => {
      const root = path.split(".")[0];
      return (Object.hasOwn(v.renames ?? {}, root) ? v.renames[root] : root) !== B;
    }),
  );
  const unread = !reaches.page && !reachesPage;
  if (unread) uses.add("NO_READS").add("stillUnread");
  const key = list.key.split(".").filter((s) => s !== "");
  const keyOf =
    key.length === 0 ? "(item) => shown(item) ?? \"\"" : key.length === 1 ? `(item) => (isRecord(item) ? (shown(field(item, ${q(key[0])})) ?? "") : "")` : `(item) => keyOf(item, ${q(list.key)})`;
  return `{
      key: ${keyOf},
      still: ${unread ? "stillUnread" : "holdsStill"},
      make(p, e, others, item, key) {
        rowsWritten += 1;
        const token = tokenOf(e, ${list.id}, key);
        const reads = ${unread ? "NO_READS" : "new Map()"};
        const computing = rowComputes.size ? { computedReads: new Map(), computedValues: [] } : null;${reaches.computed ? "\n        let v, f;" : ""}
        ${lines.join("\n        ")}
        return { key: \`${list.id}@\${token}\`, text: null, item, reads, names: e.names, computing, holes: [${holes.join(", ")}], raws: [${raws.join(", ")}], of: p, env: null, e, token, capturing: ${capturing}, others };
      },
    }`;
}

// --- the row's markup, as the build reads it -------------------------------

const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
/** Elements whose content a parser reads otherwise than as elements: a hole
 * inside one is no node of its own, or the tree is not what is written. */
const UNREAD = new Set(["script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes", "noscript", "plaintext", "template", "svg", "math"]);

/** **The tag at `at` of `html`**, as the build reads one: its name, in
 * lower case, whether it ends an element, the names of its attributes, past
 * what their quoted values hold, and where it ends; `null` where no tag
 * starts there, `undefined` where one starts and is not read to its end. */
function tagAt(html, at) {
  const tag = /^<(\/?)([A-Za-z][^\s/>]*)/.exec(html.slice(at));
  if (!tag) return null;
  let end = at + tag[0].length;
  const attributes = [];
  for (;;) {
    const attribute = /^\s*([^\s/>=]+)(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?/.exec(html.slice(end));
    if (!attribute || attribute[0].length === 0) break;
    attributes.push(attribute[1]);
    end += attribute[0].length;
  }
  const close = /^\s*\/?>/.exec(html.slice(end));
  if (!close) return undefined;
  return { name: tag[2].toLowerCase(), ends: tag[1] === "/", attributes, to: end + close[0].length };
}

// --- a row that is its own anchor (ADR-0438) --------------------------------

/** What HTML's parser does not keep as it is written, wherever it is: an
 * element it reads otherwise (`UNREAD`), and one it merges, drops or names
 * otherwise in a body. */
const NOT_KEPT = new Set([...UNREAD, "html", "head", "body", "frameset", "frame", "image"]);

const TABLE_PARTS = ["caption", "col", "colgroup", "table", "tbody", "td", "tfoot", "th", "thead", "tr"];
const HEADINGS = ["h1", "h2", "h3", "h4", "h5", "h6"];
/** What ends an open `<p>` where it starts inside it: HTML's "in body", each
 * start tag that closes a `p` element in button scope. */
const ENDS_A_P = [
  "address", "article", "aside", "blockquote", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset",
  "figcaption", "figure", "footer", "form", "header", "hgroup", "hr", "li", "listing", "main", "menu", "nav", "ol", "p",
  "plaintext", "pre", "search", "section", "summary", "table", "ul", "xmp", ...HEADINGS,
];
/** **The elements a row that is its own anchor may be**, each with the
 * elements a parser ends it at, where one starts anywhere inside it (HTML's
 * "in body", "in row" and "in cell"): the row would be two nodes, its key on
 * the first. None ends the rest. A row of any other element keeps its
 * comments. */
const ENDED_BY = new Map([
  ...["div", "span", "article", "section", "aside", "header", "footer", "nav", "main", "figure", "blockquote", "label",
    "fieldset", "details", "ul", "ol", "dl", "img", "input", "em", "strong", "small", "code", "time", "mark"].map((n) => [n, []]),
  ["li", ["li"]],
  ["dd", ["dd", "dt"]],
  ["dt", ["dd", "dt"]],
  ["a", ["a"]],
  ["button", ["button"]],
  ["p", ENDS_A_P],
  ["tr", TABLE_PARTS.filter((n) => n !== "td" && n !== "th")],
  ["td", TABLE_PARTS],
  ["th", TABLE_PARTS],
  ...HEADINGS.map((n) => [n, HEADINGS]),
].map(([n, ends]) => [n, new Set(ends)]));

/**
 * **Why a planned row cannot be its own anchor** (ADR-0438), or `null` where
 * it can: its markup one element, as HTML's parser reads it wherever it is
 * written. Read as written, element inside element, each hole where it is
 * (a text's in its element, an attribute's in its tag): one element and
 * nothing beside it, each element ended in turn, the first one of
 * `ENDED_BY` and none inside it that would end it there, nothing a parser
 * does not keep as written, and a table's row holding its cells alone (what
 * else is in it a parser moves out). A row that cannot be keeps its comments,
 * which hold whatever a parser makes of it.
 */
export function rowElement(body) {
  const html = body.map((c) => (c.chunk === "static" ? c.value : c.value.part === "text" ? "x" : " x")).join("");
  const stack = [];
  let root = null;
  for (let at = 0; at < html.length; ) {
    if (html.startsWith("<!", at) || html.startsWith("<?", at)) return "it holds a comment";
    const tag = tagAt(html, at);
    if (tag === undefined) return "its markup is not read to the end of a tag";
    if (tag === null) {
      if (html.startsWith("</", at)) return "it holds a `</` that ends no element";
      if (stack.length === 0) return "it holds text beside its element";
      if (root === "tr" && stack.length === 1) return "its `<tr>` holds text, which a parser moves out of it";
      at += 1;
      continue;
    }
    at = tag.to;
    const { name } = tag;
    if (tag.ends) {
      if (stack.at(-1) !== name) return `its \`</${name}>\` ends no element it opened`;
      stack.pop();
      continue;
    }
    if (NOT_KEPT.has(name)) return `it holds a \`<${name}>\`, which a parser does not keep as written`;
    if (stack.length === 0) {
      if (root !== null) return "it is more than one element";
      if (!ENDED_BY.has(name)) return `its element is a \`<${name}>\`, which is not one a row is its own anchor as`;
      root = name;
    } else if (ENDED_BY.get(root).has(name)) {
      return `a \`<${name}>\` inside its \`<${root}>\` would end it`;
    } else if (root === "tr" && stack.length === 1 && name !== "td" && name !== "th") {
      return `its \`<tr>\` holds a \`<${name}>\`, which a parser moves out of it`;
    }
    if (!VOID.has(name)) stack.push(name);
  }
  if (root === null) return "it is no element";
  if (stack.length > 0) return `its \`<${stack.at(-1)}>\` is not ended`;
  return null;
}

/** **Each hole's path in a row's markup**, as `planOf` marks the holes and
 * as the markup is written, element inside element: `null` where it cannot
 * be read so. The browser's parse is the truth (`planOf`), which the
 * runtime compares with this once per plan, and takes its own walk where
 * the two differ: a parser that puts a `<tbody>` around a table's rows, or
 * closes a `<p>` before a `<div>`, costs speed, not correctness. */
export function rowPaths(body) {
  let html = "";
  let count = 0;
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (c.chunk === "static") {
      html += c.value;
      continue;
    }
    while (c.value.part === "event" && body[i + 1]?.value?.part === "event" && body[i + 1].value.owner === c.value.owner) i++;
    html += c.value.part === "text" ? `<!--pwh${count}-->` : ` data-pwh${count}=""`;
    count += 1;
  }
  const root = { children: [] };
  const stack = [root];
  const holes = new Array(count).fill(null);
  const mark = (index, node) => {
    if (holes[index] !== null) return false;
    holes[index] = node;
    return true;
  };
  let at = 0;
  let text = false;
  while (at < html.length) {
    const top = stack.at(-1);
    if (html.startsWith("<!--", at)) {
      const end = html.indexOf("-->", at + 4);
      if (end < 0) return null;
      const node = { children: [], parent: top };
      top.children.push(node);
      const hole = /^pwh(\d+)$/.exec(html.slice(at + 4, end));
      if (hole && !mark(Number(hole[1]), node)) return null;
      at = end + 3;
      text = false;
      continue;
    }
    const tag = tagAt(html, at);
    if (tag === undefined) return null;
    if (tag) {
      const { name } = tag;
      at = tag.to;
      text = false;
      if (tag.ends) {
        if (top === root || top.name !== name) return null;
        stack.pop();
        continue;
      }
      if (UNREAD.has(name)) return null;
      const node = { name, children: [], parent: top };
      top.children.push(node);
      for (const a of tag.attributes) {
        const hole = /^data-pwh(\d+)$/.exec(a);
        if (hole && !mark(Number(hole[1]), node)) return null;
      }
      if (!VOID.has(name)) stack.push(node);
      continue;
    }
    if (html.startsWith("<!", at) || html.startsWith("<?", at) || html.startsWith("</", at)) return null;
    // Text, one node to the next tag or comment.
    if (!text) top.children.push({ children: [], parent: top });
    text = true;
    at += 1;
  }
  if (stack.length !== 1 || holes.some((h) => h === null)) return null;
  return holes.map((node) => {
    const path = [];
    for (let n = node; n.parent; n = n.parent) path.unshift(n.parent.children.indexOf(n));
    return path;
  });
}

/** **A walk to each path** from a row's clone `c`, written out: each node
 * reached once, from its parent's first child or the nearest sibling before
 * it already reached. A row that is its own anchor is its element's clone,
 * its paths from it (ADR-0438): one whose holes are all on its element takes
 * no step. */
export function walkSource(paths) {
  const known = new Map([["", "c"]]);
  const lines = [];
  const reach = (path) => {
    const key = path.join(",");
    if (known.has(key)) return known.get(key);
    const parent = path.slice(0, -1);
    const index = path.at(-1);
    let expression = null;
    for (let i = index - 1; i >= 0; i--) {
      const sibling = known.get([...parent, i].join(","));
      if (sibling) {
        expression = sibling + ".nextSibling".repeat(index - i);
        break;
      }
    }
    expression ??= `${reach(parent)}.firstChild${".nextSibling".repeat(index)}`;
    const name = `n${lines.length}`;
    lines.push(`${name} = ${expression}`);
    known.set(key, name);
    return name;
  };
  const holes = paths.map(reach);
  if (lines.length === 0) return `(c) => [${holes.join(", ")}]`;
  return `(c) => {\n        const ${lines.join(",\n          ")};\n        return [${holes.join(", ")}];\n      }`;
}

// --- a build's own lists ---------------------------------------------------

/**
 * **What a build's own lists are**: for `entries` (`pageLists`), the source
 * of each list's own code by its page's template and its part, spliced where
 * the runtime holds `OWN_ROWS` (inside its renderer) and `OWN_WALKS` (beside
 * `planOf`); each list written (`written`) or refused, and why (`refused`).
 * `null` sources where none is written.
 */
export function ownRows(entries) {
  const uses = new Set();
  const lists = new Map();
  const walks = new Map();
  const written = [];
  const refused = [];
  for (const entry of entries) {
    const { template, list } = entry;
    if (!planned(list)) continue;
    let source;
    try {
      source = listSource(entry, uses);
    } catch (error) {
      if (!(error instanceof Refused)) throw error;
      refused.push({ template, part: list.id, why: error.message });
      continue;
    }
    written.push({ template, part: list.id });
    if (!lists.has(template)) lists.set(template, []);
    lists.get(template).push(`    ${list.id}: ${source}`);
    const paths = rowPaths(list.body);
    if (paths === null) continue;
    if (!walks.has(template)) walks.set(template, []);
    // A row that is its own anchor is walked from its element (ADR-0438), as
    // its plan's paths are.
    const from = list.row_anchor === "element" ? paths.map((p) => p.slice(1)) : paths;
    walks.get(template).push(`    ${list.id}: {\n      paths: ${q(from.map((p) => p.join(",")).join("|"))},\n      walk: ${walkSource(from)},\n    }`);
  }
  const table = (by) => `{\n${[...by].map(([template, items]) => `  ${q(template)}: {\n${items.join(",\n")},\n  }`).join(",\n")},\n}[parts.template]`;
  if (written.length > 0) uses.add("scopeOf").add("ownFunctions");
  const helpers = Object.entries(HELPERS)
    .filter(([name]) => uses.has(name))
    .map(([, source]) => `  ${source}`)
    .join("\n");
  return {
    rows: written.length === 0 ? null : `${helpers}\n  const ownKept = ${OWN_KEPT};\n  const OWN_ROWS = ${table(lists)};`,
    walks: walks.size === 0 ? null : `const OWN_WALKS = ${table(walks)};`,
    written,
    refused,
  };
}

/** **A list the page's own code keeps** (`rowsKept`'s, by its own key, hold
 * and making): every row by its key, which is also what says a key is
 * taken. */
const OWN_KEPT = `(own, p, e, others, items) => {
    if (!own) throw malformed(\`list \${p.id} is kept by no code of its page\`);
    const was = kept.get(p.id);
    const now = new Map();
    const rows = items.map((raw) => {
      const item = opened(raw);
      const key = own.key(item);
      if (key === "") throw new Blocked("MissingLoopKey", p.key);
      if (now.has(key)) throw new Blocked("DuplicateLoopKey", key);
      const old = was?.get(key);
      const row = old && own.still(old, item, e, p) ? old : own.make(p, e, others, item, key);
      now.set(key, row);
      return row;
    });
    kept.set(p.id, now);
    return { part: p.id, rows, of: p };
  }`;
