// ADR-0280: a handler navigates after its command commits. The cart's "Place
// order" goes to the order's page, `navigate OrderPage()`, in its command's
// `Ok` arm. The page it goes to is read after the commit; a refusal stays; a
// press made before the navigation is answered first, whichever answer comes
// first; a press made while the page leaves is not taken; and an `Ok` that
// did not commit goes nowhere. Next.js's Router Cache served a page read
// before the save it followed (the owner's brief): nothing here caches one,
// busts one, or reloads.
import { expect, test } from "@playwright/test";

/** A page, ready to be pressed. Polled: WebKit runs no animation frame
 * before its first paint (`slots.spec.mjs`). */
async function ready(page, path) {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/** The cart page with one Espresso in its session's cart, the add answered
 * before the cart is read (ADR-0268). */
async function oneEspresso(page) {
  await ready(page, "/stores/47");
  const answered = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await answered;
  await ready(page, "/cart");
}

/** Whether a request is the order page's document: a load's, or, since the
 * cart and the order are shown in one layout, the fetch of a navigation that
 * keeps it (ADR-0306). */
const orderPage = (request) =>
  ["document", "fetch"].includes(request.resourceType()) &&
  new URL(request.url()).pathname === "/order";

/** The order's page, answered as another build's once: the navigation that
 * would keep the layout loads it whole, as ADR-0280 made every navigation,
 * so a browser's Stop and its back-forward cache can be tried on it. */
async function wholeOrderPage(page, hold) {
  let foreign = true;
  await page.route("**/order", async (route) => {
    if (route.request().resourceType() === "fetch" && foreign) {
      foreign = false;
      const response = await route.fetch();
      return route.fulfill({
        response,
        headers: { ...response.headers(), "pw-build": "b0000000000000000" },
      });
    }
    if (route.request().resourceType() !== "document") return route.continue();
    await hold();
    await route.continue();
  });
}

const PLACED = "Placed: the store has your order.";

test("an order placed goes to its page, read after the commit", async ({ page }) => {
  await oneEspresso(page);
  // The command reaches the server late: a page left before its answer
  // would be read before the commit, and say there is no order.
  await page.route("**/command/store.page.place_order", async (route) => {
    await new Promise((r) => setTimeout(r, 400));
    await route.continue();
  });
  const served = page.waitForResponse((r) => orderPage(r.request()));
  await page.getByRole("button", { name: "Place order" }).click();
  const response = await served;
  // As served: the order's page, read at the commit or after it.
  expect(await response.text()).toContain(PLACED);
  // At the page's own address: no parameter busts a cache.
  await page.waitForURL(/\/order$/);
  expect(new URL(page.url()).pathname).toBe("/order");
  expect(new URL(page.url()).search).toBe("");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
  await expect(page).toHaveTitle("Your order");
  await expect(page.locator("#order-status")).toHaveText(PLACED);
  // A navigation to another page, not a reload of this one: the cart's
  // layout kept, its window the one loaded first (ADR-0306).
  expect(await page.evaluate(() => window.__pwSoftNavigations)).toBe(1);
  expect(await page.evaluate(() => performance.getEntriesByType("navigation")[0].type)).toBe(
    "navigate",
  );
});

test("a refused order stays on the cart, and says why", async ({ page }) => {
  await ready(page, "/cart");
  const documents = [];
  page.on("request", (r) => orderPage(r) && documents.push(r.url()));
  await page.getByRole("button", { name: "Place order" }).click();
  // Its `Err` arm ran: the handler is done.
  await expect(page.locator("#cart-notice")).toHaveText(
    "Your cart is empty: there is nothing to order.",
  );
  expect(new URL(page.url()).pathname).toBe("/cart");
  expect(documents).toEqual([]);
});

/**
 * Places the order with a second press, "Clear", made while the order's
 * command is in flight, and the answers in the order `first` says: each
 * held until the other's is in. Returns when each answer reached the page,
 * and when the order's page was asked for.
 */
async function placedWithAPressMeanwhile(page, first) {
  await oneEspresso(page);
  const at = {};
  let clearSent;
  const clearRequested = new Promise((r) => (clearSent = r));
  let orderIn;
  const orderAnswered = new Promise((r) => (orderIn = r));
  let clearIn;
  const clearAnswered = new Promise((r) => (clearIn = r));
  await page.route("**/command/store.page.place_order", async (route) => {
    const response = await route.fetch();
    // In flight until the second press is made.
    await clearRequested;
    if (first === "clear") await clearAnswered;
    await route.fulfill({ response });
    orderIn();
  });
  await page.route("**/command/store.page.clear_cart", async (route) => {
    clearSent();
    const response = await route.fetch();
    if (first === "order") {
      await orderAnswered;
      await new Promise((r) => setTimeout(r, 300));
    }
    await route.fulfill({ response });
    clearIn();
  });
  page.on("response", (r) => {
    const path = new URL(r.url()).pathname;
    if (path === "/command/store.page.place_order") at.order = Date.now();
    if (path === "/command/store.page.clear_cart") at.clear = Date.now();
  });
  page.on("request", (r) => {
    if (orderPage(r)) at.document = Date.now();
  });
  await page.getByRole("button", { name: "Place order" }).click();
  // Pressed from the keyboard: the order's commit removes the cart's lines
  // while the press is made, and a pointer's press could land where the
  // button was, focus it, and click nothing (Firefox, under load).
  await page.locator("#clear-cart").press("Enter");
  await page.waitForURL(/\/order$/);
  return at;
}

test("a press made before the navigation is answered first, its answer last", async ({
  page,
}) => {
  const at = await placedWithAPressMeanwhile(page, "order");
  expect(at.order).toBeLessThan(at.clear);
  // The order's answer came first; the page went only once the press made
  // before it was answered.
  expect(at.document).toBeGreaterThanOrEqual(at.clear);
});

test("a press made before the navigation is answered first, its answer first", async ({
  page,
}) => {
  const at = await placedWithAPressMeanwhile(page, "clear");
  expect(at.clear).toBeLessThanOrEqual(at.order);
  expect(at.document).toBeGreaterThanOrEqual(at.order);
});

test("a press made while the page leaves is not taken", async ({ page }) => {
  await oneEspresso(page);
  // The order's page is slow to come: the cart stays, leaving, meanwhile,
  // its next page being fetched to be shown in its layout. It comes when the
  // press below has been made, however long that takes: a hold of 500 ms was
  // a window that a click of Firefox's, under load, missed (the page was
  // swapped under it, its button detached, and the click retried for the
  // test's thirty seconds).
  let asked;
  const requested = new Promise((r) => (asked = r));
  let made;
  const pressMade = new Promise((r) => (made = r));
  await page.route("**/order", async (route) => {
    asked();
    await pressMade;
    await route.continue();
  });
  const cleared = [];
  page.on("request", (r) => {
    if (new URL(r.url()).pathname === "/command/store.page.clear_cart") cleared.push(r.url());
  });
  await page.getByRole("button", { name: "Place order" }).click();
  await requested;
  await page.locator("#clear-cart").click();
  made();
  await page.waitForURL(/\/order$/);
  // The page was leaving: the press sent nothing.
  expect(cleared).toEqual([]);
});

test("a second navigation, made while the page leaves, is not taken", async ({ page }) => {
  await oneEspresso(page);
  // "Place order" twice, the second press made while the first's command
  // is in flight. The second's answer is made here, an `Ok` that committed:
  // the cart is empty by then, and the server would refuse it.
  let first = true;
  let firstSent;
  const firstRequested = new Promise((r) => (firstSent = r));
  let secondSent;
  const secondRequested = new Promise((r) => (secondSent = r));
  await page.route("**/command/store.page.place_order", async (route) => {
    if (first) {
      first = false;
      firstSent();
      const response = await route.fetch();
      await secondRequested;
      await route.fulfill({ response });
      return;
    }
    secondSent();
    await new Promise((r) => setTimeout(r, 200));
    await route.fulfill({
      status: 202,
      contentType: "application/json",
      body: JSON.stringify({ committed: true, result: { $case: "ok" } }),
    });
  });
  const documents = [];
  page.on("request", (r) => orderPage(r) && documents.push(r.url()));
  const button = page.getByRole("button", { name: "Place order" });
  await button.click();
  await firstRequested;
  // From the keyboard, as the press above: no pointer to miss a button the
  // first command's frames may move.
  await button.press("Enter");
  // The first decides; the second, waiting on it as it waits on the second,
  // would hold both for ever.
  await page.waitForURL(/\/order$/, { timeout: 10_000 });
  expect(documents).toHaveLength(1);
});

test("a navigation stopped takes presses again where the browser says it stopped", async ({
  page,
  browserName,
}) => {
  await oneEspresso(page);
  // The order's page never comes; the navigation is stopped instead. Loaded
  // whole, as another build's: a navigation that keeps the layout is no
  // browser's to stop.
  await wholeOrderPage(page, () => new Promise(() => {}));
  const cleared = [];
  page.on("request", (r) => {
    if (new URL(r.url()).pathname === "/command/store.page.clear_cart") cleared.push(r.url());
  });
  // Stopped as it starts, from within the page, as the browser's Stop does.
  await page.evaluate(() =>
    addEventListener("beforeunload", () => setTimeout(() => window.stop(), 50), { once: true }),
  );
  const stopped = page.waitForRequest((r) => orderPage(r));
  await page.getByRole("button", { name: "Place order" }).click();
  await stopped;
  await page.waitForTimeout(500);
  expect(new URL(page.url()).pathname).toBe("/cart");
  await page.locator("#clear-cart").click();
  await page.waitForTimeout(300);
  // Which engines say a navigation stopped, as probed on 2026-10-08:
  // Chromium aborts the `navigate` event's signal; Firefox 146 has no
  // Navigation API; WebKit 26.0's fires the event and aborts nothing.
  if (browserName === "chromium") {
    // Told it stopped: the press is taken.
    expect(cleared).toHaveLength(1);
  } else {
    // Nothing said so: the page is still leaving, and the press is not.
    expect(cleared).toEqual([]);
    // Shown again from the back-forward cache, it takes presses again.
    await page.evaluate(() =>
      dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })),
    );
    await page.locator("#clear-cart").click();
    await expect.poll(() => cleared.length).toBe(1);
  }
});

test("a page shown again from the back-forward cache takes a press again", async ({
  page,
  browserName,
}) => {
  // Chromium's: the test above reaches this in Firefox and WebKit, whose
  // press made as the page unloads does not finish before it goes.
  test.skip(browserName !== "chromium", "covered by the stopped navigation's test");
  await oneEspresso(page);
  // Loaded whole, as another build's (ADR-0306 keeps the
  // layout otherwise, and unloads nothing).
  await wholeOrderPage(page, () => new Promise((r) => setTimeout(r, 500)));
  const cleared = [];
  page.on("request", (r) => {
    if (new URL(r.url()).pathname === "/command/store.page.clear_cart") cleared.push(r.url());
  });
  // As the page leaves, the browser shows it again from its cache, as a
  // back navigation would, and it is pressed: the press is taken.
  await page.evaluate(() =>
    addEventListener(
      "beforeunload",
      () => {
        dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
        document.getElementById("clear-cart").click();
      },
      { once: true },
    ),
  );
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  expect(cleared).toHaveLength(1);
});

test("an `Ok` that did not commit goes nowhere", async ({ page }) => {
  await oneEspresso(page);
  // A server that answers a value without committing.
  await page.route("**/command/store.page.place_order", (route) =>
    route.fulfill({
      status: 202,
      contentType: "application/json",
      body: JSON.stringify({ committed: false, result: { $case: "ok" } }),
    }),
  );
  const documents = [];
  page.on("request", (r) => orderPage(r) && documents.push(r.url()));
  await page.getByRole("button", { name: "Place order" }).click();
  // The press failed, visibly, told beside the button (ADR-0302), and the
  // page stayed.
  await expect(page.locator("#place-order")).toHaveAttribute("data-pw-handler-error", "failed");
  await expect(page.locator("#place-order + .pw-refusal")).toHaveText("This did not work. Try again.");
  expect(new URL(page.url()).pathname).toBe("/cart");
  expect(documents).toEqual([]);
});

test("a page's address carries each value as one segment", async ({ page }) => {
  await ready(page, "/cart");
  const address = (route, args) =>
    page.evaluate(([r, a]) => window.__pw.pageAddress(r, a), [route, args]);
  expect(await address("/stores/{id}", { id: "47" })).toBe("/stores/47");
  // Every byte but RFC 3986's unreserved characters, as a link's is.
  expect(await address("/stores/{id}", { id: "a b/c?d#e" })).toBe("/stores/a%20b%2Fc%3Fd%23e");
  expect(await address("/stores/{id}", { id: "é~-._" })).toBe("/stores/%C3%A9~-._");
  // What no segment carries is refused.
  for (const id of ["", ".", ".."]) {
    const refused = await page.evaluate(
      (value) => {
        try {
          window.__pw.pageAddress("/stores/{id}", { id: value });
          return null;
        } catch (e) {
          return String(e.message);
        }
      },
      id,
    );
    expect(refused, JSON.stringify(id)).toContain("no segment carries it");
  }
});
