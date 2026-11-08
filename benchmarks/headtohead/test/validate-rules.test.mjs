// The rules of validate.mjs, pinned exactly. `validate.test.mjs` shows that each
// rule refuses the run it exists to refuse, by a pattern of its message; this
// file asserts what each rule returns, whole: the list of problems, in its
// order, deduplicated as the code deduplicates, every message in its exact
// words, for each branch of every rule and of the way `validate` joins them.
// A change to validate.mjs that changes what a run is refused for, or how it is
// told, fails here.
//
// The inputs are written by hand where no run reaches a branch (run records;
// files in memory through checkBuild's injectable readFile / exists / mtime;
// trees in temporary directories, made and removed here), and recorded where
// one does. `fixtures/validate/recorded-todomvc.json` is the kept run of record
// (`just e14-head-to-head` at dbd37a8c, 2026-11-02, the recipe's runs.json)
// made smaller: of the app todomvc, the runs 0-4 of the stacks react and svelte
// (the two whose vendored builds are in the repository, so that checkBuild reads
// them here as it does on the machine that measured) in each of the sixteen
// cells (three engines, cold and warm, and Chromium shaped to Slow 4G, cold and
// warm), and of each run only what validate.mjs reads: app, stack, engine,
// condition, network, i, sameDocument, timings.documentEnd and .ready, each
// interaction's name, toDom and toFrame, each request's phase, url, kind, status,
// type, encoding, raw, blob, minified and receivedBody; the profile is the
// record's meta.network; the two stacks' ports are rewritten from the record's
// (13662, 13663) to the default PORT's (3362, 3363). validate() of it, with the
// targets of targets.mjs and the vendored upstream.json, is an empty list, and
// a copy broken in each way the rules exist for is refused with exactly the
// problems below.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { target } from "../targets.mjs";
import {
  COMPRESSIBLE,
  MIN_RUNS,
  SCRIPT_OR_STYLE,
  checkBuild,
  checkCompressed,
  checkInteractive,
  checkMinified,
  checkRuns,
  checkShaped,
  checkWarm,
  validate,
  validateServer,
} from "../validate.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RECORD = JSON.parse(fs.readFileSync(path.join(HERE, "fixtures/validate/recorded-todomvc.json"), "utf8"));
const UPSTREAM_JSON = JSON.parse(fs.readFileSync(path.join(HERE, "../todomvc/upstream.json"), "utf8"));

// --- records, and tables of cases ----------------------------------------------------

const HOST = "http://localhost:3362";
const BUNDLE = `${HOST}/react/dist/app.bundle.js`;
const STYLE = `${HOST}/react/dist/app.css`;
const CHUNK = `${HOST}/react/dist/chunk.js`;
const CELL = "todomvc/react/chromium/cold/none";
const WRONG = "f".repeat(40);

/** A run, with the fields the rules read; `over` replaces any of them. */
const run = (over = {}) => ({
  app: "todomvc",
  stack: "react",
  engine: "chromium",
  condition: "cold",
  network: null,
  i: 0,
  sameDocument: true,
  timings: { documentEnd: 3, ready: 40 },
  interactions: [{ name: "add", toDom: 2, toFrame: 5 }],
  requests: [],
  ...over,
});

/** A response of a run (a script of 5000 bytes, Brotli-compressed, minified). */
const res = (over = {}) => ({
  phase: "load",
  url: BUNDLE,
  kind: "script",
  status: 200,
  type: "text/javascript",
  raw: 5000,
  encoding: "br",
  minified: true,
  blob: "b".repeat(40),
  receivedBody: 1000,
  ...over,
});

const cell = (n, over = {}) => Array.from({ length: n }, (_, i) => run({ i, ...over }));
const oneRun = (...requests) => [run({ requests })];

/** Each row [label, arguments, wanted]: the rule called with the arguments returns exactly the wanted list. */
const table = (rule, rows) => rows.forEach(([label, args, want]) => assert.deepEqual(rule(...args), want, label));

const minifiedMsg = (url) => `minified: ${url} is not minified, and this stack's build minifies`;
const uncompressedMsg = (url, raw, type) => `compressed: ${url} (${raw} bytes of ${type}) was sent uncompressed`;

/** The same defective response in runs that differ in one way: [label, the run's change, the response's change, whether the rule reads it]. */
const SELECTION = [
  ["a cold Chromium run", {}, {}, true],
  ["a cold Firefox run", { engine: "firefox" }, {}, true],
  ["a cold WebKit run", { engine: "webkit" }, {}, true],
  ["a warm run", { condition: "warm" }, {}, false],
  ["a cold run shaped to a profile", { network: "slow-4g" }, {}, false],
  ["the load phase", {}, { phase: "load" }, true],
  ["an interaction's phase", {}, { phase: "interaction:add" }, true],
  ["the phase interaction alone", {}, { phase: "interaction" }, true],
  ["a phase that begins with interaction", {}, { phase: "interactions" }, true],
  ["another phase", {}, { phase: "idle" }, false],
  ["a phase that begins like interaction", {}, { phase: "interact" }, false],
  ["the load phase in capitals", {}, { phase: "LOAD" }, false],
];

/** A rule reads the cold unshaped runs' load and interaction responses, of every engine, and no others. */
const readsSelection = (rule, defect, message) =>
  SELECTION.forEach(([label, runOver, resOver, read]) => {
    const runs = [run({ ...runOver, requests: [res({ ...defect, ...resOver })] })];
    assert.deepEqual(rule(runs), read ? [message] : [], label);
  });

/** Three defective responses, a script, a style and another script. */
const trio = (defect) => [
  res(defect),
  res({ url: STYLE, type: "text/css", kind: "stylesheet", raw: 7400, ...defect }),
  res({ url: CHUNK, type: "application/javascript", raw: 2048, ...defect }),
];

/** A rule tells each defective response once, in the order it was first seen. */
function readsOnceInOrder(rule, defect, [first, second, third]) {
  const [a, b, c] = trio(defect);
  assert.deepEqual(rule([run({ requests: [c, a, b] })]), [third, first, second], "in the order of the responses");
  const runs = [run({ requests: [a] }), run({ i: 1, engine: "firefox", requests: [a, b] }), run({ i: 2, engine: "webkit", requests: [b, c, a] })];
  assert.deepEqual(rule(runs), [first, second, third], "once each across runs, the first seen first");
}

// --- the patterns -------------------------------------------------------------------

const COMPRESSIBLE_TYPES = [
  ["text/html", true],
  ["text/css", true],
  ["text/plain", true],
  ["text/javascript", true],
  ["application/javascript", true],
  ["application/json", true],
  ["application/wasm", true],
  ["image/svg+xml", true],
  ["application/json; charset=utf-8", true],
  ["image/svg", false],
  ["image/png", false],
  ["application/octet-stream", false],
  ["font/woff2", false],
  ["application/pdf", false],
  ["application/x-javascript", false],
  ["application/xml", false],
  ["video/mp4", false],
  ["TEXT/HTML", false],
  ["xtext/html", false],
  ["", false],
];

const SCRIPT_OR_STYLE_TYPES = [
  ["text/javascript", true],
  ["application/javascript", true],
  ["text/css", true],
  ["text/javascript; charset=utf-8", false],
  ["application/x-javascript", false],
  ["text/ecmascript", false],
  ["text/html", false],
  ["application/json", false],
  ["text/CSS", false],
  [" text/css", false],
  ["text/css ", false],
  ["", false],
];

test("MIN_RUNS is five, and COMPRESSIBLE and SCRIPT_OR_STYLE accept exactly the media types they name", () => {
  assert.equal(MIN_RUNS, 5);
  COMPRESSIBLE_TYPES.forEach(([type, want]) => assert.equal(COMPRESSIBLE.test(type), want, `COMPRESSIBLE on ${type}`));
  SCRIPT_OR_STYLE_TYPES.forEach(([type, want]) => assert.equal(SCRIPT_OR_STYLE.test(type), want, `SCRIPT_OR_STYLE on ${type}`));
});

// --- checkRuns ----------------------------------------------------------------------

test("checkRuns names each cell below the minimum with its key and count, in the order the cells first appear", () => {
  const interleaved = [run({ stack: "svelte" }), run(), run({ stack: "svelte", i: 1 }), run({ i: 1 })];
  table(checkRuns, [
    ["a cell of one run", [cell(1)], [`runs: ${CELL} has 1 run(s), fewer than 5`]],
    ["a cell of four runs", [cell(4)], [`runs: ${CELL} has 4 run(s), fewer than 5`]],
    ["a cell of the minimum", [cell(5)], []],
    ["a cell above the minimum", [cell(6)], []],
    [
      "two cells below it, the one first seen first",
      [interleaved],
      ["runs: todomvc/svelte/chromium/cold/none has 2 run(s), fewer than 5", `runs: ${CELL} has 2 run(s), fewer than 5`],
    ],
  ]);
});

test("checkRuns holds the minimum it is given, and five when it is given none", () => {
  table(checkRuns, [
    ["two runs, a minimum of two", [cell(2), 2], []],
    ["two runs, a minimum of three", [cell(2), 3], [`runs: ${CELL} has 2 run(s), fewer than 3`]],
    ["one run, a minimum of zero", [cell(1), 0], []],
    ["four runs, the minimum undefined", [cell(4), undefined], [`runs: ${CELL} has 4 run(s), fewer than 5`]],
  ]);
});

test("checkRuns names the run counts that differ, sorted as text, after the cells below the minimum", () => {
  const svelte = (n) => cell(n, { stack: "svelte" });
  table(checkRuns, [
    ["seven and five", [[...cell(7), ...svelte(5)]], ["runs: cells have different run counts (5, 7)"]],
    ["three counts", [[...cell(7), ...svelte(5), ...cell(6, { engine: "firefox" })]], ["runs: cells have different run counts (5, 6, 7)"]],
    // Array.prototype.sort compares the counts as text: "10" comes before "5".
    ["ten and five", [[...cell(10), ...svelte(5)]], ["runs: cells have different run counts (10, 5)"]],
    [
      "one cell below the minimum and different",
      [[...cell(5), ...svelte(3)]],
      ["runs: todomvc/svelte/chromium/cold/none has 3 run(s), fewer than 5", "runs: cells have different run counts (3, 5)"],
    ],
    [
      "two cells below the minimum, of one count",
      [[...cell(3), ...svelte(3)]],
      [`runs: ${CELL} has 3 run(s), fewer than 5`, "runs: todomvc/svelte/chromium/cold/none has 3 run(s), fewer than 5"],
    ],
    ["cells of one count", [[...cell(7), ...svelte(7)]], []],
  ]);
});

test("checkRuns tells cells apart by app, stack, engine, condition and network, a missing network being none", () => {
  const apart = [run(), run({ app: "store" }), run({ stack: "svelte" }), run({ engine: "firefox" }), run({ condition: "warm" }), run({ network: "slow-4g" })];
  table(checkRuns, [
    [
      "six cells of one run each",
      [apart, 2],
      [
        `runs: ${CELL} has 1 run(s), fewer than 2`,
        "runs: store/react/chromium/cold/none has 1 run(s), fewer than 2",
        "runs: todomvc/svelte/chromium/cold/none has 1 run(s), fewer than 2",
        "runs: todomvc/react/firefox/cold/none has 1 run(s), fewer than 2",
        "runs: todomvc/react/chromium/warm/none has 1 run(s), fewer than 2",
        "runs: todomvc/react/chromium/cold/slow-4g has 1 run(s), fewer than 2",
      ],
    ],
    ["a network that is null or undefined is none", [[run({ network: null }), run({ network: undefined, i: 1 })], 2], []],
    ["an empty network name is a name", [[run({ network: "" })], 2], ["runs: todomvc/react/chromium/cold/ has 1 run(s), fewer than 2"]],
  ]);
});

test("checkRuns refuses a record with no run, whatever the minimum", () => {
  table(checkRuns, [
    ["no run", [[]], ["runs: no run was made"]],
    ["no run, a minimum of zero", [[], 0], ["runs: no run was made"]],
  ]);
});

// --- checkMinified ------------------------------------------------------------------

const MINIFIES = { minifies: true };
const unminified = (over = {}) => res({ minified: false, ...over });

test("checkMinified refuses each script or style shown unminified where the build minifies, once per URL, in the order first seen", () => {
  const style = unminified({ url: STYLE, type: "text/css", kind: "stylesheet" });
  const chunk = unminified({ url: CHUNK, type: "application/javascript" });
  table(checkMinified, [
    ["a script", [oneRun(unminified()), MINIFIES], [minifiedMsg(BUNDLE)]],
    ["a style", [oneRun(style), MINIFIES], [minifiedMsg(STYLE)]],
    ["an application/javascript script", [oneRun(chunk), MINIFIES], [minifiedMsg(CHUNK)]],
    ["a minified script", [oneRun(res()), MINIFIES], []],
    ["a run with no response", [[run()], MINIFIES], []],
    ["no run", [[], MINIFIES], []],
  ]);
  readsOnceInOrder((runs) => checkMinified(runs, MINIFIES), { minified: false }, [minifiedMsg(BUNDLE), minifiedMsg(STYLE), minifiedMsg(CHUNK)]);
});

test("checkMinified leaves out a response with no type, another type or no verdict, however unminified it looks", () => {
  const strange = [undefined, null, "", "text/html", "application/json", "image/svg+xml"];
  strange.push("text/javascript; charset=utf-8", "application/x-javascript", "TEXT/JAVASCRIPT");
  strange.forEach((type) => assert.deepEqual(checkMinified(oneRun(unminified({ type })), MINIFIES), [], `type ${type}`));
  [null, undefined].forEach((minified) => assert.deepEqual(checkMinified(oneRun(res({ minified })), MINIFIES), [], `verdict ${minified}`));
  const judged = unminified({ url: STYLE, type: "text/css", kind: "stylesheet" });
  assert.deepEqual(checkMinified(oneRun(res({ minified: null }), judged), MINIFIES), [minifiedMsg(STYLE)]);
});

test("checkMinified asks nothing of a build that does not minify, and leaves out common files by their pathname", () => {
  const base = `${HOST}/svelte/dist/base.js`;
  const runs = oneRun(unminified({ url: base }));
  table(checkMinified, [
    ["a build that does not minify", [runs, { minifies: false }], []],
    ["an expectation with no minifies", [runs, {}], []],
    ["no common file given", [runs, MINIFIES], [minifiedMsg(base)]],
    ["an empty list of common files", [runs, MINIFIES, []], [minifiedMsg(base)]],
    ["a pattern of the file", [runs, MINIFIES, [/\/base\.js$/]], []],
    ["a pattern of the file, the second of two", [runs, MINIFIES, [/\/learn\.json$/, /\/base\.js$/]], []],
    ["a pattern of another file", [runs, MINIFIES, [/\/learn\.json$/]], [minifiedMsg(base)]],
    ["a pattern of the host, which is no part of the pathname", [runs, MINIFIES, [/localhost/]], [minifiedMsg(base)]],
    ["a file with a query, which is no part of the pathname", [oneRun(unminified({ url: `${base}?v=2` })), MINIFIES, [/\/base\.js$/]], []],
  ]);
});

test("checkMinified reads the load and interaction responses of cold unshaped runs in every engine, and no others", () => {
  readsSelection((runs) => checkMinified(runs, MINIFIES), { minified: false }, minifiedMsg(BUNDLE));
  const mixed = [
    run({ condition: "warm", requests: [unminified({ url: `${HOST}/react/dist/a.js` })] }),
    run({ i: 1, network: "slow-4g", requests: [unminified({ url: `${HOST}/react/dist/b.js` })] }),
    run({ i: 2, requests: [unminified({ url: `${HOST}/react/dist/c.js` })] }),
    { ...run({ i: 3 }), requests: undefined },
  ];
  assert.deepEqual(checkMinified(mixed, MINIFIES), [minifiedMsg(`${HOST}/react/dist/c.js`)]);
});

// --- checkCompressed ----------------------------------------------------------------

const ALL = { compresses: "all" };
const plain = (over = {}) => res({ encoding: null, ...over });
const BUNDLE_MSG = uncompressedMsg(BUNDLE, 5000, "text/javascript");

test("checkCompressed asks nothing of a host that does not compress", () => {
  [false, undefined, null, 0, ""].forEach((compresses) => assert.deepEqual(checkCompressed(oneRun(plain()), { compresses }), [], `compresses ${compresses}`));
  assert.deepEqual(checkCompressed(oneRun(plain()), {}), []);
});

test("checkCompressed refuses an uncompressed 200 answer of 1 KiB or more of a compressible type, naming its size and type", () => {
  const rows = [
    ["a size of 1024 bytes", plain({ raw: 1024 }), [uncompressedMsg(BUNDLE, 1024, "text/javascript")]],
    ["a size of 1023 bytes", plain({ raw: 1023 }), []],
    ["no size", plain({ raw: undefined }), []],
    ["a size of null", plain({ raw: null }), []],
    ["a size of zero", plain({ raw: 0 }), []],
    ["a 304", plain({ status: 304 }), []],
    ["a 404", plain({ status: 404 }), []],
    ["a 206", plain({ status: 206 }), []],
    ["no status", plain({ status: undefined }), []],
    ["no type", plain({ type: undefined }), []],
    ["an empty type", plain({ type: "" }), []],
  ];
  rows.forEach(([label, response, want]) => assert.deepEqual(checkCompressed(oneRun(response), ALL), want, label));
  assert.deepEqual(checkCompressed(oneRun(plain()), ALL), [BUNDLE_MSG]);
  COMPRESSIBLE_TYPES.forEach(([type, compressible]) => {
    const want = compressible && type !== "" ? [uncompressedMsg(BUNDLE, 5000, type)] : [];
    assert.deepEqual(checkCompressed(oneRun(plain({ type })), ALL), want, `type ${type}`);
  });
});

test("checkCompressed holds a document to compression unless the host compresses only its static files", () => {
  const document = plain({ kind: "document", type: "text/html", url: `${HOST}/react/dist/`, raw: 4096 });
  const documentMsg = uncompressedMsg(`${HOST}/react/dist/`, 4096, "text/html");
  table(checkCompressed, [
    ["a document, a host that compresses all", [oneRun(document), ALL], [documentMsg]],
    ["a document, a host that compresses only static files", [oneRun(document), { compresses: "static" }], []],
    ["a document, a host that compresses (true)", [oneRun(document), { compresses: true }], [documentMsg]],
    ["a script, a host that compresses only static files", [oneRun(plain()), { compresses: "static" }], [BUNDLE_MSG]],
    ["a fetch, a host that compresses only static files", [oneRun(plain({ kind: "fetch" })), { compresses: "static" }], [BUNDLE_MSG]],
  ]);
});

test("checkCompressed takes any content encoding as compression, identity included, and none or an empty one as not", () => {
  ["br", "gzip", "deflate", "zstd", "identity"].forEach((encoding) => assert.deepEqual(checkCompressed(oneRun(res({ encoding })), ALL), [], `encoding ${encoding}`));
  [null, undefined, ""].forEach((encoding) => assert.deepEqual(checkCompressed(oneRun(res({ encoding })), ALL), [BUNDLE_MSG], `encoding ${encoding}`));
});

test("checkCompressed reads the load and interaction responses of cold unshaped runs in every engine, once per URL, in the order first seen", () => {
  readsSelection((runs) => checkCompressed(runs, ALL), { encoding: null }, BUNDLE_MSG);
  readsOnceInOrder((runs) => checkCompressed(runs, ALL), { encoding: null }, [
    BUNDLE_MSG,
    uncompressedMsg(STYLE, 7400, "text/css"),
    uncompressedMsg(CHUNK, 2048, "application/javascript"),
  ]);
});

// --- checkBuild: the files, in memory and on disk ------------------------------------

const known = (entries, name) => {
  assert.ok(Object.hasOwn(entries, name), `the build check reached for ${name}, which the test did not offer`);
  return entries[name];
};
const decode = (bytes, encoding) => (encoding ? bytes.toString(encoding) : bytes);

/** checkBuild's readFile, exists and mtime over files and times in memory; any other name asked for fails the test. */
const memory = (files = {}, times = {}) => ({
  exists: (name) => Object.hasOwn(files, name),
  readFile: (name, encoding) => decode(Buffer.from(known(files, name)), encoding),
  mtime: (name) => known(times, name),
});
const omit = (files, ...names) => Object.fromEntries(Object.entries(files).filter(([name]) => !names.includes(name)));

/** A response of the build's server: the pathname asked for, the blob of what came. */
const reply = (pathname, blob, over = {}) => res({ url: `${HOST}${pathname}`, blob, ...over });

/** Runs `body` with a directory of its own, removed after. */
function inTempDir(body) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-rules-"));
  try {
    return body(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}
const writeTree = (dir, files) =>
  Object.entries(files).forEach(([name, body]) => {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), body);
  });
const stamp = (dir, name, seconds) => fs.utimesSync(path.join(dir, name), seconds, seconds);

// git hash-object of the bodies below
const HTML_BLOB = "c50eddd41faba2ecc8928e459288fe612b999170"; // <!doctype html>
const JS_BLOB = "e14c4f2a9561ade31986f9319d5c639094509905"; // console.log(1)
const CHANGED_BLOB = "32926e25ddb39cdb2fc3013d482472c07d735c7b"; // console.log(2)
const README_BLOB = "7c84729a4ebac07cc686de852cd846abe7897ec1"; // # TodoMVC in React

const VENDORED = { kind: "vendored", root: "/srv/todomvc", prefix: "/react/dist/", dir: "react/dist" };
const COPY = {
  "/srv/todomvc/react/dist/index.html": "<!doctype html>\n",
  "/srv/todomvc/react/dist/app.bundle.js": "console.log(1)\n",
  "/srv/todomvc/react/readme.md": "# TodoMVC in React\n",
};
const UPSTREAM = {
  files: {
    "react/dist/index.html": { blob: HTML_BLOB },
    "react/dist/app.bundle.js": { blob: JS_BLOB },
    "react/readme.md": { blob: README_BLOB },
  },
};
/** checkBuild's options for a vendored build: the copy on disk and upstream's manifest. */
const vendoredOptions = (disk = COPY, manifest = UPSTREAM) => ({ manifest, ...memory(disk) });
const NO_MATCH = "build: no served file was matched to the vendored build";

test("checkBuild accepts a vendored build served and kept as upstream's, the prefix itself being its index.html", () => {
  table(checkBuild, [
    ["the document at the prefix and a script", [VENDORED, [reply("/react/dist/", HTML_BLOB), reply("/react/dist/app.bundle.js", JS_BLOB)], vendoredOptions()], []],
    ["a script asked for with a query", [VENDORED, [reply("/react/dist/app.bundle.js?v=3", JS_BLOB)], vendoredOptions()], []],
    ["a script served twice", [VENDORED, [reply("/react/dist/app.bundle.js", JS_BLOB), reply("/react/dist/app.bundle.js", JS_BLOB)], vendoredOptions()], []],
  ]);
});

test("checkBuild does not judge, nor count as matched, a response outside the prefix, not a 200 or without a blob", () => {
  const unjudged = [
    reply("/svelte/dist/app.bundle.js", WRONG),
    reply("/react/dist", WRONG),
    res({ url: "data:image/svg+xml;utf8,%3Csvg%3E", blob: WRONG }),
    reply("/react/dist/missing.js", WRONG, { status: 404 }),
    reply("/react/dist/missing.js", WRONG, { status: 304 }),
    reply("/react/dist/app.bundle.js", null),
    reply("/react/dist/app.bundle.js", undefined),
  ];
  table(checkBuild, [
    ["only such responses", [VENDORED, unjudged, vendoredOptions()], [NO_MATCH]],
    ["such responses and a matched one", [VENDORED, [...unjudged, reply("/react/dist/", HTML_BLOB)], vendoredOptions()], []],
    ["no response at all", [VENDORED, [], vendoredOptions()], [NO_MATCH]],
  ]);
});

test("checkBuild names a served file the manifest does not list, by its pathname and the name it looked up", () => {
  const served = [reply("/react/dist/extra.js", WRONG), reply("/react/dist/assets/deep/x.css", WRONG), reply("/react/dist/app.bundle.js", JS_BLOB)];
  const withoutIndex = { files: omit(UPSTREAM.files, "react/dist/index.html") };
  table(checkBuild, [
    [
      "two files the manifest lacks",
      [VENDORED, served, vendoredOptions()],
      [
        "build: /react/dist/extra.js is not a file of the vendored build (react/dist/extra.js)",
        "build: /react/dist/assets/deep/x.css is not a file of the vendored build (react/dist/assets/deep/x.css)",
      ],
    ],
    [
      "the document of a manifest that lacks index.html",
      [VENDORED, [reply("/react/dist/", HTML_BLOB), reply("/react/dist/app.bundle.js", JS_BLOB)], vendoredOptions(COPY, withoutIndex)],
      ["build: /react/dist/ is not a file of the vendored build (react/dist/index.html)"],
    ],
  ]);
});

test("checkBuild names a served file whose blob is not upstream's with both blobs, once however many runs served it", () => {
  const wrongBundle = reply("/react/dist/app.bundle.js", WRONG);
  const wrongBundleMsg = `build: /react/dist/app.bundle.js is not upstream's react/dist/app.bundle.js (blob ${WRONG}, upstream ${JS_BLOB})`;
  table(checkBuild, [
    ["a script served three times", [VENDORED, [wrongBundle, reply("/react/dist/", HTML_BLOB), wrongBundle, wrongBundle], vendoredOptions()], [wrongBundleMsg]],
    [
      "the document at the prefix, named index.html",
      [VENDORED, [reply("/react/dist/", WRONG), reply("/react/dist/app.bundle.js", JS_BLOB)], vendoredOptions()],
      [`build: /react/dist/ is not upstream's react/dist/index.html (blob ${WRONG}, upstream ${HTML_BLOB})`],
    ],
    ["the only file served, so none matched", [VENDORED, [wrongBundle], vendoredOptions()], [wrongBundleMsg, NO_MATCH]],
  ]);
});

test("checkBuild names a manifest file missing from the copy and one changed on disk, in the manifest's order", () => {
  const missing = "build: react/readme.md is missing from the vendored copy";
  const changed = `build: the vendored react/dist/app.bundle.js is not upstream's (blob ${CHANGED_BLOB}, upstream ${JS_BLOB})`;
  const served = [reply("/react/dist/", HTML_BLOB)];
  const readmeAbsent = omit(COPY, "/srv/todomvc/react/readme.md");
  const bundleChanged = { ...COPY, "/srv/todomvc/react/dist/app.bundle.js": "console.log(2)\n" };
  table(checkBuild, [
    ["a file missing", [VENDORED, served, vendoredOptions(readmeAbsent)], [missing]],
    ["a file changed", [VENDORED, served, vendoredOptions(bundleChanged)], [changed]],
    ["a file changed and a file missing", [VENDORED, served, vendoredOptions(omit(bundleChanged, "/srv/todomvc/react/readme.md"))], [changed, missing]],
    [
      "every file missing, in the manifest's order (a served file matched the manifest all the same)",
      [VENDORED, served, vendoredOptions({})],
      [
        "build: react/dist/index.html is missing from the vendored copy",
        "build: react/dist/app.bundle.js is missing from the vendored copy",
        missing,
      ],
    ],
  ]);
});

test("checkBuild lists the served files' problems, then the copy's, then the unmatched file last", () => {
  const served = [reply("/react/dist/extra.js", WRONG), reply("/react/dist/app.bundle.js", WRONG), reply("/react/dist/extra.js", WRONG)];
  const disk = omit({ ...COPY, "/srv/todomvc/react/dist/app.bundle.js": "console.log(2)\n" }, "/srv/todomvc/react/dist/index.html");
  assert.deepEqual(checkBuild(VENDORED, served, vendoredOptions(disk)), [
    "build: /react/dist/extra.js is not a file of the vendored build (react/dist/extra.js)",
    `build: /react/dist/app.bundle.js is not upstream's react/dist/app.bundle.js (blob ${WRONG}, upstream ${JS_BLOB})`,
    "build: react/dist/index.html is missing from the vendored copy",
    `build: the vendored react/dist/app.bundle.js is not upstream's (blob ${CHANGED_BLOB}, upstream ${JS_BLOB})`,
    NO_MATCH,
  ]);
});

test("checkBuild takes a missing manifest, or one with no files, to list no file: every served file is foreign and none is matched", () => {
  const served = [reply("/react/dist/app.bundle.js", JS_BLOB)];
  const foreign = ["build: /react/dist/app.bundle.js is not a file of the vendored build (react/dist/app.bundle.js)", NO_MATCH];
  table(checkBuild, [
    ["no manifest option", [VENDORED, served, memory(COPY)], foreign],
    ["a null manifest", [VENDORED, served, { manifest: null, ...memory(COPY) }], foreign],
    ["an undefined manifest", [VENDORED, served, { manifest: undefined, ...memory(COPY) }], foreign],
    ["a manifest with no files", [VENDORED, served, { manifest: {}, ...memory(COPY) }], foreign],
    ["a manifest of an empty list of files", [VENDORED, served, { manifest: { files: {} }, ...memory(COPY) }], foreign],
    ["nothing served and nothing listed", [VENDORED, [], memory()], [NO_MATCH]],
  ]);
});

test("checkBuild reads a vendored copy from the disk when it is given no other way: a file missing, one changed, the rest upstream's", () => {
  inTempDir((root) => {
    writeTree(root, { "react/dist/index.html": "<!doctype html>\n", "react/dist/app.bundle.js": "console.log(2)\n" });
    const build = { ...VENDORED, root };
    assert.deepEqual(checkBuild(build, [reply("/react/dist/", HTML_BLOB)], { manifest: UPSTREAM }), [
      `build: the vendored react/dist/app.bundle.js is not upstream's (blob ${CHANGED_BLOB}, upstream ${JS_BLOB})`,
      "build: react/readme.md is missing from the vendored copy",
    ]);
    writeTree(root, { "react/dist/app.bundle.js": "console.log(1)\n", "react/readme.md": "# TodoMVC in React\n" });
    assert.deepEqual(checkBuild(build, [reply("/react/dist/", HTML_BLOB)], { manifest: UPSTREAM }), []);
  });
});

// --- checkBuild: Pleris --------------------------------------------------------------

const RUNTIME_BLOB = "e4d05d91a7d1ddd7c94c78d0d7ff0b7a15e658ff"; // git hash-object of runtime
const WASM_BLOB = "f65e5817dd1092657b04d3b9768652220c9c8425"; // git hash-object of wasm
const APP_SHA = "11a837a2bfa2f73450a379798806a7ce08f5f9d12a93d4f0defa924f4f37fa62"; // sha256 of module app
const LIB_SHA = "f04510c6a2b844ed148cd3ed6231430d687069e2d673b5b75d69145af27f4062"; // sha256 of module lib

const DIST = "/srv/dist";
const PLERIS = { kind: "pleris", dist: DIST, sources: `${DIST}/sources.json` };
const listing = (files) => JSON.stringify({ files });
const SOURCES = [
  { path: "/src/app.pw", sha256: APP_SHA },
  { path: "/src/lib.pw", sha256: LIB_SHA },
];
const BUILT = {
  [PLERIS.sources]: listing(SOURCES),
  "/src/app.pw": "module app\n",
  "/src/lib.pw": "module lib\n",
  [`${DIST}/pw-runtime.mjs`]: "runtime\n",
  [`${DIST}/pw-app.wasm`]: "wasm\n",
  // Files that names which only resemble the compared ones would reach, unlike what is served.
  [`${DIST}/pw-app2.wasm`]: "wasm\n",
  [`${DIST}/pw-app-2.wasm`]: "wasm\n",
  [`${DIST}/pw-App.wasm`]: "wasm\n",
  [`${DIST}/sub/pw-runtime.mjs`]: "runtime\n",
  [`${DIST}/pw-runtime.mjs.map`]: "runtime\n",
  [`${DIST}/sub/pw-app.wasm`]: "wasm\n",
  [`${DIST}/pw-app.wasm.map`]: "wasm\n",
};
const RUNTIME_MSG = "build: /pw-runtime.mjs served is not the build's pw-runtime.mjs";
const WASM_MSG = "build: /pw-app.wasm served is not the build's pw-app.wasm";
const UNFINISHED = "build: /srv/dist/sources.json is missing: the build did not finish";

test("checkBuild accepts a Pleris build whose listed sources are as built and whose runtime and wasm are served as built", () => {
  const served = [reply("/pw-runtime.mjs", RUNTIME_BLOB), reply("/pw-app.wasm", WASM_BLOB), reply("/index.html", WRONG)];
  table(checkBuild, [
    ["sources unchanged, files served as built", [PLERIS, served, memory(BUILT)], []],
    ["nothing served", [PLERIS, [], memory(BUILT)], []],
  ]);
});

test("checkBuild refuses a Pleris build with no sources.json, and still compares what was served", () => {
  const without = memory(omit(BUILT, PLERIS.sources));
  table(checkBuild, [
    ["nothing served", [PLERIS, [], without], [UNFINISHED]],
    ["a runtime that is not the build's", [PLERIS, [reply("/pw-runtime.mjs", WRONG)], without], [UNFINISHED, RUNTIME_MSG]],
  ]);
});

test("checkBuild refuses a Pleris build whose sources.json lists no source: no files key, an empty list or null", () => {
  const listed = (text) => memory({ ...BUILT, [PLERIS.sources]: text });
  const none = ["build: /srv/dist/sources.json lists no source"];
  table(checkBuild, [
    ["no files key", [PLERIS, [], listed("{}")], none],
    ["an empty list", [PLERIS, [], listed(listing([]))], none],
    ["null", [PLERIS, [], listed(listing(null))], none],
  ]);
});

test("checkBuild names a Pleris source that is gone and one that changed since the build, in the order listed", () => {
  const changedLib = { "/src/lib.pw": "module lib\n// changed\n" };
  const goneApp = omit(BUILT, "/src/app.pw");
  const gone = "build: its source /src/app.pw is gone";
  const changed = "build: its source /src/lib.pw changed since it was built";
  table(checkBuild, [
    ["one gone", [PLERIS, [], memory(goneApp)], [gone]],
    ["one changed", [PLERIS, [], memory({ ...BUILT, ...changedLib })], [changed]],
    ["one gone, one changed", [PLERIS, [], memory({ ...goneApp, ...changedLib })], [gone, changed]],
    [
      "the same, listed the other way round",
      [PLERIS, [], memory({ ...goneApp, ...changedLib, [PLERIS.sources]: listing([...SOURCES].reverse()) })],
      [changed, gone],
    ],
  ]);
});

test("checkBuild compares the runtime and the wasm served with the build's own, by pathname, and no other file", () => {
  const resembling = ["/sub/pw-runtime.mjs", "/pw-runtime.mjs.map", "/sub/pw-app.wasm", "/pw-app.wasm.map"];
  resembling.push("/pw-app2.wasm", "/pw-app-2.wasm", "/pw-App.wasm", "/pw-.wasm", "/pw-gone.wasm", "/app.js");
  const unread = [reply("/pw-runtime.mjs", WRONG, { status: 304 }), reply("/pw-runtime.mjs", null), reply("/pw-app.wasm", undefined)];
  table(checkBuild, [
    ["the runtime", [PLERIS, [reply("/pw-runtime.mjs", WRONG)], memory(BUILT)], [RUNTIME_MSG]],
    ["a wasm", [PLERIS, [reply("/pw-app.wasm", WRONG)], memory(BUILT)], [WASM_MSG]],
    ["both, in the order served", [PLERIS, [reply("/pw-app.wasm", WRONG), reply("/pw-runtime.mjs", WRONG)], memory(BUILT)], [WASM_MSG, RUNTIME_MSG]],
    ["the runtime asked for with a query", [PLERIS, [reply("/pw-runtime.mjs?v=2", WRONG)], memory(BUILT)], [RUNTIME_MSG]],
    ["the runtime served by three runs", [PLERIS, Array.from({ length: 3 }, () => reply("/pw-runtime.mjs", WRONG)), memory(BUILT)], [RUNTIME_MSG]],
    ["names that only resemble them, and a wasm the build does not hold", [PLERIS, resembling.map((name) => reply(name, WRONG)), memory(BUILT)], []],
    ["a runtime served with another status, or with no blob", [PLERIS, unread, memory(BUILT)], []],
  ]);
});

test("checkBuild lets a corrupt sources.json fail the check, not pass it", () => {
  assert.throws(() => checkBuild(PLERIS, [], memory({ ...BUILT, [PLERIS.sources]: "{not json" })), SyntaxError);
});

test("checkBuild reads a Pleris build from the disk when it is given no other way: sources, runtime and wasm", () => {
  inTempDir((dir) => {
    const source = path.join(dir, "app.pw");
    const build = { kind: "pleris", dist: path.join(dir, "dist"), sources: path.join(dir, "dist/sources.json") };
    writeTree(dir, { "app.pw": "module app\n", "dist/pw-runtime.mjs": "runtime\n", "dist/pw-app.wasm": "wasm\n" });
    writeTree(dir, { "dist/sources.json": listing([{ path: source, sha256: APP_SHA }]) });
    const served = [reply("/pw-runtime.mjs", RUNTIME_BLOB), reply("/pw-app.wasm", WRONG)];
    assert.deepEqual(checkBuild(build, served), [WASM_MSG]);
    writeTree(dir, { "app.pw": "module app\n// changed\n" });
    fs.rmSync(path.join(dir, "dist/pw-runtime.mjs"));
    assert.deepEqual(checkBuild(build, served), [`build: its source ${source} changed since it was built`, WASM_MSG]);
    fs.rmSync(build.sources);
    assert.deepEqual(checkBuild(build, []), [`build: ${build.sources} is missing: the build did not finish`]);
  });
});

// --- checkBuild: Next.js and SvelteKit ------------------------------------------------

const NEXT = { kind: "next", dir: "/srv/next" };
const KIT = { kind: "sveltekit", dir: "/srv/kit" };
const NEXT_MARKER = "/srv/next/.next/BUILD_ID";
const KIT_MARKER = "/srv/kit/build/index.js";
const OLDER = "/srv/next/app/page.tsx";
const SAME = "/srv/next/app/layout.tsx";
const NEWER = "/srv/next/app/newer.tsx";
const SHARED = "/srv/shared/store.ts";
const KIT_SOURCE = "/srv/kit/src/routes/+page.svelte";
const MTIMES = { [NEXT_MARKER]: 1000, [KIT_MARKER]: 1000, [OLDER]: 999, [SAME]: 1000, [NEWER]: 1001, [SHARED]: 2000, [KIT_SOURCE]: 1001 };
/** A framework build on disk: its marker (Next.js's BUILD_ID holds the id) and every file's time. */
const frameworkFs = (marker, id = "abc123\n") => memory({ [marker]: id }, MTIMES);
const withSources = (fsOptions, sources) => ({ ...fsOptions, sources });
const page = (text, over = {}) => res({ kind: "document", type: "text/html", url: `${HOST}/stores/blue-bottle`, text, ...over });
const NAMED = '<script>{"b":"abc123"}</script>';
const NOT_NAMED = "build: the page served does not name the build abc123";

test("checkBuild accepts a Next.js build as new as its sources and named by the page served", () => {
  const next = frameworkFs(NEXT_MARKER);
  table(checkBuild, [
    ["sources older than the build and as old", [NEXT, [page(NAMED)], withSources(next, [OLDER, SAME])], []],
    ["no source listed", [NEXT, [page(NAMED)], next], []],
    ["a BUILD_ID with space around it", [NEXT, [page(NAMED)], frameworkFs(NEXT_MARKER, "  abc123 \n")], []],
  ]);
});

test("checkBuild refuses a framework build with no marker, Next.js's BUILD_ID or SvelteKit's index.js, without asking any time", () => {
  const nextMissing = ["build: /srv/next/.next/BUILD_ID is missing: the stack is not built"];
  const kitMissing = ["build: /srv/kit/build/index.js is missing: the stack is not built"];
  table(checkBuild, [
    ["Next.js", [NEXT, [], withSources(memory(), [OLDER])], nextMissing],
    ["SvelteKit", [KIT, [], withSources(memory(), [KIT_SOURCE])], kitMissing],
    ["SvelteKit, whose directory holds Next.js's marker", [KIT, [], memory({ "/srv/kit/.next/BUILD_ID": "abc123\n" })], kitMissing],
    ["Next.js, whose directory holds SvelteKit's marker", [NEXT, [], memory({ "/srv/next/build/index.js": "" })], nextMissing],
  ]);
});

test("checkBuild names each source newer than a framework build by its path from the build's directory, once, in the order given", () => {
  const next = frameworkFs(NEXT_MARKER);
  table(checkBuild, [
    ["Next.js, one source newer", [NEXT, [page(NAMED)], withSources(next, [OLDER, NEWER])], ["build: app/newer.tsx is newer than the build"]],
    ["Next.js, a source outside the directory", [NEXT, [page(NAMED)], withSources(next, [SHARED])], ["build: ../shared/store.ts is newer than the build"]],
    [
      "Next.js, two newer, one of them listed twice",
      [NEXT, [page(NAMED)], withSources(next, [SHARED, NEWER, SHARED])],
      ["build: ../shared/store.ts is newer than the build", "build: app/newer.tsx is newer than the build"],
    ],
    ["SvelteKit, one source newer", [KIT, [], withSources(frameworkFs(KIT_MARKER), [KIT_SOURCE])], ["build: src/routes/+page.svelte is newer than the build"]],
  ]);
});

test("checkBuild refuses a Next.js build that the page served does not name, and asks it of the first document served with a 200", () => {
  const next = frameworkFs(NEXT_MARKER);
  table(checkBuild, [
    ["a page naming another build", [NEXT, [page("other build")], next], [NOT_NAMED]],
    ["a page of no text", [NEXT, [page("")], next], [NOT_NAMED]],
    ["a stale source, then a page naming another build", [NEXT, [page("other build")], withSources(next, [NEWER])], ["build: app/newer.tsx is newer than the build", NOT_NAMED]],
    ["two pages, the first naming another build", [NEXT, [page("other build"), page(NAMED)], next], [NOT_NAMED]],
    ["a page that is not a 200, then one that names the build", [NEXT, [page("other build", { status: 404 }), page(NAMED)], next], []],
    ["a page that is not a 200 alone", [NEXT, [page("other build", { status: 404 })], next], []],
    ["a response that is not a document", [NEXT, [res({ text: "other build" })], next], []],
    ["no response", [NEXT, [], next], []],
    ["a page with null text", [NEXT, [page(null)], next], []],
    ["a page with no text", [NEXT, [page(undefined)], next], []],
    ["SvelteKit, which names no build", [KIT, [page("other build")], frameworkFs(KIT_MARKER)], []],
  ]);
});

test("checkBuild reads a framework build's times from the disk when it is given no other way", () => {
  inTempDir((dir) => {
    const files = { ".next/BUILD_ID": "abc123\n", "build/index.js": "export {};\n", "app/page.tsx": "a", "app/layout.tsx": "b", "src/routes/+page.svelte": "c" };
    const built = 1_700_000_000;
    const times = { ".next/BUILD_ID": built, "build/index.js": built, "app/page.tsx": built - 60, "app/layout.tsx": built, "src/routes/+page.svelte": built + 60 };
    writeTree(dir, files);
    Object.entries(times).forEach(([name, seconds]) => stamp(dir, name, seconds));
    const sources = ["app/page.tsx", "app/layout.tsx", "src/routes/+page.svelte"].map((name) => path.join(dir, name));
    const newer = ["build: src/routes/+page.svelte is newer than the build"];
    assert.deepEqual(checkBuild({ kind: "next", dir }, [page(NAMED)], { sources }), newer);
    assert.deepEqual(checkBuild({ kind: "sveltekit", dir }, [], { sources }), newer);
    stamp(dir, "src/routes/+page.svelte", 1_699_999_000);
    assert.deepEqual(checkBuild({ kind: "next", dir }, [page("other build")], { sources }), [NOT_NAMED]);
    assert.deepEqual(checkBuild({ kind: "sveltekit", dir }, [], { sources }), []);
    // A source that is not there fails the check: the caller lists the sources that exist.
    const gone = [path.join(dir, "src/gone.svelte")];
    assert.throws(() => checkBuild({ kind: "sveltekit", dir }, [], { sources: gone }), { code: "ENOENT" });
    fs.rmSync(path.join(dir, ".next"), { recursive: true });
    assert.deepEqual(checkBuild({ kind: "next", dir }, [], { sources }), [`build: ${path.join(dir, ".next/BUILD_ID")} is missing: the stack is not built`]);
  });
});

test("checkBuild refuses a build of a kind it does not know, whatever was served", () => {
  const strange = [res({ url: "not a url" })];
  table(checkBuild, [
    ["a kind it does not know", [{ kind: "mystery" }, strange, memory()], ["build: unknown build kind mystery"]],
    ["a known kind in other letters", [{ kind: "Next", dir: "/srv/next" }, [], memory()], ["build: unknown build kind Next"]],
    ["no kind, and no option", [{}, strange], ["build: unknown build kind undefined"]],
  ]);
});

// --- checkShaped -----------------------------------------------------------------------

const SLOW = { latency: 562.5 };
const SHAPED_CELL = "todomvc/react/chromium/cold/slow-4g";
const slow = (timings, over = {}) => run({ network: "slow-4g", timings, ...over });
const tooFast = (at, i = 0, cellKey = SHAPED_CELL) => `shaped: ${cellKey} run ${i}: its document's last byte came at ${at} ms, under the profile's 562.5 ms round trip`;

test("checkShaped refuses a shaped run whose document's last byte came before the profile's round trip, saying when to the millisecond", () => {
  table(checkShaped, [
    ["a document at 3 ms", [[slow({ documentEnd: 3 })], SLOW], [tooFast("3")]],
    ["a document at the round trip exactly", [[slow({ documentEnd: 562.5 })], SLOW], []],
    ["a document at 562.4 ms, which reads 562", [[slow({ documentEnd: 562.4 })], SLOW], [tooFast("562")]],
    ["a document at 99.6 ms, which reads 100", [[slow({ documentEnd: 99.6 })], SLOW], [tooFast("100")]],
    ["a document at 0 ms", [[slow({ documentEnd: 0 })], SLOW], [tooFast("0")]],
    ["a document later than the round trip", [[slow({ documentEnd: 573 })], SLOW], []],
  ]);
});

test("checkShaped refuses a shaped run that never saw its document's end, and says undefined of when", () => {
  table(checkShaped, [
    ["a document end of null", [[slow({ documentEnd: null })], SLOW], [tooFast("undefined")]],
    ["timings without a document end", [[slow({})], SLOW], [tooFast("undefined")]],
    ["no timings", [[slow(undefined)], SLOW], [tooFast("undefined")]],
  ]);
});

test("checkShaped reads the shaped runs only, each however alike another, in the order given", () => {
  const runs = [
    slow({ documentEnd: 1 }),
    run({ timings: { documentEnd: 1 } }),
    slow({ documentEnd: 900 }, { i: 1 }),
    slow({ documentEnd: 2 }, { i: 2, engine: "webkit" }),
    slow({ documentEnd: 1 }),
    run({ network: "", timings: { documentEnd: 1 } }),
  ];
  assert.deepEqual(checkShaped(runs, SLOW), [tooFast("1"), tooFast("2", 2, "todomvc/react/webkit/cold/slow-4g"), tooFast("1")]);
  assert.deepEqual(checkShaped([slow({ documentEnd: 4 }, { condition: "warm" })], SLOW), [tooFast("4", 0, "todomvc/react/chromium/warm/slow-4g")]);
  assert.deepEqual(checkShaped([], SLOW), []);
});

test("checkShaped holds a run to the round trip of the profile it is given", () => {
  const rounded = "shaped: todomvc/react/chromium/cold/slow-4g run 0: its document's last byte came at 150 ms, under the profile's 150 ms round trip";
  table(checkShaped, [
    ["a profile of 150 ms, a document at 160", [[slow({ documentEnd: 160 })], { latency: 150 }], []],
    ["a profile of no latency, a document at 0 ms, which is a time and not a missing one", [[slow({ documentEnd: 0 })], { latency: 0 }], []],
    ["a profile of 150 ms, a document at 149.9 ms, which reads 150", [[slow({ documentEnd: 149.9 })], { latency: 150 }], [rounded]],
  ]);
});

// --- checkInteractive ------------------------------------------------------------------

const STEPS = [{ name: "add" }, { name: "toggle" }, { name: "filter" }];
const ONE_STEP = [{ name: "add" }];
const TAKEN = [
  { name: "add", toDom: 2, toFrame: 5 },
  { name: "toggle", toDom: 3, toFrame: 6 },
  { name: "filter", toDom: 2, toFrame: 7 },
];
const taken = (over = {}) => run({ interactions: TAKEN, ...over });
const latencyOf = (toDom, toFrame) => run({ interactions: [{ name: "add", toDom, toFrame }] });
const RUN0 = `interactive: ${CELL} run 0: `;

test("checkInteractive accepts runs whose every interaction took effect in the document loaded, with a latency", () => {
  table(checkInteractive, [
    ["a run of three interactions", [[taken()], STEPS], []],
    ["no run", [[], STEPS], []],
    ["latencies of zero", [[latencyOf(0, 0)], ONE_STEP], []],
    ["equal latencies", [[latencyOf(4, 4)], ONE_STEP], []],
    ["a time to become interactive of zero", [[taken({ timings: { ready: 0 } })], STEPS], []],
    ["a DOM time of null, which compares as zero", [[latencyOf(null, 5)], ONE_STEP], []],
    ["a frame time of null after a DOM time of zero", [[latencyOf(0, null)], ONE_STEP], []],
  ]);
});

test("checkInteractive tells a run that failed by its error, and reads nothing else of it", () => {
  const timeout = "page.goto: Timeout 60000ms exceeded.";
  const recorded = { ...run({ i: 3 }), error: timeout, requests: [], interactions: [], timings: {}, sameDocument: undefined };
  table(checkInteractive, [
    ["a run as the instrument keeps one that failed", [[recorded], STEPS], [`interactive: ${CELL} run 3: the run failed: ${timeout}`]],
    ["a run that failed after measuring, with more wrong in it", [[taken({ error: "boom", sameDocument: false, timings: {} })], STEPS], [`${RUN0}the run failed: boom`]],
    ["an error that is an empty string", [[taken({ error: "" })], STEPS], []],
  ]);
});

test("checkInteractive refuses an interaction that loaded a new document, or a run that says nothing of it", () => {
  table(checkInteractive, [
    ["a new document", [[taken({ sameDocument: false })], STEPS], [`${RUN0}an interaction loaded a new document`]],
    ["no word of the document", [[taken({ sameDocument: undefined })], STEPS], [`${RUN0}an interaction loaded a new document`]],
  ]);
});

test("checkInteractive refuses a run with fewer interactions than the steps, or more, a missing list counting as none", () => {
  table(checkInteractive, [
    ["two of three", [[taken({ interactions: TAKEN.slice(0, 2) })], STEPS], [`${RUN0}2 of 3 interactions completed`]],
    ["none of three", [[taken({ interactions: [] })], STEPS], [`${RUN0}0 of 3 interactions completed`]],
    ["no list", [[taken({ interactions: undefined })], STEPS], [`${RUN0}0 of 3 interactions completed`]],
    ["a list of null", [[taken({ interactions: null })], STEPS], [`${RUN0}0 of 3 interactions completed`]],
    ["no list, one step", [[taken({ interactions: undefined })], ONE_STEP], [`${RUN0}0 of 1 interactions completed`]],
    ["four of three", [[taken({ interactions: [...TAKEN, TAKEN[0]] })], STEPS], [`${RUN0}4 of 3 interactions completed`]],
    ["three of one", [[taken()], ONE_STEP], [`${RUN0}3 of 1 interactions completed`]],
  ]);
});

test("checkInteractive refuses an interaction with no latency: NaN, negative, missing, or a frame before its DOM", () => {
  table(checkInteractive, [
    ["a DOM time of NaN", [[latencyOf(NaN, 5)], ONE_STEP], [`${RUN0}add has no latency (NaN, 5)`]],
    ["a negative DOM time", [[latencyOf(-1, 5)], ONE_STEP], [`${RUN0}add has no latency (-1, 5)`]],
    ["a frame before the DOM", [[latencyOf(5, 2)], ONE_STEP], [`${RUN0}add has no latency (5, 2)`]],
    ["both times missing", [[latencyOf(undefined, undefined)], ONE_STEP], [`${RUN0}add has no latency (undefined, undefined)`]],
    ["a frame time missing", [[latencyOf(2, undefined)], ONE_STEP], [`${RUN0}add has no latency (2, undefined)`]],
    ["a frame time of NaN", [[latencyOf(2, NaN)], ONE_STEP], [`${RUN0}add has no latency (2, NaN)`]],
    ["a frame time of null after a DOM time above zero", [[latencyOf(2, null)], ONE_STEP], [`${RUN0}add has no latency (2, null)`]],
  ]);
});

test("checkInteractive refuses a run that never became interactive: no ready time, or no timings", () => {
  table(checkInteractive, [
    ["a ready time of null", [[taken({ timings: { ready: null } })], STEPS], [`${RUN0}never interactive`]],
    ["timings without a ready time", [[taken({ timings: {} })], STEPS], [`${RUN0}never interactive`]],
    ["no timings", [[taken({ timings: undefined })], STEPS], [`${RUN0}never interactive`]],
  ]);
});

test("checkInteractive lists one run's problems in the order of its checks, and the runs' in the order given, without deduplicating", () => {
  const latencies = [
    { name: "add", toDom: NaN, toFrame: 1 },
    { name: "toggle", toDom: 3, toFrame: 1 },
  ];
  const broken = taken({ sameDocument: false, interactions: latencies, timings: { ready: null } });
  const all = [
    `${RUN0}an interaction loaded a new document`,
    `${RUN0}2 of 3 interactions completed`,
    `${RUN0}add has no latency (NaN, 1)`,
    `${RUN0}toggle has no latency (3, 1)`,
    `${RUN0}never interactive`,
  ];
  assert.deepEqual(checkInteractive([broken], STEPS), all);
  assert.deepEqual(checkInteractive([broken, taken({ i: 1 }), broken], STEPS), [...all, ...all]);
  const elsewhere = taken({ engine: "webkit", condition: "warm", network: "slow-4g", sameDocument: false });
  assert.deepEqual(checkInteractive([elsewhere], STEPS), ["interactive: todomvc/react/webkit/warm/slow-4g run 0: an interaction loaded a new document"]);
});

// --- checkWarm -------------------------------------------------------------------------

const CACHES = { caches: true };
const WARM_CELL = "todomvc/react/chromium/warm/none";
const BIG = res({ raw: 200000, receivedBody: 60000 });
const coldRun = (...requests) => run({ requests });
const warmRun = (i, ...requests) => run({ condition: "warm", i, requests });
const downloadedAgain = (i, bytes, pathname = "/react/dist/app.bundle.js") => `warm: ${WARM_CELL} run ${i}: ${pathname} was downloaded again (${bytes} bytes)`;

test("checkWarm refuses a warm run that received a 200 answer of at least half of its main script again, naming the script and the bytes", () => {
  const answer = (receivedBody, over = {}) => [coldRun(BIG), warmRun(0, res({ receivedBody, ...over }))];
  table(checkWarm, [
    ["the whole script again", [answer(60000), CACHES], [downloadedAgain(0, 60000)]],
    ["exactly half", [answer(30000), CACHES], [downloadedAgain(0, 30000)]],
    ["just under half", [answer(29999), CACHES], []],
    ["nothing received, a cache's answer", [answer(0), CACHES], []],
    ["no size received", [answer(undefined), CACHES], []],
    ["a 304 of the whole size", [answer(60000, { status: 304 }), CACHES], []],
    ["no status", [answer(60000, { status: undefined }), CACHES], []],
    ["a request out of the load phase", [answer(60000, { phase: "interaction:add" }), CACHES], []],
  ]);
});

test("checkWarm measures against the main script's raw size when the cold run recorded no size received", () => {
  const cold = (receivedBody) => coldRun(res({ raw: 200000, receivedBody }));
  const warm = (bytes) => warmRun(0, res({ receivedBody: bytes }));
  table(checkWarm, [
    ["no size received: half the raw size", [[cold(undefined), warm(100000)], CACHES], [downloadedAgain(0, 100000)]],
    ["the same, just under", [[cold(undefined), warm(99999)], CACHES], []],
    ["a size of null, likewise", [[cold(null), warm(100000)], CACHES], [downloadedAgain(0, 100000)]],
    ["a size of zero: any 200 answer is a download again", [[cold(0), warm(0)], CACHES], [downloadedAgain(0, 0)]],
    ["a size of zero, and an answer with no size", [[cold(0), warm(undefined)], CACHES], [downloadedAgain(0, undefined)]],
    ["a size of two bytes, and an answer with no size", [[cold(2), warm(undefined)], CACHES], []],
  ]);
});

test("checkWarm judges the largest cold script of the load phase by raw size, the first of equals, and no style", () => {
  const small = `${HOST}/react/dist/small.js`;
  const script = (url, raw, over = {}) => res({ url, raw, receivedBody: 400, ...over });
  const redone = (url) => warmRun(0, res({ url, receivedBody: 60000 }));
  const smallAgain = [downloadedAgain(0, 60000, "/react/dist/small.js")];
  table(checkWarm, [
    ["the largest of three, downloaded again", [[coldRun(script(small, 1000), script(BUNDLE, 5000), script(CHUNK, 3000)), redone(BUNDLE)], CACHES], [downloadedAgain(0, 60000)]],
    ["a smaller one downloaded again", [[coldRun(script(small, 1000), script(BUNDLE, 5000)), redone(small)], CACHES], []],
    ["two of one size, the first downloaded again", [[coldRun(script(small, 800), script(BUNDLE, 800)), redone(small)], CACHES], smallAgain],
    ["two of one size, the second downloaded again", [[coldRun(script(small, 800), script(BUNDLE, 800)), redone(BUNDLE)], CACHES], []],
    ["a script with no raw size counting as zero", [[coldRun(script(small, undefined), script(BUNDLE, 1)), redone(small)], CACHES], []],
    ["two scripts with no raw size, the first downloaded again", [[coldRun(script(small, undefined), script(BUNDLE, undefined)), redone(small)], CACHES], smallAgain],
    ["two scripts with no raw size, the second downloaded again", [[coldRun(script(small, undefined), script(BUNDLE, undefined)), redone(BUNDLE)], CACHES], []],
    ["a style larger than every script", [[coldRun(script(small, 1000), script(STYLE, 90000, { type: "text/css" })), redone(small)], CACHES], smallAgain],
    ["a larger script of an interaction's phase", [[coldRun(script(small, 1000), script(BUNDLE, 9000, { phase: "interaction:add" })), redone(small)], CACHES], smallAgain],
  ]);
});

test("checkWarm takes as scripts the responses whose type has javascript in it", () => {
  const again = warmRun(0, res({ receivedBody: 60000 }));
  const rows = [
    ["text/javascript", true],
    ["application/javascript", true],
    ["application/x-javascript", true],
    ["text/x-javascript; charset=utf-8", true],
    ["application/ecmascript", false],
    ["text/css", false],
    ["", false],
    [undefined, false],
    [null, false],
  ];
  rows.forEach(([type, script]) => {
    const cold = coldRun(res({ type, receivedBody: 400 }));
    assert.deepEqual(checkWarm([cold, again], CACHES), script ? [downloadedAgain(0, 60000)] : [], `type ${type}`);
  });
});

test("checkWarm asks nothing without the expectation of a cache, or without a cold Chromium run to take the main script from", () => {
  const again = warmRun(0, res({ receivedBody: 60000 }));
  table(checkWarm, [
    ["a host that does not cache", [[coldRun(BIG), again], { caches: false }], []],
    ["an expectation with no caches", [[coldRun(BIG), again], {}], []],
    ["no cold run", [[again], CACHES], []],
    ["a cold Firefox run only", [[run({ engine: "firefox", requests: [BIG] }), again], CACHES], []],
    ["a cold WebKit run only", [[run({ engine: "webkit", requests: [BIG] }), again], CACHES], []],
    ["a shaped cold Chromium run only", [[run({ network: "slow-4g", requests: [BIG] }), again], CACHES], []],
    ["a cold Chromium run with no script", [[coldRun(res({ type: "text/css", kind: "stylesheet" })), again], CACHES], []],
    ["a cold Chromium run with no list of requests", [[{ ...run(), requests: undefined }, again], CACHES], []],
    ["a warm Chromium run with no list of requests", [[coldRun(BIG), { ...warmRun(0), requests: undefined }], CACHES], []],
    ["no run", [[], CACHES], []],
  ]);
});

test("checkWarm holds the unshaped warm runs of Chromium, and neither Firefox's, WebKit's, a shaped run's nor a cold run's", () => {
  const again = warmRun(0, res({ receivedBody: 60000 }));
  const cold = coldRun(BIG);
  table(checkWarm, [
    ["a warm Chromium run", [[cold, again], CACHES], [downloadedAgain(0, 60000)]],
    ["a warm Firefox run", [[cold, { ...again, engine: "firefox" }], CACHES], []],
    ["a warm WebKit run", [[cold, { ...again, engine: "webkit" }], CACHES], []],
    ["a shaped warm Chromium run", [[cold, { ...again, network: "slow-4g" }], CACHES], []],
    ["a cold Chromium run that downloads the script whole", [[cold, run({ i: 1, requests: [res({ receivedBody: 60000 })] })], CACHES], []],
    ["no warm run", [[cold], CACHES], []],
  ]);
});

test("checkWarm takes the main script from the first cold Chromium run, and the first request of it in a warm run's load phase", () => {
  const other = res({ url: `${HOST}/react/dist/other.js`, raw: 900000, receivedBody: 400 });
  const cachedFirst = warmRun(0, res({ status: 304, receivedBody: 0 }), res({ receivedBody: 60000 }));
  const earlier = [run({ engine: "firefox", requests: [other] }), run({ network: "slow-4g", requests: [other] })];
  table(checkWarm, [
    ["a 304 first, a 200 after", [[coldRun(BIG), cachedFirst], CACHES], []],
    ["the first cold Chromium run, after runs that do not count", [[...earlier, coldRun(BIG), coldRun(other), warmRun(0, res({ url: other.url, receivedBody: 60000 }))], CACHES], []],
    ["the second cold Chromium run's script downloaded again", [[coldRun(BIG), coldRun(other), warmRun(0, res({ receivedBody: 60000 }))], CACHES], [downloadedAgain(0, 60000)]],
  ]);
});

test("checkWarm names each warm run that downloaded again by its script's pathname, in the order given, once for runs alike", () => {
  const url = `${BUNDLE}?v=2`;
  const cold = coldRun(res({ url, raw: 200000, receivedBody: 60000 }));
  const again = (i) => warmRun(i, res({ url, receivedBody: 60000 }));
  const cached = warmRun(1, res({ url, receivedBody: 10 }));
  assert.deepEqual(checkWarm([cold, again(2), cached, again(0), again(2)], CACHES), [downloadedAgain(2, 60000), downloadedAgain(0, 60000)]);
});

// --- validateServer --------------------------------------------------------------------

const server = (stack, i, over = {}) => ({ stack, i, sent: 1000, answered: 1000, ...over });
const rounds = (stack, n, over = {}) => Array.from({ length: n }, (_, i) => server(stack, i, over));
const FIVE_EACH = [...rounds("pleris", 5), ...rounds("react", 5)];
const measured = (over) => validateServer({ app: "todomvc", stacks: ["pleris", "react"], ...over });
const ALL_THREE = { loads: FIVE_EACH, idle: FIVE_EACH, presses: FIVE_EACH };
const NO_ROUNDS = [
  "load: todomvc/pleris has 0 run(s), fewer than 5",
  "load: todomvc/react has 0 run(s), fewer than 5",
  "idle: todomvc/pleris has 0 run(s), fewer than 5",
  "idle: todomvc/react has 0 run(s), fewer than 5",
  "press: todomvc/pleris has 0 run(s), fewer than 5",
  "press: todomvc/react has 0 run(s), fewer than 5",
];

test("validateServer accepts a load, idle visitors and a press of five rounds of every stack", () => {
  assert.deepEqual(measured(ALL_THREE), []);
});

test("validateServer takes a measurement it is not given as one in which every stack has no round", () => {
  table(measured, [
    ["no list at all", [{}], NO_ROUNDS],
    ["three empty lists", [{ loads: [], idle: [], presses: [] }], NO_ROUNDS],
    ["only the loads given", [{ loads: FIVE_EACH }], NO_ROUNDS.slice(2)],
    ["only the presses given", [{ presses: FIVE_EACH }], NO_ROUNDS.slice(0, 4)],
    [
      "the stacks in another order",
      [{ stacks: ["react", "pleris"] }],
      [
        "load: todomvc/react has 0 run(s), fewer than 5",
        "load: todomvc/pleris has 0 run(s), fewer than 5",
        "idle: todomvc/react has 0 run(s), fewer than 5",
        "idle: todomvc/pleris has 0 run(s), fewer than 5",
        "press: todomvc/react has 0 run(s), fewer than 5",
        "press: todomvc/pleris has 0 run(s), fewer than 5",
      ],
    ],
    ["no stack and no list", [{ stacks: [] }], []],
  ]);
});

test("validateServer names a stack with fewer rounds than the minimum in a measurement, and the stacks whose counts differ", () => {
  const few = (n) => [...rounds("pleris", n), ...rounds("react", n)];
  table(measured, [
    [
      "idle visitors of one round each",
      [{ ...ALL_THREE, idle: few(1) }],
      ["idle: todomvc/pleris has 1 run(s), fewer than 5", "idle: todomvc/react has 1 run(s), fewer than 5"],
    ],
    [
      "a load in which one stack has no round",
      [{ ...ALL_THREE, loads: rounds("pleris", 5) }],
      ["load: todomvc/react has 0 run(s), fewer than 5", "load: todomvc's stacks have different run counts"],
    ],
    [
      "a press of four rounds and five",
      [{ ...ALL_THREE, presses: [...rounds("pleris", 4), ...rounds("react", 5)] }],
      ["press: todomvc/pleris has 4 run(s), fewer than 5", "press: todomvc's stacks have different run counts"],
    ],
    ["a load of five rounds and six", [{ ...ALL_THREE, loads: [...rounds("pleris", 5), ...rounds("react", 6)] }], ["load: todomvc's stacks have different run counts"]],
    ["a minimum of two, two rounds each", [{ loads: few(2), idle: few(2), presses: few(2), minRuns: 2 }], []],
    [
      "a minimum of three, two rounds each in the load",
      [{ ...ALL_THREE, loads: few(2), minRuns: 3 }],
      ["load: todomvc/pleris has 2 run(s), fewer than 3", "load: todomvc/react has 2 run(s), fewer than 3"],
    ],
  ]);
});

test("validateServer counts a stack the list names but the stacks do not, after the stacks, and holds it to the rest", () => {
  const svelte = (n) => rounds("svelte", n);
  table(measured, [
    ["a third stack of five rounds", [{ ...ALL_THREE, loads: [...FIVE_EACH, ...svelte(5)] }], []],
    [
      "a third stack of two rounds",
      [{ ...ALL_THREE, loads: [...FIVE_EACH, ...svelte(2)] }],
      ["load: todomvc/svelte has 2 run(s), fewer than 5", "load: todomvc's stacks have different run counts"],
    ],
    [
      "a stack alone, the declared stacks having none",
      [{ ...ALL_THREE, loads: svelte(5) }],
      ["load: todomvc/pleris has 0 run(s), fewer than 5", "load: todomvc/react has 0 run(s), fewer than 5", "load: todomvc's stacks have different run counts"],
    ],
  ]);
});

test("validateServer tells a failed measurement of each kind, which still counts as a round, and not its requests unanswered", () => {
  const failing = (kind) => ({ ...ALL_THREE, [kind]: [...rounds("pleris", 5), ...rounds("react", 4), server("react", 4, { error: "boom", answered: 0 })] });
  table(measured, [
    ["a load", [failing("loads")], ["load: todomvc/react round 4: the measurement failed: boom"]],
    ["idle visitors", [failing("idle")], ["idle: todomvc/react round 4: the measurement failed: boom"]],
    ["a press", [failing("presses")], ["press: todomvc/react round 4: the measurement failed: boom"]],
  ]);
});

test("validateServer tells the requests of a load that went unanswered, and of no other measurement", () => {
  const dropping = (answered) => [...rounds("pleris", 2), server("pleris", 2, { answered }), ...rounds("pleris", 5).slice(3), ...rounds("react", 5)];
  const unanswered = (n) => [`load: todomvc/pleris round 2: ${n} of 1000 requests went unanswered`];
  table(measured, [
    ["ten of a thousand", [{ ...ALL_THREE, loads: dropping(990) }], unanswered(10)],
    ["all of a thousand", [{ ...ALL_THREE, loads: dropping(0) }], unanswered(1000)],
    ["one more answered than sent", [{ ...ALL_THREE, loads: dropping(1001) }], []],
    ["no count of the answered", [{ ...ALL_THREE, loads: dropping(undefined) }], []],
    ["idle visitors with the same counts", [{ ...ALL_THREE, idle: dropping(990) }], []],
    ["a press with the same counts", [{ ...ALL_THREE, presses: dropping(990) }], []],
  ]);
});

test("validateServer lists the load, then the idle visitors, then the press, each by its rounds, its stacks' counts, then their difference", () => {
  const loads = [
    ...rounds("pleris", 1),
    server("pleris", 1, { answered: 500 }),
    ...rounds("pleris", 3).slice(2),
    server("pleris", 3, { error: "stuck" }),
    server("pleris", 4),
    ...rounds("react", 3),
  ];
  const idle = [...rounds("pleris", 5), server("react", 0, { error: "gone" }), ...rounds("react", 5).slice(1)];
  const presses = [...rounds("pleris", 4), ...rounds("react", 5)];
  assert.deepEqual(measured({ loads, idle, presses }), [
    "load: todomvc/pleris round 1: 500 of 1000 requests went unanswered",
    "load: todomvc/pleris round 3: the measurement failed: stuck",
    "load: todomvc/react has 3 run(s), fewer than 5",
    "load: todomvc's stacks have different run counts",
    "idle: todomvc/react round 0: the measurement failed: gone",
    "press: todomvc/pleris has 4 run(s), fewer than 5",
    "press: todomvc's stacks have different run counts",
  ]);
});

// --- records that are not records -------------------------------------------------------------

test("the rules fail loudly, and do not pass a run, on a response with no phase or a URL that is relative", () => {
  const relative = unminified({ url: "/react/dist/app.bundle.js" });
  assert.throws(() => checkMinified(oneRun(res({ phase: undefined })), MINIFIES), TypeError);
  assert.throws(() => checkCompressed(oneRun(plain({ phase: undefined })), ALL), TypeError);
  assert.throws(() => checkMinified(oneRun(relative), MINIFIES, [/\/base\.js$/]), TypeError);
  // The URL is read before the status and the blob are asked of a response.
  assert.throws(() => checkBuild(VENDORED, [res({ url: "/react/dist/app.bundle.js", status: 404 })], vendoredOptions()), TypeError);
  assert.throws(() => checkBuild(PLERIS, [res({ url: "/pw-runtime.mjs", status: 404 })], memory(BUILT)), TypeError);
  assert.throws(() => checkBuild(PLERIS, [res({ url: "/pw-runtime.mjs", blob: null })], memory(BUILT)), TypeError);
  // With no common file to match, the URL is not parsed at all.
  assert.deepEqual(checkMinified(oneRun(relative), MINIFIES), ["minified: /react/dist/app.bundle.js is not minified, and this stack's build minifies"]);
});

// --- validate: the rules joined ------------------------------------------------------------

/** A target as targets.mjs describes one, with a build of a kind checkBuild does not know (it asks nothing of the disk). */
const handmade = (stack, over = {}) => ({
  app: "todomvc",
  stack,
  build: { kind: "mystery" },
  expect: { minifies: true, compresses: "all", caches: true },
  common: [],
  interactions: ONE_STEP,
  ...over,
});

// One cold run that is unminified, uncompressed and in a new document, a warm run that
// downloads the script again, and a shaped run whose document came at 3 ms.
const FLAWED = [run({ sameDocument: false, requests: [unminified({ encoding: null })] }), warmRun(0, res({ receivedBody: 1000 })), slow({ documentEnd: 3, ready: 40 })];
const FLAWS = [
  "todomvc/react: build: unknown build kind mystery",
  `todomvc/react: ${minifiedMsg(BUNDLE)}`,
  `todomvc/react: ${BUNDLE_MSG}`,
  `${RUN0}an interaction loaded a new document`,
  `todomvc/react: ${downloadedAgain(0, 1000)}`,
  tooFast("3"),
];
const FEW_RUNS = [
  `runs: ${CELL} has 1 run(s), fewer than 5`,
  "runs: todomvc/react/chromium/warm/none has 1 run(s), fewer than 5",
  "runs: todomvc/react/chromium/cold/slow-4g has 1 run(s), fewer than 5",
];

test("validate joins the rules target by target in their order, with the prefix app/stack on all but the interactive and shaped rules", () => {
  assert.deepEqual(validate({ runs: FLAWED, targets: [handmade("react")], network: SLOW, minRuns: 1 }), FLAWS);
});

test("validate lists the problems of the runs first, and holds the minimum it is given, five when it is given none", () => {
  const given = { runs: FLAWED, targets: [handmade("react")], network: SLOW };
  assert.deepEqual(validate(given), [...FEW_RUNS, ...FLAWS]);
  assert.deepEqual(validate({ ...given, minRuns: undefined }), [...FEW_RUNS, ...FLAWS]);
  assert.deepEqual(validate({ ...given, minRuns: 2 }).slice(0, 3), FEW_RUNS.map((message) => message.replace("fewer than 5", "fewer than 2")));
  assert.deepEqual(validate({ ...given, minRuns: 1 }), FLAWS);
});

test("validate with no target judges only the number of runs", () => {
  assert.deepEqual(validate({ runs: [], targets: [], network: SLOW }), ["runs: no run was made"]);
  assert.deepEqual(validate({ runs: cell(1), targets: [], network: SLOW }), [`runs: ${CELL} has 1 run(s), fewer than 5`]);
  assert.deepEqual(validate({ runs: cell(5, { sameDocument: false }), targets: [], network: SLOW }), []);
});

test("validate asks the shaping of a run only when it is given the profile", () => {
  const withoutShaping = FLAWS.slice(0, -1);
  const given = { runs: FLAWED, targets: [handmade("react")], minRuns: 1 };
  assert.deepEqual(validate({ ...given, network: null }), withoutShaping);
  assert.deepEqual(validate({ ...given, network: undefined }), withoutShaping);
  assert.deepEqual(validate({ ...given, network: SLOW }), FLAWS);
});

test("validate gives each target the runs of its own app and stack, and lists the targets in turn", () => {
  const runs = [...FLAWED, run({ stack: "svelte", sameDocument: false }), run({ app: "store", sameDocument: false })];
  const svelte = handmade("svelte", { build: { kind: "other" }, expect: {} });
  assert.deepEqual(validate({ runs, targets: [handmade("react"), svelte], network: null, minRuns: 1 }), [
    ...FLAWS.slice(0, -1),
    "todomvc/svelte: build: unknown build kind other",
    "interactive: todomvc/svelte/chromium/cold/none run 0: an interaction loaded a new document",
  ]);
});

test("validate gives a framework target the sources of its own stack, and none when it is given none", () => {
  inTempDir((dir) => {
    writeTree(dir, { ".next/BUILD_ID": "abc123\n", "app/page.tsx": "x" });
    stamp(dir, ".next/BUILD_ID", 1_700_000_000);
    stamp(dir, "app/page.tsx", 1_700_000_060);
    const source = path.join(dir, "app/page.tsx");
    const next = handmade("next-react", { build: { kind: "next", dir }, expect: {} });
    // Another stack's page, which does not name the build, is served first: a target reads its own stack's runs.
    const runs = [run({ stack: "sveltekit", requests: [page("other build")] }), run({ stack: "next-react", requests: [page(NAMED)] })];
    const sourcesGiven = (sources) => validate({ runs, targets: [next], network: null, minRuns: 1, sources });
    assert.deepEqual(sourcesGiven({ "next-react": [source] }), ["todomvc/next-react: build: app/page.tsx is newer than the build"]);
    assert.deepEqual(sourcesGiven({ sveltekit: [source] }), []);
    assert.deepEqual(sourcesGiven({}), []);
    assert.deepEqual(sourcesGiven(undefined), []);
  });
});

// --- validate, on the recorded run -----------------------------------------------------------

const TARGETS = [target("todomvc", "react"), target("todomvc", "svelte")];
const RECORDED_CELLS = [
  "todomvc/react/chromium/cold/none",
  "todomvc/svelte/chromium/cold/none",
  "todomvc/svelte/chromium/warm/none",
  "todomvc/react/chromium/warm/none",
  "todomvc/react/firefox/cold/none",
  "todomvc/svelte/firefox/cold/none",
  "todomvc/react/firefox/warm/none",
  "todomvc/svelte/firefox/warm/none",
  "todomvc/react/webkit/cold/none",
  "todomvc/svelte/webkit/cold/none",
  "todomvc/react/webkit/warm/none",
  "todomvc/svelte/webkit/warm/none",
  "todomvc/svelte/chromium/cold/slow-4g",
  "todomvc/react/chromium/cold/slow-4g",
  "todomvc/svelte/chromium/warm/slow-4g",
  "todomvc/react/chromium/warm/slow-4g",
];
const SVELTE_WARM = "todomvc/svelte/chromium/warm/none";
const SVELTE_SHAPED = "todomvc/svelte/chromium/cold/slow-4g";
const REACT_SHAPED = "todomvc/react/chromium/cold/slow-4g";
const REACT_WARM = "todomvc/react/chromium/warm/none";
const REACT_BUNDLE = "http://localhost:3362/react/dist/app.bundle.js";
const SVELTE_BUNDLE = "http://localhost:3363/svelte/dist/assets/index-BxPr2-fK.js";

const cellKey = (r) => [r.app, r.stack, r.engine, r.condition, r.network ?? "none"].join("/");
const cellRuns = (runs, key) => runs.filter((r) => cellKey(r) === key);
const requestOf = (r, suffix) => r.requests.find((q) => q.url.endsWith(suffix));
const patchRun = (runs, key, i, patch) => Object.assign(cellRuns(runs, key)[i], patch);
const patchRequest = (runs, key, i, suffix, patch) => Object.assign(requestOf(cellRuns(runs, key)[i], suffix), patch);
const removeRun = (runs, key, i) => runs.splice(runs.indexOf(cellRuns(runs, key)[i]), 1);
function failRun(runs, key, i, error) {
  const failed = cellRuns(runs, key)[i];
  Object.assign(failed, { error, requests: [], interactions: [], timings: {} });
  delete failed.sameDocument;
}

/** validate of the recorded run, a copy of its runs changed by `edit`, with the targets of targets.mjs and upstream's manifest. */
function validateRecorded(edit = () => {}, over = {}) {
  const runs = structuredClone(RECORD.runs);
  edit(runs);
  return validate({ runs, targets: TARGETS, network: RECORD.network, minRuns: 5, manifest: UPSTREAM_JSON, ...over });
}

test("the recorded run keeps five runs in each of its sixteen cells, in the order below", () => {
  const cells = Object.groupBy(RECORD.runs, cellKey);
  assert.deepEqual(Object.keys(cells), RECORDED_CELLS);
  assert.deepEqual(Object.values(cells).map((runs) => runs.length), Array(16).fill(5));
  assert.equal(RECORD.network.latency, 562.5);
});

test("validate returns no problem for the recorded run, shaped to Slow 4G or not", () => {
  assert.deepEqual(validateRecorded(), []);
  assert.deepEqual(validateRecorded(undefined, { network: null }), []);
});

test("each rule holds the recorded run by itself, stack by stack", () => {
  assert.deepEqual(checkRuns(RECORD.runs), []);
  TARGETS.forEach((t) => {
    const mine = RECORD.runs.filter((r) => r.stack === t.stack);
    assert.deepEqual(checkMinified(mine, t.expect, t.common), [], `${t.stack} minified`);
    assert.deepEqual(checkCompressed(mine, t.expect), [], `${t.stack} compressed`);
    assert.deepEqual(checkInteractive(mine, t.interactions), [], `${t.stack} interactive`);
    assert.deepEqual(checkWarm(mine, t.expect), [], `${t.stack} warm`);
    assert.deepEqual(checkShaped(mine, RECORD.network), [], `${t.stack} shaped`);
  });
});

test("checkMinified finds the untouched base.js of Svelte's build unminified in the recorded run, unless it is a common file", () => {
  const svelte = RECORD.runs.filter((r) => r.stack === "svelte");
  const { expect, common } = TARGETS[1];
  assert.deepEqual(checkMinified(svelte, expect, []), [minifiedMsg("http://localhost:3363/svelte/dist/base.js")]);
  assert.deepEqual(checkMinified(svelte, expect, common), []);
});

test("validate matches no Firefox response to the vendored build, Firefox reporting no body: the recorded Firefox runs alone say so", () => {
  const runs = RECORD.runs.filter((r) => r.engine === "firefox");
  const alone = validate({ runs, targets: TARGETS, network: RECORD.network, manifest: UPSTREAM_JSON });
  assert.deepEqual(alone, [`todomvc/react: ${NO_MATCH}`, `todomvc/svelte: ${NO_MATCH}`]);
});

test("checkWarm holds Chromium alone: Firefox's warm runs of the recorded run show Svelte's script received whole, and read as Chromium's they would be refused", () => {
  const svelte = RECORD.runs.filter((r) => r.stack === "svelte");
  const cold = svelte.find((r) => r.engine === "chromium" && r.condition === "cold" && !r.network);
  const firefoxWarm = svelte.filter((r) => r.engine === "firefox" && r.condition === "warm" && !r.network);
  assert.equal(firefoxWarm.length, 5);
  assert.deepEqual(checkWarm([cold, ...firefoxWarm], CACHES), []);
  const asChromium = firefoxWarm.map((r) => ({ ...r, engine: "chromium" }));
  const again = (i) => `warm: ${SVELTE_WARM} run ${i}: /svelte/dist/assets/index-BxPr2-fK.js was downloaded again (14802 bytes)`;
  assert.deepEqual(checkWarm([cold, ...asChromium], CACHES), [0, 1, 2, 3, 4].map(again));
});

test("validate refuses the recorded run with a run removed from a cell", () => {
  assert.deepEqual(validateRecorded((runs) => removeRun(runs, RECORDED_CELLS[0], 4)), [
    "runs: todomvc/react/chromium/cold/none has 4 run(s), fewer than 5",
    "runs: cells have different run counts (4, 5)",
  ]);
});

test("validate refuses the recorded run with a cell of one run", () => {
  const down = (runs) => [0, 1, 2, 3].forEach(() => removeRun(runs, "todomvc/svelte/webkit/warm/none", 0));
  assert.deepEqual(validateRecorded(down), [
    "runs: todomvc/svelte/webkit/warm/none has 1 run(s), fewer than 5",
    "runs: cells have different run counts (1, 5)",
  ]);
});

test("validate refuses the recorded run, whose cells hold five runs, when it asks for six", () => {
  assert.deepEqual(
    validateRecorded(undefined, { minRuns: 6 }),
    RECORDED_CELLS.map((key) => `runs: ${key} has 5 run(s), fewer than 6`),
  );
});

test("validate refuses the recorded run with an unminified request", () => {
  const edit = (runs) => patchRequest(runs, RECORDED_CELLS[0], 2, "/app.bundle.js", { minified: false });
  assert.deepEqual(validateRecorded(edit), [`todomvc/react: ${minifiedMsg(REACT_BUNDLE)}`]);
});

test("validate refuses the recorded run with an uncompressed answer", () => {
  const edit = (runs) => patchRequest(runs, "todomvc/svelte/webkit/cold/none", 1, "index-BxPr2-fK.js", { encoding: null });
  assert.deepEqual(validateRecorded(edit), [`todomvc/svelte: ${uncompressedMsg(SVELTE_BUNDLE, 42122, "text/javascript")}`]);
});

test("validate refuses the recorded run with a changed blob", () => {
  const edit = (runs) => patchRequest(runs, RECORDED_CELLS[0], 3, "/app.bundle.js", { blob: "0".repeat(40) });
  assert.deepEqual(validateRecorded(edit), [
    `todomvc/react: build: /react/dist/app.bundle.js is not upstream's react/dist/app.bundle.js (blob ${"0".repeat(40)}, upstream 08741691a2f21c53f816811aa5750f7448b1f376)`,
  ]);
});

test("validate refuses the recorded run with a file the vendored build does not have", () => {
  const edit = (runs) => patchRequest(runs, RECORDED_CELLS[0], 0, "/app.css", { url: "http://localhost:3362/react/dist/extra.css" });
  assert.deepEqual(validateRecorded(edit), ["todomvc/react: build: /react/dist/extra.css is not a file of the vendored build (react/dist/extra.css)"]);
});

test("validate refuses the recorded run with a document faster than the shaping, and asks it of no run when it is given no profile", () => {
  const edit = (runs) => patchRun(runs, REACT_SHAPED, 3, { timings: { documentEnd: 3, ready: 700 } });
  assert.deepEqual(validateRecorded(edit), [tooFast("3", 3, REACT_SHAPED)]);
  assert.deepEqual(validateRecorded(edit, { network: null }), []);
});

test("validate refuses the recorded run with an interaction that loaded a new document", () => {
  const edit = (runs) => patchRun(runs, RECORDED_CELLS[0], 1, { sameDocument: false });
  assert.deepEqual(validateRecorded(edit), ["interactive: todomvc/react/chromium/cold/none run 1: an interaction loaded a new document"]);
});

test("validate refuses the recorded run with an interaction that did not complete, and one with no time to become interactive", () => {
  const edit = (runs) => {
    patchRun(runs, "todomvc/react/webkit/cold/none", 0, { interactions: cellRuns(runs, "todomvc/react/webkit/cold/none")[0].interactions.slice(0, 2) });
    patchRun(runs, "todomvc/svelte/firefox/warm/none", 4, { timings: { documentEnd: 1, ready: null } });
  };
  assert.deepEqual(validateRecorded(edit), [
    "interactive: todomvc/react/webkit/cold/none run 0: 2 of 3 interactions completed",
    "interactive: todomvc/svelte/firefox/warm/none run 4: never interactive",
  ]);
});

test("validate refuses the recorded run with a warm run that downloaded its main script again, from half of what the cold run received", () => {
  const react = (receivedBody) => (runs) => patchRequest(runs, REACT_WARM, 2, "/app.bundle.js", { status: 200, receivedBody });
  const svelte = (receivedBody) => (runs) => patchRequest(runs, SVELTE_WARM, 0, "index-BxPr2-fK.js", { status: 200, receivedBody });
  const reactAgain = (bytes) => [`todomvc/react: warm: ${REACT_WARM} run 2: /react/dist/app.bundle.js was downloaded again (${bytes} bytes)`];
  const svelteAgain = ["todomvc/svelte: warm: todomvc/svelte/chromium/warm/none run 0: /svelte/dist/assets/index-BxPr2-fK.js was downloaded again (7401 bytes)"];
  assert.deepEqual(validateRecorded(react(64919)), reactAgain(64919));
  assert.deepEqual(validateRecorded(react(32460)), reactAgain(32460));
  assert.deepEqual(validateRecorded(react(32459)), []);
  assert.deepEqual(validateRecorded(svelte(7401)), svelteAgain);
  assert.deepEqual(validateRecorded(svelte(7400)), []);
});

test("validate refuses the recorded run with a shaped run that failed, which the rules tell twice", () => {
  const edit = (runs) => failRun(runs, SVELTE_SHAPED, 0, "page.goto: Timeout 60000ms exceeded.");
  assert.deepEqual(validateRecorded(edit), [
    "interactive: todomvc/svelte/chromium/cold/slow-4g run 0: the run failed: page.goto: Timeout 60000ms exceeded.",
    "shaped: todomvc/svelte/chromium/cold/slow-4g run 0: its document's last byte came at undefined ms, under the profile's 562.5 ms round trip",
  ]);
});

test("validate lists the problems of the recorded run broken in both stacks: the runs', then the first target's, then the second's", () => {
  const edit = (runs) => {
    patchRequest(runs, RECORDED_CELLS[0], 2, "/app.bundle.js", { minified: false });
    patchRequest(runs, "todomvc/svelte/webkit/cold/none", 1, "index-BxPr2-fK.js", { encoding: null });
    patchRun(runs, SVELTE_SHAPED, 0, { timings: { documentEnd: 3, ready: 700 } });
    patchRun(runs, "todomvc/react/firefox/warm/none", 1, { sameDocument: false });
    removeRun(runs, "todomvc/svelte/firefox/cold/none", 4);
  };
  assert.deepEqual(validateRecorded(edit), [
    "runs: todomvc/svelte/firefox/cold/none has 4 run(s), fewer than 5",
    "runs: cells have different run counts (4, 5)",
    `todomvc/react: ${minifiedMsg(REACT_BUNDLE)}`,
    "interactive: todomvc/react/firefox/warm/none run 1: an interaction loaded a new document",
    `todomvc/svelte: ${uncompressedMsg(SVELTE_BUNDLE, 42122, "text/javascript")}`,
    tooFast("3", 0, SVELTE_SHAPED),
  ]);
});

test("validate without a manifest takes every vendored file served for a foreign one, and matches none", () => {
  assert.deepEqual(validateRecorded(undefined, { manifest: undefined, network: null }), [
    "todomvc/react: build: /react/dist/ is not a file of the vendored build (react/dist/index.html)",
    "todomvc/react: build: /react/dist/app.css is not a file of the vendored build (react/dist/app.css)",
    "todomvc/react: build: /react/dist/base.js is not a file of the vendored build (react/dist/base.js)",
    "todomvc/react: build: /react/dist/app.bundle.js is not a file of the vendored build (react/dist/app.bundle.js)",
    `todomvc/react: ${NO_MATCH}`,
    "todomvc/svelte: build: /svelte/dist/ is not a file of the vendored build (svelte/dist/index.html)",
    "todomvc/svelte: build: /svelte/dist/assets/index-bgjkvPzV.css is not a file of the vendored build (svelte/dist/assets/index-bgjkvPzV.css)",
    "todomvc/svelte: build: /svelte/dist/base.js is not a file of the vendored build (svelte/dist/base.js)",
    "todomvc/svelte: build: /svelte/dist/assets/index-BxPr2-fK.js is not a file of the vendored build (svelte/dist/assets/index-BxPr2-fK.js)",
    `todomvc/svelte: ${NO_MATCH}`,
  ]);
});

test("validate checks every file of the manifest against the vendored copy for every vendored target, not the target's own files alone", () => {
  const disagreeing = structuredClone(UPSTREAM_JSON);
  disagreeing.files["react/dist/base.js"].blob = "0".repeat(40);
  const reactBase = "013aacecb4f9f2e87b04552cd8b571d53cf55c0d";
  const upstream = `upstream ${"0".repeat(40)}`;
  assert.deepEqual(validateRecorded(undefined, { manifest: disagreeing }), [
    `todomvc/react: build: /react/dist/base.js is not upstream's react/dist/base.js (blob ${reactBase}, ${upstream})`,
    `todomvc/react: build: the vendored react/dist/base.js is not upstream's (blob ${reactBase}, ${upstream})`,
    `todomvc/svelte: build: the vendored react/dist/base.js is not upstream's (blob ${reactBase}, ${upstream})`,
  ]);
  const ghostly = structuredClone(UPSTREAM_JSON);
  ghostly.files["react/dist/ghost.js"] = { blob: "1".repeat(40), bytes: 1 };
  assert.deepEqual(validateRecorded(undefined, { manifest: ghostly }), [
    "todomvc/react: build: react/dist/ghost.js is missing from the vendored copy",
    "todomvc/svelte: build: react/dist/ghost.js is missing from the vendored copy",
  ]);
});
