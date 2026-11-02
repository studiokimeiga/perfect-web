// The report's other branches, pinned on the record (`report.test.mjs`): each
// variant in `fixtures/recorded-run/variants.mjs` breaks the recorded run in
// one way it does not itself (a run that failed, an aborted request, servers
// not measured, a number no run reported, ...), and its report differs from
// the record's by the lines `variants.json` holds, made by the report as it
// was written when the record was made. A change to how the report reads a
// failed run, a missing count or a legacy list must show here first.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { bytesOf, renderReport } from "../report.mjs";
import { VARIANTS, delta, recorded } from "./fixtures/recorded-run/variants.mjs";

const RECORD = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures/recorded-run");
const GOLDEN = JSON.parse(fs.readFileSync(path.join(RECORD, "variants.json"), "utf8"));

function report(change) {
  const run = recorded();
  change(run);
  return renderReport(run.result, run.sections);
}

test("every variant of the recorded run has its lines pinned, and no line is pinned for a variant that is gone", () => {
  assert.deepEqual(Object.keys(GOLDEN), Object.keys(VARIANTS));
});

Object.entries(VARIANTS).forEach(([name, change]) => {
  test(`the recorded run, with ${name}, is reported as the record is but for the lines it changes`, () => {
    assert.deepEqual(delta(report(() => {}), report(change)), GOLDEN[name]);
  });
});

const response = (phase, url, over = {}) => ({
  phase,
  url,
  method: "GET",
  kind: "script",
  status: 200,
  type: "text/javascript",
  encoding: "br",
  raw: 100,
  br: 40,
  gzip: 50,
  receivedBody: 30,
  receivedHeaders: 10,
  ...over,
});

const INTERACTIONS = [
  response("load", "http://h/a.js"),
  response("interaction:add", "http://h/b.js", { raw: 10, br: 4, gzip: 5, encoding: null }),
  response("interaction:toggle", "http://h/c.css", { type: "text/css", kind: "style", status: 304, raw: undefined }),
  response("interaction:add", "http://h/stream", { failed: "net::ERR_ABORTED" }),
];

const asObject = (counted) => JSON.parse(JSON.stringify(counted, (key, value) => (value instanceof Set ? [...value] : value)));

test("the phase 'interactions' counts every interaction's responses together, Playwright's bytes, an aborted request apart", () => {
  assert.deepEqual(asObject(bytesOf({ requests: INTERACTIONS }, "interactions")), {
    requests: 2,
    aborted: 1,
    raw: 10,
    br: 44,
    gzip: 55,
    received: 80,
    byKind: { script: { requests: 1, raw: 10, br: 4 }, style: { requests: 1, raw: 0, br: 40 } },
    encodings: "br",
    notModified: 1,
    source: "playwright",
  });
});

test("a response with no Playwright byte counts adds none to the bytes received", () => {
  const bare = response("load", "http://h/a.js", { receivedBody: undefined, receivedHeaders: undefined });
  assert.equal(bytesOf({ requests: [bare] }, "load").received, 0);
  assert.equal(bytesOf({ requests: [{ ...bare, receivedBody: 5 }] }, "load").received, 5);
  assert.equal(bytesOf({ requests: [{ ...bare, receivedHeaders: 7 }] }, "load").received, 7);
});

test("a run with no requests counts nothing, and its encodings are 'none'", () => {
  assert.deepEqual(asObject(bytesOf({}, "load")), {
    requests: 0,
    aborted: 0,
    raw: 0,
    br: 0,
    gzip: 0,
    received: 0,
    byKind: {},
    encodings: "none",
    notModified: 0,
    source: "playwright",
  });
});

test("Chromium's byte counts are the received bytes of the phases asked for, an entry with no phases counting nothing", () => {
  const received = [{ url: "http://h/a.js", byPhase: { load: 7, "interaction:add": 3 } }, { url: "http://h/x.js" }];
  const counted = asObject(bytesOf({ requests: INTERACTIONS, received }, "interactions", [], {}));
  assert.deepEqual([counted.received, counted.source, counted.requests, counted.aborted], [3, "chromium", 2, 1]);
});
