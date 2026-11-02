// The head-to-head's browser instrument (measure.mjs), pinned as it is before it
// is split into small functions: the script it puts in every page, the log it
// keeps of every request, and one measured visit. No browser, no server, no
// network. A page is a vm context holding hand-written stubs of the globals the
// instrument's code reads, and the page functions it hands Playwright run in it
// from their source text alone, as Playwright runs them; Playwright is a set of
// fakes that record every call they get; time is a clock that moves only as the
// code under test waits, so a quarter of an hour of waiting takes a moment and
// every elapsed time is exact.
//
// Everything goes through `measureOnce`, the instrument's public face: the init
// script is the one it hands `context.addInitScript`, the log is the one it
// builds from the context's and the DevTools session's events. Only the last
// section imports `networkLog`, for what `measureOnce` does not show of it: what
// `quiet` and `settled` return, `quiet` with other times than its defaults, the
// log's keys and the copies `chromiumReceived` makes. (Were `networkLog` no
// longer exported, that import and that section would go; nothing else here
// uses it.)
//
// Fixtures (test/fixtures/measure/): the init script's template, as it is in
// measure.mjs with its two placeholders; TodoMVC React's index.html and
// app.css and TodoMVC Svelte's base.js, copied from benchmarks/headtohead/
// todomvc (base.js, as Svelte's build copies it, is not minified).
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { minifiedVerdict, sizes } from "../lib.mjs";
import { SLOW_4G, measureOnce, networkLog } from "../measure.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const bytes = (name) => fs.readFileSync(path.join(HERE, "fixtures/measure", name));
const plain = (value) => (value === undefined ? undefined : JSON.parse(JSON.stringify(value)));
const noop = () => {};

// --- time -------------------------------------------------------------------------------

/**
 * `Date.now()` reads the clock and `setTimeout` moves it (and, a turn later,
 * calls back). Should the code under test wait on real time instead (a sleep
 * that is not the global `setTimeout`, a `Date.now` kept in a variable), the
 * clock stands still and its wait would never end: a real timer fails the run
 * after five seconds, and the globals are put back, which ends the wait.
 */
async function withClock(body) {
  const real = { now: Date.now, setTimeout: globalThis.setTimeout };
  const clock = { t: 1_000_000, waits: 0, onWait: noop };
  let watchdog;
  const stuck = new Promise((_, reject) => {
    watchdog = real.setTimeout(() => reject(new Error("the code under test waited on real time, not on the test's clock")), 5000);
  });
  Date.now = () => clock.t;
  globalThis.setTimeout = (fn, ms) => {
    clock.t += ms;
    clock.waits += 1;
    clock.onWait(clock.waits);
    return setImmediate(fn);
  };
  try {
    return await Promise.race([body(clock), stuck]);
  } finally {
    Date.now = real.now;
    globalThis.setTimeout = real.setTimeout;
    clearTimeout(watchdog);
  }
}

// --- a page -----------------------------------------------------------------------------

// What an engine lists in PerformanceObserver.supportedEntryTypes: Chromium
// reports layout shifts; Firefox and WebKit do not (the recorded run's `cls`
// is null in both).
const CHROMIUM = ["element", "event", "first-input", "largest-contentful-paint", "layout-shift", "longtask", "mark", "measure", "navigation", "paint", "resource"];
const FIREFOX = ["largest-contentful-paint", "mark", "measure", "navigation", "paint", "resource"];

function historyOf(env) {
  const method = (name) =>
    function (state, title, url) {
      env.historyCalls.push([name, this === env.history, state, title, url]);
      this.state = state;
      return `${name} done`;
    };
  class History {}
  History.prototype.pushState = method("pushState");
  History.prototype.replaceState = method("replaceState");
  Object.defineProperty(History.prototype, "scrollRestoration", {
    get() {
      return this.restoration ?? "auto";
    },
    set(value) {
      env.historyCalls.push(["scrollRestoration", value]);
      this.restoration = value;
    },
    enumerable: env.enumerable,
    configurable: true,
  });
  const history = new History();
  history.state = null;
  return { History, history };
}

function observerClasses(env) {
  class PerformanceObserver {
    constructor(callback) {
      this.callback = callback;
      env.perf.push(this);
    }
    observe(options) {
      if (env.refuse) throw new Error("this engine does not observe that");
      this.options = options;
    }
  }
  PerformanceObserver.supportedEntryTypes = env.entryTypes;
  class MutationObserver {
    constructor(callback) {
      this.callback = callback;
      env.mutations.push(this);
    }
    observe(target, options) {
      this.target = target;
      this.options = options;
    }
  }
  return { PerformanceObserver, MutationObserver };
}

const documentOf = (env) => ({
  documentElement: { dataset: env.dataset },
  querySelector: (selector) => env.dom[selector] ?? null,
  querySelectorAll: (selector) => env.all[selector] ?? [],
  addEventListener: (type, fn) => env.docListeners.push({ type, fn }),
});

const performanceOf = (env) => ({
  now: () => env.now,
  getEntriesByType: (type) => (type === "navigation" ? env.navigation : []),
  get timeOrigin() {
    return env.timeOrigin;
  },
});

const storageOf = (env, name) => ({
  clear: () => {
    env.log.push([`${name}.clear`]);
    if (env.denied) throw new Error("SecurityError");
  },
});

function pageGlobals(env) {
  const { History, history } = historyOf(env);
  Object.assign(env, { History, history });
  const sandbox = {
    ...observerClasses(env),
    History,
    history,
    document: documentOf(env),
    performance: performanceOf(env),
    localStorage: storageOf(env, "localStorage"),
    sessionStorage: storageOf(env, "sessionStorage"),
    requestAnimationFrame: (fn) => env.raf.push(fn),
    setTimeout: (fn, ms) => {
      env.timers.push(fn);
      env.delays.push(ms);
    },
    queueMicrotask: (fn) => env.micro.push(fn),
    addEventListener: (type, fn, capture) => env.listeners.push({ type, fn, capture }),
  };
  sandbox.window = sandbox;
  sandbox.top = env.top ? sandbox : {};
  return sandbox;
}

function pageDriver(env) {
  const each = (queue) => queue.splice(0).forEach((fn) => fn());
  const animate = (t) => {
    env.now = t;
    each(env.raf);
  };
  const timeout = (t) => {
    env.now = t;
    each(env.timers);
  };
  return {
    run: (script) => vm.runInContext(script, env.sandbox, { filename: "h2h-observers.js" }),
    evaluate: (fn, arg) => vm.runInContext(`(${fn})(${JSON.stringify(arg)})`, env.sandbox),
    at: (t) => {
      env.now = t;
    },
    mutate: () => env.mutations.forEach((m) => m.callback([], m)),
    report: (type, list) => env.perf.filter((o) => o.options && o.options.type === type).forEach((o) => o.callback({ getEntries: () => list })),
    drain: () => each(env.micro),
    animate,
    timeout,
    frame: (rafAt, timerAt) => {
      animate(rafAt);
      timeout(timerAt);
    },
    fire: (type, timeStamp) => env.listeners.filter((l) => l.type === type).forEach((l) => l.fn({ type, timeStamp })),
    docFire: (type) => env.docListeners.filter((l) => l.type === type).forEach((l) => l.fn({ type })),
    counts: () => ({ observers: env.perf.length, mutations: env.mutations.length, listeners: env.listeners.length, docListeners: env.docListeners.length }),
  };
}

/**
 * A page's globals as the init script and the page functions read them:
 * `dom` and `all` answer querySelector and querySelectorAll by selector, the
 * rest is recorded. `at` sets performance.now(); `mutate`, `fire`, `drain`,
 * `animate` and `timeout` play what the browser would call back.
 */
function pageEnv(over) {
  const env = {
    now: 0,
    top: true,
    refuse: false,
    denied: false,
    enumerable: true,
    entryTypes: CHROMIUM,
    timeOrigin: 1_000_000,
    navigation: [],
    log: [],
    dom: {},
    all: {},
    dataset: {},
    raf: [],
    timers: [],
    delays: [],
    micro: [],
    perf: [],
    mutations: [],
    listeners: [],
    docListeners: [],
    historyCalls: [],
    ...over,
  };
  env.sandbox = vm.createContext(pageGlobals(env));
  return Object.assign(env, pageDriver(env));
}

// --- the init script ----------------------------------------------------------------------

/** The init script `measureOnce` hands the context for an app, got by stopping it at the page. */
async function initScriptOf(appSelector, ready) {
  const scripts = [];
  const stop = new Error("stopped at the page");
  const context = {
    addInitScript: async (script) => scripts.push(script),
    newPage: async () => {
      throw stop;
    },
    close: async () => {},
  };
  const browser = { newContext: async () => context };
  await assert.rejects(measureOnce({ browser, engine: "firefox", target: { appSelector, ready }, condition: "cold" }), stop);
  return scripts[0];
}

const TEMPLATE = bytes("observers-template.js.txt").toString("utf8");
const spliced = (app, ready) => TEMPLATE.replace('const APP = "__APP__";', `const APP = "${app}";`).replace('const READY = "__READY__";', `const READY = "${ready}";`);
const RULES = [
  ["dom", ".new-todo"],
  ["pw", ".new-todo"],
  ["next", "#menu"],
  ["sveltekit", "#menu"],
  ["bogus", ".new-todo"],
];
const SCRIPTS = Object.fromEntries(await Promise.all(RULES.map(async ([rule, app]) => [rule, await initScriptOf(app, rule)])));

const loaded = (rule, over) => {
  const env = pageEnv(over);
  env.run(SCRIPTS[rule]);
  return env;
};
const h2h = (env) => env.sandbox.__h2h;
const timesOf = (env) => plain(h2h(env).t);
const waitOf = (env, i) => plain(h2h(env).waits[i ?? 0]);
const armed = (name, cond, arg, over) => ({ name, cond, arg, armed: true, start: null, dom: null, frame: null, ...over });

test("the init script is the observers template with the app's selector and the readiness rule put into its two constants", () => {
  RULES.forEach(([rule, app]) => assert.equal(SCRIPTS[rule], spliced(app, rule)));
});

test("a selector is spliced into the init script as text, unescaped, and a placeholder in it is the one the readiness rule then replaces", async () => {
  assert.equal(await initScriptOf('[data-field="a"]', "dom"), spliced('[data-field="a"]', "dom"));
  assert.equal(await initScriptOf("__READY__", "pw"), TEMPLATE.replace('const APP = "__APP__";', 'const APP = "pw";'));
});

test("the init script puts the measurements in window.__h2h: empty times, no largest contentful paint, a layout shift of 0 and a way to arm a wait", () => {
  const env = loaded("dom", {});
  assert.deepEqual(Object.keys(h2h(env)), ["t", "lcp", "cls", "waits", "documents", "arm"]);
  assert.equal(typeof h2h(env).arm, "function");
  assert.deepEqual(plain(h2h(env)), { t: {}, lcp: null, cls: 0, waits: [], documents: 1 });
});

const ENGINES = [
  ["Chromium", CHROMIUM, ["paint", "largest-contentful-paint", "layout-shift"], 0],
  ["Firefox and WebKit", FIREFOX, ["paint", "largest-contentful-paint"], null],
  ["an engine that lists layout shifts but no largest contentful paint", ["layout-shift", "paint"], ["paint", "layout-shift"], 0],
  ["an engine that lists neither", ["mark", "paint"], ["paint"], null],
  ["an engine that lists no entry types at all", undefined, ["paint"], null],
];

function checkEngine([name, types, observed, cls]) {
  const env = loaded("dom", { entryTypes: types });
  assert.deepEqual(
    env.perf.map((o) => plain(o.options)),
    observed.map((type) => ({ type, buffered: true })),
    name,
  );
  assert.equal(h2h(env).cls, cls, name);
}

test("layout shifts and largest contentful paints are observed only where the engine lists them, and the layout shift then starts at 0, else it stays null", () => {
  ENGINES.forEach(checkEngine);
});

test("the first contentful paint is the start of the paint entry of that name, not of the first paint", () => {
  const env = loaded("dom", {});
  env.report("paint", [
    { name: "first-paint", startTime: 11 },
    { name: "first-contentful-paint", startTime: 14.5 },
  ]);
  assert.deepEqual(timesOf(env), { fcp: 14.5 });
});

test("the largest contentful paint is the start of the last entry reported, and a report with no entry leaves it as it was", () => {
  const env = loaded("dom", {});
  env.report("largest-contentful-paint", [{ startTime: 100 }, { startTime: 250 }]);
  assert.equal(h2h(env).lcp, 250);
  env.report("largest-contentful-paint", []);
  assert.equal(h2h(env).lcp, 250);
  env.report("largest-contentful-paint", [{ startTime: 300 }]);
  assert.equal(h2h(env).lcp, 300);
});

test("the layout shift is the sum of the shifts that no recent input explains, across reports", () => {
  const env = loaded("dom", {});
  env.report("layout-shift", [
    { value: 0.25, hadRecentInput: false },
    { value: 0.5, hadRecentInput: true },
    { value: 0.125, hadRecentInput: false },
  ]);
  assert.equal(h2h(env).cls, 0.375);
  env.report("layout-shift", [{ value: 0.0625, hadRecentInput: false }]);
  assert.equal(h2h(env).cls, 0.4375);
});

test("an observer the engine refuses to start does not stop the script: the times stay empty and the rest is installed", () => {
  const env = loaded("dom", { refuse: true });
  assert.deepEqual(plain(h2h(env)), { t: {}, lcp: null, cls: 0, waits: [], documents: 1 });
  assert.equal(typeof h2h(env).arm, "function");
  assert.deepEqual(env.counts(), { observers: 3, mutations: 1, listeners: 5, docListeners: 1 });
});

test("a page whose History has no scrollRestoration still gets the rest of the script's hooks", () => {
  const env = pageEnv({});
  delete env.History.prototype.scrollRestoration;
  env.run(SCRIPTS.sveltekit);
  env.history.scrollRestoration = "manual";
  assert.deepEqual(timesOf(env), {});
  env.at(9);
  env.mutate();
  assert.deepEqual(timesOf(env), { ready: 9 });
});

test("the init script does nothing in a frame below the top window", () => {
  const env = loaded("dom", { top: false });
  assert.equal(h2h(env), undefined);
  assert.deepEqual(env.counts(), { observers: 0, mutations: 0, listeners: 0, docListeners: 0 });
  assert.equal(Object.hasOwn(env.history, "pushState"), false);
});

test("the init script does nothing a second time in the same window: one set of observers, listeners and history wrappers", () => {
  const env = loaded("dom", {});
  const first = h2h(env);
  const wrapped = env.history.pushState;
  const counts = env.counts();
  env.run(SCRIPTS.dom);
  assert.equal(h2h(env), first);
  assert.equal(env.history.pushState, wrapped);
  assert.deepEqual(env.counts(), counts);
});

test("the init script listens for load and for four kinds of input in the capture phase, and watches the whole document for changes", () => {
  const env = loaded("dom", {});
  assert.deepEqual(
    env.listeners.map(({ type, capture }) => [type, capture]),
    [
      ["load", undefined],
      ["keydown", true],
      ["pointerdown", true],
      ["mousedown", true],
      ["click", true],
    ],
  );
  assert.deepEqual(
    env.docListeners.map((l) => l.type),
    ["DOMContentLoaded"],
  );
  assert.equal(env.mutations.length, 1);
  assert.equal(env.mutations[0].target, env.sandbox.document);
  assert.deepEqual(plain(env.mutations[0].options), { subtree: true, childList: true, attributes: true, characterData: true });
});

test("a client-rendered app is interactive when its first control is in the document", () => {
  const env = loaded("dom", {});
  env.at(5);
  env.mutate();
  assert.deepEqual(timesOf(env), {});
  env.dom[".new-todo"] = { tag: "input" };
  env.at(12.5);
  env.mutate();
  assert.deepEqual(timesOf(env), { appInDom: 12.5, ready: 12.5 });
});

test("a Pleris page is interactive when its runtime sets data-pw-ready to 1, whether or not its control is in the document", () => {
  const env = loaded("pw", {});
  env.dom[".new-todo"] = {};
  env.dataset.pwReady = "true";
  env.at(8);
  env.mutate();
  assert.deepEqual(timesOf(env), { appInDom: 8 });
  env.dataset.pwReady = "1";
  env.at(21);
  env.mutate();
  assert.deepEqual(timesOf(env), { appInDom: 8, ready: 21 });
  const bare = loaded("pw", {});
  bare.dataset.pwReady = "1";
  bare.at(3);
  bare.mutate();
  assert.deepEqual(timesOf(bare), { ready: 3 });
});

test("a Next.js page is interactive when its App Router has written __NA into the history state", () => {
  const env = loaded("next", {});
  env.at(30);
  env.history.replaceState({ key: "x" }, "");
  assert.deepEqual(timesOf(env), {});
  env.at(41);
  env.history.replaceState({ __NA: true }, "");
  assert.deepEqual(timesOf(env), { ready: 41 });
});

test("a SvelteKit page is interactive when its router has set scrollRestoration to manual, not when it has written its router state", () => {
  const env = loaded("sveltekit", {});
  env.at(20);
  env.history.replaceState({ "sveltekit:history": 0 }, "");
  env.history.scrollRestoration = "auto";
  assert.deepEqual(timesOf(env), {});
  env.at(44);
  env.history.scrollRestoration = "manual";
  assert.deepEqual(timesOf(env), { ready: 44 });
});

test("a readiness rule the script does not know is never interactive, whatever the page shows", () => {
  const env = loaded("bogus", {});
  env.at(7);
  env.dom[".new-todo"] = {};
  env.dataset.pwReady = "1";
  env.history.state = { __NA: true };
  env.history.scrollRestoration = "manual";
  env.mutate();
  assert.deepEqual(timesOf(env), { appInDom: 7 });
});

const TRIGGERS = {
  "a mutation of the document": (env) => env.mutate(),
  DOMContentLoaded: (env) => env.docFire("DOMContentLoaded"),
  load: (env) => env.fire("load"),
  pushState: (env) => env.history.pushState({}, "", "/a"),
  replaceState: (env) => env.history.replaceState({}, ""),
  "a scrollRestoration assignment": (env) => {
    env.history.scrollRestoration = "auto";
  },
};

function checkSeenOn([name, trigger]) {
  const env = loaded("dom", {});
  env.dom[".new-todo"] = {};
  env.at(7);
  trigger(env);
  assert.deepEqual(timesOf(env), { appInDom: 7, ready: 7 }, name);
}

test("the app and readiness are looked for on a mutation, DOMContentLoaded, load, pushState, replaceState and a scrollRestoration assignment, and not otherwise", () => {
  Object.entries(TRIGGERS).forEach(checkSeenOn);
  const env = loaded("dom", {});
  env.dom[".new-todo"] = {};
  env.at(3);
  env.drain();
  env.frame(4, 5);
  assert.deepEqual(timesOf(env), {});
});

test("the frame after the app's field is in the document is read from the first timer after the next animation frame", () => {
  const env = loaded("dom", {});
  env.dom[".new-todo"] = {};
  env.at(12);
  env.mutate();
  assert.deepEqual([env.raf.length, env.timers.length], [1, 0]);
  env.animate(14);
  assert.deepEqual([env.raf.length, env.timers.length, env.delays], [0, 1, [0]]);
  assert.deepEqual(timesOf(env), { appInDom: 12, ready: 12 });
  env.timeout(19);
  assert.deepEqual(timesOf(env), { appInDom: 12, ready: 12, appFrame: 19 });
});

test("history.pushState and replaceState run as they did, with their arguments and this, return their result, and then readiness is checked", () => {
  const env = loaded("next", {});
  assert.deepEqual(
    ["pushState", "replaceState"].map((name) => Object.hasOwn(env.history, name)),
    [true, true],
  );
  env.at(18);
  assert.equal(env.history.pushState({ __NA: true }, "t", "/a"), "pushState done");
  assert.deepEqual(timesOf(env), { ready: 18 });
  assert.equal(env.history.replaceState({ n: 1 }, "u", "/b"), "replaceState done");
  const other = {};
  env.history.replaceState.call(other, { n: 2 }, "v", "/c");
  assert.deepEqual(env.historyCalls, [
    ["pushState", true, { __NA: true }, "t", "/a"],
    ["replaceState", true, { n: 1 }, "u", "/b"],
    ["replaceState", false, { n: 2 }, "v", "/c"],
  ]);
  assert.deepEqual(other, { state: { n: 2 } });
});

test("history.scrollRestoration still reads and writes as it did, with the attributes it had, and its setter then checks readiness", () => {
  const env = loaded("sveltekit", {});
  assert.equal(env.history.scrollRestoration, "auto");
  env.at(6);
  env.history.scrollRestoration = "manual";
  assert.equal(env.history.scrollRestoration, "manual");
  assert.deepEqual(env.historyCalls, [["scrollRestoration", "manual"]]);
  assert.deepEqual(timesOf(env), { ready: 6 });
  const attributes = (e) => {
    const d = Object.getOwnPropertyDescriptor(e.History.prototype, "scrollRestoration");
    return [typeof d.get, typeof d.set, d.enumerable, d.configurable];
  };
  assert.deepEqual(attributes(env), ["function", "function", true, true]);
  assert.deepEqual(attributes(loaded("sveltekit", { enumerable: false })), ["function", "function", false, true]);
});

test("arming a wait pushes an unstarted, unfinished wait with its name, condition and argument", () => {
  const env = loaded("dom", {});
  h2h(env).arm("add", "todos", 1);
  h2h(env).arm("count", "count", "3");
  assert.deepEqual(plain(h2h(env).waits), [armed("add", "todos", 1), armed("count", "count", "3")]);
});

const INPUTS = ["keydown", "pointerdown", "mousedown", "click"];

function checkStartedBy(type) {
  const env = loaded("dom", {});
  h2h(env).arm("go", "todos", 1);
  env.fire(type, 100.5);
  assert.deepEqual(waitOf(env), armed("go", "todos", 1, { start: 100.5, event: type }));
}

test("an interaction starts at the time stamp of its first keydown, pointerdown, mousedown or click", () => {
  INPUTS.forEach(checkStartedBy);
});

test("a click gives its pointerdown's time stamp as the start, the first of its events, and no later event moves it", () => {
  const env = loaded("dom", {});
  h2h(env).arm("toggle", "completed", 1);
  env.fire("pointerdown", 200);
  env.fire("mousedown", 200.5);
  env.fire("click", 262);
  assert.deepEqual(waitOf(env), armed("toggle", "completed", 1, { start: 200, event: "pointerdown" }));
});

test("an input event starts only the last wait armed, and only one that is armed and not yet started", () => {
  const env = loaded("dom", {});
  h2h(env).arm("add", "todos", 1);
  h2h(env).arm("toggle", "completed", 1);
  env.fire("click", 40);
  assert.deepEqual(plain(h2h(env).waits), [armed("add", "todos", 1), armed("toggle", "completed", 1, { start: 40, event: "click" })]);
  h2h(env).waits[1].armed = false;
  h2h(env).waits[1].start = null;
  env.fire("click", 90);
  assert.equal(waitOf(env, 1).start, null);
});

test("an input event with no wait armed does nothing", () => {
  const env = loaded("dom", {});
  env.fire("keydown", 5);
  assert.deepEqual([plain(h2h(env).waits), env.micro.length], [[], 0]);
});

test("a started wait is checked in a microtask after the input event, not inside its handler, and not before it", () => {
  const env = loaded("dom", {});
  env.all[".todo-list li"] = [{}];
  h2h(env).arm("add", "todos", 1);
  env.at(9);
  env.mutate();
  assert.equal(waitOf(env).dom, null);
  env.fire("keydown", 10);
  assert.deepEqual([env.micro.length, waitOf(env).dom], [1, null]);
  env.at(11);
  env.drain();
  assert.equal(waitOf(env).dom, 11);
});

const CONDITIONS = [
  {
    cond: "todos",
    arg: 2,
    no: [{}, { all: { ".todo-list li": [{}] } }, { all: { ".todo-list li": [{}, {}, {}] } }],
    yes: { all: { ".todo-list li": [{}, {}] } },
  },
  {
    cond: "completed",
    arg: 1,
    no: [{}, { all: { ".todo-list li": [{}], ".todo-list li.completed": [] } }, { all: { ".todo-list li.completed": [{}, {}] } }],
    yes: { all: { ".todo-list li": [{}, {}], ".todo-list li.completed": [{}] } },
  },
  {
    cond: "filter",
    arg: "Completed",
    no: [{}, { dom: { ".filters a.selected": { textContent: " Active " } } }],
    yes: { dom: { ".filters a.selected": { textContent: "\n  Completed\n" } } },
  },
  {
    cond: "count",
    arg: "3",
    no: [{}, { dom: { "#cart-count": { textContent: "2" } } }],
    yes: { dom: { "#cart-count": { textContent: " 3 " } } },
  },
];

const show = (env, state) => {
  env.dom = state.dom ?? {};
  env.all = state.all ?? {};
};

function checkCondition({ cond, arg, no, yes }) {
  const env = loaded("dom", {});
  h2h(env).arm("step", cond, arg);
  env.fire("click", 50);
  no.forEach((state) => {
    show(env, state);
    env.at(60);
    env.mutate();
    assert.equal(waitOf(env).dom, null, `${cond} does not hold yet`);
  });
  show(env, yes);
  env.at(64);
  env.mutate();
  env.frame(66, 70);
  assert.deepEqual([waitOf(env).dom, waitOf(env).frame], [64, 70], cond);
}

test("a started wait is done when its named condition holds: the todos, the completed todos, the selected filter or the cart count", () => {
  CONDITIONS.forEach(checkCondition);
});

test("what has been seen is not seen again: the app, readiness and a wait keep their first times and schedule no second frame", () => {
  const env = loaded("dom", {});
  h2h(env).arm("add", "todos", 1);
  env.fire("keydown", 10);
  env.dom[".new-todo"] = {};
  env.all[".todo-list li"] = [{}];
  env.at(12);
  env.drain();
  env.frame(14, 16);
  const first = plain(h2h(env));
  assert.deepEqual(first.t, { appInDom: 12, ready: 12, appFrame: 16 });
  assert.deepEqual(first.waits, [armed("add", "todos", 1, { start: 10, event: "keydown", dom: 12, frame: 16 })]);
  env.at(99);
  env.mutate();
  assert.deepEqual([env.raf.length, env.timers.length], [0, 0]);
  assert.deepEqual(plain(h2h(env)), first);
});

// --- Playwright's requests, and the page they are made by ---------------------------------

const ORIGIN = "http://localhost:8080";
const later = () => new Promise((resolve) => setImmediate(resolve));
const rejectOr = (value) => {
  if (value instanceof Error) throw value;
  return value;
};

/** A Playwright response: its headers and body come a turn later, or fail. */
function fakeResponse(reads, spec) {
  return {
    status: () => spec.status,
    allHeaders: async () => {
      await later();
      return rejectOr(spec.headers);
    },
    body: async () => {
      reads.body += 1;
      await later();
      return rejectOr(spec.body);
    },
  };
}
const responseOf = (reads, spec) => (spec === null || spec instanceof Error ? rejectOr(spec) : fakeResponse(reads, spec));

/** A Playwright request, and a count of what the code read of it. */
function fakeRequest(spec) {
  const s = { url: `${ORIGIN}/react/dist/hello.js`, method: "GET", kind: "script", failure: null, response: null, reported: null, ...spec };
  const reads = { response: 0, body: 0, sizes: 0 };
  const request = {
    url: () => rejectOr(s.url),
    method: () => s.method,
    resourceType: () => s.kind,
    failure: () => s.failure,
    response: async () => {
      reads.response += 1;
      await later();
      return responseOf(reads, s.response);
    },
    sizes:
      s.sizes ??
      (async () => {
        reads.sizes += 1;
        await later();
        return rejectOr(s.reported);
      }),
  };
  return { request, reads };
}

/** An emitter that records the events it is asked to listen for. */
function recording(emitter, name, rec) {
  const on = emitter.on.bind(emitter);
  emitter.on = (event, fn) => {
    rec(`${name}.on`, event);
    return on(event, fn);
  };
  return emitter;
}

const contextCalls = (world) => ({
  addInitScript: async (script) => {
    world.rec("context.addInitScript", script);
    world.scripts.push(script);
  },
  newPage: async () => {
    world.rec("context.newPage");
    return world.page;
  },
  newCDPSession: async (page) => {
    world.rec("context.newCDPSession", page === world.page);
    return world.cdp;
  },
  close: async () => world.rec("context.close"),
});

const pageCalls = (world) => ({
  goto: async (url, options) => {
    world.rec("page.goto", url, options);
    world.navigate(url);
  },
  waitForFunction: async (fn, arg, options) => {
    world.rec("page.waitForFunction", arg, options);
    world.poll(fn, arg, options);
  },
  waitForLoadState: async (state, options) => {
    world.rec("page.waitForLoadState", state, options);
  },
  evaluate: async (fn, arg) => {
    const result = world.evaluate(fn, arg);
    world.rec("page.evaluate", arg, result);
    world.afterEvaluate(result);
    return result;
  },
  fill: async (selector, text) => {
    world.rec("page.fill", selector, text);
  },
  press: async (selector, key) => {
    world.rec("page.press", selector, key);
    world.act(`press ${selector}`);
  },
  click: async (selector) => {
    world.rec("page.click", selector);
    world.act(`click ${selector}`);
  },
  textContent: async (selector) => {
    world.rec("page.textContent", selector);
    return world.doc.dom[selector].textContent;
  },
});

const worldMethods = (world) => ({
  navigate(url) {
    world.documents += 1;
    world.doc = pageEnv({ ...world.env, timeOrigin: 1_000_000 + world.documents, log: world.calls });
    world.scripts.forEach((script) => world.doc.run(script));
    if (url !== "about:blank") world.scene.load(world.doc, world);
  },
  poll(fn, arg, options) {
    if (!world.doc.evaluate(fn, arg)) throw new Error(`page.waitForFunction: Timeout ${options.timeout}ms exceeded.`);
  },
  evaluate: (fn, arg) => plain(world.doc.evaluate(fn, arg)),
  act: (key) => world.scene.act[key](world.doc, world),
  begin(spec) {
    const made = fakeRequest(spec);
    world.context.emit("request", made.request);
    return made;
  },
  end(made, spec) {
    world.clock.t += spec.ms ?? 0;
    world.context.emit(spec.end ?? "requestfinished", made.request);
  },
  fetch(spec) {
    const made = world.begin(spec);
    world.end(made, spec);
    return made.reads;
  },
  wire: (events) => events.forEach(([name, event]) => world.cdp.emit(name, event)),
});

/** A browser, its context, page and DevTools session as fakes that record their calls, over a scene. */
function newWorld(scene, clock, env) {
  const world = { scene, clock, env, calls: [], scripts: [], documents: 0, doc: null, afterEvaluate: noop };
  world.rec = (...call) => world.calls.push(call);
  world.context = Object.assign(recording(new EventEmitter(), "context", world.rec), contextCalls(world));
  world.cdp = Object.assign(recording(new EventEmitter(), "cdp", world.rec), {
    send: async (method, params) => world.rec("cdp.send", method, params),
  });
  world.page = pageCalls(world);
  world.browser = {
    newContext: async (options) => {
      world.rec("browser.newContext", options);
      return world.context;
    },
  };
  return Object.assign(world, worldMethods(world));
}

/** `measureOnce` over a scene, and what the fakes were called with. It never throws: an error is returned. */
async function run(scene, over) {
  const o = { engine: "chromium", condition: "cold", network: null, target: null, onWait: noop, env: {}, ...over };
  return withClock(async (clock) => {
    const world = newWorld(scene, clock, { entryTypes: CHROMIUM, ...o.env });
    clock.onWait = (n) => o.onWait(world, n);
    const target = o.target ?? TODO;
    const outcome = await measureOnce({ browser: world.browser, engine: o.engine, target, condition: o.condition, network: o.network }).then(
      (result) => ({ result }),
      (error) => ({ error }),
    );
    return { ...outcome, calls: world.calls, world };
  });
}

const succeeded = ({ result, error }) => {
  if (error) throw error;
  return result;
};

// --- an app and its scenes ----------------------------------------------------------------

const DOC_URL = `${ORIGIN}/react/dist/`;
const NAVIGATION = { responseStart: 8.5, responseEnd: 12.25, domContentLoadedEventEnd: 31.5, loadEventEnd: 52 };

/** The app is drawn: marks, paints, and its field in the document at 20 ms, its frame at 26. */
function appears(env) {
  env.navigation = [NAVIGATION];
  env.report("paint", [{ name: "first-contentful-paint", startTime: 22.5 }]);
  env.report("largest-contentful-paint", [{ startTime: 40 }]);
  env.report("layout-shift", [{ value: 0.125, hadRecentInput: false }]);
  env.dom[".new-todo"] = { tag: "input" };
  env.at(20);
  env.mutate();
  env.frame(24, 26);
}

/** A key press adds a todo: the keydown at 100, the list at 104, the frame at 112. */
function added(env) {
  env.fire("keydown", 100);
  env.drain();
  env.all[".todo-list li"] = [{}];
  env.at(104);
  env.mutate();
  env.frame(108, 112);
}

/** A click completes it: pointerdown at 200, then mousedown and click, the list at 203.5, the frame at 210. */
function toggled(env) {
  env.fire("pointerdown", 200);
  env.fire("mousedown", 200.5);
  env.fire("click", 262);
  env.drain();
  env.all[".todo-list li.completed"] = [{}];
  env.at(203.5);
  env.mutate();
  env.frame(207, 210);
}

const BODY = Buffer.from(JSON.stringify({ id: 1, title: "h2h one" }));
const JS_HEADERS = { "content-type": "text/javascript; charset=utf-8", "content-encoding": "br", "cache-control": "public, max-age=31536000, immutable", etag: '"abc"' };
const TINY = Buffer.from("console.log(1);\n");
const HTML = bytes("todomvc-index.html.txt");

const DOC_SPEC = {
  url: DOC_URL,
  kind: "document",
  ms: 12,
  response: { status: 200, headers: { "content-type": "text/html; charset=utf-8", "content-encoding": "br", "cache-control": "no-cache", etag: '"2e5-1a2b3c"' }, body: HTML },
  reported: { responseBodySize: 234, responseHeadersSize: 290 },
};
const POST_SPEC = {
  url: `${ORIGIN}/api/todos`,
  method: "POST",
  kind: "fetch",
  ms: 30,
  response: { status: 201, headers: { "content-type": "application/json", "cache-control": "no-store" }, body: BODY },
  reported: { responseBodySize: 26, responseHeadersSize: 160 },
};
const DOC_ENTRY = {
  phase: "load",
  url: DOC_URL,
  method: "GET",
  kind: "document",
  failed: null,
  heldMs: 12,
  status: 200,
  type: "text/html",
  encoding: "br",
  cacheControl: "no-cache",
  validator: "etag",
  ...sizes(HTML),
  blob: "82aa905b19394a7537790670445e3848d1d02c47",
  text: HTML.toString("utf8"),
  receivedBody: 234,
  receivedHeaders: 290,
};
const POST_ENTRY = {
  phase: "interaction:add",
  url: `${ORIGIN}/api/todos`,
  method: "POST",
  kind: "fetch",
  failed: null,
  heldMs: 30,
  status: 201,
  type: "application/json",
  encoding: null,
  cacheControl: "no-store",
  validator: null,
  ...sizes(BODY),
  blob: "47d4959d9444b16e8fc0f580c10b4e8266021675",
  receivedBody: 26,
  receivedHeaders: 160,
};

/** What Chromium reports of a request: it starts, its headers and body arrive, it finishes. */
const wire = (requestId, url, status, headerBytes, bodyBytes) => [
  ["Network.requestWillBeSent", { requestId, request: { url } }],
  ["Network.responseReceived", { requestId, response: { status, encodedDataLength: headerBytes } }],
  ["Network.dataReceived", { requestId, encodedDataLength: bodyBytes }],
  ["Network.loadingFinished", { requestId, encodedDataLength: headerBytes + bodyBytes }],
];
const cdpEntry = (url, over) => ({ url, phase: "load", received: 0, byPhase: {}, total: null, failed: null, fromCache: false, bytes: 0, ...over });
const DOC_CDP = cdpEntry(DOC_URL, { received: 524, byPhase: { load: 524 }, total: 524, status: 200, bytes: 524 });
const POST_CDP = cdpEntry(`${ORIGIN}/api/todos`, { phase: "interaction:add", received: 186, byPhase: { "interaction:add": 186 }, total: 186, status: 201, bytes: 186 });

const ADD = {
  name: "add",
  cond: "todos",
  arg: 1,
  run: (page) => page.fill(".new-todo", "h2h one").then(() => page.press(".new-todo", "Enter")),
};
const TOGGLE = { name: "toggle", cond: "completed", arg: 1, run: (page) => page.click(".todo-list li .toggle") };
const TODO = { url: DOC_URL, appSelector: ".new-todo", ready: "dom", interactions: [ADD, TOGGLE] };
const NO_STEPS = { ...TODO, interactions: [] };

const TODO_SCENE = {
  load: (env, world) => {
    appears(env);
    world.fetch(DOC_SPEC);
    world.wire(wire("1", DOC_URL, 200, 290, 234));
  },
  act: {
    "press .new-todo": (env, world) => {
      added(env);
      world.fetch(POST_SPEC);
      world.wire(wire("2", `${ORIGIN}/api/todos`, 201, 160, 26));
    },
    "click .todo-list li .toggle": toggled,
  },
};

const TIMINGS = { ttfb: 8.5, documentEnd: 12.25, domContentLoaded: 31.5, load: 52, fcp: 22.5, lcp: 40, cls: 0.125, appInDom: 20, appFrame: 26, ready: 20 };
const ADD_WAIT = armed("add", "todos", 1, { start: 100, event: "keydown", dom: 104, frame: 112 });
const TOGGLE_WAIT = armed("toggle", "completed", 1, { start: 200, event: "pointerdown", dom: 203.5, frame: 210 });
const INTERACTIONS = [
  { name: "add", event: "keydown", toDom: 4, toFrame: 12 },
  { name: "toggle", event: "pointerdown", toDom: 3.5, toFrame: 10 },
];

/** The calls of one visit of the todo app: the load, its readiness, the timings, each interaction, the same-document check. */
function visitCalls(origin, timings) {
  return [
    ["page.goto", DOC_URL, { waitUntil: "commit", timeout: 60_000 }],
    ["page.waitForFunction", null, { timeout: 60_000, polling: 50 }],
    ["page.waitForLoadState", "load", { timeout: 60_000 }],
    ["page.evaluate", undefined, timings],
    ["page.evaluate", undefined, origin],
    ["page.evaluate", ["add", "todos", 1], undefined],
    ["page.fill", ".new-todo", "h2h one"],
    ["page.press", ".new-todo", "Enter"],
    ["page.waitForFunction", null, { timeout: 30_000, polling: 25 }],
    ["page.evaluate", undefined, ADD_WAIT],
    ["page.evaluate", ["toggle", "completed", 1], undefined],
    ["page.click", ".todo-list li .toggle"],
    ["page.waitForFunction", null, { timeout: 30_000, polling: 25 }],
    ["page.evaluate", undefined, TOGGLE_WAIT],
    ["page.evaluate", undefined, origin],
    ["page.evaluate", undefined, 1],
  ];
}

const open = (script) => [["browser.newContext", { viewport: { width: 1280, height: 800 } }], ["context.addInitScript", script], ["context.newPage"]];
const CDP_OPEN = [["context.newCDPSession", true], ["cdp.send", "Network.enable", undefined]];
const CONTEXT_ON = ["request", "requestfinished", "requestfailed"].map((event) => ["context.on", event]);
const CDP_ON = ["requestWillBeSent", "responseReceived", "requestServedFromCache", "dataReceived", "loadingFinished", "loadingFailed"].map((event) => ["cdp.on", `Network.${event}`]);
const SHAPE = [
  ["cdp.send", "Network.enable", undefined],
  ["cdp.send", "Network.emulateNetworkConditions", { offline: false, latency: 562.5, downloadThroughput: 188743, uploadThroughput: 86400 }],
  ["cdp.send", "Emulation.setCPUThrottlingRate", { rate: 4 }],
];
const CLEAR_STORAGE = [["localStorage.clear"], ["sessionStorage.clear"], ["page.evaluate", undefined, undefined]];
const BLANK = [["page.goto", "about:blank", undefined]];
const CLOSE = [["context.close"]];

// --- the log of requests: an entry for each response ----------------------------------------

/** The log of a load that serves these requests (a Firefox run, with nothing to interact with), and what was read of each. */
async function requestsOf(specs) {
  const reads = [];
  const scene = {
    load: (env, world) => {
      appears(env);
      specs.forEach((spec) => reads.push(world.fetch(spec)));
    },
    act: {},
  };
  const result = succeeded(await run(scene, { engine: "firefox", target: NO_STEPS }));
  return { requests: result.requests, reads };
}

const script = (over) => ({
  url: `${ORIGIN}/react/dist/hello.js`,
  response: { status: 200, headers: JS_HEADERS, body: TINY },
  reported: { responseBodySize: 40, responseHeadersSize: 180 },
  ...over,
});
const answer = (over) => script({ response: { status: 200, headers: JS_HEADERS, body: TINY, ...over } });
const EMPTY = { ...sizes(Buffer.alloc(0)), blob: null };
const base = (over) => ({ phase: "load", url: `${ORIGIN}/react/dist/hello.js`, method: "GET", kind: "script", failed: null, heldMs: 0, ...over });

// What the log makes of `script({})`, a 16-byte script answered 200, Brotli-encoded, with an etag.
const TINY_ENTRY = base({
  status: 200,
  type: "text/javascript",
  encoding: "br",
  cacheControl: "public, max-age=31536000, immutable",
  validator: "etag",
  ...sizes(TINY),
  blob: "296d5492b0034a110271fd68971eb95e0cebb0b3",
  minified: true,
  minifiedShare: null,
  minifiedBytes: 16,
  receivedBody: 40,
  receivedHeaders: 180,
});

test("a response is an entry with its status, headers, sizes, blob, minified verdict and the bytes the browser received", async () => {
  const { requests } = await requestsOf([script({ ms: 7 })]);
  assert.deepEqual(requests, [{ ...TINY_ENTRY, heldMs: 7 }]);
});

const verdictOf = (e) => ({ raw: e.raw, blob: e.blob, minified: e.minified, minifiedShare: e.minifiedShare, minifiedBytes: e.minifiedBytes });

test("a script nobody minified is reported unminified and a minified stylesheet minified, by esbuild's verdict on their bytes", async () => {
  const unminified = bytes("todomvc-base-unminified.js.txt");
  const stylesheet = bytes("todomvc-app.css.txt");
  const { requests } = await requestsOf([
    script({ response: { status: 200, headers: JS_HEADERS, body: unminified } }),
    script({ kind: "stylesheet", response: { status: 200, headers: { "content-type": "text/css; charset=utf-8" }, body: stylesheet } }),
  ]);
  const js = minifiedVerdict(unminified.toString("utf8"), "text/javascript");
  const css = minifiedVerdict(stylesheet.toString("utf8"), "text/css");
  assert.deepEqual(requests.map(verdictOf), [
    { raw: 7253, blob: "a56b5aaca980fb608faf3eb9624750c8740b0025", minified: false, minifiedShare: js.share, minifiedBytes: js.minifiedBytes },
    { raw: 7400, blob: "676204ebeac3d26572e969bdc229dbbce7237336", minified: true, minifiedShare: css.share, minifiedBytes: css.minifiedBytes },
  ]);
});

test("a document keeps its text in its entry, and no other kind of response does", async () => {
  const { requests } = await requestsOf([DOC_SPEC, script({})]);
  assert.deepEqual(
    requests.map((r) => [r.kind, "text" in r]),
    [
      ["document", true],
      ["script", false],
    ],
  );
  assert.deepEqual(requests[0], DOC_ENTRY);
  // A document's text is its bytes read as UTF-8: an e acute and three kanji are two and nine bytes.
  const accented = `<!doctype html><title>Caf${String.fromCodePoint(0xe9, 0x20, 0x65e5, 0x672c, 0x8a9e)}</title>`;
  const answered = { ...DOC_SPEC.response, body: Buffer.from(accented, "utf8") };
  assert.equal((await requestsOf([{ ...DOC_SPEC, response: answered }])).requests[0].text, accented);
});

test("a validator is the etag when there is one, else the last-modified date, else none", async () => {
  const modified = "Tue, 03 Nov 2026 10:00:00 GMT";
  const { requests } = await requestsOf([
    answer({ headers: { etag: '"a"', "last-modified": modified } }),
    answer({ headers: { "last-modified": modified } }),
    answer({ headers: { etag: '"a"' } }),
    answer({ headers: {} }),
  ]);
  assert.deepEqual(
    requests.map((r) => r.validator),
    ["etag", "last-modified", "etag", null],
  );
});

const STATUSES = [200, 299, 300, 301, 304, 399, 400, 404, 500];

test("a response's body is read unless it is a redirect or not modified: a 3xx is not read, a 2xx, 4xx or 5xx is", async () => {
  const { requests, reads } = await requestsOf(STATUSES.map((status) => answer({ status, body: Buffer.from("x") })));
  assert.deepEqual(
    requests.map((r) => [r.status, r.raw]),
    [
      [200, 1],
      [299, 1],
      [300, 0],
      [301, 0],
      [304, 0],
      [399, 0],
      [400, 1],
      [404, 1],
      [500, 1],
    ],
  );
  assert.deepEqual(
    reads.map((r) => r.body),
    [1, 1, 0, 0, 0, 0, 1, 1, 1],
  );
});

test("a 304 and a redirect are entries with their headers and an empty body, which was not read", async () => {
  const { requests, reads } = await requestsOf([
    { ...answer({ status: 304, headers: { etag: '"abc"', "cache-control": "no-cache" } }), reported: { responseBodySize: 0, responseHeadersSize: 120 } },
    { ...DOC_SPEC, ms: 0, response: { status: 302, headers: { location: "/react/dist/" }, body: HTML }, reported: { responseBodySize: 0, responseHeadersSize: 90 } },
  ]);
  assert.deepEqual(requests, [
    base({ status: 304, type: "", encoding: null, cacheControl: "no-cache", validator: "etag", ...EMPTY, receivedBody: 0, receivedHeaders: 120 }),
    base({ url: DOC_URL, kind: "document", status: 302, type: "", encoding: null, cacheControl: null, validator: null, ...EMPTY, text: "", receivedBody: 0, receivedHeaders: 90 }),
  ]);
  assert.deepEqual(
    reads.map((r) => r.body),
    [0, 0],
  );
});

test("a response whose body cannot be read, or has no bytes, is an entry with an empty body and no blob", async () => {
  const { requests, reads } = await requestsOf([answer({ body: new Error("No resource with given identifier found") }), answer({ body: Buffer.alloc(0) })]);
  const empty = base({
    status: 200,
    type: "text/javascript",
    encoding: "br",
    cacheControl: "public, max-age=31536000, immutable",
    validator: "etag",
    ...EMPTY,
    minified: true,
    minifiedShare: null,
    minifiedBytes: 0,
    receivedBody: 40,
    receivedHeaders: 180,
  });
  assert.deepEqual(requests, [empty, empty]);
  assert.deepEqual(
    reads.map((r) => r.body),
    [1, 1],
  );
});

test("a request with no response is an entry with a null status and no headers, and its sizes are still asked for", async () => {
  const { requests, reads } = await requestsOf([script({ response: null })]);
  assert.deepEqual(requests, [base({ status: null, type: "", encoding: null, cacheControl: null, validator: null, ...EMPTY, receivedBody: 40, receivedHeaders: 180 })]);
  assert.deepEqual(reads, [{ response: 1, body: 0, sizes: 1 }]);
});

test("the bytes Playwright reports are kept, a negative size as 0, and a size it cannot give as null", async () => {
  const { requests } = await requestsOf([
    script({ reported: { responseBodySize: 1234, responseHeadersSize: 56 } }),
    script({ reported: { responseBodySize: -1, responseHeadersSize: -1 } }),
    script({ reported: new Error("closed") }),
    script({ reported: null }),
  ]);
  assert.deepEqual(
    requests.map((r) => [r.receivedBody, r.receivedHeaders]),
    [
      [1234, 56],
      [0, 0],
      [null, null],
      [null, null],
    ],
  );
  assert.equal(
    requests.some((r) => "error" in r),
    false,
  );
});

test("a failed request is an entry with its error text, and no response, body or size was read of it", async () => {
  const { requests, reads } = await requestsOf([
    script({ end: "requestfailed", failure: { errorText: "net::ERR_ABORTED" } }),
    script({ end: "requestfailed", failure: null }),
    script({ end: "requestfailed", failure: {} }),
    script({ end: "requestfailed", failure: { errorText: "" } }),
  ]);
  assert.deepEqual(requests[0], base({ failed: "net::ERR_ABORTED" }));
  assert.deepEqual(
    requests.map((r) => r.failed),
    ["net::ERR_ABORTED", "failed", "failed", ""],
  );
  assert.deepEqual(reads, Array(4).fill({ response: 0, body: 0, sizes: 0 }));
});

test("a response that cannot be read leaves an entry with the first line of the error, and no other request is stopped", async () => {
  const { requests } = await requestsOf([
    script({ response: new Error("Target page, context or browser has been closed") }),
    answer({ headers: new Error("first line\nsecond line") }),
    script({}),
  ]);
  assert.deepEqual(requests.slice(0, 2), [base({ error: "Error: Target page, context or browser has been closed" }), base({ error: "Error: first line" })]);
  assert.equal(requests[2].status, 200);
});

test("an error after the body was read keeps what was read: the entry has its sizes and verdict and the error, and no received bytes", async () => {
  const unavailable = () => {
    throw new Error("request.sizes is not a function");
  };
  const { requests } = await requestsOf([script({ sizes: unavailable })]);
  const kept = Object.entries(TINY_ENTRY).filter(([key]) => !key.startsWith("received"));
  assert.deepEqual(requests, [{ ...Object.fromEntries(kept), error: "Error: request.sizes is not a function" }]);
});

const TYPES = [
  ["text/javascript", true],
  ["application/javascript", true],
  ["text/css", true],
  ["text/html", false],
  ["application/json", false],
  ["image/svg+xml", false],
  ["", false],
];

test("only a script or a stylesheet gets a minified verdict, by its content type", async () => {
  const { requests } = await requestsOf(TYPES.map(([type]) => answer({ headers: { "content-type": type } })));
  assert.deepEqual(
    requests.map((r) => [r.type, "minified" in r]),
    TYPES,
  );
});

test("entries are made as requests end, in that order, and are complete when measureOnce returns; a request that never ends makes none", async () => {
  const scene = {
    load: (env, world) => {
      appears(env);
      const [a, b] = [world.begin(script({ url: `${ORIGIN}/a.js` })), world.begin(script({ url: `${ORIGIN}/b.js` }))];
      world.begin(script({ url: `${ORIGIN}/never.js` }));
      world.end(b, {});
      world.end(a, {});
    },
    act: {},
  };
  const { requests } = succeeded(await run(scene, { engine: "firefox", target: NO_STEPS }));
  assert.deepEqual(
    requests.map((r) => [r.url, r.status]),
    [
      [`${ORIGIN}/b.js`, 200],
      [`${ORIGIN}/a.js`, 200],
    ],
  );
});

test("an entry's held time is from its request's start to its end, and 0 for a request the log did not see start", async () => {
  const unseen = fakeRequest({}).request;
  const scene = {
    load: (env, world) => {
      appears(env);
      world.fetch(script({ ms: 250 }));
      world.fetch(script({ ms: 1500 }));
      world.context.emit("requestfinished", unseen);
    },
    act: {},
  };
  const { requests } = succeeded(await run(scene, { engine: "firefox", target: NO_STEPS }));
  assert.deepEqual(
    requests.map((r) => r.heldMs),
    [250, 1500, 0],
  );
});

test("an entry is in the phase in which its request ends, not the one it began in", async () => {
  const scene = {
    load: (env, world) => {
      appears(env);
      world.opened = world.begin(script({ url: `${ORIGIN}/opened-in-load.js` }));
      world.fetch(script({ url: `${ORIGIN}/ended-in-load.js` }));
    },
    act: {
      "press .new-todo": (env, world) => {
        added(env);
        world.end(world.opened, {});
        world.fetch(script({ url: `${ORIGIN}/made-in-add.js` }));
      },
    },
  };
  const { requests } = succeeded(await run(scene, { engine: "firefox", target: { ...TODO, interactions: [ADD] } }));
  assert.deepEqual(
    requests.map((r) => [r.url.split("/").pop(), r.phase]),
    [
      ["ended-in-load.js", "load"],
      ["opened-in-load.js", "interaction:add"],
      ["made-in-add.js", "interaction:add"],
    ],
  );
});

// --- when the network is quiet ----------------------------------------------------------------

const quietScene = (load) => ({
  load: (env, world) => {
    appears(env);
    load(world);
  },
  act: {},
});
const wallMsOf = async (load, over) => succeeded(await run(quietScene(load), { engine: "firefox", target: NO_STEPS, ...over })).wallMs;
const LONG_POLL = script({ url: `${ORIGIN}/updates`, kind: "fetch" });

test("after the load the visit waits until nothing has started or ended for 400 ms", async () => {
  assert.equal(await wallMsOf((world) => world.fetch({ ...DOC_SPEC, ms: 30 })), 430);
});

test("a request held for more than a second does not keep the network busy: the visit goes on 1025 ms after it began", async () => {
  const wall = await wallMsOf((world) => {
    world.fetch({ ...DOC_SPEC, ms: 30 });
    world.begin(LONG_POLL);
  });
  assert.equal(wall, 1055);
});

test("a request that ends while the visit waits is activity, and the wait ends 400 ms after it", async () => {
  const wall = await wallMsOf((world) => world.fetch({ ...DOC_SPEC, ms: 30 }), { onWait: (world, n) => n === 10 && world.fetch(script({})) });
  assert.equal(wall, 680);
});

async function checkLongHeld([endsAtWait, wallMs, heldMs]) {
  const scene = quietScene((world) => {
    world.fetch({ ...DOC_SPEC, ms: 30 });
    world.stream = world.begin(LONG_POLL);
  });
  const result = succeeded(await run(scene, { engine: "firefox", target: NO_STEPS, onWait: (world, n) => n === endsAtWait && world.end(world.stream, {}) }));
  assert.deepEqual([result.wallMs, result.requests[1].heldMs], [wallMs, heldMs]);
}

test("a request that ends after more than a second is no activity, and one that ends after exactly a second is", async () => {
  await checkLongHeld([41, 1055, 1025]);
  await checkLongHeld([40, 1430, 1000]);
});

test("the wait for quiet gives up after 15 seconds of activity", async () => {
  assert.equal(await wallMsOf(noop, { onWait: (world) => world.context.emit("request", {}) }), 15000);
});

test("a request held for more than a second that ends after the visit's last read is complete in the result", async () => {
  const scene = quietScene((world) => {
    world.fetch(DOC_SPEC);
    world.stream = world.begin(LONG_POLL);
    world.afterEvaluate = (result) => result === 1 && world.end(world.stream, {});
  });
  const { requests } = succeeded(await run(scene, { engine: "firefox", target: NO_STEPS }));
  assert.deepEqual(requests, [DOC_ENTRY, { ...TINY_ENTRY, url: `${ORIGIN}/updates`, kind: "fetch", heldMs: 1025 }]);
});

// --- Chromium's own count -----------------------------------------------------------------------

const ev = (name, requestId, body) => [`Network.${name}`, { requestId, ...body }];
const STREAM = `${ORIGIN}/updates`;

/** The record of a Chromium run whose load and "add" interaction play these DevTools events. */
async function receivedOf(onLoad, onAdd) {
  const scene = {
    load: (env, world) => {
      appears(env);
      world.wire(onLoad);
    },
    act: {
      "press .new-todo": (env, world) => {
        added(env);
        world.wire(onAdd);
      },
      "click .todo-list li .toggle": toggled,
    },
  };
  return succeeded(await run(scene, {})).received;
}

test("Chromium's bytes are counted in the phase they arrive in, and what its total adds counts where the request finished", async () => {
  const received = await receivedOf(
    [
      ev("requestWillBeSent", "1", { request: { url: STREAM } }),
      ev("responseReceived", "1", { response: { status: 200, encodedDataLength: 300 } }),
      ev("dataReceived", "1", { encodedDataLength: 1000 }),
      ev("dataReceived", "1", { encodedDataLength: 1000 }),
    ],
    [ev("dataReceived", "1", { encodedDataLength: 30 }), ev("loadingFinished", "1", { encodedDataLength: 2400 })],
  );
  assert.deepEqual(received, [cdpEntry(STREAM, { received: 2400, byPhase: { load: 2300, "interaction:add": 100 }, total: 2400, status: 200, bytes: 2400 })]);
});

test("when Chromium's total is below what arrived, the total is the request's bytes and nothing more is counted", async () => {
  const received = await receivedOf(
    [
      ev("requestWillBeSent", "2", { request: { url: STREAM } }),
      ev("responseReceived", "2", { response: { status: 200, encodedDataLength: 100 } }),
      ev("dataReceived", "2", { encodedDataLength: 500 }),
      ev("loadingFinished", "2", { encodedDataLength: 450 }),
    ],
    [],
  );
  assert.deepEqual(received, [cdpEntry(STREAM, { received: 600, byPhase: { load: 600 }, total: 450, status: 200, bytes: 450 })]);
});

test("when Chromium's total is what arrived, nothing is counted in the phase the request finished in", async () => {
  const received = await receivedOf(
    [
      ev("requestWillBeSent", "4", { request: { url: STREAM } }),
      ev("responseReceived", "4", { response: { status: 200, encodedDataLength: 100 } }),
      ev("dataReceived", "4", { encodedDataLength: 500 }),
    ],
    [ev("loadingFinished", "4", { encodedDataLength: 600 })],
  );
  assert.deepEqual(received, [cdpEntry(STREAM, { received: 600, byPhase: { load: 600 }, total: 600, status: 200, bytes: 600 })]);
});

test("a request still open has the bytes it received so far, and no status until its response", async () => {
  const received = await receivedOf([ev("requestWillBeSent", "3", { request: { url: STREAM } }), ev("dataReceived", "3", { encodedDataLength: 40 })], []);
  assert.deepEqual(received, [cdpEntry(STREAM, { received: 40, byPhase: { load: 40 }, bytes: 40 })]);
});

const served = (id, flag) => [
  ev("requestWillBeSent", id, { request: { url: `${ORIGIN}/${id}.js` } }),
  ev("responseReceived", id, { response: { status: 200, encodedDataLength: 120, ...flag } }),
  ev("loadingFinished", id, { encodedDataLength: 0 }),
];
const cached = (id, over) => cdpEntry(`${ORIGIN}/${id}.js`, { fromCache: true, status: 200, total: 0, ...over });

test("a response a cache answered, whichever cache, received nothing from the network, and what came before the answer stays counted", async () => {
  const received = await receivedOf(
    [
      ...served("disk", { fromDiskCache: true }),
      ...served("prefetch", { fromPrefetchCache: true }),
      ...served("worker", { fromServiceWorker: true }),
      ev("requestWillBeSent", "event", { request: { url: `${ORIGIN}/event.js` } }),
      ev("requestServedFromCache", "event", {}),
      ev("responseReceived", "event", { response: { status: 200, encodedDataLength: 77 } }),
      ev("dataReceived", "event", { encodedDataLength: 500 }),
      ev("loadingFinished", "event", { encodedDataLength: 0 }),
      ev("requestWillBeSent", "late", { request: { url: `${ORIGIN}/late.js` } }),
      ev("dataReceived", "late", { encodedDataLength: 10 }),
      ev("requestServedFromCache", "late", {}),
      ev("dataReceived", "late", { encodedDataLength: 20 }),
      ev("loadingFinished", "late", { encodedDataLength: 0 }),
    ],
    [],
  );
  const late = cdpEntry(`${ORIGIN}/late.js`, { fromCache: true, total: 0, received: 10, byPhase: { load: 10 } });
  assert.deepEqual(received, [cached("disk"), cached("prefetch"), cached("worker"), cached("event"), late]);
});

test("a request that failed keeps Chromium's error text, else canceled, else failed", async () => {
  const fail = (id, body) => [ev("requestWillBeSent", id, { request: { url: `${ORIGIN}/${id}` } }), ev("loadingFailed", id, body)];
  const received = await receivedOf(
    [...fail("a", { errorText: "net::ERR_ABORTED", canceled: true }), ...fail("b", { canceled: true }), ...fail("c", {}), ...fail("d", { errorText: "", canceled: true })],
    [],
  );
  assert.deepEqual(
    received.map((r) => r.failed),
    ["net::ERR_ABORTED", "canceled", "failed", "canceled"],
  );
});

test("events for a request Chromium never announced are ignored, and a second announcement of a request keeps the first", async () => {
  const ghost = [
    ev("responseReceived", "ghost", { response: { status: 200, encodedDataLength: 9 } }),
    ev("requestServedFromCache", "ghost", {}),
    ev("dataReceived", "ghost", { encodedDataLength: 9 }),
    ev("loadingFinished", "ghost", { encodedDataLength: 9 }),
    ev("loadingFailed", "ghost", { errorText: "net::ERR_FAILED" }),
  ];
  const received = await receivedOf(
    [...ghost, ev("requestWillBeSent", "dup", { request: { url: `${ORIGIN}/first` } })],
    [ev("requestWillBeSent", "dup", { request: { url: `${ORIGIN}/second` } })],
  );
  assert.deepEqual(received, [cdpEntry(`${ORIGIN}/first`)]);
});

test("a response without an encoded length counts no bytes, but opens its phase at 0", async () => {
  const received = await receivedOf([ev("requestWillBeSent", "7", { request: { url: STREAM } }), ev("responseReceived", "7", { response: { status: 200 } })], []);
  assert.deepEqual(received, [cdpEntry(STREAM, { byPhase: { load: 0 }, status: 200 })]);
});

// --- one measured visit ---------------------------------------------------------------------

const COLD = { timings: TIMINGS, interactions: INTERACTIONS, sameDocument: true, wallMs: 842, received: [DOC_CDP, POST_CDP], requests: [DOC_ENTRY, POST_ENTRY] };
const CHROMIUM_LOG = [...CDP_ON, ...CONTEXT_ON];

test("a cold Chromium run opens a context, observes, loads, waits for quiet, interacts and closes, and returns what it saw", async () => {
  const { result, calls } = await run(TODO_SCENE, {});
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...CHROMIUM_LOG, ...visitCalls(1_000_001, TIMINGS), ...CLOSE]);
  assert.deepEqual(result, COLD);
});

test("a run in an engine without the DevTools protocol makes no session, returns no received bytes, and has no layout shift to report", async () => {
  for (const engine of ["firefox", "webkit"]) {
    const { result, calls } = await run(TODO_SCENE, { engine, env: { entryTypes: FIREFOX } });
    const timings = { ...TIMINGS, cls: null };
    assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CONTEXT_ON, ...visitCalls(1_000_001, timings), ...CLOSE], engine);
    assert.deepEqual(result, { ...COLD, timings, received: null }, engine);
  }
});

test("a warm run visits once without a log, clears the page's storage, goes blank, then measures a visit that logs only itself", async () => {
  const { result, calls } = await run(TODO_SCENE, { condition: "warm" });
  assert.deepEqual(calls, [
    ...open(SCRIPTS.dom),
    ...CDP_OPEN,
    ...visitCalls(1_000_001, TIMINGS),
    ...CLEAR_STORAGE,
    ...BLANK,
    ...CHROMIUM_LOG,
    ...visitCalls(1_000_003, TIMINGS),
    ...CLOSE,
  ]);
  assert.deepEqual(result, COLD);
});

test("a page that refuses to clear its storage does not stop a warm run", async () => {
  const { result, calls } = await run(TODO_SCENE, { condition: "warm", env: { denied: true } });
  const denied = [["localStorage.clear"], ["page.evaluate", undefined, undefined]];
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...visitCalls(1_000_001, TIMINGS), ...denied, ...BLANK, ...CHROMIUM_LOG, ...visitCalls(1_000_003, TIMINGS), ...CLOSE]);
  assert.deepEqual(result, COLD);
});

test("a condition that is neither cold nor warm throws once the page is open, and the context is still closed", async () => {
  const { result, error, calls } = await run(TODO_SCENE, { condition: "hot" });
  assert.equal(result, undefined);
  assert.equal(error.message, "unknown condition hot");
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...CLOSE]);
});

test("a shaped Chromium run applies the profile once the log is on, before its visit", async () => {
  const { result, calls } = await run(TODO_SCENE, { network: SLOW_4G });
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...CHROMIUM_LOG, ...SHAPE, ...visitCalls(1_000_001, TIMINGS), ...CLOSE]);
  assert.deepEqual(result, COLD);
});

test("a shaped warm run shapes its measured visit only, not the visit that warms the cache", async () => {
  const { calls } = await run(TODO_SCENE, { condition: "warm", network: SLOW_4G });
  assert.deepEqual(calls, [
    ...open(SCRIPTS.dom),
    ...CDP_OPEN,
    ...visitCalls(1_000_001, TIMINGS),
    ...CLEAR_STORAGE,
    ...BLANK,
    ...CHROMIUM_LOG,
    ...SHAPE,
    ...visitCalls(1_000_003, TIMINGS),
    ...CLOSE,
  ]);
});

test("a shaped run in an engine without the DevTools protocol throws a TypeError, and the context is still closed", async () => {
  const { result, error, calls } = await run(TODO_SCENE, { engine: "firefox", network: SLOW_4G });
  assert.equal(result, undefined);
  assert.equal(error instanceof TypeError, true);
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CONTEXT_ON, ...CLOSE]);
});

/** The key press is handled and the list changes, but the frame after it never comes. */
const stalled = (env) => {
  env.fire("keydown", 100);
  env.drain();
  env.all[".todo-list li"] = [{}];
  env.at(104);
  env.mutate();
};
const REFUSED = "page.goto: net::ERR_CONNECTION_REFUSED at http://localhost:8080/react/dist/";
const READY_TIMEOUT = "page.waitForFunction: Timeout 60000ms exceeded.";
const STEP_TIMEOUT = "page.waitForFunction: Timeout 30000ms exceeded.";
const refusing = {
  load: () => {
    throw new Error(REFUSED);
  },
  act: {},
};
const FAILURES = [
  ["the navigation fails", refusing, REFUSED, 1, {}],
  ["the app never gets ready", { load: noop, act: {} }, READY_TIMEOUT, 2, {}],
  ["the init script did not run in the page", TODO_SCENE, READY_TIMEOUT, 2, { env: { top: false } }],
  ["the first interaction never completes", { ...TODO_SCENE, act: { "press .new-todo": noop } }, STEP_TIMEOUT, 9, {}],
  ["the first interaction's frame never comes", { ...TODO_SCENE, act: { "press .new-todo": stalled } }, STEP_TIMEOUT, 9, {}],
  ["the second interaction never completes", { ...TODO_SCENE, act: { ...TODO_SCENE.act, "click .todo-list li .toggle": noop } }, STEP_TIMEOUT, 13, {}],
];

async function checkFailure([name, scene, message, reached, over]) {
  const { result, error, calls } = await run(scene, over);
  assert.equal(result, undefined, name);
  assert.equal(error.message, message, name);
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...CHROMIUM_LOG, ...visitCalls(1_000_001, TIMINGS).slice(0, reached), ...CLOSE], name);
}

test("the context is closed, and the error is the page's own, when the navigation fails, the init script did not run, or an interaction or its frame never comes", async () => {
  for (const failure of FAILURES) await checkFailure(failure);
});

const STORE_ADD = {
  name: "add",
  cond: "count",
  arg: null,
  before: async (page) => String(Number((await page.textContent("#cart-count")).trim()) + 1),
  run: (page) => page.click("#menu li:first-child button"),
};
const STORE = { url: `${ORIGIN}/stores/blue-bottle`, appSelector: "#menu", ready: "pw", interactions: [STORE_ADD] };
/** The store's menu is in the document at 15 ms, its frame at 19, and its runtime says it is ready at 20. */
const STORE_SCENE = {
  load: (env) => {
    env.navigation = [NAVIGATION];
    env.dom["#menu"] = {};
    env.dom["#cart-count"] = { textContent: " 2 " };
    env.at(15);
    env.mutate();
    env.frame(17, 19);
    env.dataset.pwReady = "1";
    env.at(20);
    env.mutate();
  },
  act: {
    "click #menu li:first-child button": (env) => {
      env.fire("pointerdown", 300);
      env.drain();
      env.dom["#cart-count"] = { textContent: "3" };
      env.at(312);
      env.mutate();
      env.frame(316, 330);
    },
  },
};

test("an interaction is armed with what its before step returns; a paint the page never reported is null and a layout shift of 0 stays 0", async () => {
  const { result, calls } = await run(STORE_SCENE, { target: STORE });
  const timings = { ...TIMINGS, fcp: null, lcp: null, cls: 0, appInDom: 15, appFrame: 19, ready: 20 };
  const wait = armed("add", "count", "3", { start: 300, event: "pointerdown", dom: 312, frame: 330 });
  assert.deepEqual(calls, [
    ...open(spliced("#menu", "pw")),
    ...CDP_OPEN,
    ...CHROMIUM_LOG,
    ["page.goto", STORE.url, { waitUntil: "commit", timeout: 60_000 }],
    ["page.waitForFunction", null, { timeout: 60_000, polling: 50 }],
    ["page.waitForLoadState", "load", { timeout: 60_000 }],
    ["page.evaluate", undefined, timings],
    ["page.evaluate", undefined, 1_000_001],
    ["page.textContent", "#cart-count"],
    ["page.evaluate", ["add", "count", "3"], undefined],
    ["page.click", "#menu li:first-child button"],
    ["page.waitForFunction", null, { timeout: 30_000, polling: 25 }],
    ["page.evaluate", undefined, wait],
    ["page.evaluate", undefined, 1_000_001],
    ["page.evaluate", undefined, 1],
    ...CLOSE,
  ]);
  assert.deepEqual(result, { timings, interactions: [{ name: "add", event: "pointerdown", toDom: 12, toFrame: 30 }], sameDocument: true, wallMs: 400, received: [], requests: [] });
});

test("a page that says it is ready without its control in the document has no time for the control, and a mark at 0 ms is 0, not null", async () => {
  const bare = {
    load: (env) => {
      env.navigation = [{ ...NAVIGATION, loadEventEnd: 0 }];
      env.report("paint", [{ name: "first-contentful-paint", startTime: 0 }]);
      env.dataset.pwReady = "1";
      env.at(20);
      env.mutate();
    },
    act: {},
  };
  const result = succeeded(await run(bare, { target: { ...STORE, interactions: [] } }));
  assert.deepEqual(result.timings, { ...TIMINGS, load: 0, fcp: 0, lcp: null, cls: 0, appInDom: null, appFrame: null, ready: 20 });
});

test("a page whose control is in the document but that never says it is ready is not interactive: the visit times out", async () => {
  const unready = {
    load: (env) => {
      env.dom["#menu"] = {};
      env.at(15);
      env.mutate();
    },
    act: {},
  };
  const { result, error } = await run(unready, { target: STORE });
  assert.equal(result, undefined);
  assert.equal(error.message, READY_TIMEOUT);
});

test("a page with no navigation entry has null navigation marks, and is not the same document", async () => {
  const noNavigation = {
    ...STORE_SCENE,
    load: (env, world) => {
      STORE_SCENE.load(env, world);
      env.navigation = [];
    },
  };
  const { result, calls } = await run(noNavigation, { target: STORE });
  const { ttfb, documentEnd, domContentLoaded, load } = result.timings;
  assert.deepEqual([ttfb, documentEnd, domContentLoaded, load], [null, null, null, null]);
  assert.equal(result.sameDocument, false);
  assert.deepEqual(calls.slice(-3), [["page.evaluate", undefined, 1_000_001], ["page.evaluate", undefined, 0], ...CLOSE]);
});

const thenIn = (after) => ({
  ...TODO_SCENE,
  act: {
    ...TODO_SCENE.act,
    "click .todo-list li .toggle": (env) => {
      toggled(env);
      after(env);
    },
  },
});

test("a run is not the same document if the time origin changed, and the navigations are then not counted", async () => {
  const { result, calls } = await run(
    thenIn((env) => {
      env.timeOrigin += 1;
    }),
    {},
  );
  assert.equal(result.sameDocument, false);
  assert.deepEqual(calls.slice(-3), [["page.evaluate", undefined, TOGGLE_WAIT], ["page.evaluate", undefined, 1_000_002], ...CLOSE]);
});

test("a run is not the same document if the page has more than one navigation entry, though the time origin is the same", async () => {
  const { result, calls } = await run(
    thenIn((env) => {
      env.navigation = [NAVIGATION, NAVIGATION];
    }),
    {},
  );
  assert.equal(result.sameDocument, false);
  assert.deepEqual(calls.slice(-3), [["page.evaluate", undefined, 1_000_001], ["page.evaluate", undefined, 2], ...CLOSE]);
});

test("a target with no interactions is a load alone: nothing is armed, filled, pressed or clicked", async () => {
  const { result, calls } = await run(TODO_SCENE, { target: NO_STEPS });
  const visit = visitCalls(1_000_001, TIMINGS);
  assert.deepEqual(calls, [...open(SCRIPTS.dom), ...CDP_OPEN, ...CHROMIUM_LOG, ...visit.slice(0, 5), ...visit.slice(-2), ...CLOSE]);
  assert.deepEqual(result, { timings: TIMINGS, interactions: [], sameDocument: true, wallMs: 412, received: [DOC_CDP], requests: [DOC_ENTRY] });
});

// --- the log on its own: what measureOnce does not show of it ---------------------------------

const eventsOf = (calls) => calls.map(([, event]) => event);

test("the log is its entries, settled, setPhase, quiet and chromiumReceived; it listens to the context, and to a DevTools session only when it has one", () => {
  const [context, cdp, bare] = [new EventEmitter(), new EventEmitter(), new EventEmitter()];
  assert.deepEqual(Object.keys(networkLog(context, cdp)).sort(), ["chromiumReceived", "entries", "quiet", "setPhase", "settled"]);
  assert.deepEqual(context.eventNames(), eventsOf(CONTEXT_ON));
  assert.deepEqual(cdp.eventNames(), eventsOf(CDP_ON));
  const without = networkLog(bare, null);
  assert.deepEqual(bare.eventNames(), eventsOf(CONTEXT_ON));
  assert.deepEqual(without.chromiumReceived(), []);
});

test("quiet is true when nothing has started or ended for the quiet time, and takes that long to be", async () => {
  await withClock(async (clock) => {
    const log = networkLog(new EventEmitter(), null);
    const start = clock.t;
    assert.equal(await log.quiet(100, 1000), true);
    assert.equal(clock.t - start, 100);
  });
});

test("quiet is false when the cap comes first, a request under a second old keeping the network busy until then", async () => {
  await withClock(async (clock) => {
    const context = new EventEmitter();
    const log = networkLog(context, null);
    const start = clock.t;
    context.emit("request", fakeRequest({}).request);
    assert.equal(await log.quiet(100, 300), false);
    assert.equal(clock.t - start, 300);
  });
});

test("quiet is true at once when all that is in flight has been held for more than a second and nothing else happened for the quiet time", async () => {
  await withClock(async (clock) => {
    const context = new EventEmitter();
    const log = networkLog(context, null);
    context.emit("request", fakeRequest({}).request);
    clock.t += 1500;
    const before = clock.t;
    assert.equal(await log.quiet(400, 15_000), true);
    assert.equal(clock.t, before);
  });
});

test("the start of a request is activity: a quiet time of two seconds ends two seconds after it", async () => {
  await withClock(async (clock) => {
    const context = new EventEmitter();
    const log = networkLog(context, null);
    const start = clock.t;
    clock.t += 100;
    context.emit("request", fakeRequest({}).request);
    clock.t += 1500;
    assert.equal(await log.quiet(2000, 15_000), true);
    assert.equal(clock.t - start, 2100);
  });
});

test("settled resolves with every request's reading, whether or not reading one of them failed", async () => {
  await withClock(async () => {
    const context = new EventEmitter();
    const log = networkLog(context, null);
    const unreadable = fakeRequest({ url: new Error("no url") }).request;
    const fine = fakeRequest({ response: { status: 200, headers: {}, body: TINY } }).request;
    [unreadable, fine].forEach((request) => {
      context.emit("request", request);
      context.emit("requestfinished", request);
    });
    const outcomes = await log.settled();
    assert.deepEqual(
      outcomes.map((o) => o.status),
      ["rejected", "fulfilled"],
    );
    assert.deepEqual(
      log.entries.map((e) => e.status),
      [200],
    );
  });
});

test("what chromiumReceived returns is a copy: bytes that arrive later do not change an earlier answer", () => {
  const [context, cdp] = [new EventEmitter(), new EventEmitter()];
  const log = networkLog(context, cdp);
  cdp.emit("Network.requestWillBeSent", { requestId: "1", request: { url: STREAM } });
  cdp.emit("Network.dataReceived", { requestId: "1", encodedDataLength: 10 });
  const early = log.chromiumReceived();
  cdp.emit("Network.dataReceived", { requestId: "1", encodedDataLength: 5 });
  assert.deepEqual(
    early.map((c) => [c.received, c.bytes]),
    [[10, 10]],
  );
  assert.deepEqual(
    log.chromiumReceived().map((c) => [c.received, c.bytes]),
    [[15, 15]],
  );
});
