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


/** The question the page's own code's placing asks (ADR-0439): of the rows
 * between `lo` and `hi`, which need not move. */
function ownOrder() {
  return vm.runInNewContext(`${source("function stayingIn(places, lo, hi) {", "/** **New rows, as one fragment**")}\nstayingIn`);
}

test("of the rows between two places, as many as can stay stay, those that stay in order and none new", () => {
  const stayingIn = ownOrder();
  // A seeded walk: each list the last's places, some shuffled whole, most
  // in order with a few swapped or moved, some rows new; and a range of it.
  let seed = 11;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  for (let round = 0; round < 1000; round++) {
    const size = next(41);
    const places = Array.from({ length: size }, (_, i) => i);
    if (round % 3 === 0) {
      for (let i = size - 1; i > 0; i--) {
        const j = next(i + 1);
        [places[i], places[j]] = [places[j], places[i]];
      }
    } else {
      for (let k = next(4); k > 0 && size > 1; k--) {
        const [i, j] = [next(size), next(size)];
        if (next(2) === 0) [places[i], places[j]] = [places[j], places[i]];
        else places.splice(j, 0, ...places.splice(i, 1));
      }
    }
    for (let i = 0; i < size; i++) if (next(6) === 0) places[i] = -1;
    const lo = next(size + 1);
    const hi = lo + next(size - lo + 1);
    const stays = stayingIn(Int32Array.from(places), lo, hi);
    assert.equal(stays.length, hi - lo, JSON.stringify({ places, lo, hi }));
    const still = [...stays].flatMap((s, k) => (s === 1 ? [lo + k] : []));
    assert.equal(still.length, longestRun(places.slice(lo, hi)), JSON.stringify({ places, lo, hi }));
    for (const i of still) assert.ok(places[i] >= 0, "a new row does not stay");
    for (let k = 1; k < still.length; k++) assert.ok(places[still[k - 1]] < places[still[k]], JSON.stringify({ places, lo, hi }));
  }
  // Two rows of a thousand swapped, between the rows in their places at
  // either end: the two move, the rest stay.
  const swapped = Int32Array.from({ length: 1000 }, (_, i) => i);
  [swapped[1], swapped[998]] = [998, 1];
  const stays = stayingIn(swapped, 1, 999);
  assert.equal(stays.reduce((t, s) => t + s, 0), 996);
  assert.deepEqual([stays[0], stays[1], stays[996], stays[997]], [0, 1, 1, 0]);
  assert.equal(stayingIn(Int32Array.from([-1, -1]), 0, 2).reduce((t, s) => t + s, 0), 0);
  assert.equal(stayingIn(new Int32Array(0), 0, 0).length, 0);
});

/** What the runtime finds of a list's rows by their keys, where its own
 * code found them among other rows than the document shows (ADR-0439). */
function foundOrder() {
  return vm.runInNewContext(`${source("function foundByKeys(a, b) {", "/** **The rows from `lo` to `hi` that need not move**")}\nfoundByKeys`);
}

test("rows found by their keys are each at its place among the rows before, or new, as the keys say", () => {
  const foundByKeys = foundOrder();
  let seed = 23;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  for (let round = 0; round < 600; round++) {
    const a = Array.from({ length: next(30) }, (_, i) => ({ key: `k${i}` }));
    // The next render's rows: some of these, the very rows or made again
    // under their keys, in another order, and some new.
    const b = [];
    for (const row of a) if (next(4) > 0) b.push(next(5) === 0 ? { key: row.key } : row);
    for (let i = b.length - 1; i > 0; i--) if (next(3) === 0) {
      const j = next(i + 1);
      [b[i], b[j]] = [b[j], b[i]];
    }
    for (let k = next(4); k > 0; k--) b.splice(next(b.length + 1), 0, { key: `new${round}-${k}` });
    const said = foundByKeys(a, b);
    const at = new Map(a.map((row, j) => [row.key, j]));
    const places = b.map((row) => at.get(row.key) ?? -1);
    assert.deepEqual([...said.places], places, JSON.stringify({ a: a.length, b: b.map((r) => r.key) }));
    const now = new Set(b.map((row) => row.key));
    const matched = a.filter((row) => now.has(row.key)).length;
    assert.equal(said.matched, matched);
    // Those of `a` it does not hold, where it holds some.
    assert.deepEqual([...said.gone], matched > 0 ? a.flatMap((row, j) => (now.has(row.key) ? [] : [j])) : []);
    const stay = places.filter((p) => p >= 0);
    assert.equal(said.sorted, stay.every((p, k) => k === 0 || stay[k - 1] < p));
    assert.deepEqual([...said.remade], places.flatMap((p, i) => (p >= 0 && b[i] !== a[p] ? [i] : [])));
  }
  // Shown the first time: every row new, none looked for.
  const first = foundByKeys([], [{ key: "a" }, { key: "b" }]);
  assert.deepEqual([[...first.places], first.matched, first.sorted, [...first.remade], [...first.gone]], [[-1, -1], 0, true, [], []]);
  // Found by their keys, the rows are not said to be few out of their places.
  assert.equal(foundByKeys([{ key: "a" }, { key: "b" }], [{ key: "b" }, { key: "a" }]).displaced, null);
});

/** The rows the page's own code's placing moves where no more than four are
 * out of their places and every other is where it was (ADR-0439). */
function fewOrder() {
  return vm.runInNewContext(`${source("function fewMoved(places, displaced) {", "/** **New rows, as one fragment**")}\nfewMoved`);
}

test("of rows a few of which are out of their places, as few move as leave the rest in order, found among those few", (t) => {
  const fewMoved = fewOrder();
  // A seeded walk: lists in order, the rows at two to four places put in
  // one another's, those places near one another, anywhere, or two on
  // either side of a row that stays in its place.
  let seed = 31;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  const held = { fewer: 0, inPlace: 0 };
  for (let round = 0; round < 6000; round++) {
    const size = 5 + next(40);
    const way = next(3);
    const from = next(size - 4);
    const chosen =
      way === 2
        ? [from, from + 1, from + 3, from + 4]
        : [...new Set(Array.from({ length: 2 + next(3) }, () => (way === 0 ? Math.min(size - 1, from + next(6)) : next(size))))];
    const shuffled = [...chosen];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = next(i + 1);
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    const places = Array.from({ length: size }, (_, i) => i);
    chosen.forEach((i, k) => (places[i] = shuffled[k]));
    const displaced = places.flatMap((p, i) => (p !== i ? [i] : []));
    if (displaced.length === 0) continue;
    const at = JSON.stringify({ places, displaced });
    const moved = [...fewMoved(Int32Array.from(places), displaced)];
    // Each once, in order; as many as a longest run in order leaves out;
    // and the rest in order.
    for (let k = 1; k < moved.length; k++) assert.ok(moved[k - 1] < moved[k], at);
    assert.equal(moved.length, places.length - longestRun(places), at);
    const still = places.filter((_, i) => !moved.includes(i));
    for (let k = 1; k < still.length; k++) assert.ok(still[k - 1] < still[k], at);
    if (moved.length < displaced.length) held.fewer += 1;
    if (moved.some((i) => places[i] === i)) held.inPlace += 1;
  }
  // Often a row out of its place stays where it is, and now and then one
  // in its place moves, for fewer to move: two rows on either side of one
  // swapped, the one between and two others move.
  t.diagnostic(JSON.stringify(held));
  assert.ok(held.fewer > 1_500 && held.inPlace > 150, JSON.stringify(held));
  assert.deepEqual([...fewMoved(Int32Array.from([3, 4, 2, 0, 1]), [0, 1, 3, 4])], [2, 3, 4]);
  // Two rows of a thousand swapped: those two move, and of the rows' places
  // no more than a few are read.
  const swapped = Int32Array.from({ length: 1000 }, (_, i) => i);
  [swapped[1], swapped[998]] = [998, 1];
  let reads = 0;
  const counted = new Proxy(swapped, {
    get: (target, k) => {
      if (typeof k === "string" && /^\d+$/.test(k)) reads += 1;
      return target[k];
    },
  });
  assert.deepEqual([...fewMoved(counted, [1, 998])], [1, 998]);
  assert.ok(reads <= 32, `places read: ${reads}`);
});
