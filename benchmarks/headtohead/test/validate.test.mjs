// The rules a head-to-head run is held to (validate.mjs), each refusing the
// run it exists to refuse and accepting its neighbour. Every test here is a
// witness of a mutation control in scripts/head_to_head_mutations.py.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { gitBlob, sha256 } from "../lib.mjs";
import {
  MIN_RUNS,
  checkBuild,
  checkCompressed,
  checkInteractive,
  checkMinified,
  checkRuns,
  checkShaped,
  checkWarm,
} from "../validate.mjs";

const run = (over = {}) => ({
  app: "todomvc",
  stack: "react",
  engine: "chromium",
  condition: "cold",
  network: null,
  i: 0,
  sameDocument: true,
  timings: { ready: 10, documentEnd: 5 },
  interactions: [{ name: "add", toDom: 2, toFrame: 5 }],
  requests: [],
  ...over,
});
const cell = (n, over = {}) => Array.from({ length: n }, (_, i) => run({ i, ...over }));
const response = (over = {}) => ({
  phase: "load",
  url: "http://127.0.0.1:1/react/dist/app.bundle.js",
  kind: "script",
  status: 200,
  type: "text/javascript",
  raw: 5000,
  encoding: "br",
  minified: true,
  blob: "x",
  receivedBody: 1000,
  ...over,
});

test("a cell of one run is refused, and so is a cell of fewer runs than the rule asks", () => {
  assert.match(checkRuns(cell(1)).join("\n"), /has 1 run\(s\), fewer than 5/);
  assert.match(checkRuns(cell(MIN_RUNS - 1)).join("\n"), /fewer than/);
  assert.deepEqual(checkRuns(cell(MIN_RUNS)), []);
});

test("cells of different run counts are refused", () => {
  const runs = [...cell(7), ...cell(5, { stack: "svelte" })];
  assert.match(checkRuns(runs).join("\n"), /different run counts/);
  assert.deepEqual(checkRuns([...cell(7), ...cell(7, { stack: "svelte" })]), []);
});

test("an unminified script from a stack whose build minifies is refused; Pleris's is reported, not refused", () => {
  const runs = [run({ requests: [response({ minified: false })] })];
  assert.match(checkMinified(runs, { minifies: true }).join("\n"), /is not minified/);
  assert.deepEqual(checkMinified(runs, { minifies: false }), []);
  assert.deepEqual(checkMinified([run({ requests: [response()] })], { minifies: true }), []);
});

test("TodoMVC's common files are not held to a stack's minifier", () => {
  const runs = [run({ requests: [response({ url: "http://127.0.0.1:1/svelte/dist/base.js", minified: false })] })];
  assert.deepEqual(checkMinified(runs, { minifies: true }, [/\/base\.js$/]), []);
  assert.match(checkMinified(runs, { minifies: true }, []).join("\n"), /not minified/);
});

test("an uncompressed answer of 1 KiB or more from a host that compresses is refused", () => {
  const plain = [run({ requests: [response({ encoding: null })] })];
  assert.match(checkCompressed(plain, { compresses: "all" }).join("\n"), /sent uncompressed/);
  assert.deepEqual(checkCompressed(plain, { compresses: false }), []);
  const small = [run({ requests: [response({ encoding: null, raw: 900 })] })];
  assert.deepEqual(checkCompressed(small, { compresses: "all" }), []);
  const doc = [run({ requests: [response({ encoding: null, kind: "document", type: "text/html" })] })];
  assert.deepEqual(checkCompressed(doc, { compresses: "static" }), []);
  assert.match(checkCompressed(doc, { compresses: "all" }).join("\n"), /uncompressed/);
});

function vendoredTree() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-vendored-"));
  fs.mkdirSync(path.join(root, "react/dist"), { recursive: true });
  const body = Buffer.from("console.log(1)\n");
  fs.writeFileSync(path.join(root, "react/dist/app.bundle.js"), body);
  return { root, manifest: { files: { "react/dist/app.bundle.js": { blob: gitBlob(body) } } }, blob: gitBlob(body) };
}

test("a served file that is not the vendored upstream build is refused (a metric computed on the wrong build)", () => {
  const { root, manifest, blob } = vendoredTree();
  const build = { kind: "vendored", root, prefix: "/react/dist/", dir: "react/dist" };
  assert.deepEqual(checkBuild(build, [response({ blob })], { manifest }), []);
  assert.match(checkBuild(build, [response({ blob: "0".repeat(40) })], { manifest }).join("\n"), /is not upstream's/);
  fs.writeFileSync(path.join(root, "react/dist/app.bundle.js"), "changed\n");
  assert.match(checkBuild(build, [response({ blob })], { manifest }).join("\n"), /the vendored react\/dist\/app.bundle.js is not upstream's/);
  assert.match(checkBuild(build, [response({ url: "http://127.0.0.1:1/react/dist/extra.js", blob })], { manifest }).join("\n"), /not a file of the vendored build/);
});

test("a Pleris build whose sources changed since it was built is refused", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-pleris-"));
  const source = path.join(dir, "app.pw");
  fs.writeFileSync(source, "module app\n");
  const dist = path.join(dir, "dist");
  fs.mkdirSync(dist);
  fs.writeFileSync(path.join(dist, "sources.json"), JSON.stringify({ files: [{ path: source, sha256: sha256(fs.readFileSync(source)) }] }));
  fs.writeFileSync(path.join(dist, "pw-runtime.mjs"), "runtime\n");
  const build = { kind: "pleris", dist, sources: path.join(dist, "sources.json") };
  const runtime = response({ url: "http://127.0.0.1:1/pw-runtime.mjs", blob: gitBlob(Buffer.from("runtime\n")) });
  assert.deepEqual(checkBuild(build, [runtime]), []);
  assert.match(checkBuild(build, [response({ url: "http://127.0.0.1:1/pw-runtime.mjs", blob: "f".repeat(40) })]).join("\n"), /served is not the build's/);
  fs.writeFileSync(source, "module app\n// changed\n");
  assert.match(checkBuild(build, [runtime]).join("\n"), /changed since it was built/);
  fs.rmSync(path.join(dist, "sources.json"));
  assert.match(checkBuild(build, [runtime]).join("\n"), /the build did not finish/);
});

test("a framework's build older than one of its sources is refused, and a page that names another build", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-next-"));
  fs.mkdirSync(path.join(dir, ".next"));
  const marker = path.join(dir, ".next/BUILD_ID");
  fs.writeFileSync(marker, "abc123\n");
  const source = path.join(dir, "page.tsx");
  fs.writeFileSync(source, "x");
  const old = new Date(Date.now() - 60_000);
  fs.utimesSync(source, old, old);
  const build = { kind: "next", dir };
  const doc = response({ kind: "document", type: "text/html", text: '..."b":"abc123"...' });
  assert.deepEqual(checkBuild(build, [doc], { sources: [source] }), []);
  assert.match(checkBuild(build, [response({ kind: "document", text: "other build" })], { sources: [source] }).join("\n"), /does not name the build/);
  const later = new Date(Date.now() + 60_000);
  fs.utimesSync(source, later, later);
  assert.match(checkBuild(build, [doc], { sources: [source] }).join("\n"), /is newer than the build/);
});

test("a shaped run whose document came faster than the profile's round trip is refused (shaping on one side only)", () => {
  const network = { latency: 150 };
  assert.match(checkShaped([run({ network: "slow-4g", timings: { documentEnd: 3 } })], network).join("\n"), /under the profile's 150 ms/);
  assert.deepEqual(checkShaped([run({ network: "slow-4g", timings: { documentEnd: 160 } })], network), []);
  assert.deepEqual(checkShaped([run({ timings: { documentEnd: 3 } })], network), []);
});

test("an interaction that loaded a new document, or did not complete, is refused", () => {
  const steps = [{ name: "add" }];
  assert.deepEqual(checkInteractive([run()], steps), []);
  assert.match(checkInteractive([run({ sameDocument: false })], steps).join("\n"), /loaded a new document/);
  assert.match(checkInteractive([run({ interactions: [] })], steps).join("\n"), /0 of 1 interactions/);
  assert.match(checkInteractive([run({ timings: { ready: null } })], steps).join("\n"), /never interactive/);
});

test("a warm run that downloaded its main script again is refused where its host lets a cache keep it", () => {
  const cold = run({ requests: [response({ receivedBody: 60000, raw: 200000 })] });
  const warmAgain = run({ condition: "warm", requests: [response({ receivedBody: 60000 })] });
  const warmCached = run({ condition: "warm", requests: [response({ receivedBody: 0 })] });
  assert.match(checkWarm([cold, warmAgain], { caches: true }).join("\n"), /was downloaded again/);
  assert.deepEqual(checkWarm([cold, warmCached], { caches: true }), []);
  assert.deepEqual(checkWarm([cold, warmAgain], { caches: false }), []);
});
