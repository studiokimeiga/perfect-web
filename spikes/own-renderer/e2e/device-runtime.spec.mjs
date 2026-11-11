// ADR-0413, in three engines, on a production build of
// TodoMVC (`just e14-device-runtime` builds one, and only then is this spec
// selected: `playwright.config.mjs`): a page no reader decides and with no
// layout starts a runtime with no server's code in it, and refuses a handler
// its build did not compile, as every other page does. TodoMVC is a page
// alone in its build (ADR-0426): its document holds that runtime,
// its code and its build's handler table, and it asks for nothing else.
import { readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { TODOMVC_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${TODOMVC_PORTS[testInfo.project.name]}`);
  },
});

const SPIKE = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
/** What the device's runtime measured when this was written, as sent, and a
 * little over: it may not grow by accident. */
const AT_MOST = 14_000;

/** Each path the page asks for. */
function watch(page) {
  const asked = new Set();
  page.on("request", (request) => asked.add(new URL(request.url()).pathname));
  return asked;
}

async function open(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

async function add(page, title) {
  await page.locator(".new-todo").fill(title);
  await page.locator(".new-todo").press("Enter");
}

const items = (page) => page.locator(".todo-list li");
/** Each handler the page would not attach, as its console tells a developer
 * (ADR-0400): what a production build says, which reports nothing to its
 * tests (ADR-0430). Watched from before the page is opened. */
function refusals(page) {
  const said = [];
  page.on("console", (message) => {
    if (message.type() === "warning" && message.text().startsWith("Pleris: handler ")) said.push(message.text());
  });
  return said;
}

test("a page alone in its build asks for its document and nothing else, and runs", async ({ page }) => {
  const asked = watch(page);
  await open(page);
  await add(page, "buy some cheese");
  await expect(items(page)).toHaveCount(1);
  // A browser's own request for an icon is not the page's.
  const code = [...asked].filter((p) => p !== "/favicon.ico").sort();
  expect(code).toEqual(["/"]);
});

test("its stylesheets are part of its document: it asks for none, and they apply under its policy", async ({ page }) => {
  await page.addInitScript(() => {
    window.__refused = [];
    document.addEventListener("securitypolicyviolation", (e) => window.__refused.push(e.violatedDirective));
  });
  const asked = watch(page);
  await open(page);
  expect([...asked].filter((p) => p.endsWith(".css"))).toEqual([]);
  const background = await page.evaluate(() => getComputedStyle(document.querySelector(".todoapp")).backgroundColor);
  expect(background).toBe("rgb(255, 255, 255)");
  expect(await page.evaluate(() => window.__refused)).toEqual([]);
});

test("its runtime and its code are its document's one script, which its policy allows", async ({ page }) => {
  await page.addInitScript(() => {
    window.__refused = [];
    document.addEventListener("securitypolicyviolation", (e) => window.__refused.push(e.violatedDirective));
  });
  await open(page);
  const held = await page.evaluate(() => ({
    modules: document.querySelectorAll('script[type="module"]').length,
    fetched: document.querySelectorAll("link[rel=modulepreload], link[rel=preload], script[src]").length,
    table: document.getElementById("pw-handlers")?.type ?? null,
  }));
  expect(held).toEqual({ modules: 1, fetched: 0, table: "application/json" });
  await add(page, "held in its document");
  await expect(items(page)).toHaveCount(1);
  expect(await page.evaluate(() => window.__refused)).toEqual([]);
});

/** The page's document with its handler table changed by `change`, which is
 * given the table's text and returns the text the document holds instead. */
async function tableChanged(page, change) {
  await page.route(
    (url) => url.pathname === "/",
    async (route) => {
      const response = await route.fetch();
      const html = await response.text();
      const body = html.replace(
        /(<script type="application\/json" id="pw-handlers">)([^<]*)(<\/script>)/,
        (_, open, said, close) => open + change(JSON.parse(said)) + close,
      );
      await route.fulfill({ response, body });
    },
  );
}

test("a press that changes one row parses that row alone", async ({ page }) => {
  await page.addInitScript(() => {
    window.__parsed = [];
    const set = Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML").set;
    Object.defineProperty(HTMLTemplateElement.prototype, "innerHTML", {
      set(html) {
        window.__parsed.push(html);
        set.call(this, html);
      },
    });
  });
  await open(page);
  for (const title of ["first of three", "second of three", "third of three"]) await add(page, title);
  await expect(items(page)).toHaveCount(3);
  await page.evaluate(() => (window.__parsed = []));
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  const parsed = await page.evaluate(() => window.__parsed.join(""));
  expect(parsed.includes("second of three")).toBe(true);
  expect(parsed.includes("first of three")).toBe(false);
  expect(parsed.includes("third of three")).toBe(false);
});

test("a box in a row a render left alone shows what its row says", async ({ page }) => {
  await open(page);
  for (const title of ["first", "second", "third"]) await add(page, title);
  await expect(items(page)).toHaveCount(3);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  // What nothing the page rendered says, as a press whose handler changed
  // nothing would leave it.
  await items(page).nth(0).locator(".toggle").evaluate((box) => (box.checked = true));
  await items(page).nth(1).locator(".toggle").evaluate((box) => (box.checked = false));
  await items(page).nth(2).locator(".toggle").check();
  await expect(items(page).nth(2)).toHaveClass(/completed/);
  await expect(items(page).nth(0).locator(".toggle")).not.toBeChecked();
  await expect(items(page).nth(1).locator(".toggle")).toBeChecked();
});

test("a box outside the block a render morphed keeps what it was given", async ({ page }) => {
  await open(page);
  await add(page, "one row");
  await expect(items(page)).toHaveCount(1);
  // Either side of the block, in its parent: no render of the block writes them.
  await page.evaluate(() => {
    const list = document.querySelector(".todo-list");
    for (const [id, place] of [["before", "prepend"], ["beside", "append"]]) {
      const box = document.createElement("input");
      box.type = "checkbox";
      box.id = id;
      list[place](box);
      box.checked = true;
    }
  });
  await add(page, "another row");
  await expect(items(page)).toHaveCount(2);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(page.locator("#before")).toBeChecked();
  await expect(page.locator("#beside")).toBeChecked();
});

test("a handler its table names runs, on the decision the page made", async ({ page }) => {
  const refused = refusals(page);
  await open(page);
  await add(page, "feed the cat");
  await expect(items(page)).toHaveCount(1);
  await expect(items(page).locator("label")).toHaveText("feed the cat");
  expect(refused).toEqual([]);
});

test("a table that names no handler refuses each, and nothing runs", async ({ page }) => {
  const refused = refusals(page);
  await tableChanged(page, () => JSON.stringify(""));
  await open(page);
  await expect.poll(() => refused.length > 0).toBe(true);
  expect(refused.filter((line) => !line.endsWith(": PWR04, https://pleris.dev/e/PWR04, recovery refetch-region"))).toEqual([]);
  await page.locator(".new-todo").fill("never kept");
  await page.locator(".new-todo").press("Enter");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(page)).toHaveCount(0);
});

test("a table that cannot be read refuses each, and nothing runs", async ({ page }) => {
  const refused = refusals(page);
  await tableChanged(page, () => "no");
  await open(page);
  await expect.poll(() => refused.length > 0).toBe(true);
});

test("a table of another document's schema refuses each, by that", async ({ page }) => {
  const refused = refusals(page);
  await tableChanged(page, (table) => JSON.stringify(table.replace(/^(#page\|[^|]*\|)[^|]*/m, "$1another-schema")));
  await open(page);
  await expect.poll(() => refused.length > 0).toBe(true);
  expect(refused.filter((line) => !line.includes(": PWR06, "))).toEqual([]);
});

test("the device's runtime stays small as sent", () => {
  // As a page is sent it: of a build that reports nothing (ADR-0430).
  const { report } = JSON.parse(readFileSync(path.join(SPIKE, "dist-todomvc/delivery.json"), "utf8"));
  expect(report).toBe(false);
  const sent = statSync(path.join(SPIKE, "dist-todomvc/pw-runtime-device.mjs.br")).size;
  expect(sent <= AT_MOST).toBe(true);
  const full = statSync(path.join(SPIKE, "dist-todomvc/pw-runtime.mjs.br")).size;
  expect(sent < full).toBe(true);
});
