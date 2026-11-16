// ADR-0122: an optimistic transition is shown before the round trip, and a
// rejected command is undone by restoring the value the page held.
//
// `add_to_cart` declared `optimistic Cart(current_session()) as cart =>
// Carts.with_line(cart, item, quantity)` from E4 on, and until 2026-10-02
// nothing executed it: the count moved only when the server's patch arrived.
// The command's response is held at the network here, so "before the round
// trip" is observed rather than inferred from timing.

import { test, expect } from "@playwright/test";

async function ready(page) {
  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
}

/** Hold the command's request until `release` is called. */
async function holdCommands(page) {
  const held = [];
  await page.route("**/command/store.page.add_to_cart", async (route) => {
    await new Promise((resolve) => held.push(resolve));
    await route.continue();
  });
  return {
    count: () => held.length,
    release: () => held.splice(0).forEach((r) => r()),
  };
}

test("the count moves before the server answers", async ({ page }) => {
  await ready(page);
  const hold = await holdCommands(page);

  await page.locator("#menu button").first().click();
  // The request has not reached the server, and the page already shows it.
  await expect.poll(hold.count).toBe(1);
  await expect(page.locator("#cart-count")).toHaveText("1");

  // Answered before the page is read again: a reload while the request is
  // in flight may cancel it.
  const answered = page.waitForResponse("**/command/store.page.add_to_cart");
  hold.release();
  await answered;
  await expect(page.locator("#cart-count")).toHaveText("1");
  // And the server agrees, read from a fresh document.
  await page.unroute("**/command/store.page.add_to_cart");
  await page.reload();
  await expect(page.locator("#cart-count")).toHaveText("1");
  // The count is in the server's HTML before the runtime boots, and
  // `window.__pw` is the runtime's: its log is read once the runtime is
  // ready. Firefox read it before, on PR #119's CI (2026-11-16).
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  const log = await page.evaluate(() => window.__pw.log.join("\n"));
  expect(log).not.toMatch(/speculated/);
});

test("a rejected command restores the value the page held", async ({ page }) => {
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");

  // The next add is refused by the server: answered, and not committed.
  await page.route("**/command/store.page.add_to_cart", (route) =>
    route.fulfill({ status: 202, contentType: "application/json", body: '{"committed":false}' }),
  );
  await page.locator("#menu button").first().click();
  // 1, then 2 while the command is out, then 1 again: the log says which.
  await expect
    .poll(() => page.evaluate(() => window.__pw.log.join("\n")), {
      message: "it was shown, then restored",
    })
    .toMatch(/speculated[\s\S]*restored cart/);
  await expect(page.locator("#cart-count")).toHaveText("1");
});

test("a failed request restores it too", async ({ page }) => {
  await ready(page);
  await page.route("**/command/store.page.add_to_cart", (route) => route.abort());
  await page.locator("#menu button").first().click();
  await expect
    .poll(() => page.evaluate(() => window.__pw.log.join("\n")))
    .toMatch(/speculated[\s\S]*restored cart/);
  await expect(page.locator("#cart-count")).toHaveText("0");
});

test("two pending presses show two, and reconcile to the server's two", async ({ page }) => {
  await ready(page);
  const hold = await holdCommands(page);
  await page.locator("#menu button").nth(0).click();
  await page.locator("#menu button").nth(1).click();
  await expect.poll(hold.count).toBe(2);
  await expect(page.locator("#cart-count")).toHaveText("2");

  hold.release();
  await expect(page.locator("#cart-count")).toHaveText("2");
  await expect
    .poll(() => page.evaluate(() => window.__pw.log.filter((l) => l.startsWith("reconciled")).length))
    .toBeGreaterThan(0);
});

test("one press rejected among two keeps the other's speculation", async ({ page }) => {
  await ready(page);
  let sent = 0;
  const pending = [];
  await page.route("**/command/store.page.add_to_cart", async (route) => {
    sent += 1;
    if (sent === 1) {
      // The first is held, then refused.
      await new Promise((resolve) => pending.push(resolve));
      await route.fulfill({ status: 202, contentType: "application/json", body: '{"committed":false}' });
    } else if (sent === 2) {
      await new Promise((resolve) => pending.push(resolve));
      await route.continue();
    } else {
      // The second sent again: the first, refused here, never reached the
      // server, which answered the second early once it had waited for it
      // (ADR-0305).
      await route.continue();
    }
  });
  await page.locator("#menu button").nth(0).click();
  await page.locator("#menu button").nth(1).click();
  await expect.poll(() => pending.length).toBe(2);
  await expect(page.locator("#cart-count")).toHaveText("2");

  // Refuse the first: the second is still shown.
  pending.shift()();
  await expect(page.locator("#cart-count")).toHaveText("1");
  // Commit the second: the server's value, which is one. Answered before
  // the page is read again.
  const committed = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/command/store.page.add_to_cart" && r.status() === 202,
  );
  pending.shift()();
  await committed;
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.unroute("**/command/store.page.add_to_cart");
  await page.reload();
  await expect(page.locator("#cart-count")).toHaveText("1");
});
