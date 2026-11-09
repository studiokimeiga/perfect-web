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
 * **The runtime of a page that talks to no server** (the device runtime's
 * ADR): the runtime with `PW_SERVER` false, and what only a server's page
 * uses dropped with it. Its one source is the runtime's, so the two cannot
 * drift, and a page's document names which it runs.
 */
async function packDeviceRuntime(directory, source) {
  const uses = await deviceFeatures(directory);
  let device = source.replace("const PW_SERVER = true;", "").replaceAll("PW_SERVER", "false");
  // Whether a press is heard at the document, as the views say: a literal, so
  // what only a listener on each element needs falls away with it.
  device = device.replace("const DELEGATED = !false && !PW_VIEWS;", "").replaceAll("DELEGATED", String(!uses.PW_VIEWS));
  for (const [flag, used] of Object.entries(uses)) {
    device = device.replace(`const ${flag} = true;`, "").replaceAll(flag, String(used));
  }
  const { code } = await transform(device, {
    loader: "js",
    format: "esm",
    target: "es2022",
    minify: true,
    treeShaking: true,
    legalComments: "inline",
    charset: "utf8",
  });
  const file = path.join(directory, "pw-runtime-device.mjs");
  await writeFile(file, code);
  await writeFile(file + ".br", brotliCompressSync(Buffer.from(code), BR));
  await writeFile(file + ".gz", gzipSync(Buffer.from(code), { level: 9, mtime: 0 }));
}

/** What a template's parts need of the runtime. */
function featuresIn(chunks, templates, out, seen) {
  for (const chunk of chunks) {
    if (chunk.chunk === "static") {
      if (/<dialog[\s>]/i.test(chunk.value)) out.PW_DIALOGS = true;
      continue;
    }
    const part = chunk.value;
    if (part.part === "match") out.PW_MATCH = true;
    if (part.context === "url" || part.context === "style") out.PW_LINKS = true;
    if (part.part === "instance") {
      out.PW_VIEWS = true;
      if (!seen.has(part.path)) {
        seen.add(part.path);
        const view = templates.find((t) => t.path === part.path);
        if (view) featuresIn(view.chunks, templates, out, seen);
      }
    }
    for (const inner of [part.then, part.otherwise, part.body, part.placeholder, part.ready?.body, part.failed?.body, ...(part.arms ?? []).map((a) => a.body)]) {
      if (inner) featuresIn(inner, templates, out, seen);
    }
  }
  return out;
}

/**
 * **What this build's pages that talk to no server use** (the device runtime's
 * ADR): the host starts the device's runtime for a page no reader decides and
 * with no layout, and each of these is true where one of them needs it. A build
 * that cannot be read keeps everything.
 */
async function deviceFeatures(directory) {
  const everything = { PW_LINKS: true, PW_MATCH: true, PW_VIEWS: true, PW_DIALOGS: true };
  const base = path.join(directory, "build");
  const text = await readIfThere(path.join(base, "templates.json"));
  if (text === null) return everything;
  const templates = JSON.parse(text);
  const out = { PW_LINKS: false, PW_MATCH: false, PW_VIEWS: false, PW_DIALOGS: false };
  for (const name of await readdir(path.join(base, "pages")).catch(() => [])) {
    if (!name.endsWith(".json")) continue;
    const plan = JSON.parse(await readFile(path.join(base, "pages", name), "utf8"));
    if (plan.prerender !== true || (plan.layout !== undefined && plan.layout !== null)) continue;
    const template = templates.find((t) => t.path === name.slice(0, -".json".length));
    if (!template) return everything;
    featuresIn(template.chunks, templates, out, new Set());
  }
  return out;
}

async function packAsset(directory, entry) {
  const file = path.join(directory, entry.name);
  let body = await readFile(file);
  if (entry.name === "pw-runtime.mjs") {
    await packDeviceRuntime(directory, body.toString("utf8"));
    const result = await transform(body.toString("utf8"), {
      loader: "js",
      format: "esm",
      target: "es2022",
      minify: true,
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
  for (const chunk of chunks) {
    if (chunk.chunk !== "dynamic") continue;
    const part = chunk.value;
    if (part.part === "event") out.add(part.handler);
    if (part.part === "instance" && !seen.has(part.path)) {
      seen.add(part.path);
      const view = templates.find((t) => t.path === part.path);
      if (view) handlersIn(view.chunks, templates, out, seen);
    }
    for (const inner of [part.then, part.otherwise, part.body, part.placeholder, part.ready?.body, part.failed?.body, ...(part.arms ?? []).map((a) => a.body)]) {
      if (inner) handlersIn(inner, templates, out, seen);
    }
  }
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

/**
 * **Each page's code in one module** (the page bundle's ADR): its handlers,
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

export async function optimizeAssets(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  // Deterministic order and only root-level shared assets. Compiled handler
  // modules have their own content/identity contract: they are compressed
  // beside themselves (`compressModules`) and never rewritten.
  const assets = entries.filter((entry) => entry.isFile() && ASSET.test(entry.name));
  assets.sort((a, b) => a.name.localeCompare(b.name, "en"));
  const runtime = await readFile(path.join(directory, "pw-runtime.mjs")).catch(() => null);
  for (const asset of assets) await packAsset(directory, asset);
  await compressModules(directory);
  await bundlePages(directory);
  // What the runtime was before it was packed: the browser suite's word on
  // whether this build is of the runtime it is testing (`host-metadata.mjs`).
  const built = { ...DELIVERY, ...(runtime && { runtime: createHash("sha256").update(runtime).digest("hex") }) };
  await writeFile(path.join(directory, "delivery.json"), `${JSON.stringify(built)}\n`);
  return assets.map((asset) => asset.name);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = process.argv[2];
  if (!directory) {
    process.stderr.write("usage: node optimize-assets.mjs DIR\n");
    process.exitCode = 2;
  } else {
    await optimizeAssets(directory);
  }
}
