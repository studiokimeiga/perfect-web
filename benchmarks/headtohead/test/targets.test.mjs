// What the head-to-head measures (targets.mjs), pinned as it is: the two apps
// and the stacks each is run on, the server each stack is started with, the
// expectations a run holds each build and host to, the interactions a run
// makes, and what the environment (PORT, CARGO_TARGET_DIR) moves. Whole values
// throughout. The ports and the host's path come from the environment when
// targets.mjs is imported, so they are derived here the same way, and checked
// again in child processes whose environment this file chooses.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { REPO } from "../lib.mjs";
import { APPS, OFFSET, PORT, PW_SERVER, exists, target } from "../targets.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TARGETS = path.join(HERE, "../targets.mjs");

// --- where things are, as targets.mjs derives them ---------------------------------

const BASE = Number(process.env.PORT ?? 3141);
const slot = (k) => BASE + 220 + k;
const repo = (...parts) => path.join(REPO, ...parts);
const HOST_BINARY = path.join(process.env.CARGO_TARGET_DIR ?? repo("target"), "release/pw-dev-server");
const SPIKE = repo("spikes/own-renderer");
const VENDORED = repo("benchmarks/headtohead/todomvc");
const NEXT = repo("benchmarks/baselines/next-react");
const SVELTEKIT = repo("benchmarks/baselines/sveltekit");
const PLERIS_ENV = (k) => ({ PORT: String(slot(k)), PW_FEED_DATABASE_URL: "", PW_STORE_DATABASE_URL: "" });

// --- the apps and their interactions, as plain values --------------------------------

/** A value with its functions named, so that it can be compared whole. */
const plain = (value) => Object.fromEntries(Object.entries(value).map(([key, v]) => [key, typeof v === "function" ? "function" : v]));
const plainEntries = (values) => Object.fromEntries(Object.entries(values).map(([key, value]) => [key, plain(value)]));
const plainApp = ({ steps, stacks, ...rest }) => ({ ...rest, steps: steps.map(plain), stacks: plainEntries(stacks) });
const plainApps = (apps) => Object.fromEntries(Object.entries(apps).map(([name, app]) => [name, plainApp(app)]));

const TODOMVC_STEPS = [
  { name: "add", cond: "todos", arg: 1, run: "function" },
  { name: "toggle", cond: "completed", arg: 1, run: "function" },
  { name: "filter", cond: "filter", arg: "Completed", run: "function" },
];
const STORE_STEPS = [{ name: "add", cond: "count", arg: null, before: "function", run: "function" }];

/** Each stack of each app as `APPS` holds it, but for its `server`, a function. */
const STACKS = {
  "todomvc/pleris": {
    label: "Pleris",
    who: "this project (examples/todomvc, track todomvc)",
    version: "own renderer, this commit",
    path: "/",
    ready: "pw",
    minifies: false,
    compresses: false,
    caches: false,
    build: { kind: "pleris", dist: path.join(SPIKE, "dist-todomvc"), sources: path.join(SPIKE, "dist-todomvc/sources.json") },
  },
  "todomvc/react": {
    label: "React",
    who: "TodoMVC's maintainers (tastejs/todomvc@ff43b02e, examples/react)",
    version: "React 19.2.5, react-router-dom 7.14.2, webpack 5.106.2",
    path: "/react/dist/",
    ready: "dom",
    minifies: true,
    compresses: "all",
    caches: true,
    build: { kind: "vendored", root: VENDORED, prefix: "/react/dist/", dir: "react/dist" },
    standIn: true,
  },
  "todomvc/svelte": {
    label: "Svelte",
    who: "TodoMVC's maintainers (tastejs/todomvc@ff43b02e, examples/svelte)",
    version: "Svelte 5.55.5, Vite 8.0.10",
    path: "/svelte/dist/",
    ready: "dom",
    minifies: true,
    compresses: "all",
    caches: true,
    build: { kind: "vendored", root: VENDORED, prefix: "/svelte/dist/", dir: "svelte/dist" },
    standIn: true,
  },
  "store/pleris": {
    label: "Pleris",
    who: "this project (benchmarks/baselines/pleris, ADR-0156)",
    version: "own renderer, this commit",
    path: "/StorePage.html",
    ready: "pw",
    minifies: false,
    compresses: false,
    caches: false,
    build: { kind: "pleris", dist: path.join(SPIKE, "dist-baseline"), sources: path.join(SPIKE, "dist-baseline/sources.json") },
  },
  "store/next-react": {
    label: "Next.js",
    who: "this project (benchmarks/baselines/next-react, ADR-0120)",
    version: "Next.js 16.4.1, React 19.3.0",
    path: "/stores/blue-bottle",
    ready: "next",
    minifies: true,
    compresses: "all",
    caches: true,
    build: { kind: "next", dir: NEXT },
  },
  "store/sveltekit": {
    label: "SvelteKit",
    who: "this project (benchmarks/baselines/sveltekit, ADR-0120)",
    version: "SvelteKit 2.70.3, Svelte 5.57.1, adapter-node 5.5.7",
    path: "/stores/blue-bottle",
    ready: "sveltekit",
    minifies: true,
    compresses: "static",
    caches: true,
    build: { kind: "sveltekit", dir: SVELTEKIT },
  },
};

/** What each stack's `server()` makes, as `target` hands it to the run. */
const SERVERS = {
  "todomvc/pleris": { name: "pleris-todomvc", cmd: HOST_BINARY, args: ["dist-todomvc"], cwd: SPIKE, env: PLERIS_ENV(0), port: slot(0) },
  "todomvc/react": {
    name: "static-react",
    cmd: process.execPath,
    args: [repo("benchmarks/headtohead/static-host.mjs"), "--port", String(slot(1)), "--mount", `/react/dist=${VENDORED}/react/dist`],
    cwd: REPO,
    port: slot(1),
  },
  "todomvc/svelte": {
    name: "static-svelte",
    cmd: process.execPath,
    args: [repo("benchmarks/headtohead/static-host.mjs"), "--port", String(slot(2)), "--mount", `/svelte/dist=${VENDORED}/svelte/dist`],
    cwd: REPO,
    port: slot(2),
  },
  "store/pleris": { name: "pleris-store", cmd: HOST_BINARY, args: ["dist-baseline"], cwd: SPIKE, env: PLERIS_ENV(3), port: slot(3) },
  "store/next-react": {
    name: "next-store",
    cmd: process.execPath,
    args: [path.join(NEXT, "node_modules/next/dist/bin/next"), "start", "--port", String(slot(4)), "--hostname", "127.0.0.1"],
    cwd: NEXT,
    env: { NODE_ENV: "production" },
    port: slot(4),
  },
  "store/sveltekit": {
    name: "sveltekit-store",
    cmd: process.execPath,
    args: ["build"],
    cwd: SVELTEKIT,
    env: { PORT: String(slot(5)), HOST: "127.0.0.1", ORIGIN: `http://localhost:${slot(5)}`, NODE_ENV: "production" },
    port: slot(5),
  },
};

const APP_FACTS = {
  todomvc: { title: "TodoMVC", appSelector: ".new-todo", common: [/\/base\.js$/, /\/learn\.json$/], steps: TODOMVC_STEPS },
  store: { title: "the store (ADR-0120's three stores, one contract)", appSelector: "#menu", common: [], steps: STORE_STEPS },
};

/** The stacks of one app, in the order `APPS` lists them, as `plainApp` shows them. */
const stacksOf = (app) =>
  Object.fromEntries(
    Object.entries(STACKS)
      .filter(([key]) => key.startsWith(`${app}/`))
      .map(([key, facts]) => [key.slice(app.length + 1), { ...facts, server: "function" }]),
  );

/** The run description `target(app, stack)` must give. */
function expectedTarget(app, stack) {
  const key = `${app}/${stack}`;
  const facts = STACKS[key];
  const server = SERVERS[key];
  return {
    app,
    stack,
    label: facts.label,
    url: `http://localhost:${server.port}${facts.path}`,
    appSelector: APP_FACTS[app].appSelector,
    ready: facts.ready,
    interactions: APP_FACTS[app].steps,
    common: APP_FACTS[app].common,
    expect: { minifies: facts.minifies, compresses: facts.compresses, caches: facts.caches },
    standIn: facts.standIn === true,
    build: facts.build,
    server,
    who: facts.who,
    version: facts.version,
  };
}

const described = (run) => ({ ...run, interactions: run.interactions.map(plain) });

// --- APPS and target ---------------------------------------------------------------------

test("the head-to-head measures TodoMVC on Pleris, React and Svelte, and the store on Pleris, Next.js and SvelteKit, in that order", () => {
  assert.deepEqual(Object.keys(APPS), ["todomvc", "store"]);
  assert.deepEqual(Object.keys(APPS.todomvc.stacks), ["pleris", "react", "svelte"]);
  assert.deepEqual(Object.keys(APPS.store.stacks), ["pleris", "next-react", "sveltekit"]);
});

test("each app is held to its title, its page's selector, its common files, its interactions and its stacks' facts, exactly", () => {
  const expected = {
    todomvc: { ...APP_FACTS.todomvc, stacks: stacksOf("todomvc") },
    store: { ...APP_FACTS.store, stacks: stacksOf("store") },
  };
  assert.deepEqual(plainApps(APPS), expected);
});

test("TodoMVC's common files are base.js and learn.json, wherever they are served from, and the store has none", () => {
  const common = APPS.todomvc.common;
  const matches = (url) => common.map((pattern) => pattern.test(url));
  assert.deepEqual(matches("http://localhost:3362/react/dist/base.js"), [true, false]);
  assert.deepEqual(matches("http://localhost:3363/svelte/dist/learn.json"), [false, true]);
  assert.deepEqual(matches("http://localhost:3363/svelte/dist/assets/index-BxPr2-fK.js"), [false, false]);
  assert.deepEqual(matches("http://localhost:3363/svelte/dist/notbase.js"), [false, false]);
  assert.deepEqual(matches("http://localhost:3363/svelte/dist/base.js?v=1"), [false, false]);
  assert.deepEqual(APPS.store.common, []);
});

Object.keys(STACKS).forEach((key) => {
  const [app, stack] = key.split("/");
  test(`target gives ${app} on ${stack} the description of one run: its page, build, host, expectations and interactions`, () => {
    assert.deepEqual(described(target(app, stack)), expectedTarget(app, stack));
  });
});

test("target gives every page at localhost on the stack's own port, and every stack its own port", () => {
  const urls = Object.keys(STACKS).map((key) => target(...key.split("/")).url);
  assert.deepEqual(urls, [
    `http://localhost:${slot(0)}/`,
    `http://localhost:${slot(1)}/react/dist/`,
    `http://localhost:${slot(2)}/svelte/dist/`,
    `http://localhost:${slot(3)}/StorePage.html`,
    `http://localhost:${slot(4)}/stores/blue-bottle`,
    `http://localhost:${slot(5)}/stores/blue-bottle`,
  ]);
});

test("target refuses a stack the app does not run on, and an app it does not know", () => {
  assert.throws(() => target("todomvc", "next-react"), { name: "Error", message: "no stack next-react for todomvc" });
  assert.throws(() => target("store", "react"), { name: "Error", message: "no stack react for store" });
  assert.throws(() => target("todomvc", "angular"), { name: "Error", message: "no stack angular for todomvc" });
  assert.throws(() => target("blog", "react"), { name: "TypeError", message: "Cannot read properties of undefined (reading 'stacks')" });
});

test("exists says whether a path is there", () => {
  assert.equal(exists(HERE), true);
  assert.equal(exists(TARGETS), true);
  assert.equal(exists(path.join(HERE, "no-such-file")), false);
});

// --- the environment ------------------------------------------------------------------------

test("the ports are the PORT of the environment plus 220 and the stack's slot", () => {
  assert.equal(PORT, BASE);
  assert.equal(OFFSET, 220);
  assert.deepEqual(
    Object.keys(SERVERS).map((key) => target(...key.split("/")).server.port),
    [0, 1, 2, 3, 4, 5].map((k) => PORT + OFFSET + k),
  );
});

test("the Pleris host is the release build in CARGO_TARGET_DIR, or in the repository's target", () => {
  assert.equal(PW_SERVER, HOST_BINARY);
  assert.equal(target("todomvc", "pleris").server.cmd, PW_SERVER);
  assert.equal(target("store", "pleris").server.cmd, PW_SERVER);
});

const PAIRS = Object.keys(STACKS).map((key) => key.split("/"));
const PROBE = `
const m = await import(${JSON.stringify(pathToFileURL(TARGETS).href)});
const urls = ${JSON.stringify(PAIRS)}.map(([app, stack]) => m.target(app, stack).url);
console.log(JSON.stringify({ PORT: m.PORT, OFFSET: m.OFFSET, PW_SERVER: m.PW_SERVER, urls }));
`;

/** targets.mjs imported by a new process whose PORT and CARGO_TARGET_DIR are `env`'s, or unset. */
function importedWith(env) {
  const unset = { PORT: undefined, CARGO_TARGET_DIR: undefined };
  const run = spawnSync(process.execPath, ["--input-type=module", "-e", PROBE], { env: { ...process.env, ...unset, ...env }, encoding: "utf8", timeout: 20000 });
  assert.equal(run.status, 0, run.stderr);
  return JSON.parse(run.stdout);
}

const urlsFrom = (base) => [
  `http://localhost:${base + 220}/`,
  `http://localhost:${base + 221}/react/dist/`,
  `http://localhost:${base + 222}/svelte/dist/`,
  `http://localhost:${base + 223}/StorePage.html`,
  `http://localhost:${base + 224}/stores/blue-bottle`,
  `http://localhost:${base + 225}/stores/blue-bottle`,
];

test("with no PORT and no CARGO_TARGET_DIR the ports start at 3141 + 220 and the host is the repository's target/release/pw-dev-server", () => {
  assert.deepEqual(importedWith({}), {
    PORT: 3141,
    OFFSET: 220,
    PW_SERVER: repo("target/release/pw-dev-server"),
    urls: urlsFrom(3141),
  });
});

test("a PORT and a CARGO_TARGET_DIR move the ports and the host, read once when targets.mjs is imported", () => {
  assert.deepEqual(importedWith({ PORT: "4000", CARGO_TARGET_DIR: "/opt/cargo-target" }), {
    PORT: 4000,
    OFFSET: 220,
    PW_SERVER: "/opt/cargo-target/release/pw-dev-server",
    urls: urlsFrom(4000),
  });
});

test("an empty PORT is read as 0, and an empty or relative CARGO_TARGET_DIR is used as it is: only an unset variable gets the default", () => {
  assert.deepEqual(importedWith({ PORT: "", CARGO_TARGET_DIR: "" }), { PORT: 0, OFFSET: 220, PW_SERVER: "release/pw-dev-server", urls: urlsFrom(0) });
  assert.equal(importedWith({ CARGO_TARGET_DIR: "build/cargo" }).PW_SERVER, "build/cargo/release/pw-dev-server");
});

// --- the interactions ----------------------------------------------------------------------------

/** A page that records the calls made on it and answers each with a promise. */
function fakePage(texts = {}) {
  const calls = [];
  const act = (name) => (...args) => {
    calls.push([name, ...args]);
    return Promise.resolve(`${name} done`);
  };
  const textContent = (selector) => {
    calls.push(["textContent", selector]);
    return Promise.resolve(texts[selector]);
  };
  return { calls, fill: act("fill"), press: act("press"), click: act("click"), textContent };
}

const stepOf = (app, stack, name) => target(app, stack).interactions.find((step) => step.name === name);

test("TodoMVC's add types the todo into the new-todo field and then presses Enter in it", async () => {
  const page = fakePage();
  assert.equal(await stepOf("todomvc", "react", "add").run(page), "press done");
  assert.deepEqual(page.calls, [
    ["fill", ".new-todo", "h2h one"],
    ["press", ".new-todo", "Enter"],
  ]);
});

test("TodoMVC's add presses Enter only when the typing has finished", async () => {
  const typed = Promise.withResolvers();
  const page = fakePage();
  page.fill = (...args) => {
    page.calls.push(["fill", ...args]);
    return typed.promise;
  };
  const done = stepOf("todomvc", "svelte", "add").run(page);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(page.calls, [["fill", ".new-todo", "h2h one"]]);
  typed.resolve("typed");
  assert.equal(await done, "press done");
  assert.deepEqual(page.calls, [
    ["fill", ".new-todo", "h2h one"],
    ["press", ".new-todo", "Enter"],
  ]);
});

test("TodoMVC's toggle clicks the first todo's checkbox, and its filter clicks the Completed link", async () => {
  const toggled = fakePage();
  assert.equal(await stepOf("todomvc", "pleris", "toggle").run(toggled), "click done");
  assert.deepEqual(toggled.calls, [["click", ".todo-list li .toggle"]]);
  const filtered = fakePage();
  assert.equal(await stepOf("todomvc", "pleris", "filter").run(filtered), "click done");
  assert.deepEqual(filtered.calls, [["click", '.filters a[href="#/completed"]']]);
});

test("the store's add clicks the first menu item's button", async () => {
  for (const stack of ["pleris", "next-react", "sveltekit"]) {
    const page = fakePage();
    assert.equal(await stepOf("store", stack, "add").run(page), "click done");
    assert.deepEqual(page.calls, [["click", "#menu li:first-child button"]]);
  }
});

test("the store's add expects the cart count to be one above what the page shows, read as a trimmed number", async () => {
  const shown = ["0", "41", " 7\n", "", "many"];
  const expected = ["1", "42", "8", "1", "NaN"];
  const pages = shown.map((text) => fakePage({ "#cart-count": text }));
  const step = stepOf("store", "pleris", "add");
  assert.deepEqual(await Promise.all(pages.map((page) => step.before(page))), expected);
  assert.deepEqual(pages[0].calls, [["textContent", "#cart-count"]]);
});
