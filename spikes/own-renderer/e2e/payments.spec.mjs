// NEXT 6c, step seven: where the deployment takes payments, an order awaits
// its payment on the provider's page, which the order page frames, and the
// provider's signed word places it. The store's build served with the
// development provider (`PW_PAYMENTS=dev`), one host per engine; the reader
// pays inside the frame, as on a provider's hosted fields.
import { expect, test } from "@playwright/test";
import { STORE_PAYMENTS_PORTS } from "../host-metadata.mjs";
import { violations } from "./accessibility-rules.mjs";
import { nextDocument } from "./next-document.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${STORE_PAYMENTS_PORTS[testInfo.project.name]}`);
  },
});

/** A page, ready to be pressed. Polled: WebKit runs no animation frame
 * before its first paint (`slots.spec.mjs`). */
async function ready(page, path) {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/** An espresso in the reader's cart, and the cart's page open. */
async function anEspresso(page) {
  await ready(page, "/stores/47");
  const answered = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await answered;
  await ready(page, "/cart");
}

const AWAITING = "Awaiting your payment: pay below, on the provider's page.";

test("an order awaits its payment, and a card paid in the provider's page places it", async ({
  page,
}) => {
  await anEspresso(page);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  // Nothing has charged it yet.
  await expect(page.locator("#order-total")).toHaveText(/^Total to pay: \$\d/);
  const frame = page.frameLocator("#payment");
  // A decline is no outcome for the order: the same payment takes another
  // card, on the same page.
  await frame.getByLabel("Card number").fill("4000000000000002");
  await frame.getByRole("button", { name: /^Pay / }).click();
  await expect(frame.getByRole("status")).toHaveText("Your card was declined.");
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  await frame.getByLabel("Card number").fill("4242424242424242");
  await frame.getByRole("button", { name: /^Pay / }).click();
  // Told by the provider's word, not by the frame: the page open on the
  // order is sent the change the settling committed.
  await expect(page.locator("#order-status")).toHaveText("Placed: the store has your order.");
  await expect(page.locator("#order-total")).toHaveText(/^Total charged: \$\d/);
  await expect(page.locator("#payment")).toHaveCount(0);
});

test("the order page keeps every rule while it frames the provider's page", async ({ page }) => {
  await anEspresso(page);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  expect(await page.evaluate(violations)).toEqual([]);
  // The control: the frame unnamed is a frame a screen reader cannot name.
  await page.evaluate(() => document.querySelector("#payment").removeAttribute("title"));
  expect(await page.evaluate(violations)).toEqual(["frame-title: <iframe#payment>"]);
});

test("another order waits for the first to be paid", async ({ page, browserName }) => {
  await anEspresso(page);
  const order = await nextDocument(page, browserName);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  // The order's page settled before the next `goto`, in WebKit: WebKitGTK's
  // network process can crash when a navigation cancels requests a page has
  // just started (docs/KNOWN_LIMITATIONS.md; ADR-0280, amended 2026-10-18 and
  // 2026-11-02). Chromium and Firefox still navigate at once.
  await order.settled();
  await anEspresso(page);
  const answered = page.waitForResponse("**/command/store.page.place_order");
  await page.getByRole("button", { name: "Place order" }).click();
  await answered;
  await expect(page.locator("#cart-notice")).toHaveText(
    "An order of yours awaits its payment: pay it on your order's page.",
  );
  await expect(page).toHaveURL(/\/cart$/);
});

test("a reader's press of the command the provider's word invokes is a command that is not there", async ({
  page,
}) => {
  await anEspresso(page);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  // What the provider's word carries, sent by the reader as a press: their
  // order's payment said to have succeeded.
  const forged = await page.request.post("/command/store.page.settle_payment", {
    data: [{ payment: "pi_dev_1", order: "o-0000000000000000", status: { $case: "succeeded" } }],
    headers: { "pw-interaction": "forged-settle" },
  });
  expect(forged.status()).toBe(404);
  await page.waitForTimeout(300);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  // The control: the same route takes a command a reader presses.
  const pressed = await page.request.post("/command/store.page.clear_cart", {
    data: [],
    headers: { "pw-interaction": "control-clear" },
  });
  expect(await pressed.json()).toMatchObject({ committed: true });
});
test("an order awaiting its payment is taken back, its items back in the cart", async ({
  page,
  browserName,
}) => {
  await anEspresso(page);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  await expect(page.locator("#order-status")).toHaveText(AWAITING);
  const taken = page.waitForResponse("**/command/store.page.cancel_order");
  const cart = await nextDocument(page, browserName);
  await page.getByRole("button", { name: "Cancel this order" }).click();
  await taken;
  await page.waitForURL(/\/cart$/);
  await expect(page.locator("#cart-count")).toHaveText("1");
  // The cart's page settled before the next `goto`, in WebKit (as above).
  await cart.settled();
  await ready(page, "/order");
  await expect(page.locator("#order-status")).toHaveText(
    "Canceled: its items are back in your cart.",
  );
  await expect(page.locator("#order-total")).toHaveText("Nothing was charged.");
  await expect(page.locator("#payment")).toHaveCount(0);
});
