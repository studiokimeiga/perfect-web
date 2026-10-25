// What the head-to-head measures (ADR-XXXX, "the head-to-head"): each app on
// each stack, how it is built and served, how it says it is interactive, the
// interactions a run makes, and what each stack's build is held to.
//
// Ports: the brief's `PORT` (13441 for track `head-to-head`) plus 220-249, a
// gap the own renderer's hosts leave (their offsets run to 212 of a stride of
// 256, `spikes/own-renderer/host-registry.json`), so that the head-to-head
// can run beside them.

import fs from "node:fs";
import path from "node:path";
import { REPO } from "./lib.mjs";

export const PORT = Number(process.env.PORT ?? 3141);
export const OFFSET = 220;
const port = (k) => PORT + OFFSET + k;

const SPIKE = path.join(REPO, "spikes/own-renderer");
const VENDORED = path.join(REPO, "benchmarks/headtohead/todomvc");
// The Pleris host as a production build: `cargo build --release`, as the
// frameworks' servers are their production builds (`next start`, `node
// build`). Its development build, `target/debug`, is the one the test suites
// run, and is not measured.
export const PW_SERVER = path.join(process.env.CARGO_TARGET_DIR ?? path.join(REPO, "target"), "release/pw-dev-server");
const STATIC_HOST = path.join(REPO, "benchmarks/headtohead/static-host.mjs");
const NEXT = path.join(REPO, "benchmarks/baselines/next-react");
const SVELTEKIT = path.join(REPO, "benchmarks/baselines/sveltekit");

const add = (page) =>
  page.fill(".new-todo", "h2h one").then(() => page.press(".new-todo", "Enter"));

/** TodoMVC's interactions, each a person's: add a todo, complete it, show
 * the completed ones. */
const TODOMVC_STEPS = [
  { name: "add", cond: "todos", arg: 1, run: add },
  { name: "toggle", cond: "completed", arg: 1, run: (page) => page.click(".todo-list li .toggle") },
  {
    name: "filter",
    cond: "filter",
    arg: "Completed",
    run: (page) => page.click('.filters a[href="#/completed"]'),
  },
];

/** The store's: one Add, whose count the server's answer moves. */
const STORE_STEPS = [
  {
    name: "add",
    cond: "count",
    // The count after one add: one more than the page shows (a warm visit's
    // session already holds the visit before it).
    arg: null,
    before: async (page) => String(Number((await page.textContent("#cart-count")).trim()) + 1),
    run: (page) => page.click("#menu li:first-child button"),
  },
];

/**
 * Each stack's expectations of its own build, which a run is held to
 * (`validate.mjs`): a build that minifies must serve minified scripts, and a
 * host that compresses must send compressed text. What a stack does not do
 * is not refused: it is reported, a loss in its row.
 */
export const APPS = {
  todomvc: {
    title: "TodoMVC",
    appSelector: ".new-todo",
    steps: TODOMVC_STEPS,
    // TodoMVC's common files, which its examples load for their shared
    // sidebar and TodoMVC in Pleris does not: reported, and left out of the
    // "framework and app" totals for every stack alike.
    common: [/\/base\.js$/, /\/learn\.json$/],
    stacks: {
      pleris: {
        label: "Pleris",
        who: "this project (examples/todomvc, track todomvc)",
        version: "own renderer, this commit",
        server: () => ({
          name: "pleris-todomvc",
          cmd: PW_SERVER,
          args: ["dist-todomvc"],
          cwd: SPIKE,
          env: { PORT: String(port(0)), PW_FEED_DATABASE_URL: "", PW_STORE_DATABASE_URL: "" },
          port: port(0),
        }),
        path: "/",
        ready: "pw",
        minifies: false,
        compresses: false,
        caches: false,
        build: { kind: "pleris", dist: path.join(SPIKE, "dist-todomvc"), sources: path.join(SPIKE, "dist-todomvc/sources.json") },
      },
      react: {
        label: "React",
        who: "TodoMVC's maintainers (tastejs/todomvc@ff43b02e, examples/react)",
        version: "React 19.2.5, react-router-dom 7.14.2, webpack 5.106.2",
        server: () => ({
          name: "static-react",
          cmd: process.execPath,
          args: [STATIC_HOST, "--port", String(port(1)), "--mount", `/react/dist=${path.join(VENDORED, "react/dist")}`],
          cwd: REPO,
          port: port(1),
        }),
        path: "/react/dist/",
        ready: "dom",
        minifies: true,
        compresses: "all",
        caches: true,
        build: { kind: "vendored", root: VENDORED, prefix: "/react/dist/", dir: "react/dist" },
      },
      svelte: {
        label: "Svelte",
        who: "TodoMVC's maintainers (tastejs/todomvc@ff43b02e, examples/svelte)",
        version: "Svelte 5.55.5, Vite 8.0.10",
        server: () => ({
          name: "static-svelte",
          cmd: process.execPath,
          args: [STATIC_HOST, "--port", String(port(2)), "--mount", `/svelte/dist=${path.join(VENDORED, "svelte/dist")}`],
          cwd: REPO,
          port: port(2),
        }),
        path: "/svelte/dist/",
        ready: "dom",
        minifies: true,
        compresses: "all",
        caches: true,
        build: { kind: "vendored", root: VENDORED, prefix: "/svelte/dist/", dir: "svelte/dist" },
      },
    },
  },
  store: {
    title: "the store (ADR-0120's three stores, one contract)",
    appSelector: "#menu",
    steps: STORE_STEPS,
    common: [],
    stacks: {
      pleris: {
        label: "Pleris",
        who: "this project (benchmarks/baselines/pleris, ADR-0156)",
        version: "own renderer, this commit",
        server: () => ({
          name: "pleris-store",
          cmd: PW_SERVER,
          args: ["dist-baseline"],
          cwd: SPIKE,
          env: { PORT: String(port(3)), PW_FEED_DATABASE_URL: "", PW_STORE_DATABASE_URL: "" },
          port: port(3),
        }),
        path: "/StorePage.html",
        ready: "pw",
        minifies: false,
        compresses: false,
        caches: false,
        build: { kind: "pleris", dist: path.join(SPIKE, "dist-baseline"), sources: path.join(SPIKE, "dist-baseline/sources.json") },
      },
      "next-react": {
        label: "Next.js",
        who: "this project (benchmarks/baselines/next-react, ADR-0120)",
        version: "Next.js 16.4.1, React 19.3.0",
        server: () => ({
          name: "next-store",
          cmd: process.execPath,
          args: [path.join(NEXT, "node_modules/next/dist/bin/next"), "start", "--port", String(port(4)), "--hostname", "127.0.0.1"],
          cwd: NEXT,
          env: { NODE_ENV: "production" },
          port: port(4),
        }),
        path: "/stores/blue-bottle",
        ready: "next",
        minifies: true,
        // `next start` compresses every answer (`compress` is on by default).
        compresses: "all",
        caches: true,
        build: { kind: "next", dir: NEXT },
      },
      sveltekit: {
        label: "SvelteKit",
        who: "this project (benchmarks/baselines/sveltekit, ADR-0120)",
        version: "SvelteKit 2.70.3, Svelte 5.57.1, adapter-node 5.5.7",
        server: () => ({
          name: "sveltekit-store",
          cmd: process.execPath,
          args: ["build"],
          cwd: SVELTEKIT,
          env: { PORT: String(port(5)), HOST: "127.0.0.1", ORIGIN: `http://127.0.0.1:${port(5)}`, NODE_ENV: "production" },
          port: port(5),
        }),
        path: "/stores/blue-bottle",
        ready: "sveltekit",
        minifies: true,
        // adapter-node precompresses its client assets and sirv sends them
        // (`precompress` is on by default); its server-rendered pages go out
        // as they are.
        compresses: "static",
        caches: true,
        build: { kind: "sveltekit", dir: SVELTEKIT },
      },
    },
  },
};

/** One app on one stack, as a run needs it. */
export function target(app, stack) {
  const a = APPS[app];
  const s = a.stacks[stack];
  if (!s) throw new Error(`no stack ${stack} for ${app}`);
  const srv = s.server();
  return {
    app,
    stack,
    label: s.label,
    url: `http://127.0.0.1:${srv.port}${s.path}`,
    appSelector: a.appSelector,
    ready: s.ready,
    interactions: a.steps,
    common: a.common,
    expect: { minifies: s.minifies, compresses: s.compresses, caches: s.caches },
    build: s.build,
    server: srv,
    who: s.who,
    version: s.version,
  };
}

export function exists(p) {
  return fs.existsSync(p);
}
