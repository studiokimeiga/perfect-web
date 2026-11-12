// Production-only packaging for the Pleris browser assets.
// The compiler/runtime sources stay readable; generated copies are minified
// and compressed deterministically. HTML and personalized responses are untouched.
import { createHash } from "node:crypto";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import { build, transform } from "esbuild";

const BR = {
  params: {
    [constants.BROTLI_PARAM_QUALITY]: 11,
    [constants.BROTLI_PARAM_MODE]: constants.BROTLI_MODE_GENERIC,
  },
};
const ASSET = /\.(?:mjs|js|wasm|css|svg)$/;

/**
 * **What the page reports about itself, for its tests** (ADR-0430): each a
 * statement of the runtime labeled `REPORT`, which a production build drops,
 * its arguments with it, so that no page carries what only a suite reads. A
 * build for a suite that reads them keeps them (`report`).
 */
const reports = (report) => (report ? [] : ["REPORT"]);

/**
 * **The runtime of a page that talks to no server** (ADR-0413):
 * the runtime with `PW_SERVER` false, and what only a server's page
 * uses dropped with it. Its one source is the runtime's, so the two cannot
 * drift, and a page's document names which it runs.
 */
async function packDeviceRuntime(directory, source, report) {
  const uses = await deviceFeatures(directory);
  let device = source.replace("const PW_SERVER = true;", "").replaceAll("PW_SERVER", "false");
  // Whether a press is heard at the document, as the views say: a literal, so
  // what only a listener on each element needs falls away with it.
  device = device.replace("const DELEGATED = !false && !PW_VIEWS;", "").replaceAll("DELEGATED", String(!uses.PW_VIEWS));
  for (const [flag, used] of Object.entries(uses)) {
    device = device.replace(`const ${flag} = true;`, "").replaceAll(flag, String(used));
  }
  const { code } = await transform(alone(device, false), {
    loader: "js",
    format: "esm",
    target: "es2022",
    minify: true,
    treeShaking: true,
    dropLabels: reports(report),
    legalComments: "inline",
    charset: "utf8",
  });
  const file = path.join(directory, "pw-runtime-device.mjs");
  await writeFile(file, code);
  await writeFile(file + ".br", brotliCompressSync(Buffer.from(code), BR));
  await writeFile(file + ".gz", gzipSync(Buffer.from(code), { level: 9, mtime: 0 }));
  // As folded before `PW_ALONE` is: what a page alone in its build's module
  // is made from (`aloneModule`).
  return device;
}

/** `source` with `PW_ALONE` the literal `is`, so that what only a page alone
 * in its build runs, or what only every other page runs, falls away. A
 * runtime that declares none, a test's stand-in, is as written. */
function alone(source, is) {
  const declared = "const PW_ALONE = false;";
  if (!source.includes(declared)) return source;
  return source.replace(declared, "").replaceAll("PW_ALONE", String(is));
}

/**
 * **A page alone in its build, as one module** (ADR-0426): the
 * device's runtime, the page's one module (ADR-0412) imported into it, and
 * `PW_ALONE` true, so that it reads its table from the document that holds
 * it and its code from itself. Written where the build has exactly one page,
 * that page runs on the device (ADR-0413) and has its module; the host puts
 * it in that page's document. `deviceSource` is the device's runtime as
 * `packDeviceRuntime` folded it, its reports dropped as that one's are.
 * Returns the page, or `null` for none.
 */
async function aloneModule(directory, deviceSource, report) {
  // A runtime that declares no `PW_ALONE`, a test's stand-in, has none.
  if (deviceSource === null || !deviceSource.includes("const PW_ALONE = false;")) return null;
  const base = path.join(directory, "build");
  const names = (await readdir(path.join(base, "pages")).catch(() => [])).filter((n) => n.endsWith(".json"));
  if (names.length !== 1) return null;
  const page = names[0].slice(0, -".json".length);
  const plan = JSON.parse(await readFile(path.join(base, "pages", names[0]), "utf8"));
  if (plan.prerender !== true || (plan.layout !== undefined && plan.layout !== null)) return null;
  if ((await readIfThere(path.join(base, "bundles", `${page}.mjs`))) === null) return null;
  const held = "let alonePage = null;";
  if (!deviceSource.includes(held)) throw new Error("the device's runtime holds no place for a page alone in its build");
  const contents = `import * as PW_ALONE_PAGE from ${JSON.stringify(`./bundles/${page}.mjs`)};\n` +
    alone(deviceSource, true).replace(held, "let alonePage = PW_ALONE_PAGE;");
  const result = await build({
    stdin: { contents, resolveDir: base, loader: "js", sourcefile: `${page}.alone.mjs` },
    bundle: true,
    format: "esm",
    target: "es2022",
    treeShaking: true,
    minify: true,
    dropLabels: reports(report),
    charset: "utf8",
    legalComments: "inline",
    write: false,
    logLevel: "silent",
  });
  await mkdir(path.join(base, "alone"), { recursive: true });
  await writeFile(path.join(base, "alone", `${page}.mjs`), result.outputFiles[0].contents);
  return page;
}

/** A part a row's plan sets in place, as the runtime's `planned` reads it:
 * held twice, as the packaging decides what the runtime would, and held
 * together by `optimize-assets.test.mjs`. */
// fallow-ignore-next-line code-duplication
const HOLES = (v) =>
  (v.part === "text" && v.context === "text") ||
  (v.part === "attribute" && v.context !== "content") ||
  v.part === "interpolated_attribute" ||
  v.part === "boolean_attribute" ||
  v.part === "event";

/** **Each part of `chunks`**, and of each template its instances reach,
 * through every block, each template once: `visit(part)` for a part, and
 * `visit(null, chunk)` for markup. */
function walkParts(chunks, templates, visit, seen) {
  for (const chunk of chunks) {
    if (chunk.chunk === "static") {
      visit(null, chunk);
      continue;
    }
    const part = chunk.value;
    visit(part, chunk);
    if (part.part === "instance" && !seen.has(part.path)) {
      seen.add(part.path);
      const view = templates.find((t) => t.path === part.path);
      if (view) walkParts(view.chunks, templates, visit, seen);
    }
    for (const inner of [part.then, part.otherwise, part.body, part.placeholder, part.ready?.body, part.failed?.body, ...(part.arms ?? []).map((a) => a.body)]) {
      if (inner) walkParts(inner, templates, visit, seen);
    }
  }
}

/** What a template's parts need of the runtime. */
function featuresIn(chunks, templates, out, seen) {
  walkParts(chunks, templates, function needs(part, chunk) {
    if (part === null) {
      if (/<dialog[\s>]/i.test(chunk.value)) out.PW_DIALOGS = true;
      if (/\sautofocus[\s=>/]/i.test(chunk.value)) out.PW_AUTOFOCUS = true;
      return;
    }
    if (part.part === "match") out.PW_MATCH = true;
    if (part.context === "url" || part.context === "style") out.PW_LINKS = true;
    if (part.name === "autofocus") out.PW_AUTOFOCUS = true;
    if (part.part === "each" && part.key != null && part.body.every((c) => c.chunk === "static" || HOLES(c.value))) out.PW_PLANS = true;
    if (part.part === "instance") {
      out.PW_VIEWS = true;
    }
  }, seen);
  return out;
}

/**
 * **What this build's pages that talk to no server use** (ADR-0413):
 * the host starts the device's runtime for a page no reader decides and
 * with no layout, and each of these is true where one of them needs it. A build
 * that cannot be read keeps everything.
 */
async function deviceFeatures(directory) {
  const everything = { PW_LINKS: true, PW_MATCH: true, PW_VIEWS: true, PW_DIALOGS: true, PW_KEPT: true, PW_FRAGMENT: true, PW_AUTOFOCUS: true, PW_PLANS: true, PW_GRAPHS: true };
  const base = path.join(directory, "build");
  const text = await readIfThere(path.join(base, "templates.json"));
  if (text === null) return everything;
  const templates = JSON.parse(text);
  const out = { PW_LINKS: false, PW_MATCH: false, PW_VIEWS: false, PW_DIALOGS: false, PW_KEPT: false, PW_FRAGMENT: false, PW_AUTOFOCUS: false, PW_PLANS: false, PW_GRAPHS: false };
  for (const name of await readdir(path.join(base, "pages")).catch(() => [])) {
    if (!name.endsWith(".json")) continue;
    const plan = JSON.parse(await readFile(path.join(base, "pages", name), "utf8"));
    if (plan.prerender !== true || (plan.layout !== undefined && plan.layout !== null)) continue;
    if ((plan.device ?? []).length > 0) out.PW_KEPT = true;
    if (plan.fragment) out.PW_FRAGMENT = true;
    const template = templates.find((t) => t.path === name.slice(0, -".json".length));
    if (!template) return everything;
    featuresIn(template.chunks, templates, out, new Set());
  }
  out.PW_GRAPHS = await writesGraphs(base);
  return out;
}

/** **Whether a build writes a value of a type that contains itself**
 * (ADR-0428): as its nodes, `$graph`, which the compiler writes into what
 * writes such a value, a page's first value, a handler, a computed module or
 * a component the host runs as it renders. A build none of whose files holds
 * the word holds none, and its device's runtime reads none. Every file is
 * read, whatever its kind: one this does not know of keeps the reading. What
 * the packaging writes is made from the rest and the runtime, which holds the
 * word, and is not read, so a build packed again is packed alike. */
const PACKED = new Set(["alone", "bundles"]);
async function writesGraphs(base, dir = base) {
  for (const entry of await readdir(dir, { withFileTypes: true }).catch(() => [])) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!(dir === base && PACKED.has(entry.name)) && (await writesGraphs(base, file))) return true;
    } else if (!/\.(br|gz)$/.test(entry.name) && (await readFile(file)).includes("$graph")) {
      return true;
    }
  }
  return false;
}

/**
 * **The runtime of a page with a server**: the runtime as written, `PW_SERVER`
 * the literal it is declared as, so that what only a page with no server runs
 * falls away, as the device's runtime drops a server's (`packDeviceRuntime`).
 * esbuild folds no constant into the functions that read it: until 2026-11-10
 * a server's page carried the rows' reconciliation, their plans and the
 * presses heard at the document, 2.6 KB as sent.
 */
function serverRuntimeSource(source) {
  // A runtime that declares neither, a test's stand-in, is minified as written;
  // `optimize-assets.test.mjs` holds the runtime's own to what falls away.
  const declared = "const PW_SERVER = true;";
  if (!source.includes(declared)) return source;
  // A server's page is never alone in its build (ADR-0426).
  const server = alone(source.replace(declared, "").replaceAll("PW_SERVER", "true"), false);
  const delegated = "const DELEGATED = !true && !PW_VIEWS;";
  return server.includes(delegated) ? server.replace(delegated, "").replaceAll("DELEGATED", "false") : server;
}

/** One asset packed; the device's runtime's folded source where it was the
 * runtime, else `null`. */
async function packAsset(directory, entry, report) {
  const file = path.join(directory, entry.name);
  let body = await readFile(file);
  let device = null;
  if (entry.name === "pw-runtime.mjs") {
    device = await packDeviceRuntime(directory, body.toString("utf8"), report);
    const result = await transform(serverRuntimeSource(body.toString("utf8")), {
      loader: "js",
      format: "esm",
      target: "es2022",
      minify: true,
      treeShaking: true,
      dropLabels: reports(report),
      legalComments: "inline",
      charset: "utf8",
    });
    body = Buffer.from(result.code);
    await writeFile(file, body);
  }
  if (entry.name.endsWith(".css")) {
    const result = await transform(body.toString("utf8"), { loader: "css", minify: true, legalComments: "inline", charset: "utf8" });
    body = Buffer.from(result.code);
    await writeFile(file, body);
  }
  await writeFile(file + ".br", brotliCompressSync(body, BR));
  await writeFile(file + ".gz", gzipSync(body, { level: 9, mtime: 0 }));
  return device;
}

/**
 * **How a production build is delivered** (ADR-0399): its
 * pages warm what their presses need once they are interactive, rather than
 * fetching each press's code when it is pressed (`warm`; the server tells
 * the browser on its handler table, `#warm|1`); and its files are named at
 * one version, a digest of the build and of every file beside its documents,
 * which a browser keeps until the build changes (`versioned`). The server
 * reads this file beside the assets. A development build has none: it warms
 * nothing (E7-L), and names each file as it is.
 */
export const DELIVERY = { warm: true, versioned: true, bundled: true, device: true, inline: true };

/**
 * **The compiled modules a page loads** (ADR-0399): its
 * handlers, its computed values and its speculations, in the build's
 * directory, where the server serves them from. A production page warms
 * them, so each is sent with every first visit.
 */
const MODULES = ["build/handlers", "build/computed", "build/speculations"];

/**
 * Each compiled module compressed beside itself, as it is: never minified or
 * otherwise rewritten, since its bytes are its compiler's and its URL its
 * identity's (`/handler/<identity>.mjs`). Brotli and gzip, as the root
 * assets are; the server sends the one a request accepts. Returns the
 * modules, relative to `directory`, in a deterministic order.
 */
export async function compressModules(directory) {
  const done = [];
  for (const sub of MODULES) {
    for (const name of await modulesIn(path.join(directory, sub))) {
      const file = path.join(directory, sub, name);
      const body = await readFile(file);
      await writeFile(file + ".br", brotliCompressSync(body, BR));
      await writeFile(file + ".gz", gzipSync(body, { level: 9, mtime: 0 }));
      done.push(`${sub}/${name}`);
    }
  }
  return done;
}

/** The modules in one of a build's module directories, sorted; none where
 * the build has no such directory. */
async function modulesIn(directory) {
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".mjs"))
    .map((entry) => entry.name)
    .sort((a, b) => a.localeCompare(b, "en"));
}

/** The handlers `chunks` names, and each template its instances reach,
 * through every block, each once. */
function handlersIn(chunks, templates, out, seen) {
  walkParts(chunks, templates, function named(part) {
    if (part === null) return;
    if (part.part === "event") out.add(part.handler);
  }, seen);
  return out;
}

async function readIfThere(file) {
  try {
    return await readFile(file, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

/** A prelude's paragraph that declares one name a page's handlers can
 * share: a helper, a case table or a list of literals, its comment before
 * it. A function compiled beside one handler is named by its place in it
 * (`callee0`, `closure0`), and another's of that name is another. */
const SHAREABLE = /^((?:\/\/[^\n]*\n)*)(function|const)\s+([A-Za-z_$][\w$]*)/;
const PLACED = /^(?:callee|closure)\d+$/;
const IDENTIFIER = /[A-Za-z_$][\w$]*/g;
/** **A line of a prelude in the compiler's form** (js_pure's `prelude`):
 * what is inside a declaration indented, and at the margin only a comment,
 * the close of what a line above opened, or a declaration of one name. */
const IN_FORM = /^(?:$|\s|\/\/|[}\])]|function [A-Za-z_$][\w$]*\(|const [A-Za-z_$][\w$]* = )/;
const AT_MARGIN = /^(?:function|const) ([A-Za-z_$][\w$]*)/gm;
// A second name a `const` at the margin declares.
const SECOND = /^const [^\n]*,\s*[A-Za-z_$][\w$]*\s*=/m;

/** **What a page's handlers write alike, written once** (ADR-0428): each
 * compiled handler's prelude, what precedes its `name`, is its declarations,
 * a paragraph each, or a paragraph for two where one is the other's alone
 * (`WHITE` and `trim`); one that two or more of them hold word for word is
 * a shared module's, each of its names exported, which each imports, so the
 * page's module holds it once where esbuild would hold a copy for each.
 *
 * Each name in what is shared means what it meant in each handler that
 * imports it. A handler's names are those it declares at its prelude's
 * margin, as the compiler writes every prelude; one whose prelude is in
 * another form imports nothing. A shared paragraph says no name a handler
 * declares that is not shared too, every word of it counted, a comment's
 * too; and a handler imports what it holds of them only where it holds each
 * one what it imports says, or nothing: one whose `trap` is its own keeps
 * the `int` that reads it. A paragraph fewer than two handlers import is
 * shared with none. `null` where nothing is shared. `texts`: each handler's
 * file, by its path. */
export function sharedPreludes(texts) {
  const words = (p) => [...p.matchAll(IDENTIFIER)].map(([w]) => w);
  const handlers = [];
  // Every name a handler declares.
  const named = new Set();
  for (const [file, text] of texts) {
    const at = text.indexOf("\nexport const name = ");
    if (at < 0) continue;
    // The last paragraph as any other, without the line that ends it.
    const head = text.slice(0, at).replace(/\n+$/, "").split("\n\n");
    if (!head.every((p) => p.split("\n").every((line) => IN_FORM.test(line)) && !SECOND.test(p))) continue;
    for (const p of head) for (const [, name] of p.matchAll(AT_MARGIN)) named.add(name);
    handlers.push({ file, text, at, head });
  }
  // Each paragraph a handler could share, and the names it declares.
  const counts = new Map();
  const nameOf = new Map();
  for (const { head } of handlers) {
    for (const p of new Set(head)) {
      const declared = [...p.matchAll(AT_MARGIN)].map(([, name]) => name);
      if (!SHAREABLE.test(p) || declared.some((name) => PLACED.test(name))) continue;
      counts.set(p, (counts.get(p) ?? 0) + 1);
      nameOf.set(p, declared);
    }
  }
  const held = [...counts].filter(([, n]) => n > 1).map(([p]) => p);
  // A name two paragraphs declare otherwise is shared by neither.
  const declaring = new Map();
  for (const p of held) for (const name of nameOf.get(p)) declaring.set(name, (declaring.get(name) ?? 0) + 1);
  let shared = held.filter((p) => nameOf.get(p).every((name) => declaring.get(name) === 1));
  let names;
  let imported;
  for (;;) {
    let closed = shared;
    for (;;) {
      names = new Map(closed.flatMap((p) => nameOf.get(p).map((name) => [name, p])));
      const next = closed.filter((p) => words(p).every((w) => !named.has(w) || names.has(w)));
      if (next.length === closed.length) break;
      closed = next;
    }
    imported = handlers.map(({ head }) => {
      const mine = new Set(head.filter((p) => closed.includes(p)));
      return [...mine].every((p) => words(p).every((w) => !names.has(w) || mine.has(names.get(w)))) ? mine : new Set();
    });
    const next = closed.filter((p) => imported.filter((mine) => mine.has(p)).length > 1);
    if (next.length === shared.length) break;
    shared = next;
  }
  if (shared.length === 0) return null;
  const rewritten = new Map();
  handlers.forEach(({ file, text, at, head }, i) => {
    const mine = imported[i];
    if (mine.size === 0) return;
    const kept = head.filter((p) => !mine.has(p));
    const imports = `import { ${head.filter((p) => mine.has(p)).flatMap((p) => nameOf.get(p)).join(", ")} } from "pw:shared";`;
    rewritten.set(file, `${imports}\n\n${kept.join("\n\n")}\n${text.slice(at)}`);
  });
  // Each declaration at a shared paragraph's margin, exported.
  const module = shared.map((p) => p.replace(/^(function|const) /gm, "export $1 ")).join("\n\n");
  return { module, rewritten };
}

/** The esbuild plugin that reads each handler as `shared` rewrote it, and
 * `pw:shared` as the module it shares. */
function sharing(shared) {
  return {
    name: "pw-shared-preludes",
    setup(b) {
      b.onResolve({ filter: /^pw:shared$/ }, () => ({ path: "shared", namespace: "pw-shared" }));
      b.onLoad({ filter: /.*/, namespace: "pw-shared" }, () => ({ contents: shared.module, loader: "js" }));
      b.onLoad({ filter: /\.mjs$/ }, (args) => (shared.rewritten.has(args.path) ? { contents: shared.rewritten.get(args.path), loader: "js" } : undefined));
    },
  };
}

/**
 * **Each page's code in one module** (ADR-0412): its handlers,
 * its computed values and its speculations, which a production page warms
 * and each of which was a request of its own, and each a compression of its
 * own. A module's identity, and so each module beside this, is unchanged: a
 * page falls back on them, and a refused handler is never fetched. Returns
 * the pages bundled, sorted.
 */
export async function bundlePages(directory) {
  const base = path.join(directory, "build");
  const templatesText = await readIfThere(path.join(base, "templates.json"));
  if (templatesText === null) return [];
  const templates = JSON.parse(templatesText);
  let pages;
  try {
    pages = (await readdir(path.join(base, "pages"))).filter((n) => n.endsWith(".json")).map((n) => n.slice(0, -5));
  } catch (error) {
    if (error.code === "ENOENT") return [];
    throw error;
  }
  const done = [];
  await mkdir(path.join(base, "bundles"), { recursive: true });
  for (const page of pages.sort((a, b) => a.localeCompare(b, "en"))) {
    const template = templates.find((t) => t.path === page);
    if (!template) continue;
    const handlers = [...handlersIn(template.chunks, templates, new Set(), new Set())].sort();
    const lines = [];
    for (const id of handlers) {
      if ((await readIfThere(path.join(base, "handlers", `${id}.mjs`))) === null) {
        throw new Error(`${page} names handler ${id}, and no module was compiled for it`);
      }
      lines.push(`export * as h_${id} from "./handlers/${id}.mjs";`);
    }
    if ((await readIfThere(path.join(base, "computed", `${page}.mjs`))) !== null) {
      lines.push(`export * as computed from "./computed/${page}.mjs";`);
    }
    if ((await readIfThere(path.join(base, "speculations", `${page}.mjs`))) !== null) {
      lines.push(`export * as speculation from "./speculations/${page}.mjs";`);
    }
    if (lines.length === 0) continue;
    const texts = new Map();
    for (const id of handlers) {
      const file = path.join(base, "handlers", `${id}.mjs`);
      texts.set(file, await readFile(file, "utf8"));
    }
    const shared = sharedPreludes(texts);
    const result = await build({
      stdin: { contents: lines.join("\n"), resolveDir: base, loader: "js", sourcefile: `${page}.entry.mjs` },
      bundle: true,
      format: "esm",
      target: "es2022",
      minify: true,
      charset: "utf8",
      legalComments: "none",
      write: false,
      logLevel: "silent",
      plugins: shared ? [sharing(shared)] : [],
    });
    const body = result.outputFiles[0].contents;
    const file = path.join(base, "bundles", `${page}.mjs`);
    await writeFile(file, body);
    await writeFile(file + ".br", brotliCompressSync(body, BR));
    await writeFile(file + ".gz", gzipSync(body, { level: 9, mtime: 0 }));
    done.push(page);
  }
  return done;
}

/** A production build of `directory`, in place. With `report`, its runtimes
 * keep what the page reports about itself, for a suite that reads it
 * (ADR-0430); a build to serve has none. */
export async function optimizeAssets(directory, { report = false } = {}) {
  const entries = await readdir(directory, { withFileTypes: true });
  // Deterministic order and only root-level shared assets. Compiled handler
  // modules have their own content/identity contract: they are compressed
  // beside themselves (`compressModules`) and never rewritten.
  const assets = entries.filter((entry) => entry.isFile() && ASSET.test(entry.name));
  assets.sort((a, b) => a.name.localeCompare(b.name, "en"));
  const runtime = await readFile(path.join(directory, "pw-runtime.mjs")).catch(() => null);
  let device = null;
  for (const asset of assets) device = (await packAsset(directory, asset, report)) ?? device;
  await compressModules(directory);
  await bundlePages(directory);
  await aloneModule(directory, device, report);
  // What the runtime was before it was packed, and whether it reports: the
  // browser suite's word on whether this build is of the runtime it is
  // testing (`host-metadata.mjs`), and of which build of it.
  const built = { ...DELIVERY, report, ...(runtime && { runtime: createHash("sha256").update(runtime).digest("hex") }) };
  await writeFile(path.join(directory, "delivery.json"), `${JSON.stringify(built)}\n`);
  return assets.map((asset) => asset.name);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [directory, ...rest] = process.argv.slice(2);
  const report = rest.length === 1 && rest[0] === "--report";
  if (!directory || directory.startsWith("--") || (rest.length > 0 && !report)) {
    process.stderr.write("usage: node optimize-assets.mjs DIR [--report]\n");
    process.exitCode = 2;
  } else {
    await optimizeAssets(directory, { report });
  }
}
