// E7 gate item 7: what a page downloads to run, bounded as it is sent, in
// every run of the suite (ADR-0188).
//
// Until 2026-10-04 these ran only in `just e7-performance`, beside the long
// animation frames that need the machine to themselves, and that had last run
// on 2026-08-07. They bounded the bytes the development server sends,
// uncompressed and comments and all, at 128 KiB. The runtime's script grew
// from 28,471 bytes to 84,822, the page passed the bound between ADR-0152 and
// ADR-0172, and nothing said so. A byte count does not need the machine to
// itself.
//
// Each bound is on a file compressed with Brotli at its highest quality, as a
// static file is sent; the uncompressed and gzip figures are reported beside
// it, and E10 reads the uncompressed one (`just e10-bench`).
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import { expect, test } from "@playwright/test";
import { MUTABLE_PORTS } from "../host-metadata.mjs";

// The performance run's host. `just e7-performance` starts it alone, and the
// suite starts it with the rest; nothing in the suite changes it.
test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${MUTABLE_PORTS.performance[testInfo.project.name]}`);
  },
});

test.skip(({ browserName }) => browserName !== "chromium", "a byte is a byte in every engine");

/** A file's size as a static host sends it: Brotli at its highest quality. */
const brotli = (body) =>
  brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;

async function ready(page, url = "/StorePage.html") {
  await page.goto(url);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
}

/** Every byte a route made the browser download, by kind, as served and as sent. */
async function transferred(page, run) {
  const seen = new Map();
  const on = (response) => {
    const type = response.headers()["content-type"] ?? "";
    seen.set(response.url(), { type, response });
  };
  page.on("response", on);
  await run();
  page.off("response", on);

  const out = { script: 0, wasm: 0, document: 0, other: 0, sent: { script: 0, wasm: 0 }, urls: [] };
  for (const [url, { type, response }] of seen) {
    let body;
    try {
      body = await response.body();
    } catch {
      continue;
    }
    const kind = /javascript/.test(type)
      ? "script"
      : /wasm/.test(type)
        ? "wasm"
        : /html/.test(type)
          ? "document"
          : "other";
    out[kind] += body.length;
    if (kind === "script" || kind === "wasm") {
      const sent = brotli(body);
      out.sent[kind] += sent;
      out.urls.push(`${kind} ${body.length} gzip=${gzipSync(body, { level: 9 }).length} brotli=${sent} ${url}`);
    }
  }
  return out;
}

test("gate 7a: the static route ships no browser runtime", async ({ page }) => {
  // The strongest form of "size measured": zero. A static page has nothing to
  // activate, so it downloads no runtime at all — and this is what makes the
  // interactive route's figure a cost of INTERACTIVITY rather than of using
  // the framework.
  const bytes = await transferred(page, async () => {
    await page.goto("/HelloStatic.html");
    await page.waitForLoadState("networkidle");
  });
  expect(bytes.script, "no script").toBe(0);
  expect(bytes.wasm, "no wasm").toBe(0);
  expect(bytes.document, "and the document is real").toBeGreaterThan(100);
  console.log(`EVIDENCE static-route-script-bytes=${bytes.script} wasm-bytes=${bytes.wasm}`);
});

test("the decision's WebAssembly carries none of its trace's words", async ({ request }) => {
  // ADR-0400: no page read the decision's causal trace, and its
  // words were a tenth of the decision every page downloads. A refusal is
  // told by its code (`PWR04`), which pleris.dev explains, and its recovery.
  const wasm = await (await request.get("/pw-resume.wasm")).body();
  for (const words of ["handler identity found", "capture schema matches exactly", "does not admit"]) {
    expect(wasm.includes(words), `the words "${words}"`).toBe(false);
  }
  const { instance } = await WebAssembly.instantiate(wasm, {});
  const exports = Object.keys(instance.exports);
  expect(exports, "nor an export of them").not.toContain("last_trace_ptr");
  expect(exports, "and the decision is whole").toEqual(expect.arrayContaining(["decide_manifest", "last_recovery"]));
});

test("gate 7b: the interactive route's runtime is bounded as it is sent", async ({ page }) => {
  const bytes = await transferred(page, () => ready(page));
  console.log(`EVIDENCE interactive-script-bytes=${bytes.script} brotli=${bytes.sent.script}`);
  console.log(`EVIDENCE interactive-wasm-bytes=${bytes.wasm} brotli=${bytes.sent.wasm}`);
  for (const u of bytes.urls) console.log(`EVIDENCE  ${u}`);

  // A bound, not a target. It exists so the number cannot quietly become a
  // megabyte; the figure itself is the evidence.
  expect(bytes.sent.script + bytes.sent.wasm, "activation, as sent").toBeLessThan(64 * 1024);
  expect(bytes.script, "the runtime really was downloaded").toBeGreaterThan(0);

  // Handler code is NOT in that figure, and that is E7-L's claim restated as a
  // size: behaviour is not part of activation.
  expect(bytes.urls.filter((u) => u.includes("/handler/")), "no handler bytes").toEqual([]);
});

test("gate 7d: a block a signal decides is rendered with what the page already holds", async ({ page }) => {
  // The browser's renderer is the runtime's (the browser renderer's ADR): it
  // was a file of its own, fetched the first time a block was rendered again.
  await page.goto("/page/demo.panel.PanelPage~ui");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  const during = await transferred(page, async () => {
    await page.locator("#open-cart").click();
    await expect(page.locator("#cart-panel")).toBeVisible();
  });
  expect(during.urls.filter((u) => /wasm|render/.test(u)), "no renderer is fetched").toEqual([]);
  expect((await page.request.get("/pw-render.wasm")).status(), "and none is served").toBe(404);
});
