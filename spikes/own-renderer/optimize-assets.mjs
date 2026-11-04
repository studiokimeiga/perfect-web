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

export async function optimizeAssets(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const packed = [];
  for (const entry of entries) {
    // Compiled executable artifacts under build/ are served by a separate
    // route and must not be rewritten without their own identity contract.
    if (!entry.isFile() || !ASSET.test(entry.name)) continue;
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
    packed.push(entry.name);
  }
  return packed;
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
