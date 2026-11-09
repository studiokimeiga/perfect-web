// The longer-holds ADR, in three engines: a page's subscription is held until
// something happens only where that costs no other request a connection. On
// HTTP/1.1 a browser has six connections to an origin, so one tab of the origin
// is held long and the rest as briefly as ever; on HTTP/2 every tab may be.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";

const STORE = "/stores/47";

async function ready(page) {
  await page.goto(STORE);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

const asksLong = (request) => {
  const url = new URL(request.url());
  return url.pathname === "/stream" && url.searchParams.get("hold") === "long";
};
const asks = (request) => new URL(request.url()).pathname === "/stream";

/** Each subscription request `page` makes, as whether it asked to be held long. */
function watch(page) {
  const seen = [];
  page.on("request", (request) => {
    if (asks(request)) seen.push(asksLong(request));
  });
  return seen;
}

test("a tab alone is held long", async ({ page }) => {
  const long = page.waitForRequest(asksLong);
  await ready(page);
  await long;
});

test("a tab pinned to the long poll is held long too", async ({ page }) => {
  const long = page.waitForRequest(asksLong);
  await page.goto(`${STORE}?transport=poll`);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await long;
});

test("the second tab of an origin on HTTP/1.1 is not, until the first closes", async ({ page, context }) => {
  const held = page.waitForRequest(asksLong);
  await ready(page);
  await held;

  const second = await context.newPage();
  const seen = watch(second);
  await ready(second);
  // Three more of its subscriptions, each held briefly, none of them asking.
  for (let i = 0; i < 3; i += 1) await second.waitForRequest(asks);
  expect(seen.length >= 3).toBe(true);
  expect(seen.filter(Boolean)).toEqual([]);

  const handed = second.waitForRequest(asksLong);
  await page.close();
  await handed;
});

test("on HTTP/2 every tab is held long", async ({ page, context }) => {
  await context.addInitScript(() => {
    const real = performance.getEntriesByType.bind(performance);
    performance.getEntriesByType = (type) =>
      type === "navigation" ? [{ nextHopProtocol: "h2" }] : real(type);
  });
  const first = page.waitForRequest(asksLong);
  await ready(page);
  await first;
  const second = await context.newPage();
  const held = second.waitForRequest(asksLong);
  await ready(second);
  await held;
});

test("a change reaches a page held long at once", async ({ page, context }) => {
  // A second tab of the same visitor changes the cart; the first, held long,
  // shows it within far less than its hold.
  const held = page.waitForRequest(asksLong);
  await ready(page);
  await held;
  const other = await context.newPage();
  await ready(other);
  const before = Number(await page.locator("#cart-count").textContent());
  await other.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText(String(before + 1), { timeout: 5_000 });
});
