import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { brotliDecompressSync, gunzipSync } from "node:zlib";
import { optimizeAssets } from "./optimize-assets.mjs";

test("the production build minifies and compresses only public client assets", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "pw-assets-"));
  try {
    const javascript = "export function answer ( ) { return 42; } // comment\n";
    const binary = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0, 0, 0]);
    const privatePage = "<html>session-specific</html>";
    await writeFile(path.join(dir, "pw-runtime.mjs"), javascript);
    await writeFile(path.join(dir, "pw-render.wasm"), binary);
    await writeFile(path.join(dir, "Page.html"), privatePage);
    await writeFile(path.join(dir, "sources.json"), "{}");
    await writeFile(path.join(dir, "license.txt"), "licence");
    const files = await optimizeAssets(dir);
    assert.deepEqual(files, ["pw-render.wasm", "pw-runtime.mjs"].sort());
    const minified = await readFile(path.join(dir, "pw-runtime.mjs"));
    assert.ok(minified.length < Buffer.byteLength(javascript));
    assert.match(minified.toString(), /42/);
    for (const name of files) {
      const bytes = await readFile(path.join(dir, name));
      assert.deepEqual(brotliDecompressSync(await readFile(path.join(dir, name + ".br"))), bytes);
      assert.deepEqual(gunzipSync(await readFile(path.join(dir, name + ".gz"))), bytes);
    }
    assert.equal((await readFile(path.join(dir, "Page.html"))).toString(), privatePage);
    await assert.rejects(readFile(path.join(dir, "Page.html.br")), { code: "ENOENT" });
    await assert.rejects(readFile(path.join(dir, "sources.json.br")), { code: "ENOENT" });
    assert.equal((await readFile(path.join(dir, "license.txt"))).toString(), "licence");
    const originalBr = await readFile(path.join(dir, "pw-runtime.mjs.br"));
    await optimizeAssets(dir);
    assert.deepEqual(await readFile(path.join(dir, "pw-runtime.mjs.br")), originalBr);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
