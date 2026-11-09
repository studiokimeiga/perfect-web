// ADR-0382: the modules a press needs are loaded within the deadline, and a
// load that failed is not kept. A handler's module and the page's speculation
// module each load when a press first needs them (E7-L). Until 2026-10-21 one
// failed load of the
// speculation module was kept: every later press that speculates failed the
// same way until the page was reloaded, and the network was not asked again.
// And no load had a deadline: one that never answered held its press and, as
// presses run in the order they were made (ADR-0152), every press after it.
// `?load-deadline=` lowers a load's ten seconds, as `?deadline=` lowers a
// send's (ADR-0381), so a test here waits a second and a half for a load.
//
// Each fault is the page's own, so the suite shares its host.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";

const ADD = "store.page.add_to_cart";
/** The deadline these tests give a load, in milliseconds. */
const DEADLINE = 1500;

/** A page, ready to be pressed. Polled: WebKit runs no animation frame
 * before its first paint (`slots.spec.mjs`). */
async function ready(page, path = "/stores/47") {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/** A press that failed, as the runtime marks its control. */
const failed = (page) => page.locator("[data-pw-handler-error]");
const logged = (page, prefix) =>
  page.evaluate((p) => (window.__pw?.log ?? []).filter((l) => l.startsWith(p)).length, prefix);

/** No answer, and no failure either: the request is held open. */
const neverAnswered = () => new Promise(() => {});

for (const [what, pattern] of [["speculation module", "**/speculation/**"]]) {
  for (const [how, fault] of [
    ["failed to load", (route) => route.abort("failed")],
    ["never loaded", neverAnswered],
  ]) {
    test(`a press whose ${what} ${how} is sent unspeculated, and the next press loads it again`, async ({
      page,
    }) => {
      await ready(page, `/stores/47?load-deadline=${DEADLINE}`);
      let calls = 0;
      await page.route(pattern, (route) => (++calls === 1 ? fault(route) : route.continue()));
      try {
        await page.getByRole("button", { name: "Add Espresso" }).click();
        // Sent all the same: the cart is the server's, and the press is not
        // failed for what only shows it early.
        await expect.poll(() => logged(page, `${ADD} sent unspeculated`), { timeout: 8_000 }).toBe(1);
        await expect(page.locator("#cart-count")).toHaveText("1");
        await expect(failed(page)).toHaveCount(0);
        expect(await logged(page, `speculated ${ADD}`)).toBe(0);
        // The next press asks the network again, and is shown before its
        // answer.
        await page.getByRole("button", { name: "Add Espresso" }).click();
        await expect.poll(() => logged(page, `speculated ${ADD}`), { timeout: 8_000 }).toBe(1);
        await expect(page.locator("#cart-count")).toHaveText("2");
        expect(calls).toBe(2);
      } finally {
        await page.unrouteAll({ behavior: "ignoreErrors" });
      }
    });
  }
}

test("a press whose handler's module never loads fails at the deadline, and the next press loads it again", async ({
  page,
}) => {
  await ready(page, `/stores/47?load-deadline=${DEADLINE}`);
  let calls = 0;
  await page.route("**/handler/**", (route) => (++calls === 1 ? neverAnswered() : route.continue()));
  try {
    await page.getByRole("button", { name: "Add Espresso" }).click();
    await expect(failed(page)).toHaveCount(1, { timeout: 6_000 });
    await expect(failed(page)).toHaveAttribute("data-pw-handler-error", "failed");
    await expect(page.locator("#cart-count")).toHaveText("0");
    // Pressed again: its module asked for again, and the press made.
    await page.getByRole("button", { name: "Add Espresso" }).click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    await expect(failed(page)).toHaveCount(0);
    expect(calls).toBe(2);
  } finally {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
});

test("a press made after one whose module never loads runs once that load is given up", async ({ page }) => {
  // One Espresso in the cart, answered before the cart is read.
  await ready(page);
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await expect
    .poll(async () => (await logged(page, "reconciled cart")) > 0, { timeout: 10_000 })
    .toBe(true);
  await ready(page, `/cart?load-deadline=${DEADLINE}`);
  // The first module asked for, the order's handler's, never comes.
  let calls = 0;
  await page.route("**/handler/**", (route) => (++calls === 1 ? neverAnswered() : route.continue()));
  try {
    // From the keyboard: no pointer to miss a button the order's commit
    // moves (`navigate.spec.mjs`).
    await page.getByRole("button", { name: "Place order" }).press("Enter");
    await page.locator("#clear-cart").press("Enter");
    // The order's press fails at the deadline, and the clear, made after it,
    // then runs.
    await expect(page.locator("#cart-count")).toHaveText("0", { timeout: 8_000 });
    await expect(failed(page)).toHaveCount(1);
    expect(calls).toBe(2);
  } finally {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
  // Read again: the clear ran, and no order was placed.
  await ready(page, "/cart");
  await expect(page.locator("#cart-count")).toHaveText("0");
  await ready(page, "/order");
  await expect(page.locator("#order-status")).toHaveText("You have no order yet.");
});
