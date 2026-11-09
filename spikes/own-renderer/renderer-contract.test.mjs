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

test("a runtime asks for its files at the delivery it was asked for at, a development one at its path", () => {
  for (const [delivery, address] of [
    [null, "/pw-resume.wasm"],
    ["3f a&b", "/pw-resume.wasm?b=3f%20a%26b"],
  ]) {
    const actual = vm.runInNewContext(`${source("function delivered(", "async function bootDecision(")}\n({ delivered })`, {
      DELIVERY: delivery,
    });
    assert.equal(actual.delivered("/pw-resume.wasm"), address);
  }
});

test("a speculation module that does not load sends the press unspeculated, and is asked for again", async () => {
  const key = `pw-renderer-contract-${randomUUID()}`;
  const imported = deferred();
  globalThis[key] = imported.promise;
  const url = `data:text/javascript,${encodeURIComponent(`await globalThis[${JSON.stringify(key)}]; export const commands = {};`)}#module`;
  const log = [];
  const actual = vm.runInNewContext(
    `${source("let speculationModule = null;", "function current(")}${source("async function speculate(", "/** The command answered:")}
    \n({ speculate, speculations })`,
    {
      parts: { speculation: url }, log, speculationLoaded: null, nextSpeculation: 0,
      attempted: (specifier, attempts) => (attempts === 0 ? specifier : `${specifier}${attempts}`),
      within: (promise) => promise, restoreSpeculation: async () => {}, current: () => null, render: () => {},
    },
    { importModuleDynamically: vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER },
  );
  const press = actual.speculate("reply", [], "press-a");
  try {
    imported.reject(new Error("test-only ordinary import failure"));
    assert.deepEqual(await press, []);
    assert.equal(log.length, 1);
    assert.match(log[0], /^reply sent unspeculated: test-only/);
  } finally {
    delete globalThis[key];
  }
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
    speculated: states, speculationsWithin: () => load.promise, log: [],
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
