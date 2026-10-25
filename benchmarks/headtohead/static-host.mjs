#!/usr/bin/env node
// The head-to-head's static host (ADR-XXXX, "the head-to-head"): how a
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
export const COMPRESSIBLE = /^(text\/|application\/(javascript|json|wasm)|image\/svg\+xml)/;

/** A name that carries its content's hash, as Vite and webpack's `[contenthash]` write it. */
export const HASHED = /[.-][A-Za-z0-9_-]{8,}\.(js|mjs|css|wasm|svg|woff2?)$/;

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

/** The encoding a request accepts, best first: br, then gzip, else none. */
export function chooseEncoding(accept = "") {
  const offered = new Map();
  for (const part of accept.split(",")) {
    const [name, ...params] = part.trim().split(";");
    if (!name) continue;
    const q = params.map((p) => p.trim()).find((p) => p.startsWith("q="));
    offered.set(name.trim().toLowerCase(), q ? Number(q.slice(2)) : 1);
  }
  const ok = (name) => (offered.get(name) ?? offered.get("*") ?? 0) > 0;
  if (ok("br")) return "br";
  if (ok("gzip")) return "gzip";
  return null;
}

function parseArgs(argv) {
  const out = { port: 0, mounts: [], log: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--port") out.port = Number(argv[++i]);
    else if (a === "--mount") {
      const [prefix, dir] = argv[++i].split("=");
      out.mounts.push({ prefix: prefix.replace(/\/$/, ""), dir: path.resolve(dir) });
    } else if (a === "--log") out.log = path.resolve(argv[++i]);
    else throw new Error(`static-host: unknown argument ${a}`);
  }
  if (!out.mounts.length) throw new Error("static-host: no --mount");
  return out;
}

/** Every file under each mount, read and compressed once. */
export function loadFiles(mounts) {
  const files = new Map();
  const walk = (dir, prefix) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      const url = `${prefix}/${e.name}`;
      if (e.isDirectory()) walk(p, url);
      else if (e.isFile()) {
        const body = fs.readFileSync(p);
        const type = TYPES[path.extname(e.name)] ?? "application/octet-stream";
        files.set(url, {
          path: p,
          type,
          body,
          etag: `"${createHash("sha256").update(body).digest("base64url")}"`,
          cache: cacheControl(p),
          encoded: COMPRESSIBLE.test(type) ? compressions(body) : {},
        });
      }
    }
  };
  for (const m of mounts) walk(m.dir, m.prefix);
  return files;
}

export function createHost({ mounts, log = null }) {
  const files = loadFiles(mounts);
  const dirs = new Set(mounts.map((m) => m.prefix));
  for (const url of files.keys()) {
    let d = path.posix.dirname(url);
    while (d && d !== "/" && !dirs.has(d)) {
      dirs.add(d);
      d = path.posix.dirname(d);
    }
  }
  const logStream = log ? fs.createWriteStream(log, { flags: "a" }) : null;
  const record = (entry) => logStream?.write(`${JSON.stringify(entry)}\n`);

  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://host");
    let pathname = decodeURIComponent(url.pathname);
    if (dirs.has(pathname)) {
      res.writeHead(301, { location: `${pathname}/${url.search}`, "content-length": 0 });
      res.end();
      record({ method: req.method, path: pathname, status: 301, encoding: null, bytes: 0 });
      return;
    }
    if (pathname.endsWith("/")) pathname += "index.html";
    const file = files.get(pathname);
    if (!file || (req.method !== "GET" && req.method !== "HEAD")) {
      const status = file ? 405 : 404;
      res.writeHead(status, { "content-type": "text/plain; charset=utf-8", "content-length": 9 });
      res.end(req.method === "HEAD" ? undefined : "not found");
      record({ method: req.method, path: pathname, status, encoding: null, bytes: 0 });
      return;
    }
    const headers = {
      "content-type": file.type,
      "cache-control": file.cache,
      etag: file.etag,
      "x-content-type-options": "nosniff",
    };
    if (file.encoded.br) headers.vary = "Accept-Encoding";
    if (req.headers["if-none-match"] === file.etag) {
      res.writeHead(304, headers);
      res.end();
      record({ method: req.method, path: pathname, status: 304, encoding: null, bytes: 0 });
      return;
    }
    const encoding = file.encoded.br ? chooseEncoding(req.headers["accept-encoding"]) : null;
    const body = encoding ? file.encoded[encoding] : file.body;
    if (encoding) headers["content-encoding"] = encoding;
    headers["content-length"] = body.length;
    res.writeHead(200, headers);
    res.end(req.method === "HEAD" ? undefined : body);
    record({
      method: req.method,
      path: pathname,
      status: 200,
      encoding,
      bytes: req.method === "HEAD" ? 0 : body.length,
    });
  });
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
