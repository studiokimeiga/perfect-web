// ADR-0306: a navigation between two pages of one layout
// and one build keeps the layout, and shows the next page in it, read after
// the commit before it (ADR-0280). The owner's Next.js bug, its second half:
// save, go to a page, and see it fresh, the header not mounted again.
//
// "Kept" is observed, not inferred: a property set on the header's node is
// still on it after the navigation, and a property set on `window` is too,
// which no load keeps.
import { expect, test } from "@playwright/test";

/** A page, ready to be pressed. */
async function ready(page, path) {
  await page.goto(path);
  await settled(page);
}

/** The runtime of the document shown, booted. */
async function settled(page) {
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/** Marks what a load would lose: the window, and the layout's header. */
function mark(page) {
  return page.evaluate(() => {
    window.__kept = true;
    document.querySelector("header").__kept = true;
  });
}

/** Whether the window and the header are the ones marked. */
function kept(page) {
  return page.evaluate(() => ({
    window: window.__kept === true,
    header: document.querySelector("header").__kept === true,
    soft: window.__pwSoftNavigations ?? 0,
  }));
}

test("a link to a page of the same layout keeps the layout, and shows the page", async ({
  page,
}) => {
  await ready(page, "/");
  await mark(page);
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 1 });
  await expect(page).toHaveTitle("Your cart");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your cart");
  // The page's heading holds the focus: where the new content begins.
  await expect(page.getByRole("heading", { level: 1 })).toBeFocused();
  // And the runtime is the cart page's: its parts, its template.
  expect(await page.evaluate(() => window.__pw.parts.template)).toBe("store.page.CartPage~ui");
});

test("a change, then a link: the page read after it, and the header kept and right", async ({
  page,
}) => {
  // The owner's Next.js bug: the cart changed on the store's page, then its
  // link followed. Nothing kept from before the change is shown.
  await ready(page, "/stores/47");
  await mark(page);
  const answered = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await answered;
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 1 });
  await expect(page.locator("#cart-lines li")).toHaveCount(1);
  await expect(page.locator("#cart-count")).toHaveText("1");
  await expect(page.locator("#header-cart-count")).toHaveText("1");
});

test("back and forward read each page again", async ({ page, context }) => {
  await ready(page, "/");
  await mark(page);
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  // Changed in another tab of the session while this one shows the cart.
  const other = await context.newPage();
  await ready(other, "/stores/47");
  const answered = other.waitForResponse("**/command/store.page.add_to_cart");
  await other.getByRole("button", { name: "Add Espresso" }).click();
  await answered;
  // Back: the stores, read again, the cart counted as it is now.
  await page.goBack();
  await expect(page).toHaveURL(/\/$/);
  await settled(page);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Stores");
  await expect(page.locator("#header-cart-count")).toHaveText("1");
  // Forward: the cart, read again, its line shown.
  await page.goForward();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  await expect(page.locator("#cart-lines li")).toHaveCount(1);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 3 });
});

test("the page it shows is told what changes, and the one it left is not", async ({
  page,
  context,
}) => {
  await ready(page, "/stores/47");
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  const other = await context.newPage();
  await ready(other, "/stores/47");
  await other.getByRole("button", { name: "Add Cortado" }).click();
  // The cart's page hears it, live, as any page does (ADR-0219).
  await expect(page.locator("#cart-lines li")).toHaveCount(1);
  await expect(page.locator("#header-cart-count")).toHaveText("1");
  // Nothing of the store's page it left is applied: no part refused for an
  // address it no longer shows, and nothing read again.
  expect(await page.evaluate(() => window.__pw.refused ?? 0)).toBe(0);
});

test("the page's regions that come after it are filled", async ({ page }) => {
  // The store's page streams its estimate and its recommendations (ADR-0165),
  // after its own content: followed to softly, each is filled.
  await ready(page, "/");
  await mark(page);
  await page.getByRole("link", { name: "Harbor Coffee" }).click();
  await settled(page);
  expect((await kept(page)).header).toBe(true);
  await expect(page.locator("section[aria-label='Delivery']")).toContainText(/Delivery in \d+ to \d+ min/);
  await expect(page.locator("section[aria-label='Recommendations'] li").first()).toBeVisible();
});

test("a handler's navigation after its commit keeps the layout", async ({ page }) => {
  // ADR-0280's `navigate OrderPage()`: the order placed, its page read after
  // the commit, shown in the layout the cart's was.
  await ready(page, "/stores/47");
  const added = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await added;
  await ready(page, "/cart");
  await mark(page);
  await page.getByRole("button", { name: "Place order" }).click();
  await expect(page).toHaveURL(/\/order$/);
  await settled(page);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 1 });
  await expect(page.locator("#order-status")).toHaveText("Placed: the store has your order.");
});

test("a page of another layout, or none, is loaded whole", async ({ page }) => {
  await ready(page, "/");
  await mark(page);
  // A page that names no layout: the dialog's demonstration page.
  await page.evaluate(() => {
    const a = document.createElement("a");
    a.id = "elsewhere";
    a.href = "/page/demo.dialog.DialogPage~ui";
    a.textContent = "Elsewhere";
    document.querySelector("main").append(a);
  });
  await page.locator("#elsewhere").click();
  await expect(page).toHaveURL(/demo\.dialog\.DialogPage~ui$/);
  await page.waitForLoadState("load");
  expect(await page.evaluate(() => window.__kept ?? false)).toBe(false);
});

test("a page of another build is loaded whole", async ({ page }) => {
  await ready(page, "/");
  await mark(page);
  // The cart's page, answered once as another build's would be.
  let answered = false;
  await page.route("**/cart", async (route) => {
    if (answered) return route.continue();
    answered = true;
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: { ...response.headers(), "pw-build": "b0000000000000000" },
    });
  });
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect(await page.evaluate(() => window.__kept ?? false)).toBe(false);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your cart");
});

// ADR-0399: a versioned build's runtime asks for the files a
// browser keeps a year at its own delivery. A host given new files of the
// same build answers with another delivery (`pw-delivery`), whose runtime
// must not meet this one's kept Wasm, so its page is loaded whole. This
// development build names none, so any delivery named is another.
test("a page of another delivery of the same build is loaded whole", async ({ page }) => {
  await ready(page, "/");
  await mark(page);
  let answered = false;
  await page.route("**/cart", async (route) => {
    if (answered) return route.continue();
    answered = true;
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: { ...response.headers(), "pw-delivery": "0000000000000000" },
    });
  });
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect(await page.evaluate(() => window.__kept ?? false)).toBe(false);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your cart");
});

test("a link opened elsewhere, or with a key held, is the browser's", async ({ page }) => {
  await ready(page, "/");
  // A key held: the browser's own (a new tab or window), not this page's.
  const opened = page.context().waitForEvent("page");
  await page.locator("#header-cart").click({ modifiers: ["ControlOrMeta"] });
  const popup = await opened;
  await popup.waitForURL(/\/cart$/);
  await expect(page).toHaveURL(/\/$/);
  expect(await page.evaluate(() => window.__pwSoftNavigations ?? 0)).toBe(0);
});

test("the layout shows what the next page's document read, though this one heard nothing", async ({
  page,
  context,
}) => {
  // This page hears nothing: its subscription is held at the network from
  // the start. The cart changes in another tab of the session.
  await page.route("**/stream?*", () => {});
  await ready(page, "/");
  await mark(page);
  const other = await context.newPage();
  await ready(other, "/stores/47");
  const answered = other.waitForResponse("**/command/store.page.add_to_cart");
  await other.getByRole("button", { name: "Add Espresso" }).click();
  await answered;
  await expect(page.locator("#header-cart-count")).toHaveText("0");
  // Followed to the cart, still hearing nothing: the header kept, its count
  // the one the next document read, which the host derives the next
  // document's patches from. Until 2026-10-10 the page heard again before it
  // left, and under load it could be told the change itself: a layout kept
  // whole then passed too (the mutation controls' survivor).
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect((await kept(page)).header).toBe(true);
  await expect(page.locator("#header-cart-count")).toHaveText("1");
  await page.unroute("**/stream?*");
});

test("what this page is told while it leaves is applied to nothing", async ({ page }) => {
  await ready(page, "/");
  await mark(page);
  // The next page is slow to come, and meanwhile this one is told to read
  // itself again, as a server tells a document it has forgotten.
  let release;
  const held = new Promise((r) => (release = r));
  let asked;
  const requested = new Promise((r) => (asked = r));
  await page.route("**/cart", async (route) => {
    if (route.request().resourceType() !== "fetch") return route.continue();
    asked();
    await held;
    await route.continue();
  });
  await page.locator("#header-cart").click();
  await requested;
  await page.evaluate(() =>
    window.__pwTestBatch({
      frames: [{ frame: "recovery", protocol: 1, recovery: { recovery: "reload" } }],
      cursor: 0,
    }),
  );
  release();
  // Not read again: the navigation went on, and kept the layout.
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 1 });
});

test("a page shown again from the back-forward cache listens again", async ({ page, context }) => {
  // Found mapping the runtime for this ADR: a page hidden into the cache
  // ended its subscription, and shown again never asked again.
  await ready(page, "/cart");
  let failing = true;
  let ended;
  const stopped = new Promise((r) => (ended = r));
  await page.route("**/stream?*", (route) => {
    if (!failing) return route.continue();
    ended();
    return route.abort();
  });
  // Hidden: the browser ends its connections, and it stops asking.
  await page.evaluate(() =>
    dispatchEvent(new PageTransitionEvent("pagehide", { persisted: true })),
  );
  await stopped;
  failing = false;
  // Shown again, and told what changes while it is.
  await page.evaluate(() =>
    dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })),
  );
  const other = await context.newPage();
  await ready(other, "/stores/47");
  await other.getByRole("button", { name: "Add Espresso" }).click();
  await expect(page.locator("#cart-count")).toHaveText("1");
});

test("a reader's next choice abandons a navigation still in flight", async ({ page }) => {
  // A link followed while another is loading: the second is the reader's
  // choice, as in Turbo, SvelteKit and React Router. The first's page,
  // when it comes, is shown nowhere.
  await ready(page, "/stores/47");
  await mark(page);
  let release;
  const held = new Promise((r) => (release = r));
  let asked;
  const requested = new Promise((r) => (asked = r));
  await page.route("**/cart", async (route) => {
    if (route.request().resourceType() !== "fetch") return route.continue();
    asked();
    await held;
    await route.continue().catch(() => {});
  });
  await page.locator("#header-cart").click();
  await requested;
  await page.getByRole("link", { name: "Stores", exact: true }).click();
  await expect(page).toHaveURL(/\/$/);
  await settled(page);
  release();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Stores");
  await page.waitForTimeout(300);
  await expect(page).toHaveURL(/\/$/);
  expect(await kept(page)).toEqual({ window: true, header: true, soft: 1 });
});

test("the document left is asked for nothing more", async ({ page }) => {
  // Its runtime's life ended with the hand-over: its subscription asks the
  // server for its document no more, as a page closed would not.
  await ready(page, "/");
  const left = await page.evaluate(() => window.__pw.parts.cursor);
  await page.locator("#header-cart").click();
  await expect(page).toHaveURL(/\/cart$/);
  await settled(page);
  const asked = [];
  page.on("request", (r) => {
    const url = new URL(r.url());
    if (url.pathname === "/stream" && url.searchParams.get("doc") === String(left)) {
      asked.push(r.url());
    }
  });
  // Longer than the server holds a stream open (about 2.5 s): a
  // subscription still going would have asked again.
  await page.waitForTimeout(4000);
  expect(asked).toEqual([]);
});
