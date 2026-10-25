// The head-to-head's instrument, piece by piece: the static host's policy,
// sizes and blobs, the minified verdict, the statistics, the report's byte
// counts and its losses, a patch's statistics, the museum's record read.
// Each test is a witness of a mutation control in
// scripts/head_to_head_mutations.py.
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { brotliDecompressSync, gunzipSync } from "node:zlib";
import { diffStats } from "../feature-change.mjs";
import { gitBlob, minifiedVerdict, quantile, shuffled, sizes, summary } from "../lib.mjs";
import { parseEvidence } from "../museum-classes.mjs";
import { bytesOf, compareSpread, lossesFirst } from "../report.mjs";
import { cacheControl, chooseEncoding, createHost } from "../static-host.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

function serve(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-host-"));
  for (const [name, body] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), body);
  }
  const { server } = createHost({ mounts: [{ prefix: "/app/dist", dir }] });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve({ server, base: `http://127.0.0.1:${server.address().port}` })));
}

const BIG = "function f(){return 1}\n".repeat(400);

/** An answer's status, headers and bytes as sent: no client decompresses them. */
function raw(url, headers) {
  return new Promise((resolve, reject) => {
    http
      .get(url, { headers }, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      })
      .on("error", reject);
  });
}

test("the static host sends Brotli where it is accepted, gzip where only it is, and the bytes as they are otherwise", async () => {
  const { server, base } = await serve({ "app.js": BIG });
  try {
    const br = await raw(`${base}/app/dist/app.js`, { "accept-encoding": "br, gzip" });
    assert.equal(br.headers["content-encoding"], "br");
    assert.equal(brotliDecompressSync(br.body).toString(), BIG);
    assert.ok(br.body.length < BIG.length / 10);
    const gz = await raw(`${base}/app/dist/app.js`, { "accept-encoding": "gzip" });
    assert.equal(gz.headers["content-encoding"], "gzip");
    assert.equal(gunzipSync(gz.body).toString(), BIG);
    const plain = await raw(`${base}/app/dist/app.js`, { "accept-encoding": "identity" });
    assert.equal(plain.headers["content-encoding"], undefined);
    assert.equal(plain.body.toString(), BIG);
  } finally {
    server.close();
  }
});

test("the static host answers a matching validator 304, a hashed name immutable and any other no-cache", async () => {
  const { server, base } = await serve({ "app.js": BIG, "assets/index-BxPr2-fK.js": BIG });
  try {
    const first = await fetch(`${base}/app/dist/app.js`);
    await first.arrayBuffer();
    assert.equal(first.headers.get("cache-control"), "no-cache");
    const again = await fetch(`${base}/app/dist/app.js`, { headers: { "if-none-match": first.headers.get("etag") } });
    assert.equal(again.status, 304);
    const hashed = await fetch(`${base}/app/dist/assets/index-BxPr2-fK.js`);
    await hashed.arrayBuffer();
    assert.equal(hashed.headers.get("cache-control"), "public, max-age=31536000, immutable");
    const dir = await fetch(`${base}/app/dist`, { redirect: "manual" });
    assert.equal(dir.status, 301);
    assert.equal(dir.headers.get("location"), "/app/dist/");
  } finally {
    server.close();
  }
  assert.equal(cacheControl("index.html"), "no-cache");
  assert.equal(chooseEncoding("gzip;q=0, br;q=0"), null);
  assert.equal(chooseEncoding("*"), "br");
});

test("sizes are the bytes and their Brotli 11 and gzip 9 compressions; a blob is git's", () => {
  const body = Buffer.from(BIG);
  const s = sizes(body);
  assert.equal(s.raw, body.length);
  assert.ok(s.br < s.gzip && s.gzip < s.raw, JSON.stringify(s));
  assert.equal(gitBlob(Buffer.from("hello\n")), "ce013625030ba8dba906f756967f9e9ca394464a");
  assert.equal(gunzipSync(Buffer.from([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])).length, 0);
});

test("a script a minifier wrote is minified; one with its indentation and comments is not", () => {
  // Real files: Svelte's build (Vite's minifier), and todomvc-common's
  // base.js as that build copies it, untouched.
  const svelte = path.join(HERE, "../todomvc/svelte/dist");
  const built = minifiedVerdict(fs.readFileSync(path.join(svelte, "assets/index-BxPr2-fK.js"), "utf8"));
  assert.equal(built.minified, true, JSON.stringify(built));
  const copied = minifiedVerdict(fs.readFileSync(path.join(svelte, "base.js"), "utf8"));
  assert.equal(copied.minified, false, JSON.stringify(copied));
  assert.ok(copied.share > 0.3, JSON.stringify(copied));
  assert.equal(minifiedVerdict(fs.readFileSync(path.join(svelte, "assets/index-bgjkvPzV.css"), "utf8"), "text/css").minified, true);
  assert.equal(minifiedVerdict(`.app {\n  /* the page */\n  color: red;\n  margin: 0 auto;\n}\n`.repeat(30), "text/css").minified, false);
  assert.equal(minifiedVerdict(".a { color: red; }\n").minified, true);
});

test("a summary's median and quartiles are by linear interpolation, and a missing value is counted, not zero", () => {
  assert.equal(quantile([1, 2, 3, 4], 0.5), 2.5);
  const s = summary([4, null, 1, 3, 2]);
  assert.deepEqual([s.n, s.missing, s.median, s.q1, s.q3, s.min, s.max], [4, 1, 2.5, 1.75, 3.25, 1, 4]);
});

test("a run's order is shuffled from its seed, the same seed the same order", () => {
  const xs = ["pleris", "react", "svelte"];
  assert.deepEqual(shuffled(xs, 7), shuffled(xs, 7));
  const orders = new Set(Array.from({ length: 20 }, (_, i) => shuffled(xs, i + 1).join(" ")));
  assert.ok(orders.size > 1, "every seed gave one order");
});

test("a phase's bytes count its responses, an aborted request apart, and Chromium's bytes where they arrived", () => {
  const r = {
    requests: [
      { phase: "load", url: "http://h/a.js", status: 200, type: "text/javascript", raw: 100, br: 40, gzip: 50, receivedBody: 40, receivedHeaders: 10 },
      { phase: "load", url: "http://h/stream", failed: "net::ERR_ABORTED" },
      { phase: "interaction:add", url: "http://h/h.mjs", status: 200, type: "text/javascript", raw: 10, br: 8, gzip: 9, receivedBody: 10, receivedHeaders: 5 },
    ],
  };
  const load = bytesOf(r, "load");
  assert.deepEqual([load.requests, load.aborted, load.raw, load.received], [1, 1, 100, 50]);
  const chromium = { ...r, received: [{ url: "http://h/stream", byPhase: { load: 7, "interaction:add": 30 } }, { url: "http://h/a.js", byPhase: { load: 60 } }] };
  assert.equal(bytesOf(chromium, "load").received, 67);
  assert.equal(bytesOf(chromium, "interaction:add").received, 30);
});

test("times differ only where their spreads do not overlap", () => {
  const s = (q1, median, q3) => ({ n: 7, q1, median, q3 });
  assert.equal(compareSpread(s(30, 32, 34), s(10, 12, 14)), "loses");
  assert.equal(compareSpread(s(10, 12, 14), s(30, 32, 34)), "wins");
  assert.equal(compareSpread(s(10, 20, 30), s(25, 27, 29)), "within the spread");
  // A higher median is no loss while the quartiles overlap.
  assert.equal(compareSpread(s(20, 28, 36), s(10, 25, 30)), "within the spread");
});

test("a row in which another stack does better is a loss, and one in which Pleris is best of all is a win", () => {
  const result = { apps: [] };
  const sections = {
    loc: {
      counts: {
        store: {
          pleris: { app: { lines: 500, files: [1, 2], languages: { Pleris: 500 } }, host: { lines: 2000, languages: { Rust: 2000 } }, build: { lines: 9 } },
          "next-react": { app: { lines: 150, files: [1], languages: { TypeScript: 150 } }, host: { lines: 0, languages: {} }, build: { lines: 60 } },
        },
      },
    },
  };
  const { losses, wins } = lossesFirst(result, sections);
  assert.ok(losses.some((l) => /app lines: Pleris 500 \| Next.js 150/.test(l)), losses.join("\n"));
  assert.ok(wins.some((w) => /build and configuration lines: Pleris 9 \| Next.js 60/.test(w)), wins.join("\n"));
});

test("a patch's statistics read each hunk by its counts: a content line that begins like a header is content", () => {
  const patch = [
    "diff --git a/lib/a.ts b/lib/a.ts",
    "--- a/lib/a.ts",
    "+++ b/lib/a.ts",
    "@@ -1,3 +1,3 @@",
    " keep",
    "--- old header-like line",
    "+++ new header-like line",
    " keep",
    "diff --git a/new.svelte b/new.svelte",
    "new file mode 100644",
    "--- /dev/null",
    "+++ b/new.svelte",
    "@@ -0,0 +1,2 @@",
    "+<p>one</p>",
    "+<p>two</p>",
    "",
  ].join("\n");
  const d = diffStats(patch);
  assert.deepEqual([d.files, d.created, d.added, d.removed], [2, 1, 3, 1]);
  assert.deepEqual(d.languages, ["Svelte", "TypeScript"]);
});

test("the museum's record is read per exhibit: where its bug was shown, its repair, and Pleris's verdict", () => {
  const text = [
    "== G01-retried-command-twice: One press applied twice",
    "   next-react, the bug: 1 passed, 0 failed",
    "   next-react, repaired: 0 passed, 1 failed (the spec fails: it detects the bug)",
    "   => refused (diagnostic PW0338)",
    "",
  ].join("\n");
  const e = parseEvidence(text)["G01-retried-command-twice"];
  assert.deepEqual(e.bug, { stack: "next-react", passed: 1, failed: 0 });
  assert.equal(e.repaired.failed, 1);
  assert.equal(e.verdict, "refused (diagnostic PW0338)");
});
