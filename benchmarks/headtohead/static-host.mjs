#!/usr/bin/env node
// The head-to-head's static host (ADR-0392, "the head-to-head"): how a
// production static host sends a client-rendered app's files, one policy for
// every app it serves.
//
//   node static-host.mjs --port 13661 --mount /react/dist=benchmarks/headtohead/todomvc/react/dist \
//                        [--mount /svelte/dist=...] [--log requests.jsonl]
//
// TodoMVC's React and Svelte apps are static files: any static host or CDN
// serves them, and neither framework has a server of its own in the
// picture. So both are served here, by one policy, which is what a
// production static host does (Netlify, Vercel, Cloudflare Pages, nginx with
// gzip_static/brotli_static), and the head-to-head states it beside every
// number:
//
// - **Compression** by the request's `Accept-Encoding`: Brotli at its highest
//   quality, else gzip at its highest level, each made once when the host
//   starts (a static host precompresses), for text, JavaScript, JSON, SVG and
//   WebAssembly. `Vary: Accept-Encoding`.
// - **Caching**: a strong `ETag` (the content's SHA-256) on every file, and
//   `If-None-Match` answered `304`. A file whose name carries a content hash
//   (`index-BxPr2-fK.js`, as Vite names its output) is `immutable` for a
//   year; every other file, `index.html` and React's `app.bundle.js` among
//   them, is `no-cache`: kept, and revalidated before each use.
// - **HTTP/1.1 keep-alive**, Node's default.
// - A directory's address without its slash is redirected to it (301), as
//   `serve-static` does, so the page's relative addresses resolve; a
//   directory serves its `index.html`.
//
// No file is changed: the bytes sent are the file's, or its compression.
// `--log` writes one JSON line per answer (method, path, status, encoding,
// bytes of body sent), which the head-to-head reads to check what was served.
// It answers `SIGTERM` by closing.

import { createHash } from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".wasm": "application/wasm",
  ".txt": "text/plain; charset=utf-8",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

/** What a static host compresses: text, scripts, JSON, SVG and WebAssembly. */
const COMPRESSIBLE = /^(text\/|application\/(javascript|json|wasm)|image\/svg\+xml)/;

/** A name that carries its content's hash, as Vite and webpack's `[contenthash]` write it. */
const HASHED = /[.-][A-Za-z0-9_-]{8,}\.(js|mjs|css|wasm|svg|woff2?)$/;

export function cacheControl(file) {
  return HASHED.test(path.basename(file)) ? "public, max-age=31536000, immutable" : "no-cache";
}

/** Brotli at quality 11 and gzip at level 9: what a static host precompresses with. */
export function compressions(body) {
  return {
    br: brotliCompressSync(body, {
      params: {
        [constants.BROTLI_PARAM_QUALITY]: 11,
        [constants.BROTLI_PARAM_SIZE_HINT]: body.length,
      },
    }),
    gzip: gzipSync(body, { level: 9 }),
  };
}

/** One `Accept-Encoding` entry as [name, weight]: the name lower-cased, the
 * weight 1 unless a `q=` parameter says; null for an empty name. */
function parseOffer(part) {
  const [name, ...params] = part.trim().split(";");
  if (!name) return null;
  const q = params.map((p) => p.trim()).find((p) => p.startsWith("q="));
  return [name.trim().toLowerCase(), q ? Number(q.slice(2)) : 1];
}

/** The encoding a request accepts, best first: br, then gzip, else none. */
export function chooseEncoding(accept = "") {
  const offered = new Map(accept.split(",").map(parseOffer).filter(Boolean));
  const ok = (name) => (offered.get(name) ?? offered.get("*") ?? 0) > 0;
  return ["br", "gzip"].find(ok) ?? null;
}

/** What each option does with the argument that follows it. */
const OPTIONS = new Map([
  ["--port", (out, value) => (out.port = Number(value))],
  ["--mount", (out, value) => out.mounts.push(mountOf(value))],
  ["--log", (out, value) => (out.log = path.resolve(value))],
]);

/** A `--mount prefix=dir` argument: the prefix without its trailing slash, the directory made absolute. */
function mountOf(value) {
  const [prefix, dir] = value.split("=");
  return { prefix: prefix.replace(/\/$/, ""), dir: path.resolve(dir) };
}

export function parseArgs(argv) {
  const out = { port: 0, mounts: [], log: null };
  for (let i = 0; i < argv.length; i++) {
    const option = OPTIONS.get(argv[i]);
    if (!option) throw new Error(`static-host: unknown argument ${argv[i]}`);
    option(out, argv[++i]);
  }
  if (!out.mounts.length) throw new Error("static-host: no --mount");
  return out;
}

/** A file as the host holds it: its bytes, type, validator, cache policy and precompressed forms. */
function readFile(file, name) {
  const body = fs.readFileSync(file);
  const type = TYPES[path.extname(name)] ?? "application/octet-stream";
  return {
    path: file,
    type,
    body,
    etag: `"${createHash("sha256").update(body).digest("base64url")}"`,
    cache: cacheControl(file),
    encoded: COMPRESSIBLE.test(type) ? compressions(body) : {},
  };
}

/** Every file under `dir`, by its address under `prefix`, into `files`. */
function walk(files, dir, prefix) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    const url = `${prefix}/${e.name}`;
    if (e.isDirectory()) walk(files, p, url);
    else if (e.isFile()) files.set(url, readFile(p, e.name));
  }
}

/** Every file under each mount, read and compressed once. */
export function loadFiles(mounts) {
  const files = new Map();
  for (const m of mounts) walk(files, m.dir, m.prefix);
  return files;
}

/** The directories above `url`, up to the root, into `dirs`. */
function addParents(dirs, url) {
  let d = path.posix.dirname(url);
  while (d && d !== "/" && !dirs.has(d)) {
    dirs.add(d);
    d = path.posix.dirname(d);
  }
}

/** Each mount's prefix and every directory a file is in: the addresses redirected to their slash. */
function directoriesOf(mounts, files) {
  const dirs = new Set(mounts.map((m) => m.prefix));
  for (const url of files.keys()) addParents(dirs, url);
  return dirs;
}

/** Writes one JSON line per answer to `log` (appended to), or nothing. */
function recorder(log) {
  const stream = log ? fs.createWriteStream(log, { flags: "a" }) : null;
  return (entry) => stream?.write(`${JSON.stringify(entry)}\n`);
}

/** An answer: its status, headers, body (none for undefined), the encoding
 * sent and the bytes of body the log counts. */
const answer = (status, headers, body, encoding, bytes) => ({ status, headers, body, encoding, bytes });

const isRead = (req) => req.method === "GET" || req.method === "HEAD";

/** The address without its slash, sent to the one with it, query kept. */
function redirection(pathname, search) {
  return answer(301, { location: `${pathname}/${search}`, "content-length": 0 }, undefined, null, 0);
}

/** An unknown address (404) or a method that does not read a file (405). */
function refusal(file, req) {
  const headers = { "content-type": "text/plain; charset=utf-8", "content-length": 9 };
  return answer(file ? 405 : 404, headers, req.method === "HEAD" ? undefined : "not found", null, 0);
}

/** The encoding a request is sent a file in: none, where the file has none. */
function encodingFor(file, req) {
  return file.encoded.br ? chooseEncoding(req.headers["accept-encoding"]) : null;
}

/** A file's bytes as sent: a HEAD is sent no body and counts none. */
function sentBody(req, body) {
  return req.method === "HEAD" ? [undefined, 0] : [body, body.length];
}

function fileAnswer(file, req, headers) {
  const encoding = encodingFor(file, req);
  const body = encoding ? file.encoded[encoding] : file.body;
  if (encoding) headers["content-encoding"] = encoding;
  headers["content-length"] = body.length;
  const [sent, bytes] = sentBody(req, body);
  return answer(200, headers, sent, encoding, bytes);
}

/** A file's answer to a read: not modified (304) where its validator is
 * the request's, else the file itself (200). */
function readAnswer(file, req) {
  const headers = {
    "content-type": file.type,
    "cache-control": file.cache,
    etag: file.etag,
    "x-content-type-options": "nosniff",
  };
  if (file.encoded.br) headers.vary = "Accept-Encoding";
  if (req.headers["if-none-match"] === file.etag) return answer(304, headers, undefined, null, 0);
  return fileAnswer(file, req, headers);
}

const answerFor = (file, req) => (file && isRead(req) ? readAnswer(file, req) : refusal(file, req));

function send(host, req, res, pathname, a) {
  res.writeHead(a.status, a.headers);
  res.end(a.body);
  host.record({ method: req.method, path: pathname, status: a.status, encoding: a.encoding, bytes: a.bytes });
}

function handle(host, req, res) {
  const url = new URL(req.url, "http://host");
  const pathname = decodeURIComponent(url.pathname);
  if (host.dirs.has(pathname)) return send(host, req, res, pathname, redirection(pathname, url.search));
  const served = pathname.endsWith("/") ? `${pathname}index.html` : pathname;
  return send(host, req, res, served, answerFor(host.files.get(served), req));
}

export function createHost({ mounts, log = null }) {
  const files = loadFiles(mounts);
  const dirs = directoriesOf(mounts, files);
  const record = recorder(log);
  const server = http.createServer((req, res) => handle({ files, dirs, record }, req, res));
  return { server, files };
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  const args = parseArgs(process.argv.slice(2));
  const { server, files } = createHost(args);
  server.listen(args.port, "127.0.0.1", () => {
    console.log(`static-host: ${files.size} files on http://127.0.0.1:${server.address().port}`);
  });
  process.on("SIGTERM", () => server.close(() => process.exit(0)));
}
