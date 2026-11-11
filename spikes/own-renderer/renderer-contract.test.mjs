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
  // What the developer is told, as the platform tells an error nothing
  // caught (ADR-0431).
  const reported = [];
  const actual = vm.runInNewContext(
    `${source("let speculationModule = null;", "function current(")}${source("async function speculate(", "/** The command answered:")}
    \n({ speculate, speculations })`,
    {
      parts: { speculation: url }, log, reportError: (error) => reported.push(error), speculationLoaded: null, nextSpeculation: 0,
      attempted: (specifier, attempts) => (attempts === 0 ? specifier : `${specifier}${attempts}`),
      pageCode: (_piece, url) => import(url),
      within: (promise) => promise, restoreSpeculation: async () => {}, current: () => null, render: () => {},
    },
    { importModuleDynamically: vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER },
  );
  const press = actual.speculate("reply", [], "press-a");
  try {
    imported.reject(new Error("test-only ordinary import failure"));
    assert.equal((await press).length, 0);
    assert.equal(log.length, 1);
    assert.match(log[0], /^reply sent unspeculated: test-only/);
    assert.deepEqual(reported.map((e) => e.message), ["test-only ordinary import failure"]);
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

/** The two pure questions a list's rows are put in order by. */
function rowOrder() {
  return vm.runInNewContext(`${source("function placesOf(a, b) {", "// --- end of which rows changed")}\n({ placesOf, staying })`);
}

const keyed = (keys) => keys.map((key) => ({ key }));

test("each row of a render is at its place in the last, or new", () => {
  const { placesOf } = rowOrder();
  assert.deepEqual([...placesOf(keyed(["a", "b", "c"]), keyed(["c", "x", "a"]))], [2, -1, 0]);
  assert.deepEqual([...placesOf([], keyed(["a"]))], [-1]);
});

/** The longest run of `places` that increases, by trying them all. */
function longestRun(places) {
  const best = places.map(() => 1);
  let most = 0;
  places.forEach((v, i) => {
    if (v < 0) return;
    for (let j = 0; j < i; j++) if (places[j] >= 0 && places[j] < v) best[i] = Math.max(best[i], best[j] + 1);
    most = Math.max(most, best[i]);
  });
  return most;
}

test("as many rows as can stay stay, and those that stay are in order", () => {
  const { staying } = rowOrder();
  // A seeded walk: each list a shuffle of the last's places, some rows new.
  let seed = 7;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  for (let round = 0; round < 500; round++) {
    const size = 1 + next(40);
    const places = Array.from({ length: size }, (_, i) => i);
    for (let i = size - 1; i > 0; i--) {
      const j = next(i + 1);
      [places[i], places[j]] = [places[j], places[i]];
    }
    for (let i = 0; i < size; i++) if (next(5) === 0) places[i] = -1;
    const still = [...staying(places)].sort((a, b) => a - b);
    assert.equal(still.length, longestRun(places), JSON.stringify(places));
    for (const i of still) assert.ok(places[i] >= 0, "a new row does not stay");
    for (let k = 1; k < still.length; k++) assert.ok(places[still[k - 1]] < places[still[k]], JSON.stringify(places));
  }
});

test("a list with nothing in common has nothing that stays", () => {
  const { staying } = rowOrder();
  assert.equal(staying([-1, -1, -1]).size, 0);
  assert.equal(staying([]).size, 0);
  assert.deepEqual([...staying([0, 1, 2])].sort(), [0, 1, 2]);
});

