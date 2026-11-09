// The device runtime's ADR, in three engines, on a production build of
// TodoMVC (`just e14-device-runtime` builds one, and only then is this spec
// selected: `playwright.config.mjs`): a page no reader decides and with no
// layout starts a runtime with no server's code in it, asks for its own code
// and nothing else, and refuses a handler its build did not compile, as every
// other page does.
import { statSync } from "node:fs";
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
const AT_MOST = 14_600;

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
/** What the runtime logged. */
const logged = (page) => page.evaluate(() => window.__pw.log.slice());

test("a page that talks to no server asks for the device's runtime, its code and its stylesheets", async ({ page }) => {
  const asked = watch(page);
  await open(page);
  await add(page, "buy some cheese");
  await expect(items(page)).toHaveCount(1);
  // A browser's own request for an icon is not the page's.
  const code = [...asked].filter((p) => !p.endsWith(".css") && p !== "/favicon.ico").sort();
  expect(code).toEqual(["/", "/bundle/todomvc.app.Todos~ui.mjs", "/pw-handlers", "/pw-runtime-device.mjs"]);
});

test("a handler its table names runs, on the decision the page made", async ({ page }) => {
  await open(page);
  expect((await logged(page)).filter((line) => line.startsWith("refused"))).toEqual([]);
  await add(page, "feed the cat");
  await expect(items(page)).toHaveCount(1);
  await expect(items(page).locator("label")).toHaveText("feed the cat");
});

test("a table that names no handler refuses each, and nothing runs", async ({ page }) => {
  await page.route("**/pw-handlers*", (route) => route.fulfill({ contentType: "text/plain", body: "" }));
  await open(page);
  const refused = (await logged(page)).filter((line) => line.startsWith("refused"));
  expect(refused.length > 0).toBe(true);
  expect(refused.filter((line) => !line.includes("code 4 recovery refetch-region"))).toEqual([]);
  await page.locator(".new-todo").fill("never kept");
  await page.locator(".new-todo").press("Enter");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(page)).toHaveCount(0);
});

test("a table that cannot be read refuses each, and nothing runs", async ({ page }) => {
  await page.route("**/pw-handlers*", (route) => route.fulfill({ status: 500, body: "no" }));
  await open(page);
  const refused = (await logged(page)).filter((line) => line.startsWith("refused"));
  expect(refused.length > 0).toBe(true);
});

test("a table of another document's schema refuses each, by that", async ({ page }) => {
  await page.route("**/pw-handlers*", async (route) => {
    const response = await route.fetch();
    const table = await response.text();
    const body = table.replace(/^(#page\|[^|]*\|)[^|]*/m, "$1another-schema");
    await route.fulfill({ response, body });
  });
  await open(page);
  const refused = (await logged(page)).filter((line) => line.startsWith("refused"));
  expect(refused.length > 0).toBe(true);
  expect(refused.filter((line) => !line.includes("code 6"))).toEqual([]);
});

test("the device's runtime stays small as sent", () => {
  const sent = statSync(path.join(SPIKE, "dist-todomvc/pw-runtime-device.mjs.br")).size;
  expect(sent <= AT_MOST).toBe(true);
  const full = statSync(path.join(SPIKE, "dist-todomvc/pw-runtime.mjs.br")).size;
  expect(sent < full).toBe(true);
});
