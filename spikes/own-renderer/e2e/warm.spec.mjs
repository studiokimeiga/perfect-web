// The first press's ADR: a build that warms fetches, once its page is
// interactive and loaded, the code its presses will need, so that its first
// press waits for no network. E7-L's order still holds: the decision is
// asked first, and a refused handler is never fetched.
//
// The store page is `lazy-handler.spec.mjs`'s, for the same reason it is
// there: five handlers, so "each authorised one" and "only those" are
// claims about sets, not about one URL. A development build warms nothing
// (that spec's last test); a production build says it warms on its handler
// table (`#warm|1`, from `delivery.json`). Here the table is the build's
// own, with that line added, so the page is the one every other spec loads
// and only the build's word differs.

import { test, expect } from "@playwright/test";

/** Every handler module this page has asked for, by identity, in order. */
function watch(page) {
  const fetched = [];
  page.on("request", (r) => {
    const m = /\/handler\/([^/?]+)\.mjs/.exec(r.url());
    if (m) fetched.push(m[1]);
  });
  return fetched;
}

/** The build's handler table, saying that its build warms. */
async function warming(page) {
  await page.route("**/pw-handlers", async (route) => {
    const response = await route.fetch();
    const table = await response.text();
    await route.fulfill({ response, body: `${table}#warm|1\n` });
  });
}

/** The handler identities the document declares, by name. */
async function identities(page) {
  return page.evaluate(() =>
    Object.fromEntries(
      (window.__pw.parts.parts ?? [])
        .filter((p) => p.kind === "event")
        .map((p) => [p.name, p.value]),
    ),
  );
}

async function ready(page, path = "/StorePage.html") {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** The runtime's word that it has warmed, and the number it warmed. */
async function warmed(page) {
  const line = await (
    await page.waitForFunction(() => (window.__pw?.log ?? []).find((l) => /^warming \d+ handler/.test(l)))
  ).jsonValue();
  return Number(/^warming (\d+)/.exec(line)[1]);
}

/** Each of these handlers loaded, by the runtime's own word: fetched and
 * evaluated, its `run` defined and not run. */
async function settled(page, ids) {
  await page.waitForFunction(
    (all) => all.every((id) => (window.__pw?.log ?? []).includes(`loaded ${id}`)),
    ids,
  );
}

test("a build that warms fetches each authorised handler once, after the page is interactive", async ({
  page,
}) => {
  const fetched = watch(page);
  await warming(page);
  await ready(page);
  const ids = await identities(page);
  const all = Object.values(ids).sort();
  expect(all.length, "five handlers on this page").toBe(5);

  expect(await warmed(page)).toBe(5);
  await expect.poll(() => [...fetched].sort()).toEqual(all);
  await settled(page, all);

  // After it was interactive, never before: each module's request started
  // after the runtime's activation ended, as the browser timed both.
  const order = await page.evaluate(() => {
    const active = performance.getEntriesByName("pw:activate:end")[0]?.startTime;
    const starts = performance
      .getEntriesByType("resource")
      .filter((e) => /\/handler\/[^/?]+\.mjs/.test(e.name))
      .map((e) => e.startTime);
    return { active, starts };
  });
  expect(order.active, "the runtime marked its activation").toBeGreaterThan(0);
  expect(order.starts.length).toBe(5);
  for (const start of order.starts) expect(start).toBeGreaterThan(order.active);

  // Loaded, never run: no handler ran before a press.
  expect(await page.evaluate(() => window.__pw.handlerErrors ?? 0)).toBe(0);
  expect(await page.locator("#cart-count").textContent()).toBe("0");
});

test("a warmed handler's first press fetches nothing", async ({ page }) => {
  const fetched = watch(page);
  await warming(page);
  await ready(page);
  const ids = await identities(page);
  await warmed(page);
  await expect.poll(() => fetched.length).toBe(5);
  await settled(page, Object.values(ids));

  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.locator("#clear-cart").click();
  await expect(page.locator("#cart-count")).toHaveText("0");

  expect(fetched.length, "no handler fetched again by its press").toBe(5);
  // And each press ran the module the warming loaded: loaded once each.
  const log = await page.evaluate(() => window.__pw.log);
  expect(log.filter((l) => l === `loaded ${ids.add_to_cart}`)).toHaveLength(1);
  expect(log.filter((l) => l === `loaded ${ids.clear_cart}`)).toHaveLength(1);
});

test("a handler the decision refuses is never warmed", async ({ page }) => {
  // The first press's ADR keeps E7-L's order: decide, then fetch. A
  // document naming a handler this build did not compile (as one cached
  // from another build would) is refused it, and its code never fetched,
  // while the four it may run are warmed.
  const fetched = watch(page);
  await warming(page);
  let refused = "";
  await page.route("**/StorePage.html", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    const manifest = JSON.parse(body.match(/id="pw-parts">(.*?)<\/script>/s)[1]);
    refused = manifest.parts.find((p) => p.kind === "event" && p.name === "add_to_cart").value;
    await route.fulfill({ response, body: body.replaceAll(refused, "ffffffffffffffff") });
  });
  await ready(page);

  expect(await warmed(page)).toBe(4);
  await expect.poll(() => fetched.length).toBe(4);
  await settled(page, fetched);
  expect(fetched).not.toContain("ffffffffffffffff");
  expect(fetched).not.toContain(refused);
  const log = await page.evaluate(() => window.__pw.log.join("\n"));
  expect(log).toMatch(/refused \d+: code 4 /);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("nothing is warmed where every handler is refused", async ({ page }) => {
  const fetched = watch(page);
  await warming(page);
  await page.route("**/StorePage.html", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    await route.fulfill({ response, body: body.replace('"abi":"1"', '"abi":"99"') });
  });
  await ready(page);
  expect(await warmed(page)).toBe(0);
  // A turn of the page's idle time and one of its event loop after the
  // word, for anything the warming left to ask for.
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 50)));
  expect(fetched, "and nothing was fetched").toEqual([]);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("a warm load that fails leaves the first press to fetch it again", async ({ page }) => {
  // The browser keeps a failed module preload under its URL. The press
  // after it must not fail with it: it asks under its next specifier, as a
  // press does after its own failed load (ADR-0382). Every warm load fails
  // here (none is asked for under a retry's specifier); every press's own
  // load is let through.
  await warming(page);
  const asked = [];
  await page.route("**/handler/**", async (route) => {
    const url = new URL(route.request().url());
    asked.push(url.pathname + url.search);
    if (url.searchParams.has("attempt")) await route.continue();
    else await route.abort("failed");
  });
  await ready(page);
  const ids = await identities(page);
  expect(await warmed(page)).toBe(5);
  await page.waitForFunction(
    (all) => all.every((id) => (window.__pw?.log ?? []).some((l) => l.startsWith(`warming ${id} failed`))),
    Object.values(ids),
  );

  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(asked).toContain(`/handler/${ids.add_to_cart}.mjs`);
  expect(asked).toContain(`/handler/${ids.add_to_cart}.mjs?attempt=1`);
  // Not told as a failure: the press worked.
  expect(await page.evaluate(() => window.__pw.handlerErrors ?? 0)).toBe(0);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("the renderer and the speculation module are warmed with the handlers", async ({ page }) => {
  // The store page speculates (ADR-0122) and renders its cart's rows in the
  // browser (ADR-0172): its first Add needs the speculation module and the
  // renderer, and a build that warms has both before it.
  const asked = [];
  page.on("request", (r) => asked.push(new URL(r.url()).pathname));
  await warming(page);
  await ready(page);
  const speculation = await page.evaluate(() => window.__pw.parts.speculation ?? null);
  expect(speculation, "the store page speculates").not.toBeNull();
  await warmed(page);
  await expect.poll(() => asked.filter((p) => p === "/pw-render.wasm").length).toBe(1);
  await expect.poll(() => asked.filter((p) => p === speculation).length).toBe(1);
  await settled(page, Object.values(await identities(page)));
  await page.waitForFunction(() => performance.getEntriesByName(new URL("/pw-render.wasm", location.href).href).length > 0);

  const before = asked.length;
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  const during = asked.slice(before);
  expect(during.filter((p) => p === "/pw-render.wasm" || p === speculation || p.startsWith("/handler/"))).toEqual([]);
});

test("a build that does not warm fetches no handler before a press", async ({ page }) => {
  // The development build's behaviour, unchanged (E7-L): its table says
  // nothing of warming, and the page waits for a press.
  const fetched = watch(page);
  await ready(page);
  // A turn of the browser's idle time, where it has one (WebKit has none).
  await page.evaluate(
    () =>
      new Promise((resolve) =>
        window.requestIdleCallback ? requestIdleCallback(() => resolve()) : setTimeout(resolve, 0),
      ),
  );
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 300)));
  expect(fetched).toEqual([]);
  const log = await page.evaluate(() => window.__pw.log.join("\n"));
  expect(log).not.toMatch(/^warming/m);
});
