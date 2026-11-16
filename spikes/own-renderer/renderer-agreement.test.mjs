// **The browser's renderer and the server's give one answer** (the browser
// renderer's ADR).
//
// A page renders a block again with a page of JavaScript, and the server
// renders the same templates in Rust. Two implementations of a function that
// escapes can disagree, and a disagreement shows nothing on the page until it
// is an exploit. So they are held to each other here, in every run of the
// suite: the same requests to each, over the compiler's own templates, over
// generated ones of every part and context, and over every code point in
// every escaping. Where the two differ, the suite fails.
//
// Tokens are compared by which they are the same as: the server's are a keyed
// hash, and the browser's are a counter, because a page does not hold the key.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { withOwnRows } from "./optimize-assets.mjs";
import { ELEMENT_LINES, OWN_ROWS, keptLists as keptOf, ownRows, planned as plannable, rowElement } from "./own-rows.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, "../..");
const runtime = readFileSync(path.join(here, "public/pw-runtime.mjs"), "utf8");
const BEGIN = "const renderer = (() => {";
const END = "// --- end of the browser's renderer";

/** The renderer of `source`, between its markers. */
function rendererOf(source) {
  const start = source.indexOf(BEGIN);
  const end = source.indexOf(END);
  assert.ok(start >= 0 && end > start, "the runtime holds the renderer between its markers");
  return source.slice(start, end);
}

/** The renderer as a build none of whose lists its page's own code keeps
 * has it (ADR-0437): the packaging's own lines cut, as it cuts them. */
function loadRenderer(server = true, flags = {}) {
  return vm.runInNewContext(`${rendererOf(withOwnRows(runtime, { rows: null }))}\nrenderer`, {
    TextEncoder,
    PW_SERVER: server,
    PW_LINKS: true,
    PW_MATCH: true,
    PW_VIEWS: true,
    PW_DIALOGS: true,
    PW_PLANS: true,
    // Each value of a type that contains itself is read as the server reads
    // it (ADR-0205), which the agreement holds: the renderer as a build that
    // writes graphs has it (ADR-0428).
    PW_GRAPHS: true,
    ...flags,
  });
}
const renderer = loadRenderer();

const oracleBinary = path.join(process.env.CARGO_TARGET_DIR ?? path.join(root, "target"), "debug/oracle");

/** Each request's answer from the server's renderer. */
function fromTheServer(requests) {
  const input = requests.map(([name, request]) => `${name}\t${wire(request)}\n`).join("");
  const ran = spawnSync(oracleBinary, { input, maxBuffer: 2 ** 31 - 1, encoding: "utf8" });
  assert.equal(ran.error, undefined, `the oracle ran: ${oracleBinary} (just builds it)`);
  assert.equal(ran.status, 0, `the oracle answered: ${ran.stderr}`);
  const lines = ran.stdout.split("\n").filter(Boolean).map((line) => JSON.parse(line));
  assert.equal(lines.length, requests.length, "one answer for each request");
  return lines;
}

/** A value as JSON, each integer exact: the form the server's reader is given. */
function wire(v) {
  if (typeof v === "bigint") return v.toString();
  if (v === null || v === undefined) return "null";
  if (Array.isArray(v)) return `[${v.map(wire).join(",")}]`;
  if (typeof v === "object") {
    return `{${Object.entries(v)
      .map(([k, x]) => `${JSON.stringify(k)}:${wire(x)}`)
      .join(",")}}`;
  }
  return JSON.stringify(v);
}

/** The same request of the browser's renderer. */
function fromTheBrowser([name, request]) {
  const templates = request.templates ?? [];
  const env = renderer.env(Object.entries(request.values ?? {}));
  try {
    switch (name) {
      case "render_part":
        return [0, renderer.part(request.part, env, templates)];
      case "render_instance":
        return [0, renderer.instance(request.part, request.item, env, templates)];
      case "instance_changes":
        return [0, JSON.stringify(renderer.changes(request.part, request.was, request.now, env, templates))];
      case "attribute_value":
        return [0, JSON.stringify(renderer.attribute(request.part, env))];
      case "captures_value":
        return [0, JSON.stringify(renderer.captures(request.parts, env))];
      default:
        throw new Error(`no request is a ${name}`);
    }
  } catch (error) {
    if (error.kind === undefined) throw error;
    return [1, error.kind];
  }
}

/** Tokens numbered by when each is first seen, a comment's and a row's own
 * anchor's (`data-pw-row`, ADR-0438): which are the same is what the two
 * renderers must agree on, and what a token is, they need not. */
function numbered(html) {
  const seen = new Map();
  return html.replace(/@([A-Za-z0-9_-]+?)(-->|")/g, (_, token, end) => {
    if (!seen.has(token)) seen.set(token, seen.size);
    return `@${seen.get(token)}${end}`;
  });
}

/** What a refusal is, from the server's words for it. */
const refusal = ([code, text]) => (code === 2 ? "Malformed" : /^\w+/.exec(text)?.[0]);

/** Every request answered alike by both. */
function agree(requests) {
  const server = fromTheServer(requests);
  let refused = 0;
  requests.forEach((request, i) => {
    const said = server[i];
    assert.notEqual(said[0], 3, `the server's renderer panicked on ${wire(request[1])}`);
    const ours = fromTheBrowser(request);
    if (said[0] === 0) {
      assert.equal(ours[0], 0, `only the browser refused ${wire(request[1])}: ${ours[1]}`);
      assert.equal(numbered(ours[1]), numbered(said[1]), `the two rendered ${wire(request[1])} differently`);
    } else {
      refused += 1;
      assert.equal(ours[0], 1, `only the server refused ${wire(request[1])}: ${said[1]}`);
      assert.equal(ours[1], refusal(said), `the two refused ${wire(request[1])} differently: ${said[1]}`);
    }
  });
  return { answered: requests.length, refused };
}

// --- what is asked ---------------------------------------------------------

function random(seed) {
  let s = seed >>> 0;
  const next = () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const int = (n) => Math.floor(next() * n);
  return { next, int, pick: (xs) => xs[int(xs.length)], chance: (p) => next() < p };
}

/** Values that have been an exploit, or that a parser treats specially. */
const NASTY = [
  "", " ", "plain", "two words", "<b>x</b>", "<script>alert(1)</script>", "a&b", "a&amp;b", "&#106;", '"quoted"',
  "' onclick='x", '" onclick="x', "`tick`", ">", "<!--", "-->", "</textarea>", "</script", "\n", "\nlead", "\r\nlead",
  "\tx", "é", "日本語", "😀", "𝒳y", " ", "\u0085", " ", "﻿", "\u0000", "\u001f", "\u007f",
  "javascript:alert(1)", "JaVaScRiPt:alert(1)", " javascript:x", "java\nscript:x", "java\tscript:x", "\u0001javascript:x",
  "java\u0085script:x", "java script:x", "java﻿script:x", "ＪＡＶＡ:x", "&#106;avascript:x", "&#x6a;avascript:x",
  "&#x6A;avascript:x", "&#0000106;avascript:x", "&#+106;avascript:x", "&#x+6a;avascript:x", "&colon;", "javascript&colon;x",
  "&Tab;javascript:x", "&NewLine;javascript:x", "java&Tab;script:x", "&amp;#106;avascript:x", "vbscript:x", "data:text/html,x",
  "DATA:x", "&#xé;x", "&aaaaaaaaaaé;javascript:", "&aaaaaaaaé;javascript:", "&aaaaaaaaaaaa;", "&é;javascript:x", "&é&#106;x",
  "&#1114112;", "&#55357;", "&#xD83D;", "&#x110000;", "&#99999999999;", "&#xffffffffff;", "&#;", "&#x;", "&;", "&", "&&#106;x",
  "expression(alert(1))", "EXPRESSION(", "width:expression(1)", "url(javascript:x)", "URL('javascript:x)", "url(data:x)",
  'url("javascript:x")', "background:red", "/stores/1", "https://example.com/a?b=c#d", "a/b?c#d:e", "~.-_", "100%", "+ ",
];

const SCALARS = [
  ...NASTY, 0, 1, -1, 7, 2 ** 31, 2 ** 53, -(2 ** 53), 2 ** 53 + 2, 2 ** 63, 9007199254740993n, -9007199254740993n, 0n, 12n,
  1.5, -0.25, Number.NaN, Number.POSITIVE_INFINITY, true, false, null,
];

const FIELDS = ["id", "name", "note", "on", "r", "n", "deep.er", "ключ", "😀k", "k", "𐐀k", "__proto__x", "constructor"];

function generated(seed) {
  const r = random(seed);
  let ids = 0;
  const id = () => ids++;

  function value(depth) {
    const roll = r.next();
    if (depth <= 0 || roll < 0.45) return r.pick(SCALARS);
    if (roll < 0.65) return rows(depth - 1);
    if (roll < 0.85) return record(depth - 1);
    if (roll < 0.95) return variant(depth - 1);
    return graph();
  }

  function record(depth) {
    const o = {};
    for (let i = r.int(4); i >= 0; i--) o[r.pick(FIELDS)] = value(depth);
    return o;
  }

  function variant(depth) {
    const v = { $case: r.pick(["Some", "None", "Ok", "Err", "circle"]) };
    if (r.chance(0.7)) v.value = r.chance(0.3) ? [value(depth), value(depth)] : value(depth);
    return v;
  }

  function graph() {
    return { $graph: [{ n: r.pick(NASTY), next: [{ $node: 1 }] }, { n: r.pick(NASTY), next: [{ $node: 2 }] }, { n: 1, next: [] }] };
  }

  function rows(depth) {
    const shape = { id: 0, name: "", ...(r.chance(0.5) ? { r: { id: "" } } : {}), ...(r.chance(0.5) ? { on: true } : {}) };
    const list = [];
    for (let i = r.int(4); i > 0; i--) {
      const row = {};
      for (const k of Object.keys(shape)) row[k] = r.chance(0.8) && k === "id" ? r.int(5) : k === "r" ? { id: r.pick(NASTY) } : value(depth);
      list.push(row);
    }
    return list;
  }

  /** Every path into `v`, by what it holds: `{ scalar, list, variant, any }`. */
  function paths(name, v, out) {
    out.any.push([name, v]);
    if (Array.isArray(v)) out.list.push([name, v]);
    else if (typeof v === "object" && v !== null) {
      if (typeof v.$case === "string") out.variant.push([name, v]);
      else if ("$graph" in v || "$node" in v) out.scalar.push([name, v]);
      else for (const [k, x] of Object.entries(v)) paths(`${name}.${k}`, x, out);
    } else out.scalar.push([name, v]);
    return out;
  }

  function scope(values) {
    const out = { scalar: [], list: [], variant: [], any: [] };
    for (const [k, v] of Object.entries(values)) paths(k, v, out);
    return out;
  }

  const CONTEXTS = ["text", "attribute", "url", "style", "raw_html", "content"];

  function choose(kind, values) {
    const at = scope(values);
    if (r.chance(0.04)) return `missing${r.int(3)}`;
    const list = at[kind].length && r.chance(0.92) ? at[kind] : at.any;
    return list.length ? r.pick(list)[0] : "missing";
  }

  function chunks(values, depth, others) {
    const out = [];
    for (let i = r.int(4); i >= 0; i--) {
      if (r.chance(0.35)) out.push({ chunk: "static", value: r.pick(["<p>", "</p>", '<li data-pw="0" ', " ", "x ", "<a ", ">", "<input ", "<b>&amp;</b>"]) });
      if (r.chance(0.8)) out.push(...dynamic(values, depth, others));
    }
    return out;
  }

  function dynamic(values, depth, others) {
    const roll = depth <= 0 ? r.int(6) : r.int(14);
    const owner = r.int(4);
    switch (roll) {
      case 0:
      case 1:
        return [{ chunk: "dynamic", value: { part: "text", id: id(), value: choose("scalar", values), context: r.chance(0.7) ? "text" : r.pick(CONTEXTS) } }];
      case 2:
        return [{ chunk: "dynamic", value: { part: "attribute", id: id(), owner, name: r.pick(["title", "href", "style", "value", "class"]), value: choose("scalar", values), context: r.pick(CONTEXTS) } }];
      case 3:
        return [{ chunk: "static", value: "<input " }, { chunk: "dynamic", value: { part: "boolean_attribute", id: id(), owner, name: "disabled", value: choose("any", values) } }];
      case 4: {
        const segments = [];
        for (let i = r.int(4); i >= 0; i--) {
          segments.push(r.chance(0.5) ? { segment: "static", value: r.pick(["/a/", "?b=", "x", "&amp;"]) } : { segment: "value", value: choose("scalar", values) });
        }
        return [{ chunk: "dynamic", value: { part: "interpolated_attribute", id: id(), owner, name: "href", segments, context: r.pick(["url", "url", "attribute", "attribute", "style", "text"]) } }];
      }
      case 5: {
        const run = [];
        for (let i = r.int(3); i >= 0; i--) {
          const captures = [];
          for (let j = r.int(3); j > 0; j--) captures.push(choose("any", values));
          const renames = {};
          if (captures.length && r.chance(0.3)) renames[captures[0].split(".")[0]] = r.pick(Object.keys(values)) ?? "x";
          run.push({ chunk: "dynamic", value: { part: "event", id: id(), owner, event: "press", handler: "h", name: "n", captures, ...(Object.keys(renames).length ? { renames } : {}) } });
        }
        return run;
      }
      case 6:
      case 7:
        return [{ chunk: "dynamic", value: { part: "conditional", id: id(), value: choose("any", values), then: chunks(values, depth - 1, others), otherwise: chunks(values, depth - 1, others) } }];
      case 8:
      case 9: {
        const collection = choose("list", values);
        const list = readPath(values, collection);
        const row = Array.isArray(list) && list.length ? list[0] : { id: 1, name: "n" };
        const binding = `it${depth}`;
        const key = r.chance(0.85) ? r.pick(["id", "id", "name", "r.id", "nope", ""]) : null;
        return [{ chunk: "dynamic", value: { part: "each", id: id(), collection, binding, key, body: chunks({ ...values, [binding]: row }, depth - 1, others) } }];
      }
      case 10: {
        const value = choose("variant", values);
        const held = readPath(values, value);
        const arm = (name) => {
          const inner = { ...values };
          const out = { case: name };
          const payload = held?.value;
          if (r.chance(0.6)) { out.binding = `p${depth}`; inner[out.binding] = payload ?? "x"; }
          else if (r.chance(0.3)) { out.fields = ["f0", "f1"]; inner.f0 = "x"; inner.f1 = "y"; }
          out.body = chunks(inner, depth - 1, others);
          return out;
        };
        const names = ["Some", "None", "Ok", "Err", "circle"].filter(() => r.chance(0.6));
        return [{ chunk: "dynamic", value: { part: "match", id: id(), value, arms: names.map(arm) } }];
      }
      case 11:
        return [{ chunk: "dynamic", value: { part: "stream", id: id(), query: "q", args: [], streamed: r.chance(0.7), placeholder: chunks(values, depth - 1, others), ready: { body: [] }, failed: { body: [] } } }];
      case 12: {
        if (others.length === 0 || r.chance(0.1)) return [{ chunk: "dynamic", value: { part: "instance", id: id(), path: "t.Nowhere", args: [] } }];
        const t = r.pick(others);
        const elements = r.chance(0.1) ? 600 : r.int(3);
        return [{ chunk: "dynamic", value: { part: "instance", id: id(), path: t.path, args: t.params.map((p) => [p, choose("any", values)]), elements, deepest: r.int(3) } }];
      }
      default:
        return [r.pick([
          { chunk: "dynamic", value: { part: "raw_html", id: id(), value: choose("scalar", values), capability: "c" } },
          { chunk: "dynamic", value: { part: "blocked", reason: "no", at: "here" } },
          { chunk: "dynamic", value: { part: "title", id: id(), pieces: [] } },
          { chunk: "dynamic", value: { part: "meta", id: id(), attribute: "name", key: "k", content: [] } },
          { chunk: "dynamic", value: { part: "canonical", id: id(), route: "r", args: [] } },
        ])];
    }
  }

  return { r, value, chunks, record, scope, rows };
}

function readPath(values, at) {
  let v = values;
  for (const k of at.split(".")) v = v?.[k];
  return v;
}

/** One value changed somewhere inside it. */
function perturbed(r, v, depth = 3) {
  // A graph is whole: a node changed here would be a graph that is no tree.
  if (typeof v === "object" && v !== null && "$graph" in v) return r.chance(0.5) ? v : r.pick(SCALARS);
  if (Array.isArray(v)) {
    const out = v.map((x) => (r.chance(0.3) ? perturbed(r, x, depth - 1) : x));
    // An item gone, one more, moved, or written twice.
    if (r.chance(0.25) && out.length) out.splice(r.int(out.length), 1);
    if (r.chance(0.25)) out.splice(r.int(out.length + 1), 0, { id: r.int(9), name: r.pick(NASTY) });
    if (r.chance(0.25) && out.length > 1) out.reverse();
    if (r.chance(0.2) && out.length) out.push(out[0]);
    return out;
  }
  if (typeof v === "object" && v !== null && depth > 0) {
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, k !== "$case" && k !== "id" && r.chance(0.5) ? perturbed(r, x, depth - 1) : x]));
  }
  return r.chance(0.8) ? r.pick(SCALARS) : v;
}

/** Every kind of request about one part. */
function requestsAbout(r, part, values, templates) {
  const out = [["render_part", { part, values, templates }]];
  if (part.part === "each" || r.chance(0.02)) {
    const list = readPath(values, part.collection ?? "");
    const rows = Array.isArray(list) && list.length ? list : [{ id: 1, name: "n" }];
    for (const item of rows.slice(0, 3)) {
      out.push(["render_instance", { part, item, values, templates }]);
      out.push(["instance_changes", { part, was: item, now: perturbed(r, item), values, templates }]);
    }
  }
  if (/attribute/.test(part.part) || r.chance(0.02)) out.push(["attribute_value", { part, values }]);
  if (part.part === "event") out.push(["captures_value", { parts: [part], values }]);
  return out;
}

function dynamicParts(chunks, out = []) {
  for (const c of chunks) {
    if (c.chunk !== "dynamic") continue;
    out.push(c.value);
    const p = c.value;
    for (const inner of [p.then, p.otherwise, p.body, p.placeholder, ...(p.arms ?? []).map((a) => a.body)]) {
      if (inner) dynamicParts(inner, out);
    }
  }
  return out;
}

function generatedRequests(cases) {
  const requests = [];
  for (let seed = 1; seed <= cases; seed++) {
    const g = generated(seed);
    const values = {};
    for (let i = g.r.int(5); i >= 0; i--) values[g.r.pick(["a", "b", "c", "list", "rec", "#t~1", "X~holds", "flag"])] = g.value(3);
    const others = [];
    for (let i = g.r.int(3); i > 0; i--) {
      const path = `t.V${i}`;
      const template = { path, name: `V${i}`, params: ["p"], schema: "s", chunks: [] };
      others.push(template);
    }
    others.forEach((t, i) => {
      // Only a later view's instance: a loop of views that does not descend
      // is a program the server would render forever, and no program here.
      t.chunks = g.chunks({ p: g.value(2) }, 2, others.slice(i + 1));
      // A view inside itself, under a block that ends it.
      t.chunks.push({ chunk: "dynamic", value: { part: "conditional", id: 900, value: "p.next", then: [
        { chunk: "dynamic", value: { part: "instance", id: 901, path: t.path, args: [["p", "p.next"]], elements: 1, deepest: 2 } },
      ], otherwise: [] } });
    });
    const chunks = g.chunks(values, 3, others);
    // Half the keyed lists said to be their rows' own anchors (ADR-0438),
    // whatever their rows are: each renderer writes what the list says, and
    // refuses a row that begins with no element.
    for (const p of dynamicParts(chunks)) if (p.part === "each" && p.key != null && g.r.chance(0.5)) p.row_anchor = "element";
    for (const part of dynamicParts(chunks).slice(0, 24)) requests.push(...requestsAbout(g.r, part, values, others));
    const run = dynamicParts(chunks).filter((p) => p.part === "event");
    if (run.length) requests.push(["captures_value", { parts: run, values }]);
  }
  return requests;
}

/** A value for every path a template's parts read. */
function shaped(r, chunks, values) {
  const put = (at, make) => {
    const names = at.split(".");
    let v = values;
    names.slice(0, -1).forEach((k) => {
      if (typeof v[k] !== "object" || v[k] === null || Array.isArray(v[k])) v[k] = {};
      v = v[k];
    });
    const last = names.at(-1);
    if (v[last] === undefined) v[last] = make();
    return v[last];
  };
  for (const part of chunks.flatMap((c) => (c.chunk === "dynamic" ? [c.value] : []))) {
    switch (part.part) {
      case "text":
      case "attribute":
        put(part.value, () => r.pick(NASTY));
        break;
      case "boolean_attribute":
        put(part.value, () => r.chance(0.5));
        break;
      case "interpolated_attribute":
        for (const s of part.segments) if (s.segment === "value") put(s.value, () => r.pick(NASTY));
        break;
      case "event":
        for (const c of part.captures ?? []) put(part.renames?.[c] ?? c, () => r.int(100));
        break;
      case "conditional":
        put(part.value, () => r.chance(0.5));
        shaped(r, part.then, values);
        shaped(r, part.otherwise, values);
        break;
      case "each": {
        const list = put(part.collection, () => [{}, {}, {}]);
        if (Array.isArray(list)) {
          list.forEach((row, i) => {
            const inner = { ...values, [part.binding]: row };
            shaped(r, part.body, inner);
            Object.assign(row, inner[part.binding]);
            if (part.key) {
              let at = row;
              const names = part.key.split(".");
              names.slice(0, -1).forEach((k) => (at = at[k] ??= {}));
              at[names.at(-1)] = `key ${i}`;
            }
          });
        }
        break;
      }
      case "match": {
        const arm = r.pick(part.arms);
        if (arm) {
          const inner = { ...values };
          const variant = put(part.value, () => ({ $case: arm.case }));
          if (arm.binding && variant.value === undefined) variant.value = {};
          if (arm.binding) inner[arm.binding] = variant.value;
          for (const f of arm.fields ?? []) inner[f] = r.pick(NASTY);
          if (arm.fields?.length) variant.value = arm.fields.map((f) => inner[f]);
          shaped(r, arm.body, inner);
        }
        break;
      }
      default:
    }
  }
  return values;
}

/** Each part of `chunks`, each asked about in the scope it is rendered in. */
function askedInScope(r, chunks, values, templates, out) {
  for (const c of chunks) {
    if (c.chunk !== "dynamic") continue;
    const p = c.value;
    out.push(...requestsAbout(r, p, values, templates));
    switch (p.part) {
      case "conditional":
        askedInScope(r, p.then, values, templates, out);
        askedInScope(r, p.otherwise, values, templates, out);
        break;
      case "each": {
        const rows = readPath(values, p.collection);
        if (Array.isArray(rows) && rows.length) askedInScope(r, p.body, { ...values, [p.binding]: rows[0] }, templates, out);
        break;
      }
      case "match": {
        const variant = readPath(values, p.value);
        const arm = p.arms.find((a) => a.case === variant?.$case);
        if (arm) {
          const inner = { ...values };
          if (arm.binding) inner[arm.binding] = variant.value;
          (arm.fields ?? []).forEach((f, i) => (inner[f] = variant.value?.[i]));
          askedInScope(r, arm.body, inner, templates, out);
        }
        break;
      }
      default:
    }
  }
  return out;
}

/** The compiler's templates of the store and of kiokun, recorded. */
const COMPILED = ["spikes/own-renderer/store-ir.json", "docs/evidence/E10/kiokun/templates.json"];

/** And the large list's and its fixture of presses, as their builds write
 * them (`rows.sh`, `row-events.sh`): pages whose lists a plan makes. */
const BUILT = ["spikes/own-renderer/dist-rows/build/templates.json", "spikes/own-renderer/dist-row-events/build/templates.json"];

function compiledRequests(files = COMPILED) {
  const requests = [];
  const r = random(424242);
  for (const file of files) {
    let text;
    try {
      text = readFileSync(path.join(root, file), "utf8");
    } catch (error) {
      assert.fail(`${file}: ${error.code} (spikes/own-renderer/rows.sh and row-events.sh build it; just e14-browser-renderer runs them)`);
    }
    const templates = JSON.parse(text);
    for (const template of templates) {
      for (let round = 0; round < 4; round++) {
        askedInScope(r, template.chunks, shaped(r, template.chunks, {}), templates, requests);
      }
    }
  }
  return requests;
}

// --- the tests -------------------------------------------------------------

test("the two renderers agree over generated templates of every part and context", () => {
  const { answered, refused } = agree(generatedRequests(1500));
  assert.ok(answered > 20_000, `asked ${answered}`);
  // Most render, and the refusals are there too: a generator that only ever
  // reached a refusal would be holding the two to very little.
  assert.ok(refused > answered / 50, `${refused} of ${answered} refused`);
  assert.ok(refused < answered * 0.7, `${refused} of ${answered} refused`);
});

/** A block as a page that talks to no server keeps it, as its text. */
function textOfKept(block) {
  return block.lists.reduce((text, list, i) => text + list.rows.map((row) => row.text).join("") + block.strs[i + 1], block.strs[0]);
}

test("a block kept in pieces is the text the renderer writes, the second time too", () => {
  // Views stay on, so nothing leaves out a text's anchors: the same text, to
  // the byte, whatever is asked for.
  const device = loadRenderer(false);
  let rendered = 0;
  let again = 0;
  for (const [name, request] of generatedRequests(1500)) {
    if (name !== "render_part") continue;
    const values = () => Object.entries(request.values ?? {});
    const templates = request.templates ?? [];
    let whole;
    try {
      whole = renderer.part(request.part, renderer.env(values()), templates);
    } catch (error) {
      if (error.kind === undefined) throw error;
      assert.throws(() => device.block(request.part, device.env(values()), templates), (e) => e.kind === error.kind, wire(request));
      continue;
    }
    const first = device.block(request.part, device.env(values()), templates);
    assert.equal(numbered(textOfKept(first)), numbered(whole), wire(request));
    rendered += 1;
    const second = device.block(request.part, device.env(values()), templates);
    assert.equal(numbered(textOfKept(second)), numbered(whole), wire(request));
    first.lists.forEach((list, i) => list.rows.forEach((row, k) => {
      assert.equal(row, second.lists[i].rows[k], "a row nothing changed is the row kept");
      again += 1;
    }));
  }
  assert.ok(rendered > 500, `rendered ${rendered}`);
  assert.ok(again > 50, `kept ${again} rows`);
});

test("a row is written again when what it reads changes, and not otherwise", () => {
  const device = loadRenderer(false);
  const part = {
    part: "each", id: 3, collection: "items", binding: "item", key: "id", body: [
      { chunk: "static", value: "<li>" },
      { chunk: "dynamic", value: { part: "text", id: 4, value: "item.name", context: "text" } },
      { chunk: "static", value: ":" },
      { chunk: "dynamic", value: { part: "text", id: 5, value: "note", context: "text" } },
      { chunk: "static", value: "</li>" },
    ],
  };
  const items = [{ id: 1, name: "a" }, { id: 2, name: "b" }];
  const render = (values) => device.block(part, device.env(Object.entries(values)), []);
  const first = render({ items, note: "n" }).lists[0].rows;
  const same = render({ items: items.map((i) => ({ ...i })), note: "n" }).lists[0].rows;
  assert.equal(same[0], first[0], "an equal value is the row kept");
  const renamed = render({ items: [{ id: 1, name: "z" }, items[1]], note: "n" }).lists[0].rows;
  assert.notEqual(renamed[0], same[0], "a value that changed is written");
  assert.equal(renamed[1], same[1], "the row beside it is kept");
  const noted = render({ items: [{ id: 1, name: "z" }, items[1]], note: "m" }).lists[0].rows;
  assert.notEqual(noted[1], renamed[1], "what a row read of the page changed");
  assert.match(noted[1].text, /b:<!--pw:s5-->m|b<!--pw:e4-->:<!--pw:s5-->m/);
});

test("a row is written again when what a block around it bound changes", () => {
  const device = loadRenderer(false);
  const part = {
    part: "match", id: 2, value: "o", arms: [
      {
        case: "Some", binding: "f", fields: [], body: [{ chunk: "dynamic", value: {
          part: "each", id: 3, collection: "items", binding: "item", key: "id", body: [
            { chunk: "dynamic", value: { part: "text", id: 5, value: "f", context: "text" } },
          ],
        } }],
      },
      { case: "None", binding: null, fields: [], body: [] },
    ],
  };
  const items = [{ id: 1 }];
  const render = (f) => device.block(part, device.env(Object.entries({ items, o: { $case: "Some", value: f } })), []);
  const first = render("a");
  assert.match(textOfKept(first), />a<|-->a<!--/);
  const again = render("a");
  assert.equal(again.lists[0].rows[0], first.lists[0].rows[0], "what it read is the same");
  const second = render("b");
  assert.notEqual(second.lists[0].rows[0], first.lists[0].rows[0], "what it read changed");
  assert.match(textOfKept(second), />b<|-->b<!--/);
});

test("a row that read NaN of the page is the row kept, and its computed value is not computed again", () => {
  // `NaN` is no `===` of itself: a row that read it of the page was made
  // again at every render, and a value it computed from it computed again
  // (found by the planned rows' agreement, 2026-11-12). With views and with
  // none, where a plan makes the row.
  for (const device of [loadRenderer(false), loadRenderer(false, { PW_VIEWS: false })]) {
    const items = [{ id: 1, name: "a" }];
    const part = { part: "each", id: 3, collection: "items", binding: "item", key: "id", body: [text("item.name", 4), text("note", 5)] };
    const render = (note) => device.block(part, device.env(Object.entries({ items, note })), []).lists[0].rows[0];
    const first = render(Number.NaN);
    assert.equal(render(Number.NaN), first, "a row that read NaN of the page is kept");
    assert.notEqual(render(0), first, "and one whose NaN became a number is not");
    let computed = 0;
    device.rowsComputed({ "#t~9": (item, n) => `${item.name}${(computed += 1, n)}` }, [{ name: "#t~9", inputs: ["item", "n"] }]);
    const counted = { part: "each", id: 6, collection: "items", binding: "item", key: "id", body: [text("item.#t~9", 7)] };
    const again = () => device.block(counted, device.env(Object.entries({ items, n: Number.NaN })), []).lists[0].rows[0];
    const once = again();
    assert.equal(again(), once, "a row whose computed value read NaN is kept");
    assert.equal(computed, 1, "and its value is computed once");
  }
});

test("the two renderers agree over the compiler's own templates", () => {
  const { answered, refused } = agree(compiledRequests());
  assert.ok(answered > 1_000, `asked ${answered}`);
  assert.ok(refused < answered * 0.5, `${refused} of ${answered} refused`);
});

// --- a planned row, as a production device build makes it -----------------
//
// A page that talks to no server and shows no view (ADR-0413) makes the rows
// of a keyed list at the top of a block by its plan (ADR-0427): each hole
// computed alone (`plannedRowOf`), the row's text written only when something
// reads it (`textOf`), and what its handlers capture only when one is
// pressed. Its text is the server's, less the anchors of each text inside a
// row, which nothing there addresses (`out.bare`); each hole is what the
// server writes for its part in the row's scope; and a capture the server
// refuses as it renders is refused as the row's text is read, as a press reads
// it (ADR-0427, decision 4). Until 2026-11-12 no test held a planned row to
// the server: the renderer above has views, so no list of it is a plan's.

/** The renderer as a production device build with a plan has it. */
const device = loadRenderer(false, { PW_VIEWS: false });

/** A part a row's plan sets, as the runtime's `planned` reads it. */
const HOLE = (v) =>
  (v.part === "text" && v.context === "text") ||
  (v.part === "attribute" && v.context !== "content") ||
  v.part === "interpolated_attribute" ||
  v.part === "boolean_attribute" ||
  v.part === "event";

const isPlan = (list) => list.body.every((c) => c.chunk === "static" || HOLE(c.value));

/** The keyed lists a block keeps (`rowsKept`), in its order: each at its
 * top, in no row of a keyed list. An unkeyed list's rows are the block's. */
function keptLists(chunks, out = []) {
  for (const c of chunks) {
    if (c.chunk !== "dynamic") continue;
    const p = c.value;
    if (p.part === "each" && p.key !== null && p.key !== undefined) {
      out.push(p);
      continue;
    }
    for (const inner of [p.then, p.otherwise, p.body, p.placeholder, ...(p.arms ?? []).map((a) => a.body)]) {
      if (inner) keptLists(inner, out);
    }
  }
  return out;
}

const wrapped = (part) => [{ chunk: "dynamic", value: part }];

/** `part`, each id inside it one no other request's part has: what the
 * renderer keeps of a list is by its id, and one request's lists are no
 * other's. */
let numberedParts = 0;
function renumbered(part) {
  const copy = structuredClone(part);
  const ids = new Map();
  for (const q of dynamicParts(wrapped(copy))) {
    if (typeof q.id !== "number") continue;
    if (!ids.has(q.id)) ids.set(q.id, numberedParts++);
    q.id = ids.get(q.id);
  }
  return copy;
}

/** `part` as the device renders it before a press: no handler of a list a
 * plan makes captures anything yet. */
function uncaptured(part) {
  const copy = structuredClone(part);
  for (const list of keptLists(wrapped(copy)).filter(isPlan)) {
    for (const c of list.body) {
      if (c.chunk !== "dynamic" || c.value.part !== "event") continue;
      c.value.captures = [];
      delete c.value.renames;
    }
  }
  return copy;
}

/** The server's text of `part`, less the anchors of each text inside a row of
 * a list a block keeps. */
function bare(html, part) {
  const ids = new Set();
  for (const list of keptLists(wrapped(part))) for (const q of dynamicParts(list.body)) if (q.part === "text") ids.add(q.id);
  return ids.size === 0 ? html : html.replace(new RegExp(`<!--pw:[se](?:${[...ids].join("|")})-->`, "g"), "");
}

/** A planned list's holes in its plan's order, as `holesOf` has them: each
 * part, and an element's handlers side by side as one run. */
function holesOfList(list) {
  const out = [];
  for (let i = 0; i < list.body.length; i++) {
    const c = list.body[i];
    if (c.chunk === "static") continue;
    if (c.value.part !== "event") {
      out.push(c.value);
      continue;
    }
    const run = [c.value];
    while (list.body[i + 1]?.value?.part === "event" && list.body[i + 1].value.owner === c.value.owner) run.push(list.body[++i].value);
    out.push(run);
  }
  return out;
}

const escText = (v) => v.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

/** Each hole of a planned row, held to what the server's renderer writes for
 * its part in the row's own scope; the number of holes held. */
function holesAgree(row, at) {
  const parts = holesOfList(row.of);
  assert.equal(row.holes.length, parts.length, `a hole for each part: ${at}`);
  parts.forEach((q, h) => {
    if (Array.isArray(q)) {
      assert.equal(row.holes[h], "", `a handler's hole holds nothing: ${at}`);
      assert.equal(row.raws[h], undefined, `a handler's hole sets nothing: ${at}`);
    } else if (q.part === "text") {
      assert.equal(`<!--pw:s${q.id}-->${escText(row.holes[h])}<!--pw:e${q.id}-->`, renderer.part(q, row.env, []), `text hole ${q.id}: ${at}`);
      assert.equal(row.raws[h], row.holes[h], `text hole ${q.id} is set as it is compared: ${at}`);
    } else {
      const [name, value] = renderer.attribute(q, row.env);
      // An array of the renderer's realm, read into one of this.
      const raw = row.raws[h] === null || row.raws[h] === undefined ? row.raws[h] : [...row.raws[h]];
      if (q.part === "boolean_attribute") {
        assert.deepEqual([row.holes[h], raw], value === null ? ["", null] : [name, [name, ""]], `boolean hole ${q.id}: ${at}`);
      } else {
        assert.deepEqual([row.holes[h], raw], [`${name}="${value}"`, [name, value]], `attribute hole ${q.id}: ${at}`);
      }
    }
  });
  const captures = parts.some((q) => Array.isArray(q) && q.some((v) => v.captures?.length > 0));
  assert.equal(row.capturing, captures, `whether its handlers capture: ${at}`);
  return parts.length;
}

/** Each request to render a part that holds a list a block keeps, and no
 * view, which no planned build has; each numbered apart, and each list said
 * to be its rows' own anchor where the packaging says it (`rowAnchors`,
 * ADR-0438): a planned list whose row is one element. */
function plannedRequests(requests) {
  return requests
    .filter(([name, r]) => name === "render_part" && keptLists(wrapped(r.part)).length > 0)
    .filter(([, r]) => !dynamicParts(wrapped(r.part)).some((q) => q.part === "instance"))
    .map(([name, r]) => [name, { part: anchoredAsPacked(renumbered(r.part)), values: r.values }]);
}

/** `part`, each list in it its rows' own anchor where the packaging would
 * say so, and no other. */
function anchoredAsPacked(part) {
  for (const p of dynamicParts(wrapped(part))) if (p.part === "each") delete p.row_anchor;
  for (const list of keptLists(wrapped(part))) if (plannable(list) && rowElement(list.body) === null) list.row_anchor = "element";
  return part;
}

/** Every request rendered by the device as the server renders it; how many
 * rows, planned rows and holes were held, and how many were refused. */
function plannedAgree(requests) {
  const server = fromTheServer(requests);
  const before = fromTheServer(requests.map(([name, r]) => [name, { ...r, part: uncaptured(r.part) }]));
  const held = { rows: 0, planned: 0, anchored: 0, holes: 0, refused: 0 };
  /** Request `i`, its planned rows held to the server's. */
  const plannedAt = ([, request], i) => {
    const at = wire(request);
    const values = () => device.env(Object.entries(request.values ?? {}));
    let block;
    try {
      block = device.block(request.part, values(), []);
    } catch (error) {
      if (error.kind === undefined) throw error;
      assert.notEqual(before[i][0], 0, `only the device refused ${at}: ${error.kind}`);
      assert.equal(error.kind, refusal(before[i]), `the two refused ${at} differently: ${before[i][1]}`);
      held.refused += 1;
      return;
    }
    assert.equal(before[i][0], 0, `only the server refused ${at}: ${before[i][1]}`);
    let text = null;
    let failed = null;
    try {
      text = block.lists.reduce((t, list, k) => t + list.rows.map(device.textOf).join("") + block.strs[k + 1], block.strs[0]);
    } catch (error) {
      if (error.kind === undefined) throw error;
      failed = error.kind;
    }
    if (server[i][0] === 0) {
      assert.equal(failed, null, `only the device refused ${at} as its rows were read`);
      assert.equal(numbered(text), numbered(bare(server[i][1], request.part)), `the two rendered ${at} differently`);
    } else {
      assert.equal(failed, refusal(server[i]), `the two refused ${at} differently: ${server[i][1]}`);
      held.refused += 1;
    }
    for (const list of block.lists) {
      for (const row of list.rows) {
        held.rows += 1;
        if (!row.holes) continue;
        held.planned += 1;
        // Its own anchor, where the packaging says its list's row is one
        // element (ADR-0438): its text held to the server's above, either way.
        if (row.of.row_anchor === "element") held.anchored += 1;
        held.holes += holesAgree(row, at);
      }
    }
    // Rendered again with nothing changed, each row is the one kept, where
    // the block shows its list once.
    const again = device.block(request.part, values(), []);
    const shown = new Map();
    for (const list of block.lists) shown.set(list.part, (shown.get(list.part) ?? 0) + 1);
    block.lists.forEach((list, k) => {
      if (shown.get(list.part) !== 1) return;
      list.rows.forEach((row, n) => assert.equal(again.lists[k].rows[n], row, `a row nothing changed is the row kept: ${at}`));
    });
  };
  requests.forEach(plannedAt);
  return held;
}

/** Keyed lists of the generated parts a row's plan sets, mostly: each at the
 * top of a block, or inside a block's branch, a case's arm or an unkeyed
 * list's row, over rows of one shape and the page's values. */
function plannedLists(cases) {
  const requests = [];
  for (let seed = 1; seed <= cases; seed++) {
    const g = generated(1_000_000 + seed);
    const { r } = g;
    const values = {};
    for (let i = r.int(4); i >= 0; i--) values[r.pick(["a", "b", "c", "#t~1", "X~holds", "flag"])] = g.value(2);
    const items = g.rows(2);
    // Most lists' keys are each row's own; some repeat, and some are missing.
    if (r.chance(0.85)) items.forEach((item, i) => (item.id = i));
    values.list = items;
    const binding = r.pick(["it", "row"]);
    const key = r.pick(["id", "id", "id", "id", "name", "r.id", ""]);
    const body = g.chunks({ ...values, [binding]: items[0] ?? { id: 0, name: "n" } }, 0, []);
    // The body's parts are numbered from 0, these past them.
    let part = { part: "each", id: 900, collection: "list", binding, key, body };
    const around = r.int(5);
    if (around === 1) part = { part: "conditional", id: 901, value: "flag", then: [{ chunk: "dynamic", value: part }], otherwise: [] };
    if (around === 2) part = { part: "match", id: 901, value: "v", arms: [{ case: "Some", binding: "x", body: [{ chunk: "dynamic", value: part }] }, { case: "None", body: [] }] };
    if (around === 3) part = { part: "each", id: 901, collection: "outer", binding: "o", key: null, body: [{ chunk: "dynamic", value: part }] };
    if (around === 1) values.flag = true;
    if (around === 2) values.v = { $case: "Some", value: r.pick(NASTY) };
    if (around === 3) values.outer = [{ n: 1 }, { n: 2 }];
    requests.push(["render_part", { part, values }]);
  }
  return requests;
}

/** The elements a generated row is written of (ADR-0438): each a row may be
 * its own anchor as, and what a parser ends, moves, drops or reads otherwise. */
const ROOTS = ["tr", "tr", "tr", "li", "li", "div", "span", "p", "a", "button", "td", "h2", "dd", "section", "ul", "label", "img", "input", "option", "select", "b", "my-row"];
const INSIDE = ["span", "b", "div", "p", "a", "button", "li", "td", "th", "tr", "table", "input", "img", "br", "h3", "ul", "dd", "textarea", "template", "svg", "select", "em"];
const VOIDS = new Set(["img", "input", "br"]);

/**
 * **Planned lists whose rows are of every shape** a row's own anchor is read
 * for (ADR-0438), written as the compiler writes a row: an element with its
 * holes in its tag, its children and its end; mostly one element, of each
 * kind a row may be, or one a parser ends from inside, moves something out
 * of, or reads otherwise; some two elements, text or a space beside one, a
 * comment, an element not ended. Every hole is one a plan sets.
 */
function rowShapes(cases) {
  const requests = [];
  for (let seed = 1; seed <= cases; seed++) {
    const r = random(3_000_000 + seed);
    let ids = 0;
    let owners = 0;
    const body = [];
    const st = (s) => {
      const last = body.at(-1);
      if (last?.chunk === "static") last.value += s;
      else body.push({ chunk: "static", value: s });
    };
    const hole = (value) => body.push({ chunk: "dynamic", value });
    const text = () => hole({ part: "text", id: ids++, value: r.pick(["row.name", "row.id", "sel", "row.nope"]), context: "text" });
    const element = (tag, depth) => {
      const owner = owners++;
      st(`<${tag} data-pw="${owner}"`);
      for (let i = r.int(3); i > 0; i--) {
        st(" ");
        const roll = r.int(4);
        if (roll === 0) hole({ part: "boolean_attribute", id: ids++, owner, name: "hidden", value: r.pick(["row.on", "row.id", "sel"]) });
        else if (roll === 1) hole({ part: "interpolated_attribute", id: ids++, owner, name: "href", segments: [{ segment: "static", value: "/r/" }, { segment: "value", value: "row.id" }], context: r.pick(["url", "attribute"]) });
        else hole({ part: "attribute", id: ids++, owner, name: r.pick(["class", "title", "href", "style"]), value: r.pick(["row.name", "row.id", "sel"]), context: r.pick(["attribute", "attribute", "url", "style"]) });
      }
      if (r.chance(0.3)) hole({ part: "event", id: ids++, owner, event: "press", handler: "h", name: "n", captures: r.chance(0.5) ? ["row.id"] : [] });
      st(">");
      if (VOIDS.has(tag)) return;
      for (let i = r.int(3); i >= 0; i--) {
        const roll = r.next();
        if (roll < 0.35) text();
        else if (roll < 0.45) st(r.pick(["x", " ", "a&amp;b"]));
        else if (depth < 2) element(tag === "tr" && r.chance(0.85) ? r.pick(["td", "th"]) : r.pick(INSIDE), depth + 1);
      }
      // Most elements are ended; some are not.
      if (depth > 0 || r.chance(0.97)) st(`</${tag}>`);
    };
    const shape = r.next();
    if (shape < 0.05) st(r.pick([" ", "x", "<!--c-->"]));
    else if (shape < 0.08) text();
    element(r.pick(ROOTS), 0);
    if (shape > 0.97) element(r.pick(ROOTS), 0);
    else if (shape > 0.94) st(r.pick([" ", "y"]));
    const items = [0, 1, 2].slice(0, r.int(4)).map((id) => ({ id, name: r.pick(NASTY), on: r.chance(0.5) }));
    const values = { list: items, sel: r.pick([0, 1, "x", true]) };
    let part = { part: "each", id: 900, collection: "list", binding: "row", key: "id", body };
    if (r.chance(0.3)) {
      part = { part: "conditional", id: 901, value: "flag", then: [{ chunk: "dynamic", value: part }], otherwise: [] };
      values.flag = true;
    }
    requests.push(["render_part", { part, values }]);
  }
  return requests;
}

test("a planned row agrees with the server over generated templates", (t) => {
  const held = plannedAgree(plannedRequests([...generatedRequests(1500), ...plannedLists(6000), ...rowShapes(4000)]));
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  // Enough rows a plan made, and refusals among them, that this holds the
  // planned rows to something; and enough of them their own anchors, and
  // between comments, that it holds both (ADR-0438).
  assert.ok(held.planned > 3_000 && held.holes > 5_000, JSON.stringify(held));
  assert.ok(held.refused > 1_000 && held.rows > held.planned, JSON.stringify(held));
  assert.ok(held.anchored > 1_500 && held.planned - held.anchored > 3_000, JSON.stringify(held));
});

test("a row its own anchor writes its key where the server does, however its element's name ends", () => {
  // After the element's name, as HTML's tokenizer ends it: a space of any of
  // its kinds, a `/`, a `>`, or the markup before a hole; and refused alike
  // where the row begins with no element (ADR-0438).
  const hidden = { chunk: "dynamic", value: { part: "boolean_attribute", id: 4, owner: 0, name: "hidden", value: "item.on" } };
  const bodies = [
    ...["<li>x</li>", '<tr\tclass="r"><td>x</td></tr>', '<tr\nclass="r"></tr>', '<tr\fclass="r"></tr>', '<tr\rclass="r"></tr>', "<img/>", "<my-row>x</my-row>", "<LI>x</LI>", "<li"]
      .map((s) => [{ chunk: "static", value: s }]),
    [{ chunk: "static", value: "<li" }, hidden, { chunk: "static", value: ">x</li>" }],
    [{ chunk: "static", value: "<li " }, hidden, { chunk: "static", value: ">x</li>" }],
  ];
  const refused = [" <li>x</li>", "x", "<1>", "</li>", "<!-- -->"].map((s) => [{ chunk: "static", value: s }]);
  const requests = [];
  for (const body of [...bodies, ...refused, [hidden]]) {
    const part = { part: "each", id: 3, collection: "items", binding: "item", key: "id", row_anchor: "element", body };
    requests.push(["render_part", { part, values: { items: [{ id: 1, on: true }, { id: 2, on: false }] } }]);
    requests.push(["render_instance", { part, item: { id: 7, on: true }, values: {} }]);
  }
  assert.equal(agree(requests).refused, (refused.length + 1) * 2);
});

test("a list the packaging says is its rows' own anchor has a row of one element, and each other its comments", () => {
  // Each shape's word, as the packaging reads it (`rowElement`), held to what
  // the two renderers write (above): of the generated rows, both kinds.
  let one = 0;
  let not = 0;
  for (const [, { part }] of plannedRequests(rowShapes(4000))) {
    for (const list of keptLists(wrapped(part))) {
      if (list.row_anchor === "element") one += 1;
      else not += 1;
    }
  }
  assert.ok(one > 1_000 && not > 1_000, `${one} of one element, ${not} not`);
});

test("a planned row agrees with the server over the compiler's own templates", (t) => {
  const held = plannedAgree(plannedRequests(compiledRequests([...COMPILED, ...BUILT])));
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  assert.ok(held.planned > 20 && held.holes > 60, JSON.stringify(held));
});

// --- a list the page's own code keeps (ADR-0437) ---------------------------
//
// The packaging writes, for each list a device's page keeps whose rows a plan
// makes, the list's own key, hold and making (`own-rows.mjs`), spliced into
// the renderer where it holds a place for them. That code is the general
// code's for the list, partially evaluated, so it is held to it here: each row
// it makes is the row `plannedRowOf` makes, field by field, refused where that
// one is; its text and what its handlers capture are the same; and it keeps
// the rows the general code keeps, render after render. The general code is
// held to the server above.

/** The device's renderer with the code `ownRows` writes for `entries`
 * spliced in as the packaging splices it (`withOwnRows`), for a page of
 * template `template` some of whose lists are the general code's. */
function loadOwn(entries, template) {
  const own = ownRows(entries);
  assert.equal(rendererOf(runtime).split(OWN_ROWS).length, 2, "the renderer holds one place for a page's own lists");
  const source = `${rendererOf(withOwnRows(runtime, { ...own, unowned: true }))}\nrenderer`;
  const flags = { PW_SERVER: false, PW_LINKS: true, PW_MATCH: true, PW_VIEWS: false, PW_DIALOGS: true, PW_PLANS: true, PW_GRAPHS: true };
  return { renderer: vm.runInNewContext(source, { TextEncoder, ...flags, parts: { template } }), own };
}

/** What the general code reads of a row, in a form two realms compare. */
function rowFields(row) {
  return wire({
    key: row.key,
    holes: row.holes ? [...row.holes] : null,
    raws: row.raws ? [...row.raws].map((raw) => (raw === undefined ? "(none)" : raw === null ? "(null)" : typeof raw === "string" ? raw : [...raw])) : null,
    reads: [...row.reads],
    names: row.names,
    capturing: row.capturing ?? null,
    computing: row.computing && { reads: [...row.computing.computedReads], values: row.computing.computedValues },
  });
}

/** `fn()`'s answer, or the kind of what refused it. */
function answerOf(fn) {
  try {
    return [0, fn()];
  } catch (error) {
    if (error.kind === undefined) throw error;
    return [1, error.kind];
  }
}

/** Every row of `block` by its list and key. */
const rowsBy = (block) => new Map(block.lists.flatMap((list, k) => list.rows.map((row) => [`${k}|${row.key}`, row])));

/**
 * **What a list's own code says it found** (ADR-0439), held to what the
 * rows' keys say of the rows it found them among (`from`, its rows at its
 * last render): each row's place there, or -1 where none had its key
 * (`places`); which rows there one of these is (`taken`), how many
 * (`matched`), and, where it holds some, those it does not (`gone`); whether
 * the places of the rows that stay increase (`sorted`);
 * and the rows made again, a row of their key there and not that row
 * (`remade`). The runtime places the rows by these alone, looking no key up.
 * And of `items`, the list's items at this render: each whose row of the
 * last render is at its place, or as far from the list's end, is found
 * there, by its item, and no other is (`keyed`, the rows looked for by their
 * keys). Returns what it held: whether any row moved, went, or was made
 * again.
 */
function foundAgree(list, at, items) {
  const from = list.from;
  assert.ok(from && typeof from.length === "number", `a list its own code keeps says what it found its rows among: ${at}`);
  // Each array read into one of this realm, as the renderer's are its own.
  const rows = [...list.rows];
  const before = new Map([...from].map((row, j) => [row.key, j]));
  const places = rows.map((row) => before.get(row.key) ?? -1);
  assert.deepEqual([...(list.places ?? [])], places, `each row's place at its list's last render: ${at}`);
  const now = new Set(rows.map((row) => row.key));
  const taken = [...from].map((row) => (now.has(row.key) ? 1 : 0));
  assert.deepEqual([...(list.taken ?? [])], taken, `which rows of its last render it holds: ${at}`);
  assert.equal(list.matched, taken.filter((t) => t === 1).length, `how many rows of its last render it holds: ${at}`);
  // Those it does not hold, where it holds some: where it holds none, every
  // row goes at once.
  const gone = list.matched > 0 ? taken.flatMap((t, j) => (t === 0 ? [j] : [])) : [];
  assert.deepEqual([...(list.gone ?? [])], gone, `the rows of its last render gone: ${at}`);
  const stay = places.filter((p) => p >= 0);
  const sorted = stay.every((p, k) => k === 0 || stay[k - 1] < p);
  assert.equal(list.sorted, sorted, `whether the rows that stay are in their order: ${at}`);
  const remade = places.flatMap((p, i) => (p >= 0 && rows[i] !== from[p] ? [i] : []));
  assert.deepEqual([...(list.remade ?? [])], remade, `the rows made again: ${at}`);
  // Where the list has as many rows as it had and holds each, those out of
  // their places, while they are four at most: the runtime looks among them
  // alone for the rows to move.
  const out = places.flatMap((p, i) => (p !== i ? [i] : []));
  const displaced = rows.length === from.length && list.matched === rows.length && out.length <= 4 ? out : null;
  assert.ok(list.displaced === null || Array.isArray(list.displaced), `says which rows are out of their places, or that it does not: ${at}`);
  assert.deepEqual(list.displaced === null ? null : [...list.displaced], displaced, `the rows out of their places: ${at}`);
  // A graph's item is the tree read from it, which this realm does not see.
  if (Array.isArray(items) && !items.some((v) => typeof v === "object" && v !== null && "$graph" in v)) {
    const [m, n] = [from.length, items.length];
    const there = items.filter((v, i) => from[i]?.item === v || (i + m - n >= 0 && from[i + m - n]?.item === v)).length;
    assert.equal(list.keyed, n - there, `the rows looked for by their keys, none found by its item: ${at}`);
  }
  return { moved: !sorted, gone: taken.includes(0), remade: remade.length > 0, kept: places.some((p, i) => p >= 0 && rows[i] === from[p]), few: !sorted && displaced !== null };
}

/** **Each request rendered by both**, the page's own code and the general
 * code, twice, the second time over values changed a little: the same rows,
 * the same refusals, the same text and captures, the same rows kept; and what
 * the page's own code says it found, held to the rows' keys (`foundAgree`).
 * `rounds` and `next` render each more times, over the values `next` gives
 * after each. */
function ownAgree(requests, computes = new Map(), functions = null, { rounds = 2, next = null } = {}) {
  const entries = [];
  for (const [, request] of requests) {
    const names = new Set(Object.keys(request.values ?? {}));
    for (const { list, around } of keptOf(wrapped(request.part))) entries.push({ template: "t", list, around, names, computes });
  }
  const { renderer: mine, own } = loadOwn(entries, "t");
  const general = loadRenderer(false, { PW_VIEWS: false });
  if (functions) {
    const asked = [...computes].map(([name, inputs]) => ({ name, inputs }));
    mine.rowsComputed(functions, asked);
    general.rowsComputed(functions, asked);
  }
  const written = new Set(own.written.map((w) => w.part));
  const held = { written: written.size, refusedLists: own.refused.length, rows: 0, own: 0, kept: 0, refused: 0, why: {}, found: 0, moved: 0, few: 0, gone: 0, remade: 0, keptFound: 0, last: 0 };
  // Why the packaging refused what it refused, by the start of what it says.
  for (const { why } of own.refused) {
    const said = why.split(" ").slice(0, 5).join(" ");
    held.why[said] = (held.why[said] ?? 0) + 1;
  }
  /** Request `i`, its rows kept and made by the page's own code and the
   * general code over rounds of values perturbed. */
  const ownAt = ([, request], i) => {
    const r = random(7_000_000 + i);
    let values = request.values ?? {};
    let before = [null, null];
    // Each list its own code keeps, its rows at the render before, where
    // that render showed it and nothing refused since.
    const last = new Map();
    for (let round = 0; round < rounds; round++) {
      const at = `${wire({ part: request.part, values })} (render ${round + 1})`;
      const ours = answerOf(() => mine.block(request.part, mine.env(Object.entries(values)), []));
      const theirs = answerOf(() => general.block(request.part, general.env(Object.entries(values)), []));
      assert.equal(ours[0], theirs[0], `only one refused ${at}: ${ours[1]} / ${theirs[1]}`);
      if (ours[0] === 1) {
        assert.equal(ours[1], theirs[1], `the two refused ${at} differently`);
        held.refused += 1;
        before = [null, null];
        // A list kept before the render refused keeps what it kept.
        last.clear();
      } else {
        const [a, b] = [ours[1], theirs[1]];
        assert.deepEqual([...a.strs], [...b.strs], `the block around its lists: ${at}`);
        assert.equal(a.lists.length, b.lists.length, at);
        a.lists.forEach((list, k) => {
          const other = b.lists[k];
          assert.equal(list.part, other.part, at);
          assert.equal(list.of, other.of, `whether a plan makes its rows: ${at}`);
          assert.equal(list.rows.length, other.rows.length, at);
          list.rows.forEach((row, n) => {
            const theirRow = other.rows[n];
            held.rows += 1;
            if (written.has(list.part)) held.own += 1;
            assert.equal(row.item, theirRow.item, `the row's item: ${at}`);
            assert.equal(rowFields(row), rowFields(theirRow), `row ${row.key}: ${at}`);
            // What each of its handlers captures, as a press computes it,
            // before anything has read the row's text, which makes its
            // scope: a press can come first.
            (row.holes ?? []).forEach((_, h) => {
              const nodes = row.holes.map((_, x) => ({ x }));
              const theirNodes = row.holes.map((_, x) => ({ x }));
              const captured = answerOf(() => mine.capturesOf(row, nodes, nodes[h]));
              assert.deepEqual(captured, answerOf(() => general.capturesOf(theirRow, theirNodes, theirNodes[h])), `row ${row.key}'s captures at ${h}: ${at}`);
            });
            const text = answerOf(() => mine.textOf(row));
            assert.deepEqual(text, answerOf(() => general.textOf(theirRow)), `row ${row.key}'s text: ${at}`);
            assert.equal(rowFields(row), rowFields(theirRow), `row ${row.key}, its text read: ${at}`);
          });
        });
        // The rows each kept of the render before, by its list and key.
        if (before[0]) {
          const [oursThen, theirsThen] = before;
          for (const [key, row] of rowsBy(a)) {
            const kept = oursThen.get(key) === row;
            assert.equal(kept, theirsThen.get(key) === rowsBy(b).get(key), `whether row ${key} is the one kept: ${at}`);
            if (kept) held.kept += 1;
          }
        }
        before = [rowsBy(a), rowsBy(b)];
        // What each list its own code keeps says it found (ADR-0439): the
        // rows of its last render, where the render before showed it, and
        // none where it is shown the first time.
        for (const list of a.lists) {
          if (!written.has(list.part)) continue;
          if (last.has(list.part)) {
            assert.equal(list.from, last.get(list.part), `the rows it found its rows among are its last render's: ${at}`);
            held.last += 1;
          } else if (round === 0) {
            assert.equal(list.from?.length, 0, `a list kept the first time found its rows among none: ${at}`);
          }
          const said = foundAgree(list, at, readPath(values, list.of.collection));
          held.found += 1;
          if (said.moved) held.moved += 1;
          if (said.few) held.few += 1;
          if (said.gone) held.gone += 1;
          if (said.remade) held.remade += 1;
          if (said.kept) held.keptFound += 1;
        }
        last.clear();
        for (const list of a.lists) if (written.has(list.part)) last.set(list.part, list.rows);
      }
      values = next ? next(r, values) : round === 0 && r.chance(0.5) ? values : perturbed(r, values);
    }
  };
  requests.forEach(ownAt);
  return held;
}

/** The names a row's computed value is held under: of one field, and of two
 * as the compiler names each (`#t.P~ui~5`, a `BrowserRow`'s path less its
 * row's name). */
const COMPUTED_NAMES = ["#t~c", "#t.P~ui~5"];

/** Planned lists whose rows compute a value (ADR-0424), as the large list's
 * class does from its row and the page's selection: given by the page's
 * computed module, held by a row the host rendered, or none to be had. */
function computedLists(cases) {
  const requests = [];
  for (let seed = 1; seed <= cases; seed++) {
    const g = generated(2_000_000 + seed);
    const { r } = g;
    const items = g.rows(1);
    // A name of one field, or of two as the compiler names one
    // (`row.#t.P~ui~5`), whose first a host's item may hold a record at.
    const name = r.pick(COMPUTED_NAMES);
    items.forEach((item, i) => {
      item.id = i;
      if (r.chance(0.2)) item[name] = r.pick(NASTY);
      if (name.includes(".") && r.chance(0.3)) item["#t"] = r.chance(0.7) ? { "P~ui~5": r.pick(NASTY) } : r.pick(NASTY);
    });
    const values = { list: items, sel: r.pick([0, 1, 2, "x", Number.NaN]) };
    if (r.chance(0.1)) delete values.sel;
    const computed = `row.${name}`;
    const read = r.chance(0.8) ? computed : r.pick(["row.id", "row.name", "sel"]);
    const body = [
      { chunk: "static", value: "<tr " },
      { chunk: "dynamic", value: { part: "attribute", id: 0, owner: 0, name: "class", value: read, context: "attribute" } },
      { chunk: "static", value: "><td>" },
      { chunk: "dynamic", value: { part: "text", id: 1, value: "row.id", context: "text" } },
      { chunk: "static", value: "</td><td>" },
      { chunk: "dynamic", value: { part: "text", id: 2, value: computed, context: "text" } },
      { chunk: "static", value: "</td><td><button " },
      { chunk: "dynamic", value: { part: "event", id: 3, owner: 1, event: "press", handler: "h", name: "n", captures: ["row.id"] } },
      { chunk: "static", value: ">x</button></td></tr>" },
    ];
    requests.push(["render_part", { part: { part: "each", id: 900, collection: "list", binding: "row", key: "id", body }, values }]);
  }
  return requests;
}

test("a list's own code keeps and makes its rows as the general code does, over generated templates", (t) => {
  const held = ownAgree(plannedRequests([...generatedRequests(1500), ...plannedLists(6000), ...rowShapes(2000)]));
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  // Enough lists written, rows made by them and kept, and refusals of both
  // kinds (of a render and of a list), that this holds the code to much.
  assert.ok(held.written > 3_000 && held.own > 3_000 && held.kept > 1_000, JSON.stringify(held));
  assert.ok(held.refused > 1_000 && held.refusedLists > 100, JSON.stringify(held));
});

test("a list's own code computes what its rows compute as the general code does", (t) => {
  const compute = (row, sel) => (row.id === sel ? "danger" : row.id === 2 ? [row.id] : "");
  const computes = new Map(COMPUTED_NAMES.map((name) => [name, ["row", "sel"]]));
  const functions = Object.fromEntries(COMPUTED_NAMES.map((name) => [name, compute]));
  const held = ownAgree(plannedRequests(computedLists(1500)), computes, functions);
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  assert.ok(held.written > 1_000 && held.own > 1_000 && held.kept > 300 && held.refused > 50, JSON.stringify(held));
  // And where the page's computed module gives no function for it: its
  // path's fields, as the general code reads them, which most rows lack.
  const unanswered = ownAgree(plannedRequests(computedLists(600)), computes);
  t.diagnostic(`held with no function: ${JSON.stringify(unanswered)}`);
  assert.ok(unanswered.written > 400 && unanswered.refused > 300 && unanswered.own > 60, JSON.stringify(unanswered));
});

test("a list's own code keeps and makes its rows as the general code does, over the compiler's own templates", (t) => {
  const held = ownAgree(plannedRequests(compiledRequests([...COMPILED, ...BUILT])));
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  assert.ok(held.written > 10 && held.own > 50, JSON.stringify(held));
});

/** A list's item no other is, of the shape of `like`: its id, name and
 * `r.id` its own, so that a list keyed by any of them takes it. */
let freshItems = 0;
function freshItem(r, like) {
  const n = 1_000_000 + freshItems++;
  const item = { ...(like && typeof like === "object" && !Array.isArray(like) ? like : {}), id: n, name: `n${n}` };
  if ("r" in item) item.r = { id: `r${n}` };
  if (r.chance(0.3) && "on" in item) item.on = !item.on;
  return item;
}

/** **A list as a press leaves it** (ADR-0439): its very items, two swapped,
 * one gone, added, moved, made again or changed, written twice, all
 * reversed, shuffled, rotated, cleared or replaced, or the list itself. */
function reordered(r, list) {
  const out = [...list];
  const at = (extra = 0) => r.int(out.length + extra);
  switch (r.int(16)) {
    case 0:
      return list;
    case 1:
      if (out.length > 1) {
        const [i, j] = [at(), at()];
        [out[i], out[j]] = [out[j], out[i]];
      }
      return out;
    case 2:
      if (out.length) out.splice(at(), 1);
      return out;
    case 3:
      if (out.length > 2) out.splice(at(), 1 + r.int(2));
      return out;
    case 4:
      out.splice(at(1), 0, freshItem(r, out[0]));
      return out;
    case 5:
      if (out.length > 1) {
        const [moved] = out.splice(at(), 1);
        out.splice(at(1), 0, moved);
      }
      return out;
    case 6:
      return out.reverse();
    case 7:
      for (let i = out.length - 1; i > 0; i--) {
        const j = r.int(i + 1);
        [out[i], out[j]] = [out[j], out[i]];
      }
      return out;
    case 8:
      return out.length ? [...out.slice(1), out[0]] : out;
    case 9:
      return [];
    case 10:
      return out.map((item) => freshItem(r, item));
    case 11:
      if (out.length) {
        const i = at();
        out[i] = typeof out[i] === "object" && out[i] !== null ? { ...out[i] } : out[i];
      }
      return out;
    case 12:
      if (out.length) {
        const i = at();
        out[i] = typeof out[i] === "object" && out[i] !== null ? { ...out[i], name: r.pick(NASTY) } : out[i];
      }
      return out;
    case 13:
      if (out.length) out.splice(at(1), 0, out[at()]);
      return out;
    case 14:
      return [...out, freshItem(r, out[0]), ...(r.chance(0.5) ? [freshItem(r, out[0])] : [])];
    default:
      return [freshItem(r, out[0]), ...out];
  }
}

/** **The page's values after a press**: each list's very items reordered,
 * a value beside them changed now and then, or values changed a little. */
function pressed(r, values) {
  if (r.chance(0.15)) return perturbed(r, values);
  const out = { ...values };
  for (const [name, v] of Object.entries(values)) if (Array.isArray(v) && r.chance(0.85)) out[name] = reordered(r, v);
  if (r.chance(0.25)) {
    const beside = Object.keys(values).filter((k) => !Array.isArray(values[k]));
    if (beside.length) {
      const k = r.pick(beside);
      out[k] = perturbed(r, values[k]);
    }
  }
  return out;
}

/** Planned lists of as many as forty rows of one shape, a table's as the
 * large list's is: their keys their own, a row's class read of the page or
 * of the row, and a handler that captures the row. */
function longLists(cases) {
  const requests = [];
  for (let seed = 1; seed <= cases; seed++) {
    const r = random(4_000_000 + seed);
    const items = Array.from({ length: r.int(41) }, (_, id) => ({ id, name: r.pick(NASTY), on: r.chance(0.5) }));
    const body = [
      { chunk: "static", value: "<tr " },
      { chunk: "dynamic", value: { part: "attribute", id: 0, owner: 0, name: "class", value: r.pick(["row.name", "sel", "row.id"]), context: "attribute" } },
      { chunk: "static", value: "><td>" },
      { chunk: "dynamic", value: { part: "text", id: 1, value: "row.id", context: "text" } },
      { chunk: "static", value: "</td><td>" },
      { chunk: "dynamic", value: { part: "text", id: 2, value: r.pick(["row.name", "note"]), context: "text" } },
      { chunk: "static", value: "</td><td><button " },
      { chunk: "dynamic", value: { part: "event", id: 3, owner: 1, event: "press", handler: "h", name: "n", captures: r.chance(0.8) ? ["row.id"] : ["sel"] } },
      { chunk: "static", value: ">x</button></td></tr>" },
    ];
    requests.push(["render_part", { part: { part: "each", id: 900, collection: "list", binding: "row", key: "id", body }, values: { list: items, sel: r.pick([0, 1, "x"]), note: r.pick(NASTY) } }]);
  }
  return requests;
}

test("a list's own code finds its rows where they were, and says what it found as their keys say", (t) => {
  // Render after render, each list's very items reordered as presses do:
  // what its own code keeps and makes is the general code's, and what it
  // says it found is what the rows' keys say (ADR-0439).
  const held = ownAgree(plannedRequests([...plannedLists(2500), ...rowShapes(1500), ...longLists(800)]), new Map(), null, { rounds: 8, next: pressed });
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  assert.ok(held.found > 15_000 && held.last > 12_000, JSON.stringify(held));
  assert.ok(held.moved > 1_500 && held.few > 600 && held.gone > 1_800 && held.remade > 1_500 && held.keptFound > 7_000 && held.own > 100_000, JSON.stringify(held));
  assert.ok(held.refused > 1_000, JSON.stringify(held));
});

test("a list's own code finds its rows where they were as what they compute moves and does not", (t) => {
  // The large list's kind: a value each row computes from its row and the
  // page's selection, which the press beside moves or does not.
  const compute = (row, sel) => (row.id === sel ? "danger" : row.id === 2 ? [row.id] : "");
  const computes = new Map(COMPUTED_NAMES.map((name) => [name, ["row", "sel"]]));
  const functions = Object.fromEntries(COMPUTED_NAMES.map((name) => [name, compute]));
  const held = ownAgree(plannedRequests(computedLists(1200)), computes, functions, { rounds: 8, next: pressed });
  t.diagnostic(`held: ${JSON.stringify(held)}`);
  assert.ok(held.found > 3_000 && held.moved > 300 && held.few > 250 && held.gone > 300 && held.keptFound > 1_500, JSON.stringify(held));
});

const PARTS = [
  { part: "text", id: 1, value: "v", context: "text" },
  { part: "text", id: 1, value: "v", context: "attribute" },
  { part: "text", id: 1, value: "v", context: "url" },
  { part: "text", id: 1, value: "v", context: "style" },
  { part: "text", id: 1, value: "v", context: "raw_html" },
  { part: "attribute", id: 1, owner: 0, name: "x", value: "v", context: "content" },
  { part: "interpolated_attribute", id: 1, owner: 0, name: "href", segments: [{ segment: "value", value: "v" }], context: "url" },
  { part: "interpolated_attribute", id: 1, owner: 0, name: "title", segments: [{ segment: "value", value: "v" }], context: "attribute" },
  { part: "event", id: 1, owner: 0, event: "press", handler: "h", captures: ["v"] },
];

/** Every code point there is, in runs short enough to read a failure. */
function codePoints(each) {
  for (let at = 0; at <= 0x10ffff; at += 0x400) {
    let run = "";
    for (let c = at; c < at + 0x400 && c <= 0x10ffff; c++) {
      if (c < 0xd800 || c > 0xdfff) run += String.fromCodePoint(c);
    }
    if (run) each(run);
  }
}

test("each escaping agrees on every code point", () => {
  const requests = [];
  codePoints((run) => {
    for (const part of PARTS) requests.push(["render_part", { part, values: { v: run } }]);
  });
  assert.ok(requests.length > 9_000);
  assert.equal(agree(requests).refused, 0);
});

test("an address agrees on every code point inside and around a scheme", () => {
  const requests = [];
  for (let c = 0; c <= 0x2fff; c++) {
    const ch = String.fromCodePoint(c);
    if (c >= 0xd800 && c <= 0xdfff) continue;
    for (const v of [`java${ch}script:x`, `${ch}javascript:x`, `&#${c};avascript:x`, `&#x${c.toString(16)};avascript:x`]) {
      requests.push(["render_part", { part: { part: "text", id: 1, value: "v", context: "url" }, values: { v } }]);
    }
  }
  // The widest characters where a reference's window ends.
  for (const pad of [8, 9, 10, 11, 12, 13]) {
    for (const ch of ["é", "日", "😀"]) {
      const v = `&${"a".repeat(pad)}${ch};javascript:x`;
      requests.push(["render_part", { part: { part: "text", id: 1, value: "v", context: "url" }, values: { v } }]);
    }
  }
  assert.equal(agree(requests).refused, 0);
});

test("keys sort as the server's do, by code point", () => {
  const event = { part: "event", id: 1, owner: 0, event: "press", handler: "h", captures: ["v"] };
  const requests = [["captures_value", { parts: [event], values: { v: { a: 1, "": 2, "𐐀": 3, "é": 4, Z: 5, "": 6 } } }]];
  assert.equal(agree(requests).refused, 0);
});

test("a view inside itself stops where the server's does", () => {
  const chain = (length) => ({
    $graph: Array.from({ length }, (_, i) => (i + 1 < length ? { n: i, next: { $node: i + 1 } } : { n: i, next: false })),
  });
  const view = {
    path: "t.V", name: "V", params: ["p"], schema: "s", chunks: [
      { chunk: "dynamic", value: { part: "text", id: 1, value: "p.n", context: "text" } },
      { chunk: "dynamic", value: { part: "conditional", id: 2, value: "p.next", otherwise: [], then: [
        { chunk: "dynamic", value: { part: "instance", id: 3, path: "t.V", args: [["p", "p.next"]], elements: 1, deepest: 1 } },
      ] } },
    ],
  };
  const part = { part: "instance", id: 9, path: "t.V", args: [["p", "c"]], elements: 0, deepest: 1 };
  const requests = [480, 498, 499, 500, 501, 502, 600].map((length) => ["render_part", { part, values: { c: chain(length) }, templates: [view] }]);
  const { refused } = agree(requests);
  assert.ok(refused >= 1 && refused < requests.length, `${refused} of ${requests.length} refused`);
});

test("a graph that is no tree is refused where it is read", () => {
  const part = { part: "text", id: 1, value: "v", context: "text" };
  const requests = [
    { $graph: [] },
    { $graph: 3 },
    { $graph: [{ a: { $node: 0 } }] },
    { $graph: [{ a: { $node: 1 }, b: { $node: 1 } }, { x: 1 }] },
    { $graph: [{ a: 1 }, { b: 2 }] },
    { $graph: [{ a: { $node: 9 } }] },
    { $graph: [{ a: { $node: "x" } }] },
    { $node: 1 },
  ].map((v) => ["render_part", { part, values: { v } }]);
  assert.equal(agree(requests).refused, requests.length);
});

test("what has been an exploit agrees in every context", () => {
  const spellings = [];
  for (const scheme of ["javascript:x", "vbscript:x", "data:x", "JAVASCRIPT:x", "VbScript:x", "DaTa:x"]) {
    for (const lead of ["", " ", "\t", "\u0001", "&#9;", "&#x20;", "&#133;", "&#1114112;", "&#xD83D;", "&#xD;&#xA;", "&;", "&amp;"]) {
      spellings.push(lead + scheme);
    }
    spellings.push(scheme.replace(":", "&colon;"), scheme.replace(":", "&#58;"), scheme.replace(":", "&#x3a;"));
    for (let zeros = 0; zeros < 11; zeros++) {
      // A reference on either side of where its window ends.
      spellings.push(`&#${"0".repeat(zeros)}${scheme.charCodeAt(0)};${scheme.slice(1)}`);
    }
  }
  const requests = [];
  for (const v of [...NASTY, ...spellings]) {
    for (const part of PARTS) requests.push(["render_part", { part, values: { v } }]);
    for (const context of ["url", "style", "attribute"]) {
      requests.push(["attribute_value", { part: { part: "attribute", id: 1, owner: 0, name: "x", value: "v", context }, values: { v } }]);
    }
  }
  assert.equal(agree(requests).refused, 0);
});

const text = (value, id = 1) => ({ chunk: "dynamic", value: { part: "text", id, value, context: "text" } });
const event = (owner, name, id) => ({ chunk: "dynamic", value: { part: "event", id, owner, event: "press", handler: "h", captures: [name] } });

test("a path reads a record's fields and nothing else's", () => {
  const each = (key) => ({ part: "each", id: 1, collection: "list", binding: "row", key, body: [text("row.x", 2)] });
  const variant = { $case: "Some", value: { x: 1 } };
  const requests = [
    ["render_part", { part: text("v.value").value, values: { v: variant } }],
    ["render_part", { part: text("v.$case").value, values: { v: variant } }],
    ["render_part", { part: text("v.value.x").value, values: { v: variant } }],
    ["render_part", { part: text("v.length").value, values: { v: [1, 2] } }],
    ["render_part", { part: text("v.length").value, values: { v: "text" } }],
    ["render_part", { part: text("v.toString").value, values: { v: { a: 1 } } }],
    ["render_part", { part: each("value"), values: { list: [variant] } }],
    ["render_part", { part: each("$case"), values: { list: [variant, { $case: "None" }] } }],
    ["render_part", { part: each("length"), values: { list: [[1], [2, 3]] } }],
    ["render_part", { part: each(""), values: { list: ["a", "b"] } }],
    ["render_part", { part: each(""), values: { list: [{ x: 1 }] } }],
    ["render_part", { part: text("v.a").value, values: { v: { a: 1 } } }],
    ["render_part", { part: each("id"), values: { list: [{ id: 1, x: "a" }] } }],
  ];
  const { answered, refused } = agree(requests);
  assert.ok(refused > 0 && refused < answered, `${refused} of ${answered} refused`);
});

test("each element's handlers write one attribute of their own", () => {
  const values = { a: 1, b: "two" };
  const part = { part: "conditional", id: 9, value: "t", then: [event(1, "a", 1), event(1, "b", 2), event(2, "a", 3), { chunk: "static", value: ">" }, event(3, "b", 4)], otherwise: [] };
  const requests = [["render_part", { part, values: { ...values, t: true } }]];
  assert.equal(agree(requests).refused, 0);
});

test("an instance reads its parameters and none of the page's values", () => {
  const view = (read) => ({ path: "t.V", name: "V", params: ["p"], schema: "s", chunks: [text(read)] });
  const part = { part: "instance", id: 9, path: "t.V", args: [["p", "given"]] };
  const values = { given: "yes", outer: "no" };
  assert.equal(agree([["render_part", { part, values, templates: [view("p")] }]]).refused, 0);
  assert.equal(agree([["render_part", { part, values, templates: [view("outer")] }]]).refused, 1);
});

test("what changed in a row agrees on a block, a case and a list inside it", () => {
  const match = { chunk: "dynamic", value: { part: "match", id: 3, value: "row.state", arms: [
    { case: "Some", binding: "x", body: [text("x", 4)] },
    { case: "None", body: [{ chunk: "static", value: "<i>none</i>" }] },
  ] } };
  const list = { chunk: "dynamic", value: { part: "each", id: 5, collection: "row.items", binding: "it", key: "id", body: [text("it.v", 6)] } };
  const each = { part: "each", id: 0, collection: "rows", binding: "row", key: "id", body: [match, list] };
  const row = (state, items) => ({ id: "a", state, items });
  const some = (x) => ({ $case: "Some", value: x });
  const none = { $case: "None" };
  const requests = [
    // The arm it showed, a value inside it changed: set in place.
    [some("one"), [], some("two"), []],
    // Another arm: rendered again.
    [some("one"), [], none, []],
    [none, [], none, []],
    // A list inside: the same, a value of another type, one more, one fewer, moved.
    [none, [{ id: 1, v: "1" }], none, [{ id: 1, v: "1" }]],
    [none, [{ id: 1, v: "1" }], none, [{ id: 1, v: 1 }]],
    [none, [{ id: 1, v: 1 }], none, [{ id: 1, v: 1n }]],
    [none, [{ id: 1, v: true }], none, [{ id: 1, v: "true" }]],
    [none, [{ id: 1, v: null }], none, [{ id: 1, v: "" }]],
    [none, [{ id: 1, v: "a" }], none, [{ id: 1, v: "a" }, { id: 2, v: "b" }]],
    [none, [{ id: 1, v: "a" }, { id: 2, v: "b" }], none, [{ id: 2, v: "b" }]],
    [none, [{ id: 1, v: "a" }, { id: 2, v: "b" }], none, [{ id: 2, v: "b" }, { id: 1, v: "a" }]],
  ].map(([a, x, b, y]) => ["instance_changes", { part: each, was: row(a, x), now: row(b, y), values: {} }]);
  assert.equal(agree(requests).refused, 0);
});

test("a list inside a row is refused, not walked, when its keys repeat or are missing", () => {
  const list = { chunk: "dynamic", value: { part: "each", id: 5, collection: "row.items", binding: "it", key: "k", body: [text("it.k", 6)] } };
  const each = { part: "each", id: 0, collection: "rows", binding: "row", key: "id", body: [list] };
  const row = (items) => ({ id: "a", items });
  const k = (name) => ({ k: name });
  const requests = [
    [[k("a")], [k("a"), k("a")]],
    [[k("p"), k("q")], [k("z"), k("z")]],
    [[], [k("a"), k("a")]],
    [[k("a")], [k("a"), {}]],
    [[k("a")], [{}]],
    [[{}], [{}, k("b")]],
  ].map(([was, now]) => ["instance_changes", { part: each, was: row(was), now: row(now), values: {} }]);
  assert.equal(agree(requests).refused, requests.length);
});

test("an item of a list inside a row is refused as rendering it would be", () => {
  const listOf = (body) => ({ part: "each", id: 0, collection: "rows", binding: "row", key: "id", body: [
    { chunk: "dynamic", value: { part: "each", id: 5, collection: "row.items", binding: "it", key: "id", body } },
  ] });
  const row = (items) => ({ id: "a", items });
  const asks = (body, was, now) => ["instance_changes", { part: listOf(body), was: row(was), now: row(now), values: {} }];
  const reads = [text("it.v", 6)];
  const flips = [{ chunk: "dynamic", value: { part: "conditional", id: 7, value: "it.flag", then: [text("it.absent", 8)], otherwise: [] } }];
  // New to the list, and rendering it finds nothing to show.
  assert.equal(agree([asks(reads, [{ id: 1, v: "x" }], [{ id: 1, v: "x" }, { id: 2 }])]).refused, 1);
  // Kept in the list, set in place, and refused by what it must render again.
  assert.equal(agree([asks(flips, [{ id: 1, flag: false }], [{ id: 1, flag: true }])]).refused, 1);
  // And each of those, with what it needs, is no refusal.
  assert.equal(agree([asks(reads, [{ id: 1, v: "x" }], [{ id: 1, v: "x" }, { id: 2, v: "y" }])]).refused, 0);
  assert.equal(agree([asks(flips, [{ id: 1, flag: false }], [{ id: 1, flag: true, absent: "here" }])]).refused, 0);
});

test("a change in a row is what the server says it is", () => {
  const each = {
    part: "each", id: 0, collection: "lines", binding: "line", key: "id", body: [
      { chunk: "dynamic", value: { part: "interpolated_attribute", id: 1, owner: 0, name: "aria-label", segments: [{ segment: "static", value: "Remove " }, { segment: "value", value: "line.name" }], context: "attribute" } },
      { chunk: "dynamic", value: { part: "text", id: 2, value: "line.name", context: "text" } },
      { chunk: "dynamic", value: { part: "conditional", id: 3, value: "line.gift", then: [{ chunk: "static", value: "<b>gift</b>" }], otherwise: [] } },
    ],
  };
  const row = (name, gift) => ({ id: "a", name, gift });
  const requests = [
    ["instance_changes", { part: each, was: row("Tea", false), now: row("Green Tea", false), values: {} }],
    ["instance_changes", { part: each, was: row("Tea", false), now: row("Tea", true), values: {} }],
    ["instance_changes", { part: each, was: row("Tea", false), now: row("Tea", false), values: {} }],
  ];
  assert.equal(agree(requests).refused, 0);
});

test("the renderer is small", async () => {
  const { transform } = await import("esbuild");
  const { brotliCompressSync, constants } = await import("node:zlib");
  const sentOf = async (source) => {
    const { code } = await transform(rendererOf(source), { minify: true, format: "esm" });
    return brotliCompressSync(Buffer.from(code), { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;
  };
  // As a build none of whose rows is its own anchor has it (ADR-0438), with
  // what a list keeps of its rows ("a press changes what it changes"), a
  // planned row emitted into its holes (ADR-0420), and what a row computes
  // and keeps (ADR-0424): 6,283 bytes when the last two came, from under 6 KB.
  const sent = await sentOf(runtime.replace(ELEMENT_LINES, ""));
  assert.ok(sent < 6.5 * 1024, `the renderer is ${sent} bytes as sent, and was 77 KB as WebAssembly`);
  // And a row its own anchor, its key written after its element's name: 146
  // bytes when it came.
  const anchored = (await sentOf(runtime)) - sent;
  assert.ok(anchored > 0 && anchored < 200, `a row its own anchor is ${anchored} bytes of the renderer`);
});
