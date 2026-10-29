// ADR-0172: small lifecycle seams from the actual browser runtime. These
// tests do not boot a page; feed.spec.mjs exercises the real renderer there.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { randomUUID } from "node:crypto";

const runtime = readFileSync(new URL("./public/pw-runtime.mjs", import.meta.url), "utf8");
function source(from, until) {
  assert.equal(runtime.split(from).length, 2, `unique runtime seam: ${from}`);
  assert.equal(runtime.split(until).length, 2, `unique runtime seam: ${until}`);
  const start = runtime.indexOf(from);
  const end = runtime.indexOf(until, start);
  assert.ok(end > start, "ordered runtime seam");
  return runtime.slice(start, end);
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function loader(fetch, speculate = false, speculationUrl = null) {
  const code = source("let renderer = null;", "/** A path read from the signals") +
    source("function renderCall(", "/** A value as the renderer reads it:") +
    (speculate ? source("let speculationModule = null;", "function current(") +
      source("async function renderBlock(", "/** The nodes from `start`") +
      source("async function speculate(", "/** The command answered:") : "");
  const module = { commands: {} };
  return vm.runInNewContext(code + `\n({ renderModule, forgetRenderer, retireRenderer, callRenderer,
    ${speculate ? "speculations, renderBlock, speculate," : ""}
    bind: (owner) => {
      wasm = owner; speculationModule = Promise.resolve({});
      speculationOwner = { renderer: owner, retired: false };
    },
    held: () => ({ wasm, rendererLoaded, speculationModule, speculationOwner }) })`, {
    fetch, WebAssembly, log: [], wasm: null, speculationModule: null, speculationOwner: null,
    speculationLoaded: speculationUrl ? null : module,
    TextEncoder, TextDecoder, Uint8Array, withTemplates: (request) => request,
    parts: { speculation: speculationUrl ?? "unused-cached-js-module", regions: {}, blocks: { 1: {} } },
    within: (promise) => promise, signals: new Map(), computedNow: async () => [],
  }, speculationUrl ? { importModuleDynamically: vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER } : {});
}
// A valid native module. Distinct instantiations have distinct export owners.
const response = (bytes = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0)) => new Response(bytes, {
  headers: { "Content-Type": "application/wasm" },
});

// Native test ABI: memory(1), alloc(i32)->0, render_part(i32,i32)->code
// (or unreachable), out_ptr()->0, out_len()->0. No imports or JS exports.
function nativeRenderer(code) {
  const section = (id, bytes) => [id, bytes.length, ...bytes];
  const name = (text) => [...new TextEncoder().encode(text)];
  const exported = (text, kind, index) => [text.length, ...name(text), kind, index];
  const constant = (value) => [4, 0, 0x41, value, 0x0b];
  const bytes = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [3, 0x60, 1, 0x7f, 1, 0x7f, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 0x60, 0, 1, 0x7f]),
    ...section(3, [4, 0, 1, 2, 2]), ...section(5, [1, 0, 1]),
    ...section(7, [5, ...exported("memory", 2, 0), ...exported("alloc", 0, 0),
      ...exported("render_part", 0, 1), ...exported("out_ptr", 0, 2), ...exported("out_len", 0, 3)]),
    ...section(10, [4, ...constant(0), ...(code === "trap" ? [3, 0, 0, 0x0b] : constant(code)),
      ...constant(0), ...constant(0)]),
  ]);
  assert.equal(WebAssembly.validate(bytes), true);
  return bytes;
}

test("an abandoned renderer load cannot publish over a newer native instance", async () => {
  const first = deferred();
  let requests = 0;
  const actual = loader(() => ++requests === 1 ? first.promise : Promise.resolve(response()));
  const abandoned = actual.renderModule();
  actual.forgetRenderer();
  const current = await actual.renderModule();
  first.resolve(response());
  const obsolete = await abandoned;
  assert.notEqual(obsolete, current);
  assert.equal(await actual.renderModule(), current);
  assert.equal(requests, 2);
});

test("an abandoned renderer rejection cannot forget a newer native instance", async () => {
  const first = deferred();
  let requests = 0;
  const actual = loader(() => ++requests === 1 ? first.promise : Promise.resolve(response()));
  const failure = new Error("the abandoned network request failed");
  const abandoned = actual.renderModule();
  const rejected = assert.rejects(abandoned, (error) => error === failure);
  actual.forgetRenderer();
  const current = await actual.renderModule();
  first.reject(failure);
  await rejected;
  assert.equal(await actual.renderModule(), current);
  assert.equal(requests, 2);
});

test("an absent failed owner cannot retire an in-flight renderer", async () => {
  const waiting = deferred();
  let requests = 0;
  const actual = loader(() => { requests++; return waiting.promise.then(() => response()); });
  const pending = actual.renderModule();
  actual.retireRenderer(null);
  const again = actual.renderModule();
  try {
    assert.equal(again, pending);
    assert.equal(requests, 1);
  } finally {
    waiting.resolve();
    await Promise.all([pending, again]);
  }
});

test("a native renderer refusal keeps its loaded owner", async () => {
  let requests = 0;
  const actual = loader(async () => { requests++; return response(nativeRenderer(1)); });
  const owner = await actual.renderModule();
  actual.bind(owner);
  assert.throws(() => actual.callRenderer(owner, "render_part", "{}"), /render_part did not render:/);
  assert.equal(await actual.renderModule(), owner);
  assert.equal(actual.held().wasm, owner);
  assert.equal(requests, 1);
});

test("a trap from an older native owner cannot retire its healthy replacement", async () => {
  let requests = 0;
  const actual = loader(async () => response(nativeRenderer(++requests === 1 ? "trap" : 0)));
  const old = await actual.renderModule();
  actual.bind(old);
  assert.throws(() => actual.callRenderer(old, "render_part", "{}"), WebAssembly.RuntimeError);
  const current = await actual.renderModule();
  actual.bind(current);
  const bound = actual.held().speculationModule;
  assert.throws(() => actual.callRenderer(old, "render_part", "{}"), WebAssembly.RuntimeError);
  assert.equal(await actual.renderModule(), current);
  assert.equal(actual.held().wasm, current);
  assert.equal(actual.held().speculationModule, bound);
  assert.equal(actual.held().speculationOwner.retired, false);
  assert.equal(requests, 2);
});

for (const caller of ["speculations", "speculate"]) {
for (const outcome of ["resolve", "reject"]) {
test(`a renderer retired by a signal fails ${caller} when its import will ${outcome}`, async () => {
  const waiting = deferred();
  const imported = deferred();
  const key = `pw-renderer-contract-${randomUUID()}`;
  globalThis[key] = imported.promise;
  const url = `data:text/javascript,${encodeURIComponent(`await globalThis[${JSON.stringify(key)}]; export const commands = {};`)}#module`;
  let requests = 0;
  const actual = loader(() => ++requests === 1
    ? waiting.promise.then(() => response(nativeRenderer("trap")))
    : Promise.resolve(response(nativeRenderer(0))), true, url);
  const block = actual.renderBlock(1);
  const failed = assert.rejects(block, WebAssembly.RuntimeError);
  const loading = actual[caller]("reply", [], "press-a").then(
    () => ({ loaded: true }), (error) => ({ loaded: false, error }),
  );
  waiting.resolve();
  try {
    await failed;
    assert.equal(actual.held().wasm, null);
    // The actual JS import is held until the native signal call has failed.
    if (outcome === "resolve") imported.resolve();
    else imported.reject(new Error("test-only import failure after native trap"));
    const result = await loading;
    assert.equal(result.loaded, false);
    assert.match(result.error.message, /renderer.*retired/i);
    assert.equal(actual.held().wasm, null);
    // A rejected import is requested under a new specifier on the next
    // need. That fresh real module must have a healthy test dependency.
    globalThis[key] = Promise.resolve();
    await actual.speculations();
    assert.notEqual(actual.held().wasm, null);
    assert.equal(requests, 2);
  } finally {
    imported.resolve();
    await loading;
    delete globalThis[key];
  }
});
}
}

test("an import rejection without native retirement still permits the load-only fallback", async () => {
  const imported = deferred();
  const key = `pw-renderer-contract-${randomUUID()}`;
  globalThis[key] = imported.promise;
  const url = `data:text/javascript,${encodeURIComponent(`await globalThis[${JSON.stringify(key)}]; export const commands = {};`)}#module`;
  let requests = 0;
  const actual = loader(async () => { requests++; return response(nativeRenderer(0)); }, true, url);
  const press = actual.speculate("reply", [], "press-a");
  try {
    const healthy = await actual.renderModule();
    imported.reject(new Error("test-only ordinary import failure"));
    const ids = await press;
    assert.equal(Array.isArray(ids), true);
    assert.equal(ids.length, 0);
    assert.equal(await actual.renderModule(), healthy);
    assert.equal(requests, 1);
  } finally {
    imported.resolve();
    await press;
    delete globalThis[key];
  }
});

test("a press resuming after renderer retirement creates no unsent transition", async () => {
  const state = { value: "held", pending: [] };
  const module = { commands: { reply: [{ binding: "thread", transition: (value) => value }] } };
  const actual = vm.runInNewContext(
    source("class RetiredRenderer extends Error", "/** The page's speculation module,") +
    source("async function speculate(", "/** The command answered:") +
    "\n({ speculate })", {
    speculations: async () => module, current: () => state, render: () => {}, log: [],
    parts: { regions: {} }, wasm: null, rendererLoaded: null, nextSpeculation: 0,
    restoreSpeculation: async () => {},
  });
  await assert.rejects(actual.speculate("reply", [], "press-a"), /renderer.*retired/i);
  assert.deepEqual(state.pending, []);
});

function settlement() {
  const load = deferred();
  const states = new Map([
    ["first", { entry: "entry-a", version: 1, value: "held-a", pending: [{ id: 10 }, { id: 11 }] }],
    ["second", { entry: "entry-b", version: 2, raw: "held-b", pending: [{ id: 12 }, { id: 13 }] }],
  ]);
  const drawn = [];
  const actual = vm.runInNewContext(source("async function resolveSpeculation(", "/** An `entry_value` frame:") +
    "\n({ resolveSpeculation })", {
    speculated: states, speculations: () => load.promise, log: [],
    render: (_module, binding) => drawn.push({ binding, state: structuredClone(states.get(binding)) }),
  });
  return { actual, load, states, drawn };
}

test("all failed press IDs are removed before loading or redrawing any binding", async () => {
  const { actual, load, states, drawn } = settlement();
  const resolved = actual.resolveSpeculation([["first", 10], ["second", 12]], false);
  assert.deepEqual(states.get("first").pending, [{ id: 11 }]);
  assert.deepEqual(states.get("second").pending, [{ id: 13 }]);
  assert.equal(drawn.length, 0);
  // An authoritative frame while the redraw loads must remain authoritative.
  states.get("first").version = 3;
  states.get("first").value = "new held-a";
  load.resolve({});
  await resolved;
  assert.deepEqual(drawn.map(({ binding }) => binding).sort(), ["first", "second"]);
  assert.equal(drawn[0].state.value, "new held-a");
  assert.equal(states.get("first").version, 3);
  assert.equal(states.get("second").raw, "held-b");
});

test("a failed rollback load leaves neither owned ID pending", async () => {
  const { actual, load, states, drawn } = settlement();
  const failure = new Error("rollback renderer unavailable");
  const resolved = actual.resolveSpeculation([["first", 10], ["second", 12]], false);
  const rejected = assert.rejects(resolved, (error) => error === failure);
  load.reject(failure);
  await rejected;
  assert.deepEqual(states.get("first").pending, [{ id: 11 }]);
  assert.deepEqual(states.get("second").pending, [{ id: 13 }]);
  assert.equal(drawn.length, 0);
});
