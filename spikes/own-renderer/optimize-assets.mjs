// Production-only packaging for the Pleris browser assets.
// The compiler/runtime sources stay readable; generated copies are minified
// and compressed deterministically. HTML and personalized responses are untouched.
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import { transform } from "esbuild";

const BR = {
  params: {
    [constants.BROTLI_PARAM_QUALITY]: 11,
    [constants.BROTLI_PARAM_MODE]: constants.BROTLI_MODE_GENERIC,
  },
};
const ASSET = /\.(?:mjs|js|wasm|css|svg)$/;

async function packAsset(directory, entry) {
  const file = path.join(directory, entry.name);
  let body = await readFile(file);
  if (entry.name === "pw-runtime.mjs") {
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
  await writeFile(file + ".br", brotliCompressSync(body, BR));
  await writeFile(file + ".gz", gzipSync(body, { level: 9, mtime: 0 }));
}

/**
 * **How a production build is delivered** (the first press's ADR): its
 * pages warm what their presses need once they are interactive, rather than
 * fetching each press's code when it is pressed (`warm`; the server tells
 * the browser on its handler table, `#warm|1`); and its files are named at
 * one version, a digest of the build and of every file beside its documents,
 * which a browser keeps until the build changes (`versioned`). The server
 * reads this file beside the assets. A development build has none: it warms
 * nothing (E7-L), and names each file as it is.
 */
export const DELIVERY = { warm: true, versioned: true };

/**
 * **The compiled modules a page loads** (the first press's ADR): its
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
    let entries;
    try {
      entries = await readdir(path.join(directory, sub), { withFileTypes: true });
    } catch (error) {
      if (error.code === "ENOENT") continue;
      throw error;
    }
    const names = entries
      .filter((entry) => entry.isFile() && entry.name.endsWith(".mjs"))
      .map((entry) => entry.name)
      .sort((a, b) => a.localeCompare(b, "en"));
    for (const name of names) {
      const file = path.join(directory, sub, name);
      const body = await readFile(file);
      await writeFile(file + ".br", brotliCompressSync(body, BR));
      await writeFile(file + ".gz", gzipSync(body, { level: 9, mtime: 0 }));
      done.push(`${sub}/${name}`);
    }
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
  for (const asset of assets) await packAsset(directory, asset);
  await compressModules(directory);
  await writeFile(path.join(directory, "delivery.json"), `${JSON.stringify(DELIVERY)}\n`);
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
