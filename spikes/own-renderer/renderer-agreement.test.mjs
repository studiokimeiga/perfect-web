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

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, "../..");
const runtime = readFileSync(path.join(here, "public/pw-runtime.mjs"), "utf8");
const BEGIN = "const renderer = (() => {";
const END = "// --- end of the browser's renderer";

function loadRenderer(server = true) {
  const start = runtime.indexOf(BEGIN);
  const end = runtime.indexOf(END);
  assert.ok(start >= 0 && end > start, "the runtime holds the renderer between its markers");
  return vm.runInNewContext(`${runtime.slice(start, end)}\nrenderer`, {
    TextEncoder,
    PW_SERVER: server,
    PW_LINKS: true,
    PW_MATCH: true,
    PW_VIEWS: true,
    PW_DIALOGS: true,
    PW_PLANS: true,
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

/** Tokens numbered by when each is first seen: which are the same is what
 * the two renderers must agree on, and what a token is, they need not. */
function numbered(html) {
  const seen = new Map();
  return html.replace(/@([A-Za-z0-9_-]+)-->/g, (_, token) => {
    if (!seen.has(token)) seen.set(token, seen.size);
    return `@${seen.get(token)}-->`;
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

  return { r, value, chunks, record, scope };
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

function compiledRequests() {
  const requests = [];
  const r = random(424242);
  for (const file of ["spikes/own-renderer/store-ir.json", "docs/evidence/E10/kiokun/templates.json"]) {
    const templates = JSON.parse(readFileSync(path.join(root, file), "utf8"));
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

test("the two renderers agree over the compiler's own templates", () => {
  const { answered, refused } = agree(compiledRequests());
  assert.ok(answered > 1_000, `asked ${answered}`);
  assert.ok(refused < answered * 0.5, `${refused} of ${answered} refused`);
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
  const start = runtime.indexOf(BEGIN);
  const end = runtime.indexOf(END);
  const { code } = await transform(runtime.slice(start, end), { minify: true, format: "esm" });
  const sent = brotliCompressSync(Buffer.from(code), { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;
  // With what a list keeps of its rows ("a press changes what it changes"),
  // a planned row emitted into its holes (the planned row's ADR), and what a
  // row computes and keeps (the row's computed holes' ADR): 6,283 bytes when
  // the last two came, from under 6 KB.
  assert.ok(sent < 6.5 * 1024, `the renderer is ${sent} bytes as sent, and was 77 KB as WebAssembly`);
});
