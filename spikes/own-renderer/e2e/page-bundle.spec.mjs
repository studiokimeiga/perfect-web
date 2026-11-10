// ADR-0412: a production build puts each page's code in one
// module, and a page asks for that rather than for each piece of it.
//
// The build these run against is the development one, whose table names no
// bundle. So the bundle is made here, by the packaging's own `bundlePages`
// from this build's own compiled modules, and the table is the build's with
// that line added: the page is the one every other spec loads, and only the
// build's word differs.

import { cp, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "@playwright/test";
import { bundlePages } from "../optimize-assets.mjs";

const SPIKE = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const PAGE = "store.page.StorePage~ui";
const BUNDLE = "/bundle-under-test.mjs";

/** This build's bundle of `PAGE`, as a production build's packaging makes it. */
async function bundleOf(page) {
  const dir = await mkdtemp(path.join(tmpdir(), "pw-bundle-"));
  try {
    await cp(path.join(SPIKE, "dist/build"), path.join(dir, "build"), { recursive: true });
    await bundlePages(dir);
    return await readFile(path.join(dir, `build/bundles/${page}.mjs`));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Every module of this page's code that is asked for, by its path. */
function watch(page) {
  const asked = [];
  page.on("request", (r) => {
    const p = new URL(r.url()).pathname;
    if (p === BUNDLE || p.startsWith("/handler/") || p.startsWith("/computed/") || p.startsWith("/speculation/")) asked.push(p);
  });
  return asked;
}

/** The build's handler table, naming the bundle, and warming where asked. */
async function tableNamesTheBundle(page, { warm = false } = {}) {
  await page.route("**/pw-handlers", async (route) => {
    const response = await route.fetch();
    const table = await response.text();
    await route.fulfill({ response, body: `${table}${warm ? "#warm|1\n" : ""}#bundle|${PAGE}|${BUNDLE}\n` });
  });
}

async function serves(page, body) {
  await page.route(`**${BUNDLE}`, (route) => route.fulfill({ contentType: "text/javascript", body }));
}

async function ready(page, url = "/StorePage.html") {
  await page.goto(url);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

test("a bundle that does not hold a page's handler loads that handler on its own", async ({ page }) => {
  // A document from another build can be authorised a handler this page's
  // bundle does not hold: the press still runs, from the handler's own module.
  const asked = watch(page);
  await tableNamesTheBundle(page);
  await serves(page, "export const nothing = 1;\n");
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(asked.filter((p) => p.startsWith("/handler/")).length > 0).toBe(true);
});

test("a page whose build bundles its code asks for one module, and its first press runs from it", async ({ page }) => {
  const asked = watch(page);
  await tableNamesTheBundle(page);
  await serves(page, await bundleOf(PAGE));
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.locator("#clear-cart").click();
  await expect(page.locator("#cart-count")).toHaveText("0");
  expect(asked).toEqual([BUNDLE]);
  expect(await page.evaluate(() => window.__pw.handlerErrors ?? 0)).toBe(0);
});

test("a build that warms has the one module before a press, and fetches nothing for it", async ({ page }) => {
  const asked = watch(page);
  await tableNamesTheBundle(page, { warm: true });
  await serves(page, await bundleOf(PAGE));
  await ready(page);
  await page.waitForFunction(() => (window.__pw?.log ?? []).some((l) => /^warming \d+ handler/.test(l)));
  await expect.poll(() => asked.length).toBe(1);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(asked).toEqual([BUNDLE]);
});

test("a page with a handler the decision refuses asks for no bundle", async ({ page }) => {
  // A refused handler is never fetched (E7-L), and a bundle would fetch it.
  const asked = watch(page);
  await tableNamesTheBundle(page, { warm: true });
  await serves(page, await bundleOf(PAGE));
  await page.route("**/StorePage.html", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    const manifest = JSON.parse(body.match(/id="pw-parts">(.*?)<\/script>/s)[1]);
    const refused = manifest.parts.find((p) => p.kind === "event" && p.name === "add_to_cart").value;
    await route.fulfill({ response, body: body.replaceAll(refused, "ffffffffffffffff") });
  });
  await ready(page);
  await page.waitForFunction(() => (window.__pw?.log ?? []).some((l) => /^warming \d+ handler/.test(l)));
  await expect.poll(() => asked.filter((p) => p.startsWith("/handler/")).length).toBe(4);
  expect(asked).not.toContain(BUNDLE);
  expect(asked).not.toContain("/handler/ffffffffffffffff.mjs");
});

test("a bundle that does not load leaves the press to fetch its own module", async ({ page }) => {
  const asked = watch(page);
  await tableNamesTheBundle(page);
  await page.route(`**${BUNDLE}`, (route) => route.abort("failed"));
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(asked[0]).toBe(BUNDLE);
  expect(asked.filter((p) => p.startsWith("/handler/"))).toHaveLength(1);
  // And is not asked for again: the next press fetches its own.
  await page.locator("#clear-cart").click();
  await expect(page.locator("#cart-count")).toHaveText("0");
  expect(asked.filter((p) => p === BUNDLE)).toHaveLength(1);
  expect(await page.evaluate(() => window.__pw.handlerErrors ?? 0)).toBe(0);
  // Told once: the browser keeps a failed module under its address, so
  // asking again would only fail again.
  expect(await page.evaluate(() => window.__pw.log.filter((l) => l.startsWith("the page's module was not loaded")).length)).toBe(1);
});

test("a build with no bundle asks for each module, as it did", async ({ page }) => {
  const asked = watch(page);
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(asked.filter((p) => p.startsWith("/handler/"))).toHaveLength(1);
  expect(asked).not.toContain(BUNDLE);
});
