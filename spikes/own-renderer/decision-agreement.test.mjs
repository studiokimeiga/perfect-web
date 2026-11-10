// **The browser decides as `pw-resume` decides** (ADR-0414).
//
// A page answers the decision in a few lines of its runtime, from the build's
// table, and `pw-resume` is the Rust crate that specifies it. Two
// implementations of a decision can disagree, and a disagreement shows nothing
// on the page until a handler this build never compiled attaches. So they are
// held to each other here, in every run of the suite: the crate, built as the
// WebAssembly module pages once fetched, and the runtime's own lines, the same
// tables and the same manifests, and every field of every answer compared.
//
// The manifests are a real one changed: each field alone to each of the values
// that could confuse it, every pair of fields, and a seeded walk over all nine.
// Each table is one a host writes: the scopes a page has, a page the table
// does not speak for, a handler that captures nothing.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, "../..");
const runtime = readFileSync(path.join(here, "public/pw-runtime.mjs"), "utf8");
const BEGIN = "const decider = (lines, page) => {";
const END = "// --- end of the decision";

function loadDecider() {
  const start = runtime.indexOf(BEGIN);
  const end = runtime.indexOf(END);
  assert.ok(start >= 0 && end > start, "the runtime holds the decision between its markers");
  return vm.runInNewContext(`${runtime.slice(start, end)}\ndecider`);
}
const decider = loadDecider();

const wasmFile = path.join(
  process.env.CARGO_TARGET_DIR ?? path.join(root, "target"),
  "wasm32-unknown-unknown/browser/pw_resume_wasm.wasm",
);
const wasm = readFileSync(wasmFile);

// The module's recoveries, in its order: `last_recovery` returns an index into
// this list, and `RefetchRegion` is 0 (ADR-0155).
const RECOVERY = [
  "refetch-region",
  "rerender-private-slot",
  "reload",
  "retry-interaction",
  "require-user-confirmation",
  "reject-irrecoverable",
];

const PAGE = "todo.app.Todos~ui";

/** The real module, told a table as a page tells it. */
async function theModule(table) {
  const { instance } = await WebAssembly.instantiate(wasm, {});
  const { memory, alloc, decide_manifest, last_recovery, know, know_document } = instance.exports;
  const write = (s) => {
    const bytes = new TextEncoder().encode(s);
    const ptr = alloc(bytes.length);
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  };
  know(...write(table));
  const line = table.split("\n").find((l) => l.startsWith(`#page|${PAGE}|`));
  if (line) {
    const [, , schema, scope] = line.split("|");
    know_document(...write(`${schema}|${scope}`));
  }
  return (manifest) => {
    const code = decide_manifest(...write(manifest));
    return { attach: code === 0 || code === 1, code, recovery: RECOVERY[last_recovery()] };
  };
}

const header = "#build|B1\n#warm|1\n";
const handlers = "h1|item.id\nh2|\nh3\n";
const TABLES = [
  `${header}#page|${PAGE}|doc|public\n${handlers}`,
  `${header}#bundle|${PAGE}|/bundle/x.mjs\n#page|${PAGE}|doc|public\n${handlers}`,
  `${header}#page|${PAGE}||public\n${handlers}`,
  `${header}#page|${PAGE}|doc|session:\n${handlers}`,
  `${header}#page|${PAGE}|doc|session:a\n${handlers}`,
  `${header}#page|${PAGE}|doc|user:a\n${handlers}`,
  `${header}#page|${PAGE}|doc|org\n${handlers}`,
  // Another page's word is not this one's.
  `${header}#page|other.app.Other~ui|doc|public\n${handlers}`,
  // A page whose name begins as this one's does.
  `${header}#page|${PAGE}x|doc|public\n${handlers}`,
  `${header}${handlers}`,
  `#page|${PAGE}|doc|public\n`,
  "",
];

const VALUES = [
  // scheme
  ["", "0", "1", "2", "3", "02", "002", "+2", "-2", "2 ", " 2", "2.0", "0x2", "abc", "99999999999999999999"],
  // abi
  ["", "0", "1", "2", "01", "+1", "-1", "1 ", "1.0", "99999999999999999999"],
  // build: nothing this decision asks
  ["B1", "B2", ""],
  // handler
  ["h1", "h2", "h3", "nope", "", "#page", "#build", "h1 ", "é", "constructor", "__proto__"],
  // capture
  ["item.id", "", "other", "item.id ", "é"],
  // document
  ["doc", "", "other", "Doc", "doc "],
  // scope
  ["public", "", "Public", "session:", "session:a", "session:b", "user:a", "user:", "org", "é"],
  // captures
  ["", "x", "é"],
  // construct
  ["", "region", "private", "command", "resource", "other", "constructor", "__proto__", "PRIVATE"],
];

const BASES = [
  ["2", "1", "B1", "h1", "item.id", "doc", "public", "x", "region"],
  ["2", "1", "B1", "h2", "", "doc", "public", "", "region"],
  ["2", "1", "B1", "h3", "", "doc", "public", "", "region"],
];

/** The two asked about each of `manifests`; what they differ on, and the
 * codes they gave. */
async function compare(manifests) {
  const codes = new Set();
  const differences = [];
  let asked = 0;
  for (const table of TABLES) {
    const real = await theModule(table);
    const device = decider(table.split("\n"), PAGE);
    for (const manifest of manifests) {
      const a = real(manifest);
      const b = device(manifest);
      asked += 1;
      codes.add(a.code);
      const same = a.attach === b.attach && a.code === b.code && (a.attach || a.recovery === b.recovery);
      if (!same && differences.length < 12) {
        differences.push({ table, manifest, module: a, device: b });
      }
    }
  }
  assert.deepEqual(differences, [], `${differences.length} difference(s) shown, of ${asked} asked`);
  return { codes, asked };
}

const singles = () => {
  const out = [];
  for (const base of BASES) {
    out.push(base.join("|"));
    for (let i = 0; i < base.length; i += 1) {
      for (const value of VALUES[i]) {
        const changed = [...base];
        changed[i] = value;
        out.push(changed.join("|"));
      }
    }
    // Too few fields and too many.
    for (let n = 0; n < base.length; n += 1) out.push(base.slice(0, n).join("|"));
    out.push(`${base.join("|")}|more`);
    out.push(`${base.join("|")}|`);
  }
  return out;
};

const pairs = () => {
  const out = [];
  for (const base of BASES) {
    for (let i = 0; i < base.length; i += 1) {
      for (let j = i + 1; j < base.length; j += 1) {
        for (const first of VALUES[i]) {
          for (const second of VALUES[j]) {
            const changed = [...base];
            changed[i] = first;
            changed[j] = second;
            out.push(changed.join("|"));
          }
        }
      }
    }
  }
  return out;
};

/** A fixed walk: one seed, so a failure is the same failure next time. */
const walk = (count) => {
  let state = 0x9e3779b9;
  const next = (n) => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    return state % n;
  };
  const out = [];
  for (let k = 0; k < count; k += 1) {
    out.push(VALUES.map((values) => values[next(values.length)]).join("|"));
  }
  return out;
};

test("every change to one field of a manifest is answered as the module answers", async () => {
  await compare(singles());
});

test("every change to two fields is answered as the module answers", async () => {
  await compare(pairs());
});

test("a walk over all nine fields is answered as the module answers", async () => {
  await compare(walk(4000));
});

test("the manifests reach each answer this decision has", async () => {
  const { codes } = await compare(singles());
  // Resume, and each refusal a manifest can earn: an unsupported scheme, an
  // unsupported ABI, an unknown handler, a widened scope, another document,
  // no bytes under a schema. The rest (a migration, a mixed build) need
  // something no page has.
  assert.deepEqual([...codes].sort((a, b) => a - b), [0, 2, 3, 4, 5, 6, 9]);
});

test("each recovery a refusal can name is one the module names", async () => {
  const seen = new Set();
  for (const table of TABLES) {
    const device = decider(table.split("\n"), PAGE);
    for (const manifest of singles()) {
      const verdict = device(manifest);
      if (!verdict.attach) seen.add(verdict.recovery);
    }
  }
  assert.deepEqual(
    [...seen].sort(),
    ["refetch-region", "reject-irrecoverable", "rerender-private-slot", "retry-interaction"],
  );
});

test("a handler's identity is not read as anything but the table's own lines", () => {
  const device = decider(`#page|${PAGE}|doc|public\n#build|h1\nh1|a\n`.split("\n"), PAGE);
  assert.equal(device("2|1|B|#build|h1|doc|public|x|region").code, 4);
  assert.equal(device("2|1|B|#page|x|doc|public|x|region").code, 4);
  assert.equal(device("2|1|B|h1|a|doc|public|x|region").code, 0);
});
