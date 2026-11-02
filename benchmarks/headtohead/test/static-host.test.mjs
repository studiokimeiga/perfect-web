// The static host (static-host.mjs), pinned as it is: the encoding it chooses,
// the cache policy it gives a name, the compressions it makes, the arguments
// it reads, the files it loads, everything its request handler answers over
// HTTP (and the log line each answer leaves), and the program itself, run.
// Whole values throughout, so that no part of what the host does can move
// unseen when its functions are split: where the host does something that
// looks like a mistake, the test says so and pins it all the same.
//
// A body is compared by its length and SHA-256 (`digest`), never as a Buffer:
// when two Buffers differ throughout, node's assertion diff grows with the
// square of their size (about a second for 9 KB), so a failing comparison of
// a bundle or a compression would look like a hang.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { brotliCompressSync, brotliDecompressSync, constants, gunzipSync, gzipSync } from "node:zlib";
import { cacheControl, chooseEncoding, compressions, createHost, loadFiles, parseArgs } from "../static-host.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOST = path.join(HERE, "../static-host.mjs");
const VENDORED = path.join(HERE, "../todomvc");
const MOUNT = "/app/dist";
const IMMUTABLE = "public, max-age=31536000, immutable";
const HTML_UTF8 = "text/html; charset=utf-8";
const JS_UTF8 = "text/javascript; charset=utf-8";
const CSS_UTF8 = "text/css; charset=utf-8";
const TEXT_UTF8 = "text/plain; charset=utf-8";
const OCTETS = "application/octet-stream";

/** A body as a short string that a failed comparison can print: its length and SHA-256. */
const digest = (body) => `${body.length} bytes, sha256 ${createHash("sha256").update(body).digest("hex")}`;

// --- the site the host is run over ------------------------------------------------

// Modelled on todomvc/svelte/dist/index.html, which Vite wrote.
const INDEX = [
  "<!DOCTYPE html>",
  '<html lang="en" data-framework="svelte">',
  "  <head>",
  '    <meta charset="UTF-8" />',
  "    <title>TodoMVC: Svelte</title>",
  '    <script type="module" crossorigin src="./assets/index-BxPr2-fK.js"></script>',
  '    <link rel="stylesheet" crossorigin href="./assets/index-bgjkvPzV.css">',
  "  </head>",
  '  <body><section class="todoapp"></section><script src="./base.js"></script></body>',
  "</html>",
  "",
].join("\n");
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52]);
const ICO = Buffer.from([0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x10, 0x10]);
const WASM = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
const WOFF2 = Buffer.from([0x77, 0x4f, 0x46, 0x32, 0x00, 0x01, 0x00, 0x00]);

const file = (body, type, cache, etag, compressed) => ({ body, type, cache, etag, compressed });

/**
 * Every extension in the host's table and some it has none for: each file's
 * bytes, and what the host must make of it (its type, cache policy, ETag and
 * whether it is compressible). The ETags are the SHA-256 of the bytes in
 * base64url, in double quotes, as sha256sum and base64 give them: not the
 * host's own computation.
 */
const SITE = {
  "index.html": file(INDEX, HTML_UTF8, "no-cache", '"AZa6Xrwna5kZH4JMBqePgtTIjto4JFYu6CL1cYTigF4"', true),
  "app.bundle.js": file("function f(){return 1}\n".repeat(400), JS_UTF8, "no-cache", '"d8DSl3alkzjpyYuaJ63xn-IqlOXI_7raMnfNODy7SGA"', true),
  "module.mjs": file("export default 1;\n".repeat(100), JS_UTF8, "no-cache", '"_1wmB6UsGKucifQf6BZOjt1ZnZC3NOQ4bA-KlaU2Au4"', true),
  "assets/index-BxPr2-fK.js": file("export const a = 1;\n".repeat(300), JS_UTF8, IMMUTABLE, '"YjUsB0QtGMMA3x6cOyRJEi1i1ahhASCiTJyDPy6MGGY"', true),
  "assets/index-bgjkvPzV.css": file(".app { color: red; }\n".repeat(200), CSS_UTF8, IMMUTABLE, '"uQpgobvXGrBL79kthoVjt2002EhQeJVIG904zq5dfgY"', true),
  "assets/font-1a2b3c4d.woff2": file(WOFF2, OCTETS, IMMUTABLE, '"v-cnfTgSucZ87VdIUCqy4gMz2jStwSzL6aKH-5Smjiw"', false),
  "assets/index-BxPr2-fK.js.map": file('{"version":3,"sources":["index.js"],"mappings":"AAAA"}\n', OCTETS, "no-cache", '"qs5LbkoukT61UDShTlyIWkwDDbvM8SJW3PW3FTItFq4"', false),
  "data/learn.json": file('{"todomvc":{"name":"TodoMVC"}}\n', "application/json; charset=utf-8", "no-cache", '"3nhRvpcI13p5OiSKCvNzB_3-qbpTUmJMtTON-f9WG2g"', true),
  "icon.svg": file('<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>\n', "image/svg+xml", "no-cache", '"OPr0FTdQ_bPYtKw8NGUNzkwhKPXHsdzgwfXvtcJSKAk"', true),
  "m.wasm": file(WASM, "application/wasm", "no-cache", '"k6RLu5bHUSGOTADUeeTBQ1gSKjiazKFiBbHk0NxflHY"', true),
  "notes.txt": file("hello\n", TEXT_UTF8, "no-cache", '"WJG1tSLV3whtD_CxEPvZ0hu0_HFjrzTQgoai6Eb2vgM"', true),
  "pic.png": file(PNG, "image/png", "no-cache", '"AqPimPFTP2JVjFjkxw7cq5r1pQ1i2SX9U5CUICD7D7g"', false),
  "favicon.ico": file(ICO, "image/x-icon", "no-cache", '"wCrltzwEMnhKZLUYMMKHdtmd-wBO15omhVClTd5GMH4"', false),
  LICENSE: file("MIT\n", OCTETS, "no-cache", '"rcNzZvQDg1wUcKst-T04N9Rxk3L8HvhZPZIuBvAz-LI"', false),
  ".htaccess": file("Options -Indexes\n", OCTETS, "no-cache", '"dNfAoFfMak5LdidX-PaRlKEkkCi_8-3o7YCmbwZJTSE"', false),
  "UPPER.JS": file("var a;\n", OCTETS, "no-cache", '"zFY_SYnujNUANg3Q3oPO3lH0srEzvQq7QSl6xiz1CF4"', false),
  "my file.txt": file("spaced\n", TEXT_UTF8, "no-cache", '"lvqhhWj43m0r4JJyZdTzFzJFZLQcoCGIulQwI0qHhg0"', true),
  "café.txt": file("accent\n", TEXT_UTF8, "no-cache", '"j435ljyWKHQb_urH77c5Fk0IWP0D6xlQ84W7JlEs71U"', true),
  "deep/er/leaf.txt": file("leaf\n", TEXT_UTF8, "no-cache", '"JtC6yfDHo1svMyKg9K1FFyZfVrLA9LLtfLXL0wxYaOI"', true),
  "my dir/inner.txt": file("inner\n", TEXT_UTF8, "no-cache", '"lApoEE07aQRCRT9L45SwoUchoXQSfYTBwvg0t60F1oQ"', true),
};
const NAMES = Object.keys(SITE);
const TREE = Object.fromEntries(NAMES.map((name) => [name, SITE[name].body]));
const bytes = (name) => Buffer.from(SITE[name].body);

/** Brotli at quality 11 and gzip at level 9, stated here again, not read from the host. */
const REFERENCE = {
  br: (body) =>
    brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: body.length } }),
  gzip: (body) => gzipSync(body, { level: 9 }),
};
const DECOMPRESS = { br: brotliDecompressSync, gzip: gunzipSync };

// --- what a host answers, as whole values ------------------------------------------

const EMPTY = Buffer.alloc(0);

/** The bytes a file is sent as: its own, or its compression in an encoding. */
const bytesSent = (name, encoding = null) => (encoding ? REFERENCE[encoding](bytes(name)) : bytes(name));

/** The headers a file's validators and type carry on a 200 and on a 304. */
function fileHeaders(name) {
  const { type, cache, etag, compressed } = SITE[name];
  const headers = { "content-type": type, "cache-control": cache, etag, "x-content-type-options": "nosniff" };
  return compressed ? { ...headers, vary: "Accept-Encoding" } : headers;
}

/** An answer as the tests compare it: its status, its headers (none but the host's own) and its body's digest. */
const answer = (status, headers, body) => ({ status, headers, body: digest(body) });

/** The 200 for a file, plain or in an encoding: its length is the length of the bytes sent. */
function served(name, encoding = null) {
  const sent = bytesSent(name, encoding);
  const encoded = encoding ? { "content-encoding": encoding } : {};
  return answer(200, { ...fileHeaders(name), ...encoded, "content-length": String(sent.length) }, sent);
}

/** What a HEAD gets of a GET's answer: all its headers and none of its body. */
const withoutBody = (expected) => ({ ...expected, body: digest(EMPTY) });
const notModified = (name) => answer(304, fileHeaders(name), EMPTY);
const redirect = (location) => answer(301, { location, "content-length": "0" }, EMPTY);
const refused = (status) => answer(status, { "content-type": TEXT_UTF8, "content-length": "9" }, Buffer.from("not found"));

/** The log's line for an answer, as JSON.stringify writes it. */
const entry = (method, target, status, encoding, sent) =>
  `{"method":"${method}","path":"${target}","status":${status},"encoding":${JSON.stringify(encoding)},"bytes":${sent}}`;

// --- trees, hosts and a client -----------------------------------------------------

function put(dir, name, body) {
  fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
  fs.writeFileSync(path.join(dir, name), body);
}

/** A temporary directory with a tree in `site/` (the mount) and room beside it for a log. */
function makeSite(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-static-host-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const dir = path.join(root, "site");
  fs.mkdirSync(dir);
  Object.entries(files).forEach(([name, body]) => put(dir, name, body));
  return { root, dir, log: path.join(root, "requests.jsonl") };
}

/** Closes a server, its connections with it. */
function stop(server) {
  return new Promise((resolve) => {
    server.close(resolve);
    server.closeAllConnections?.();
  });
}

async function startHost(t, options) {
  const host = createHost(options);
  host.server.listen(0, "127.0.0.1");
  await once(host.server, "listening");
  t.after(() => stop(host.server));
  return { ...host, port: host.server.address().port };
}

/** The tree on disk, served at /app/dist with a log, once `prepare` has changed it. */
async function openSite(t, { files = TREE, prepare = () => {} } = {}) {
  const site = makeSite(t, files);
  prepare(site);
  const host = await startHost(t, { mounts: [{ prefix: MOUNT, dir: site.dir }], log: site.log });
  return { ...site, ...host };
}

const ADDED_BY_NODE = new Set(["date", "connection", "keep-alive"]);
const ownHeaders = (headers) => Object.fromEntries(Object.entries(headers).filter(([name]) => !ADDED_BY_NODE.has(name)));

function collect(res, resolve) {
  const chunks = [];
  res.on("data", (chunk) => chunks.push(chunk));
  res.on("end", () => resolve({ status: res.statusCode, headers: ownHeaders(res.headers), body: Buffer.concat(chunks) }));
}

/** One request on a connection of its own; the answer as sent, no client decompressing it. */
function exchange(port, method, target, headers = {}) {
  return new Promise((resolve, reject) => {
    const options = { host: "127.0.0.1", port, method, path: target, headers, agent: false, timeout: 5000 };
    const req = http.request(options, (res) => collect(res, resolve));
    req.on("timeout", () => req.destroy(new Error(`${method} ${target}: no answer in 5 s`)));
    req.on("error", reject);
    req.end();
  });
}

const seen = (raw) => answer(raw.status, raw.headers, raw.body);
const request = async (...args) => seen(await exchange(...args));

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const readLines = (log) => (fs.existsSync(log) ? fs.readFileSync(log, "utf8").split("\n").filter(Boolean) : []);

/** The log's lines once `count` are there: the host writes its log after it answers. */
async function logLines(log, count) {
  for (let waited = 0; waited < 4000; waited += 5) {
    if (readLines(log).length >= count) break;
    await sleep(5);
  }
  return readLines(log);
}

/** The requests in order, on a site opened by `openSite`: the answers, and the log's lines. */
async function visit(site, requests) {
  const answers = [];
  for (const [method, target, headers] of requests) answers.push(await request(site.port, method, target, headers));
  return { answers, lines: await logLines(site.log, requests.length) };
}

// --- chooseEncoding -----------------------------------------------------------------

function assertChoices(cases) {
  cases.forEach(([accept, chosen]) => assert.equal(chooseEncoding(accept), chosen, `Accept-Encoding: ${JSON.stringify(accept)}`));
}

test("the static host chooses no encoding when none is asked for, or only encodings it cannot make", () => {
  assert.equal(chooseEncoding(), null);
  assertChoices([
    [undefined, null],
    ["", null],
    [" ", null],
    [",", null],
    [" , , ", null],
    ["identity", null],
    ["deflate", null],
    ["compress", null],
    ["x-gzip", null],
    ["identity, deflate", null],
    // Names of Object.prototype's members are encodings like any other.
    ["constructor", null],
    ["__proto__;q=0, toString, hasOwnProperty", null],
    ["__proto__, gzip", "gzip"],
  ]);
});

test("the static host chooses Brotli where it is acceptable, gzip where only it is, and Brotli when both are", () => {
  assertChoices([
    ["br", "br"],
    ["gzip", "gzip"],
    ["br, gzip", "br"],
    ["gzip, br", "br"],
    ["gzip, deflate, br", "br"],
    ["deflate, gzip", "gzip"],
    ["gzip;q=0.5", "gzip"],
    ["br;q=0", null],
    ["br;q=0, gzip", "gzip"],
    ["br;q=0, gzip;q=0", null],
    ["gzip;q=0, br;q=0", null],
  ]);
});

test("the static host reads an Accept-Encoding without regard to the case of its names, its spaces, or its empty entries", () => {
  assertChoices([
    ["BR", "br"],
    ["GZIP", "gzip"],
    ["Gzip", "gzip"],
    ["bR, GzIp", "br"],
    ["BR;q=0, GZIP", "gzip"],
    [" br ", "br"],
    ["\tbr\t", "br"],
    [" gzip , deflate ", "gzip"],
    ["br ; q=0", null],
    ["br ; q=0 , gzip", "gzip"],
    ["br;q=0 ", null],
    [",br", "br"],
    ["br,", "br"],
    [",,gzip,,", "gzip"],
    ["gzip,,br", "br"],
    [", ,br", "br"],
  ]);
});

test("the static host refuses an encoding whose q is zero or no number, accepts any other, and ranks nothing by q", () => {
  assertChoices([
    ["br;q=0", null],
    ["br;q=0.0", null],
    ["br;q=0.000", null],
    ["br;q=-1", null],
    ["br;q=0.001", "br"],
    ["br;q=1", "br"],
    ["br;q=1.5", "br"],
    ["br;q=.5", "br"],
    ["br;q=1e-1", "br"],
    ["br;q=0x10", "br"],
    ["br;q=Infinity", "br"],
    ["br;q= 0.5", "br"],
    ["br;q=abc", null],
    ["br;q=", null],
    ["br;q=NaN", null],
    ["gzip;q=abc", null],
    ["br;q=abc, gzip", "gzip"],
    ["br;q=abc, *", "gzip"],
    ["gzip;q=1, br;q=0.1", "br"],
    ["gzip, br;q=0.001", "br"],
  ]);
});

test("the static host reads the first q among an entry's parameters, and only a lowercase q= is a weight", () => {
  assertChoices([
    ["br;level=5;q=0.5", "br"],
    ["br;level=5;q=0", null],
    ["br;q=0;level=5", null],
    ["br ;q=0.5 ; level=1", "br"],
    ["br;q=0;q=1", null],
    ["br;q=1;q=0", "br"],
    // Not weights, so the entry counts as q=1: an uppercase Q, a space before
    // the equals sign, a longer name, a q with no value, another parameter.
    ["br;Q=0", "br"],
    ["br;q =0", "br"],
    ["br;qq=0", "br"],
    ["br;q", "br"],
    ["br;level=0", "br"],
  ]);
});

test("the static host lets the wildcard stand for the encodings not named, and a named encoding's own q win over it", () => {
  assertChoices([
    ["*", "br"],
    ["*;q=0", null],
    ["*;q=0.1", "br"],
    ["*;q=abc", null],
    ["identity, *", "br"],
    ["*;q=0, br", "br"],
    ["*;q=0, gzip", "gzip"],
    ["gzip, *;q=0", "gzip"],
    ["*;q=0.1, gzip;q=0", "br"],
    ["br;q=0, *", "gzip"],
    ["gzip;q=0, *", "br"],
    ["br;q=0, gzip;q=0, *", null],
    ["*, br;q=0, gzip;q=0", null],
    ["*;q=abc, gzip", "gzip"],
  ]);
});

test("the static host lets a later entry for an encoding replace an earlier one", () => {
  assertChoices([
    ["br;q=0, br", "br"],
    ["br, br;q=0", null],
    ["gzip;q=0, gzip", "gzip"],
    ["gzip, gzip;q=0, br;q=0", null],
  ]);
});

// --- cacheControl ---------------------------------------------------------------------

function assertCache(names, cache) {
  names.forEach((name) => assert.equal(cacheControl(name), cache, name));
}

test("the static host keeps a name that carries a content hash for a year, as Vite and webpack write such names", () => {
  assertCache(
    [
      // Vite's, as todomvc/svelte/dist has them.
      "index-BxPr2-fK.js",
      "index-bgjkvPzV.css",
      "vendor-DeqkhH9H.js",
      // webpack's [contenthash], in full and cut to eight.
      "main.3b9f2c1a7d4e5f60.js",
      "runtime~main.abcdef12.js",
      "vendors-node_modules_react_index_js.8a1b2c3d.js",
      "styles.4b5c6d7e.css",
      "bundle.0a1b2c3d4e5f6071.mjs",
      // The other extensions that take a hash.
      "module-1a2b3c4d.wasm",
      "logo-1a2b3c4d.svg",
      "font-1a2b3c4d.woff2",
      "font-1a2b3c4d.woff",
      // The directory is not looked at: only the file's name is.
      "assets/index-BxPr2-fK.js",
      "/srv/site/dist/assets/index-BxPr2-fK.js",
    ],
    IMMUTABLE,
  );
});

test("the static host takes a hash to be eight or more characters of a name's last dot or dash part, with an underscore among them", () => {
  assertCache(["x-12345678.js", "x-123456789.js", "a.abcdefgh.js", "chunk-ABCD_efgh.js", "index-abcdefghijklmnopqrstuvwxyz0123456789.js"], IMMUTABLE);
  assertCache(["x-1234567.js", "a.abcdefg.js", "abcdefghij.js", "index-.js"], "no-cache");
});

test("the static host takes some names for hashed that are not: any dash part of eight characters counts", () => {
  // Pinned as the regular expression reads them, not as their authors meant them.
  assertCache(["my-long-name.js", "Inter-SemiBold.woff2", "a-b1234567.js", ".abcdefgh.js"], IMMUTABLE);
});

test("the static host revalidates every other name: plain names, other extensions, and webpack's chunk suffix", () => {
  assertCache(
    [
      "index.html",
      "base.js",
      "app.bundle.js",
      "app.bundle.js.LICENSE.txt",
      "app.css",
      "learn.json",
      // An extension that does not take a hash, whatever the name.
      "logo-abcdefgh.png",
      "data-abcdefgh.json",
      "page-abcdefgh.html",
      "index-abcdefgh.map",
      "index-BxPr2-fK.js.map",
      "font-1a2b3c4d.woff3",
      "font-1a2b3c4d.ttf",
      "photo-1a2b3c4d.jpg",
      "notes-abcdefgh.txt",
      // An extension needs its dot.
      "index-abcdefghijs",
      // Extensions are matched in lowercase only.
      "index-BxPr2-fK.JS",
      // The hash must come last: CRA's `[name].[contenthash:8].chunk.js` has a suffix after it.
      "123.abcdef12.chunk.js",
      // Only the name is looked at, so a hashed directory makes nothing of it.
      "dir-abcdefgh/app.js",
      "assets-abcdefgh/index.html",
    ],
    "no-cache",
  );
});

// --- compressions -----------------------------------------------------------------------

test("the static host's compressions are Brotli at quality 11 and gzip at level 9 of the same bytes, Brotli the smaller", () => {
  // A real script: the Vite bundle of TodoMVC's Svelte, 42 KB.
  const body = fs.readFileSync(path.join(VENDORED, "svelte/dist/assets/index-BxPr2-fK.js"));
  const made = compressions(body);
  assert.deepEqual(Object.keys(made), ["br", "gzip"]);
  assert.equal(digest(brotliDecompressSync(made.br)), digest(body));
  assert.equal(digest(gunzipSync(made.gzip)), digest(body));
  assert.ok(made.br.length < made.gzip.length && made.gzip.length < body.length, `${made.br.length} ${made.gzip.length} ${body.length}`);
  // The same bytes as the stated settings give, and not those of the neighbouring ones.
  assert.equal(digest(made.br), digest(REFERENCE.br(body)));
  assert.equal(digest(made.gzip), digest(REFERENCE.gzip(body)));
  assert.notEqual(digest(made.br), digest(brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 10 } })));
  assert.notEqual(digest(made.gzip), digest(gzipSync(body, { level: 8 })));
});

// --- parseArgs ---------------------------------------------------------------------------

const cwd = (...parts) => path.join(process.cwd(), ...parts);

test("the static host's arguments default to port 0 and no log, and a mount's directory is resolved against the working directory", () => {
  assert.deepEqual(parseArgs(["--mount", "/app/dist=dist"]), {
    port: 0,
    mounts: [{ prefix: "/app/dist", dir: cwd("dist") }],
    log: null,
  });
  assert.deepEqual(parseArgs(["--mount", "/react/dist=benchmarks/headtohead/todomvc/react/dist"]).mounts, [
    { prefix: "/react/dist", dir: cwd("benchmarks/headtohead/todomvc/react/dist") },
  ]);
  assert.deepEqual(parseArgs(["--mount", "/up=../up/dir"]).mounts, [{ prefix: "/up", dir: cwd("../up/dir") }]);
  assert.deepEqual(parseArgs(["--mount", "/abs=/srv/site"]).mounts, [{ prefix: "/abs", dir: "/srv/site" }]);
});

test("the static host's arguments take a port, a log and several mounts in order, the last port and log winning", () => {
  assert.deepEqual(parseArgs(["--port", "13661", "--mount", "/react/dist=a"]), {
    port: 13661,
    mounts: [{ prefix: "/react/dist", dir: cwd("a") }],
    log: null,
  });
  const argv = ["--mount", "/react/dist/=a", "--port", "1", "--log", "first.jsonl", "--mount", "/svelte/dist=/abs/b", "--port", "8080", "--log", "/tmp/last.jsonl"];
  assert.deepEqual(parseArgs(argv), {
    port: 8080,
    mounts: [
      { prefix: "/react/dist", dir: cwd("a") },
      { prefix: "/svelte/dist", dir: "/abs/b" },
    ],
    log: "/tmp/last.jsonl",
  });
  assert.equal(parseArgs(["--mount", "/a=d", "--log", "requests.jsonl"]).log, cwd("requests.jsonl"));
});

test("the static host's arguments strip one trailing slash of a prefix, make the root's prefix empty, and cut a mount at its second equals sign", () => {
  const argv = ["--mount", "/a//=x", "--mount", "/=y", "--mount", "=z", "--mount", "/p=q=r", "--mount", "/s/=t"];
  assert.deepEqual(parseArgs(argv).mounts, [
    { prefix: "/a/", dir: cwd("x") },
    { prefix: "", dir: cwd("y") },
    { prefix: "", dir: cwd("z") },
    { prefix: "/p", dir: cwd("q") },
    { prefix: "/s", dir: cwd("t") },
  ]);
});

test("the static host's arguments refuse an unknown one wherever it stands, and a run with no mount", () => {
  const refusals = [
    [[], "static-host: no --mount"],
    [["--port", "1", "--log", "x"], "static-host: no --mount"],
    [["--bogus"], "static-host: unknown argument --bogus"],
    [["positional"], "static-host: unknown argument positional"],
    // Names of Object.prototype's members are unknown arguments like any other.
    [["constructor"], "static-host: unknown argument constructor"],
    [["__proto__"], "static-host: unknown argument __proto__"],
    [["toString", "--mount", "/a=d"], "static-host: unknown argument toString"],
    [["--mount", "/a=d", "--bogus"], "static-host: unknown argument --bogus"],
    [["--bogus", "--mount", "/a=d"], "static-host: unknown argument --bogus"],
    // An option takes the next argument whatever it is, and has no `--option=value` form.
    [["--port", "--mount", "/a=d"], "static-host: unknown argument /a=d"],
    [["--mount", "/a=d", "--port=8080"], "static-host: unknown argument --port=8080"],
  ];
  refusals.forEach(([argv, message]) => assert.throws(() => parseArgs(argv), { name: "Error", message }, argv.join(" ")));
});

test("the static host's arguments fail with a TypeError on a mount or log with no value or a mount with no directory, and read a bad port as NaN", () => {
  assert.throws(() => parseArgs(["--mount"]), { name: "TypeError" });
  assert.throws(() => parseArgs(["--mount", "/app"]), { name: "TypeError", code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => parseArgs(["--mount", ""]), { name: "TypeError", code: "ERR_INVALID_ARG_TYPE" });
  assert.throws(() => parseArgs(["--mount", "/a=d", "--log"]), { name: "TypeError", code: "ERR_INVALID_ARG_TYPE" });
  assert.equal(parseArgs(["--mount", "/a=d", "--port", "abc"]).port, NaN);
  assert.equal(parseArgs(["--mount", "/a=d", "--port"]).port, NaN);
  // A port is read as a JavaScript number is, not as an integer.
  assert.deepEqual(["1e3", "0x1f90", "8080.5", ""].map((port) => parseArgs(["--mount", "/a=d", "--port", port]).port), [1000, 8080, 8080.5, 0]);
});

// --- loadFiles ---------------------------------------------------------------------------

const FIELDS = ["body", "cache", "encoded", "etag", "path", "type"];

const unpacked = (encoded) => Object.fromEntries(Object.entries(encoded).map(([name, packed]) => [name, digest(DECOMPRESS[name](packed))]));
const summary = (loaded, dir) => ({
  fields: Object.keys(loaded).sort(),
  path: path.relative(dir, loaded.path),
  type: loaded.type,
  etag: loaded.etag,
  cache: loaded.cache,
  body: digest(loaded.body),
  encoded: unpacked(loaded.encoded),
});
const summarize = (files, dir) => Object.fromEntries([...files].map(([url, loaded]) => [url, summary(loaded, dir)]));

/** What loadFiles must make of a file of SITE: its compressions decompress to its bytes. */
function expectedLoad(name) {
  const { type, cache, etag, compressed } = SITE[name];
  const encoded = compressed ? { br: digest(bytes(name)), gzip: digest(bytes(name)) } : {};
  return { fields: FIELDS, path: name, type, etag, cache, body: digest(bytes(name)), encoded };
}

test("loadFiles reads every file under a mount by its URL path, with its type, ETag, cache policy, bytes and compressions", (t) => {
  const { dir } = makeSite(t, TREE);
  const files = loadFiles([{ prefix: MOUNT, dir }]);
  assert.ok(files instanceof Map);
  const expected = Object.fromEntries(NAMES.map((name) => [`${MOUNT}/${name}`, expectedLoad(name)]));
  assert.deepEqual(summarize(files, dir), expected);
});

test("loadFiles makes compressions only for compressible types, Brotli at quality 11 and gzip at level 9", (t) => {
  const { dir } = makeSite(t, TREE);
  const files = loadFiles([{ prefix: MOUNT, dir }]);
  const compressible = NAMES.filter((name) => SITE[name].compressed);
  const plain = NAMES.filter((name) => !SITE[name].compressed);
  assert.deepEqual([compressible.length, plain.length], [13, 7]);
  const packedOf = (name) => Object.fromEntries(Object.entries(files.get(`${MOUNT}/${name}`).encoded).map(([how, packed]) => [how, digest(packed)]));
  assert.deepEqual(
    compressible.map(packedOf),
    compressible.map((name) => ({ br: digest(REFERENCE.br(bytes(name))), gzip: digest(REFERENCE.gzip(bytes(name))) })),
  );
  assert.deepEqual(plain.map(packedOf), plain.map(() => ({})));
});

/** A unix socket at a path: an entry of a directory that is not a file. */
async function bindSocket(t, socketPath) {
  const server = net.createServer();
  server.listen(socketPath);
  await once(server, "listening");
  t.after(() => stop(server));
}

test("loadFiles ignores symbolic links, sockets and empty directories", async (t) => {
  const { dir } = makeSite(t, { "real.txt": "real\n", "sub/inner.txt": "inner\n" });
  fs.mkdirSync(path.join(dir, "empty"));
  fs.symlinkSync(path.join(dir, "real.txt"), path.join(dir, "link.txt"));
  fs.symlinkSync(path.join(dir, "sub"), path.join(dir, "linked"));
  fs.symlinkSync(path.join(dir, "missing"), path.join(dir, "broken"));
  await bindSocket(t, path.join(dir, "socket"));
  const files = loadFiles([{ prefix: "/m", dir }]);
  assert.deepEqual([...files.keys()].sort(), ["/m/real.txt", "/m/sub/inner.txt"]);
});

test("loadFiles walks every mount under its prefix, a later mount's file replacing an earlier one's at the same URL", (t) => {
  const first = makeSite(t, { "a.txt": "first a\n", "only-first.txt": "1\n" });
  const second = makeSite(t, { "a.txt": "second a\n", "nested/b.txt": "2\n" });
  const files = loadFiles([
    { prefix: "/one", dir: first.dir },
    { prefix: "/two", dir: second.dir },
    { prefix: "/one", dir: second.dir },
  ]);
  assert.deepEqual([...files.keys()].sort(), ["/one/a.txt", "/one/nested/b.txt", "/one/only-first.txt", "/two/a.txt", "/two/nested/b.txt"]);
  assert.deepEqual([files.get("/one/a.txt").body.toString(), files.get("/two/a.txt").body.toString()], ["second a\n", "second a\n"]);
  assert.equal(files.get("/one/only-first.txt").body.toString(), "1\n");
  assert.deepEqual([...loadFiles([]).keys()], []);
});

test("loadFiles joins a prefix to a name with one slash, as the prefix is given, and keeps a relative directory relative", (t) => {
  const { dir } = makeSite(t, { "a.txt": "a\n", "sub/b.txt": "b\n" });
  const relative = path.relative(process.cwd(), dir);
  const files = loadFiles([
    { prefix: "/slashed/", dir },
    { prefix: "", dir: relative },
  ]);
  assert.deepEqual([...files.keys()].sort(), ["/a.txt", "/slashed//a.txt", "/slashed//sub/b.txt", "/sub/b.txt"]);
  assert.deepEqual([files.get("/slashed//a.txt").path, files.get("/a.txt").path], [path.join(dir, "a.txt"), path.join(relative, "a.txt")]);
});

test("loadFiles throws where a mount's directory is missing", (t) => {
  const { root } = makeSite(t, {});
  const missing = path.join(root, "nowhere");
  assert.throws(() => loadFiles([{ prefix: "/m", dir: missing }]), { code: "ENOENT", syscall: "scandir", path: missing });
});

// The vendored builds, which `targets.mjs` mounts at these prefixes: committed
// upstream files, byte for byte (todomvc/UPSTREAM.md), so they do not move.
const VENDORED_BUILDS = [
  ["react/dist/app.bundle.js", JS_UTF8, "no-cache"],
  ["react/dist/app.bundle.js.LICENSE.txt", TEXT_UTF8, "no-cache"],
  ["react/dist/app.css", CSS_UTF8, "no-cache"],
  ["react/dist/base.js", JS_UTF8, "no-cache"],
  ["react/dist/index.html", HTML_UTF8, "no-cache"],
  ["svelte/dist/assets/index-BxPr2-fK.js", JS_UTF8, IMMUTABLE],
  ["svelte/dist/assets/index-bgjkvPzV.css", CSS_UTF8, IMMUTABLE],
  ["svelte/dist/base.js", JS_UTF8, "no-cache"],
  ["svelte/dist/index.html", HTML_UTF8, "no-cache"],
];

/** What an answer for a vendored file shows: its status, type, cache policy, validator and encoding, and its bytes unpacked. */
const shown = (raw) => ({
  status: raw.status,
  type: raw.headers["content-type"],
  cache: raw.headers["cache-control"],
  etag: raw.headers.etag,
  encoding: raw.headers["content-encoding"],
  body: digest(brotliDecompressSync(raw.body)),
});

test("the static host serves the vendored TodoMVC builds as the head-to-head mounts them: each file in Brotli, hashed names immutable", async (t) => {
  const mounts = ["react/dist", "svelte/dist"].map((rel) => ({ prefix: `/${rel}`, dir: path.join(VENDORED, rel) }));
  const host = await startHost(t, { mounts });
  assert.deepEqual([...host.files.keys()].sort(), VENDORED_BUILDS.map(([rel]) => `/${rel}`));
  const asked = VENDORED_BUILDS.map(([rel]) => exchange(host.port, "GET", `/${rel}`, { "accept-encoding": "br, gzip" }));
  const expected = VENDORED_BUILDS.map(([rel, type, cache]) => {
    const body = fs.readFileSync(path.join(VENDORED, rel));
    const etag = `"${createHash("sha256").update(body).digest("base64url")}"`;
    return { status: 200, type, cache, etag, encoding: "br", body: digest(body) };
  });
  assert.deepEqual((await Promise.all(asked)).map(shown), expected);
});

// --- createHost: what it returns ------------------------------------------------------------

test("createHost returns the server, not listening, and the files it loaded, and makes no log when it is given none", (t) => {
  const { dir, root } = makeSite(t, { "a.txt": "a\n" });
  const host = createHost({ mounts: [{ prefix: "/m", dir }] });
  assert.deepEqual(Object.keys(host).sort(), ["files", "server"]);
  assert.ok(host.server instanceof http.Server);
  assert.equal(host.server.listening, false);
  assert.deepEqual([...host.files.keys()], ["/m/a.txt"]);
  assert.deepEqual(fs.readdirSync(root), ["site"]);
});

// --- createHost: a GET ------------------------------------------------------------------------

test("the static host answers a GET 200 with the file's headers and its bytes, or those of the best compression the request accepts", async (t) => {
  const site = await openSite(t);
  const js = `${MOUNT}/app.bundle.js`;
  const { answers, lines } = await visit(site, [
    ["GET", js, { "accept-encoding": "gzip, deflate, br" }],
    ["GET", js, { "accept-encoding": "gzip" }],
    ["GET", js, { "accept-encoding": "*" }],
    ["GET", js, { "accept-encoding": "br;q=0, gzip;q=0" }],
    ["GET", js, { "accept-encoding": "identity" }],
    ["GET", js, {}],
  ]);
  const [br, gzip, plain] = [served("app.bundle.js", "br"), served("app.bundle.js", "gzip"), served("app.bundle.js")];
  assert.deepEqual(answers, [br, gzip, br, plain, plain, plain]);
  const [brLength, gzipLength] = [bytesSent("app.bundle.js", "br").length, bytesSent("app.bundle.js", "gzip").length];
  assert.deepEqual(lines, [
    entry("GET", js, 200, "br", brLength),
    entry("GET", js, 200, "gzip", gzipLength),
    entry("GET", js, 200, "br", brLength),
    entry("GET", js, 200, null, 9200),
    entry("GET", js, 200, null, 9200),
    entry("GET", js, 200, null, 9200),
  ]);
});

test("the static host compresses every compressible type, with Vary, and sends every other type as it is, without", async (t) => {
  const site = await openSite(t);
  const targets = NAMES.map((name) => `${MOUNT}/${encodeURI(name)}`);
  const { answers, lines } = await visit(site, targets.map((target) => ["GET", target, { "accept-encoding": "br, gzip" }]));
  const encodings = NAMES.map((name) => (SITE[name].compressed ? "br" : null));
  assert.deepEqual(answers, NAMES.map((name, i) => served(name, encodings[i])));
  assert.deepEqual(
    lines,
    NAMES.map((name, i) => entry("GET", `${MOUNT}/${name}`, 200, encodings[i], bytesSent(name, encodings[i]).length)),
  );
});

test("the static host sends a compressible file's Vary and ETag even when it sends the bytes as they are", async (t) => {
  const site = await openSite(t);
  const { answers } = await visit(site, [["GET", `${MOUNT}/notes.txt`], ["GET", `${MOUNT}/pic.png`, { "accept-encoding": "br" }]]);
  assert.deepEqual(answers, [served("notes.txt"), served("pic.png")]);
  assert.deepEqual(
    answers.map((one) => one.headers.vary),
    ["Accept-Encoding", undefined],
  );
});

test("the static host sends a directory's index.html at its slash, as a request for the file itself gets it, and the log names the file", async (t) => {
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [
    ["GET", `${MOUNT}/`, { "accept-encoding": "br" }],
    ["GET", `${MOUNT}/index.html`, { "accept-encoding": "br" }],
    ["GET", `${MOUNT}/`],
    ["GET", `${MOUNT}/?v=1`],
  ]);
  const html = served("index.html", "br");
  assert.deepEqual(answers, [html, html, served("index.html"), served("index.html")]);
  const brLength = bytesSent("index.html", "br").length;
  assert.deepEqual(lines, [
    entry("GET", `${MOUNT}/index.html`, 200, "br", brLength),
    entry("GET", `${MOUNT}/index.html`, 200, "br", brLength),
    entry("GET", `${MOUNT}/index.html`, 200, null, 388),
    entry("GET", `${MOUNT}/index.html`, 200, null, 388),
  ]);
});

test("the static host decodes a percent-encoded path before looking the file up, and logs the path decoded", async (t) => {
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [
    ["GET", `${MOUNT}/my%20file.txt`],
    ["GET", `${MOUNT}/caf%C3%A9.txt`],
    ["GET", `${MOUNT}/caf%c3%a9.txt`],
    ["GET", `${MOUNT}/deep%2Fer%2Fleaf.txt`],
    ["GET", `${MOUNT}/%61pp.bundle.js`, { "accept-encoding": "br" }],
    ["GET", `${MOUNT}/my%20dir/inner.txt`],
  ]);
  const js = served("app.bundle.js", "br");
  assert.deepEqual(answers, [served("my file.txt"), served("café.txt"), served("café.txt"), served("deep/er/leaf.txt"), js, served("my dir/inner.txt")]);
  assert.deepEqual(lines, [
    entry("GET", `${MOUNT}/my file.txt`, 200, null, 7),
    entry("GET", `${MOUNT}/café.txt`, 200, null, 7),
    entry("GET", `${MOUNT}/café.txt`, 200, null, 7),
    entry("GET", `${MOUNT}/deep/er/leaf.txt`, 200, null, 5),
    entry("GET", `${MOUNT}/app.bundle.js`, 200, "br", bytesSent("app.bundle.js", "br").length),
    entry("GET", `${MOUNT}/my dir/inner.txt`, 200, null, 6),
  ]);
});

test("the static host leaves a query out of the lookup and the log, and resolves dot segments before it", async (t) => {
  const site = await openSite(t);
  const notes = `${MOUNT}/notes.txt`;
  const { answers, lines } = await visit(site, [
    ["GET", `${notes}?v=1&x=2`],
    ["GET", `${MOUNT}/../dist/notes.txt`],
    ["GET", `${MOUNT}/./notes.txt`],
    ["GET", `${MOUNT}/%2e%2e/dist/notes.txt`],
    ["GET", `${MOUNT}/deep/../notes.txt`],
  ]);
  assert.deepEqual(answers, Array(5).fill(served("notes.txt")));
  assert.deepEqual(lines, Array(5).fill(entry("GET", notes, 200, null, 6)));
});

test("the static host reads a target that starts with two slashes as a URL with a host, and serves its path", async (t) => {
  // new URL("//evil/app/dist/notes.txt", "http://host") has the host `evil`
  // and the path /app/dist/notes.txt: path confusion, pinned as it is.
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [["GET", "//evil/app/dist/notes.txt"]]);
  assert.deepEqual(answers, [served("notes.txt")]);
  assert.deepEqual(lines, [entry("GET", `${MOUNT}/notes.txt`, 200, null, 6)]);
});

test("the static host keeps a connection alive for the request that follows on it", async (t) => {
  const site = await openSite(t);
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  t.after(() => agent.destroy());
  const get = (target) =>
    new Promise((resolve, reject) => {
      const req = http.get({ host: "127.0.0.1", port: site.port, path: target, agent, timeout: 5000 }, (res) => {
        res.resume();
        res.on("end", () => resolve({ connection: res.headers.connection, reused: req.reusedSocket }));
      });
      req.on("timeout", () => req.destroy(new Error(`${target}: no answer in 5 s`)));
      req.on("error", reject);
    });
  const first = await get(`${MOUNT}/notes.txt`);
  const second = await get(`${MOUNT}/pic.png`);
  assert.deepEqual([first, second], [{ connection: "keep-alive", reused: false }, { connection: "keep-alive", reused: true }]);
  // The host writes its log after it answers: the test ends with both lines written,
  // not while the second is, or the directory it is written in is gone by then.
  assert.equal((await logLines(site.log, 2)).length, 2);
});

// --- createHost: HEAD and validators --------------------------------------------------------

test("the static host answers a HEAD as the GET, headers and length included, and no body", async (t) => {
  const site = await openSite(t);
  const js = `${MOUNT}/app.bundle.js`;
  const { answers, lines } = await visit(site, [
    ["HEAD", js, { "accept-encoding": "br" }],
    ["HEAD", js, { "accept-encoding": "gzip" }],
    ["HEAD", js],
    ["HEAD", `${MOUNT}/pic.png`, { "accept-encoding": "br" }],
    ["HEAD", `${MOUNT}/`],
  ]);
  assert.deepEqual(answers, [
    withoutBody(served("app.bundle.js", "br")),
    withoutBody(served("app.bundle.js", "gzip")),
    withoutBody(served("app.bundle.js")),
    withoutBody(served("pic.png")),
    withoutBody(served("index.html")),
  ]);
  assert.deepEqual(lines, [
    entry("HEAD", js, 200, "br", 0),
    entry("HEAD", js, 200, "gzip", 0),
    entry("HEAD", js, 200, null, 0),
    entry("HEAD", `${MOUNT}/pic.png`, 200, null, 0),
    entry("HEAD", `${MOUNT}/index.html`, 200, null, 0),
  ]);
});

test("the static host answers 304, with the file's headers but no length or encoding, to a GET or HEAD that sends its ETag", async (t) => {
  const site = await openSite(t);
  const [js, png] = [`${MOUNT}/app.bundle.js`, `${MOUNT}/pic.png`];
  const jsTag = { "if-none-match": SITE["app.bundle.js"].etag, "accept-encoding": "br" };
  const { answers, lines } = await visit(site, [
    ["GET", js, jsTag],
    ["HEAD", js, jsTag],
    ["GET", png, { "if-none-match": SITE["pic.png"].etag }],
    ["HEAD", `${MOUNT}/`, { "if-none-match": SITE["index.html"].etag }],
  ]);
  assert.deepEqual(answers, [notModified("app.bundle.js"), notModified("app.bundle.js"), notModified("pic.png"), notModified("index.html")]);
  assert.deepEqual(lines, [
    entry("GET", js, 304, null, 0),
    entry("HEAD", js, 304, null, 0),
    entry("GET", png, 304, null, 0),
    entry("HEAD", `${MOUNT}/index.html`, 304, null, 0),
  ]);
});

test("the static host answers 200 to any validator that is not exactly the file's ETag", async (t) => {
  const site = await openSite(t);
  const js = `${MOUNT}/app.bundle.js`;
  const etag = SITE["app.bundle.js"].etag;
  const validators = [`W/${etag}`, '"other"', "*", `"x", ${etag}`, `${etag}, "x"`, SITE["notes.txt"].etag, etag.slice(1, -1)];
  const { answers, lines } = await visit(
    site,
    validators.map((validator) => ["GET", js, { "if-none-match": validator }]),
  );
  assert.deepEqual(answers, Array(validators.length).fill(served("app.bundle.js")));
  assert.deepEqual(lines, Array(validators.length).fill(entry("GET", js, 200, null, 9200)));
});

// --- createHost: directories, unknown paths, other methods ---------------------------------

test("the static host redirects a directory's address without its slash to it, 301, keeping the query and whatever the method", async (t) => {
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [
    ["GET", MOUNT],
    ["GET", `${MOUNT}?x=1&y=2`],
    ["GET", `${MOUNT}?`],
    ["HEAD", MOUNT],
    ["POST", MOUNT],
    ["GET", `${MOUNT}/assets`],
    ["GET", `${MOUNT}/deep`],
    ["GET", `${MOUNT}/deep/er?q=a%20b`],
    ["GET", `${MOUNT}/my%20dir`],
    ["GET", `${MOUNT}/my%20dir?v=1`],
  ]);
  assert.deepEqual(answers, [
    redirect(`${MOUNT}/`),
    redirect(`${MOUNT}/?x=1&y=2`),
    redirect(`${MOUNT}/`),
    redirect(`${MOUNT}/`),
    redirect(`${MOUNT}/`),
    redirect(`${MOUNT}/assets/`),
    redirect(`${MOUNT}/deep/`),
    redirect(`${MOUNT}/deep/er/?q=a%20b`),
    redirect(`${MOUNT}/my dir/`),
    redirect(`${MOUNT}/my dir/?v=1`),
  ]);
  assert.deepEqual(lines, [
    entry("GET", MOUNT, 301, null, 0),
    entry("GET", MOUNT, 301, null, 0),
    entry("GET", MOUNT, 301, null, 0),
    entry("HEAD", MOUNT, 301, null, 0),
    entry("POST", MOUNT, 301, null, 0),
    entry("GET", `${MOUNT}/assets`, 301, null, 0),
    entry("GET", `${MOUNT}/deep`, 301, null, 0),
    entry("GET", `${MOUNT}/deep/er`, 301, null, 0),
    entry("GET", `${MOUNT}/my dir`, 301, null, 0),
    entry("GET", `${MOUNT}/my dir`, 301, null, 0),
  ]);
});

test("the static host answers 404 where a directory has no index.html, and for a file's address with a slash, in any case or with a doubled slash", async (t) => {
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [
    ["GET", `${MOUNT}/assets/`],
    ["GET", `${MOUNT}/deep/`],
    ["GET", `${MOUNT}/deep/er/`],
    ["GET", `${MOUNT}/notes.txt/`],
    ["GET", "/APP/DIST/"],
    ["GET", "/app//dist/notes.txt"],
  ]);
  assert.deepEqual(answers, Array(6).fill(refused(404)));
  assert.deepEqual(lines, [
    entry("GET", `${MOUNT}/assets/index.html`, 404, null, 0),
    entry("GET", `${MOUNT}/deep/index.html`, 404, null, 0),
    entry("GET", `${MOUNT}/deep/er/index.html`, 404, null, 0),
    entry("GET", `${MOUNT}/notes.txt/index.html`, 404, null, 0),
    entry("GET", "/APP/DIST/index.html", 404, null, 0),
    entry("GET", "/app//dist/notes.txt", 404, null, 0),
  ]);
});

test("the static host answers 404 'not found' with a length of 9 to an unknown path, whatever the method, and no body to a HEAD", async (t) => {
  const site = await openSite(t);
  const { answers, lines } = await visit(site, [
    ["GET", `${MOUNT}/missing.js`],
    ["HEAD", `${MOUNT}/missing.js`],
    ["POST", `${MOUNT}/missing.js`],
    ["GET", "/"],
    ["GET", "/app"],
    ["GET", "/app/"],
    ["GET", "/nope"],
  ]);
  assert.deepEqual(answers, [refused(404), withoutBody(refused(404)), refused(404), refused(404), refused(404), refused(404), refused(404)]);
  assert.deepEqual(lines, [
    entry("GET", `${MOUNT}/missing.js`, 404, null, 0),
    entry("HEAD", `${MOUNT}/missing.js`, 404, null, 0),
    entry("POST", `${MOUNT}/missing.js`, 404, null, 0),
    entry("GET", "/index.html", 404, null, 0),
    entry("GET", "/app", 404, null, 0),
    entry("GET", "/app/index.html", 404, null, 0),
    entry("GET", "/nope", 404, null, 0),
  ]);
});

test("the static host answers 405, with a 404's headers and body and no Allow, to a method but GET and HEAD on a file that exists", async (t) => {
  const site = await openSite(t);
  const js = `${MOUNT}/app.bundle.js`;
  const { answers, lines } = await visit(site, [
    ["POST", js],
    ["PUT", js],
    ["DELETE", js],
    ["OPTIONS", js],
    ["PATCH", js],
    ["POST", js, { "if-none-match": SITE["app.bundle.js"].etag }],
    ["POST", `${MOUNT}/`],
    ["DELETE", `${MOUNT}/`],
  ]);
  assert.deepEqual(answers, Array(8).fill(refused(405)));
  assert.deepEqual(lines, [
    entry("POST", js, 405, null, 0),
    entry("PUT", js, 405, null, 0),
    entry("DELETE", js, 405, null, 0),
    entry("OPTIONS", js, 405, null, 0),
    entry("PATCH", js, 405, null, 0),
    entry("POST", js, 405, null, 0),
    entry("POST", `${MOUNT}/index.html`, 405, null, 0),
    entry("DELETE", `${MOUNT}/index.html`, 405, null, 0),
  ]);
});

test("the static host knows a directory only by the files under it: an empty one and a symbolic link are not found", async (t) => {
  const prepare = ({ dir }) => {
    fs.mkdirSync(path.join(dir, "emptydir"));
    fs.symlinkSync(path.join(dir, "notes.txt"), path.join(dir, "link.txt"));
    fs.symlinkSync(path.join(dir, "deep"), path.join(dir, "linked"));
  };
  const site = await openSite(t, { prepare });
  const { answers, lines } = await visit(site, [
    ["GET", `${MOUNT}/emptydir`],
    ["GET", `${MOUNT}/emptydir/`],
    ["GET", `${MOUNT}/link.txt`],
    ["GET", `${MOUNT}/linked`],
    ["GET", `${MOUNT}/linked/er/leaf.txt`],
    ["GET", `${MOUNT}/deep/er/leaf.txt`],
  ]);
  assert.deepEqual(answers, [...Array(5).fill(refused(404)), served("deep/er/leaf.txt")]);
  assert.deepEqual(lines, [
    entry("GET", `${MOUNT}/emptydir`, 404, null, 0),
    entry("GET", `${MOUNT}/emptydir/index.html`, 404, null, 0),
    entry("GET", `${MOUNT}/link.txt`, 404, null, 0),
    entry("GET", `${MOUNT}/linked`, 404, null, 0),
    entry("GET", `${MOUNT}/linked/er/leaf.txt`, 404, null, 0),
    entry("GET", `${MOUNT}/deep/er/leaf.txt`, 200, null, 5),
  ]);
});

// --- createHost: mounts, the log, and what it does not catch ------------------------------

test("the static host serves a mount at the root: its index.html at /, its directories redirected, everything else not found", async (t) => {
  const { dir } = makeSite(t, TREE);
  const host = await startHost(t, { mounts: [{ prefix: "", dir }] });
  const asked = ["/", "/index.html", "/assets", "/assets/index-bgjkvPzV.css", "/nope", `${MOUNT}/`];
  const answers = await Promise.all(asked.map((target) => request(host.port, "GET", target)));
  assert.deepEqual(answers, [served("index.html"), served("index.html"), redirect("/assets/"), served("assets/index-bgjkvPzV.css"), refused(404), refused(404)]);
});

test("the static host serves each mount under its own prefix, and knows no directory above a prefix", async (t) => {
  const one = makeSite(t, { "index.html": "one\n", "only-one.txt": "1\n" });
  const two = makeSite(t, { "index.html": "two\n", "nested/only-two.txt": "2\n" });
  const mounts = [
    { prefix: "/react/dist", dir: one.dir },
    { prefix: "/svelte/dist", dir: two.dir },
  ];
  const host = await startHost(t, { mounts });
  const asked = ["/react/dist/", "/svelte/dist/", "/react/dist/only-one.txt", "/svelte/dist/only-one.txt", "/svelte/dist/nested", "/react", "/svelte/", "/"];
  const answers = await Promise.all(asked.map((target) => exchange(host.port, "GET", target)));
  const bodies = answers.map((raw) => [raw.status, raw.body.toString()]);
  const notFound = [404, "not found"];
  assert.deepEqual(bodies, [[200, "one\n"], [200, "two\n"], [200, "1\n"], notFound, [301, ""], notFound, notFound, notFound]);
  assert.equal(host.files.size, 4);
});

test("the static host appends one line to its log for each answer, after any lines the file already has", async (t) => {
  const prepare = ({ log }) => fs.writeFileSync(log, "an earlier run's line\n");
  const site = await openSite(t, { prepare });
  await visit(site, [["GET", `${MOUNT}/notes.txt`], ["GET", `${MOUNT}/missing`]]);
  const lines = await logLines(site.log, 3);
  assert.deepEqual(lines, ["an earlier run's line", entry("GET", `${MOUNT}/notes.txt`, 200, null, 6), entry("GET", `${MOUNT}/missing`, 404, null, 0)]);
});

test("the static host answers without a log when it is given none", async (t) => {
  const { dir } = makeSite(t, TREE);
  const host = await startHost(t, { mounts: [{ prefix: MOUNT, dir }], log: null });
  assert.deepEqual(await request(host.port, "GET", `${MOUNT}/notes.txt`), served("notes.txt"));
  assert.deepEqual(await request(host.port, "GET", `${MOUNT}/missing`), refused(404));
});

test("the static host lets a path of malformed percent-encoding, and a target that is no URL, throw out of its request handler unanswered", (t) => {
  // Over a connection either takes the whole process down (an uncaught
  // exception in a request listener): pinned by calling the listener, which
  // is how the server calls it.
  const { dir } = makeSite(t, TREE);
  const { server } = createHost({ mounts: [{ prefix: MOUNT, dir }] });
  const through = (url) => () => server.emit("request", { url, method: "GET", headers: {} }, {});
  assert.throws(through(`${MOUNT}/%zz`), { name: "URIError", message: "URI malformed" });
  assert.throws(through(`${MOUNT}/%E0%A4%A`), { name: "URIError", message: "URI malformed" });
  assert.throws(through("//"), { name: "TypeError", code: "ERR_INVALID_URL", message: "Invalid URL" });
});

// --- the program ----------------------------------------------------------------------------

/** The error the process died of, as its message line: the stack around it holds line numbers. */
const errorLine = (stderr) => stderr.split("\n").find((line) => line.startsWith("Error:"));

function runHost(args, cwdDir = os.tmpdir()) {
  return spawnSync(process.execPath, [HOST, ...args], { cwd: cwdDir, encoding: "utf8", timeout: 20000 });
}

/** The program started with `args`: it is waited for until it has said where it listens, or has died. */
async function startProgram(t, args, cwdDir) {
  const child = spawn(process.execPath, [HOST, ...args], { cwd: cwdDir, stdio: ["ignore", "pipe", "pipe"] });
  t.after(() => child.kill("SIGKILL"));
  const exited = once(child, "exit");
  const said = once(readline.createInterface({ input: child.stdout }), "line");
  const [line] = await Promise.race([said, exited.then(() => ["it exited before it said where it listens"])]);
  return { child, exited, line };
}

test("run as a program the static host says how many files it serves and where, serves them, logs, and exits 0 on SIGTERM", { timeout: 30000 }, async (t) => {
  const { root } = makeSite(t, TREE);
  const { child, exited, line } = await startProgram(t, ["--port", "0", "--mount", `${MOUNT}/=site`, "--log", "requests.jsonl"], root);
  const port = Number(line.split(":").pop());
  assert.equal(line, `static-host: ${NAMES.length} files on http://127.0.0.1:${port}`);
  const log = path.join(root, "requests.jsonl");
  assert.deepEqual(await request(port, "GET", `${MOUNT}/notes.txt`, { "accept-encoding": "br" }), served("notes.txt", "br"));
  assert.deepEqual(await request(port, "GET", MOUNT), redirect(`${MOUNT}/`));
  assert.equal((await logLines(log, 2)).length, 2);
  child.kill("SIGTERM");
  assert.deepEqual(await exited, [0, null]);
  const brLength = bytesSent("notes.txt", "br").length;
  assert.deepEqual(readLines(log), [entry("GET", `${MOUNT}/notes.txt`, 200, "br", brLength), entry("GET", MOUNT, 301, null, 0)]);
});

/** The rows of a Linux socket table as columns; none where the system has no such table. */
function socketRows(table) {
  const file = `/proc/net/${table}`;
  return fs.existsSync(file) ? fs.readFileSync(file, "utf8").split("\n").slice(1).map((row) => row.trim().split(/\s+/)) : [];
}

/** The addresses (hex, as the table writes them) that the sockets listening on a port are bound to. */
function boundTo(table, port) {
  const suffix = `:${port.toString(16).toUpperCase().padStart(4, "0")}`;
  const listening = socketRows(table).filter((columns) => columns[3] === "0A" && columns[1].endsWith(suffix));
  return listening.map((columns) => columns[1].slice(0, -suffix.length));
}

test("run as a program the static host listens on the IPv4 loopback alone", { skip: !fs.existsSync("/proc/net/tcp") }, async (t) => {
  const { root } = makeSite(t, TREE);
  const { line } = await startProgram(t, ["--port", "0", "--mount", `${MOUNT}=site`], root);
  const port = Number(line.split(":").pop());
  assert.deepEqual([boundTo("tcp", port), boundTo("tcp6", port)], [["0100007F"], []]);
});

test("run as a program with an argument it does not know the static host exits 1 and writes the error", () => {
  const run = runHost(["--bogus"]);
  assert.deepEqual([run.status, run.stdout, errorLine(run.stderr)], [1, "", "Error: static-host: unknown argument --bogus"]);
});

test("run as a program with no mount the static host exits 1 and writes the error", () => {
  const run = runHost(["--port", "0"]);
  assert.deepEqual([run.status, run.stdout, errorLine(run.stderr)], [1, "", "Error: static-host: no --mount"]);
});

test("run as a program on a mount whose directory is missing the static host exits 1 with the system's error", (t) => {
  const { root } = makeSite(t, {});
  const run = runHost(["--mount", "/app=nowhere"], root);
  const missing = path.join(root, "nowhere");
  assert.deepEqual([run.status, run.stdout, errorLine(run.stderr)], [1, "", `Error: ENOENT: no such file or directory, scandir '${missing}'`]);
});

test("run as a program with a log in a directory that is missing the static host exits 1 with the system's error", (t) => {
  // The log's stream fails after the host has started, with no handler for
  // it; what the program printed before it died is not pinned.
  const { root } = makeSite(t, TREE);
  const run = runHost(["--mount", `${MOUNT}=site`, "--log", "nowhere/requests.jsonl"], root);
  const missing = path.join(root, "nowhere/requests.jsonl");
  assert.deepEqual([run.status, errorLine(run.stderr)], [1, `Error: ENOENT: no such file or directory, open '${missing}'`]);
});

test("imported by another program, and not run, the static host module starts nothing", () => {
  const script = `await import(${JSON.stringify(pathToFileURL(HOST).href)}); console.log("imported");`;
  const run = spawnSync(process.execPath, ["--input-type=module", "-e", script], { encoding: "utf8", timeout: 20000 });
  assert.deepEqual([run.status, run.stdout, run.stderr], [0, "imported\n", ""]);
});

test("run through a symbolic link the static host starts nothing and exits 0: it takes itself for the main script by an unresolved path", (t) => {
  // process.argv[1] is the link's path and import.meta.url the file's real
  // one, so the host does not recognise that it was run: the same silence
  // as when it is imported. Pinned as it is (and `--mount` is not even read).
  const { root } = makeSite(t, TREE);
  const link = path.join(root, "linked-static-host.mjs");
  fs.symlinkSync(HOST, link);
  const run = spawnSync(process.execPath, [link, "--bogus"], { cwd: root, encoding: "utf8", timeout: 20000 });
  assert.deepEqual([run.status, run.stdout, run.stderr], [0, "", ""]);
});
