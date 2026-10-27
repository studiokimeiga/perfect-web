// Track `store-accounts` (ADR-0318, ADR-0319): the store's customer side, in three
// engines, on hosts of its own (STORE_ACCOUNTS_PORTS), one per engine, its
// sessions signed in through the development identity provider
// (`PW_IDENTITY=dev-accounts`).
//   - milestone 1: a guest's cart follows them in at sign-up; a user's cart is
//     theirs in each of their sessions, live, and another user's page shows
//     none of it; a tab whose reader signed in or out in another is refused
//     at its next press, and told so where the press was (Q3);
//   - milestone 2: a store out of reach of the chosen address says so where
//     the menu is, and refuses its Add;
//   - milestone 3, the owner's Next.js bug as acceptance: an address saved,
//     the handler going to the store's page (`navigate StorePage(id)`,
//     ADR-0280), whose estimate is the new address's as served, with no
//     parameter to bust a cache, no load of the document, and the header
//     not mounted again (the soft navigation, ADR-0306);
import { expect, test } from "@playwright/test";
import { STORE_ACCOUNTS_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${STORE_ACCOUNTS_PORTS[testInfo.project.name]}`);
  },
});

/** `path`, its handlers attached. */
async function ready(page, path) {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/**
 * The document an in-page navigation is about to show, and a wait for it to
 * settle: asked before the press that navigates, `settled` awaited once the
 * address has changed. Settled is the next document's runtime booted and its
 * subscription answered, so that nothing the page has just asked for is in
 * flight when the test navigates again. A navigation that cancels requests a
 * page started a few milliseconds before can crash WebKitGTK's network
 * process (a use-after-free in libsoup 3.x), and the navigation then fails or
 * hangs: found by train-10's verify, reproduced on master and outside this
 * program (docs/KNOWN_LIMITATIONS.md; ADR-0280, amended 2026-10-18). A person
 * cannot navigate that soon after a page appears. In WebKit only: Chromium
 * and Firefox still navigate at once, as when such a `goto` found ADR-0306's
 * rule that the browser's own load wins (Firefox's NS_BINDING_ABORTED).
 */
async function nextDocument(page, browserName) {
  if (browserName !== "webkit") return { settled: async () => {} };
  const shown = await page.evaluate(
    () => JSON.parse(document.getElementById("pw-parts").textContent).cursor,
  );
  // A later document's subscription: the one shown now asks with its own.
  const subscribed = page.waitForResponse((response) => {
    const doc = /\/stream\?doc=(\d+)/.exec(response.url())?.[1];
    return doc !== undefined && Number(doc) > shown;
  });
  subscribed.catch(() => {});
  return {
    settled: async () => {
      await subscribed;
      await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
        polling: 50,
      });
    },
  };
}

/** A handle no other test or engine has: at most 15 of a-z, 0-9 and _. */
const handle = (testInfo, who) =>
  `${who}${testInfo.project.name.slice(0, 2)}${(Date.now() % 1e7).toString(36)}`.slice(0, 15);

const PASSWORD = "correct horse battery";

/** Signed up at the development provider, and back at the store's home. */
async function signUp(page, who) {
  await page.goto("/sign-up");
  await expect(page.getByRole("note")).toContainText("not for production");
  await page.getByLabel("Handle").fill(who);
  await page.getByLabel("Name").fill(who);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign up" }).click();
  await expect(page).toHaveURL(/\/$/);
}

/** Signed in at the development provider, and back at the store's home. */
async function signIn(page, who) {
  await page.goto("/sign-in");
  await page.getByLabel("Handle").fill(who);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/$/);
}

/** `name` added from the store's page, its command answered. */
async function add(page, name) {
  const answered = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: `Add ${name}` }).click();
  return answered;
}

const delivery = (page) => page.getByRole("region", { name: "Delivery" });

test("a guest's cart follows them in", async ({ page }, testInfo) => {
  await ready(page, "/stores/47");
  await add(page, "Espresso");
  await expect(page.locator("#cart-count")).toHaveText("1");
  await signUp(page, handle(testInfo, "guest"));
  // Joined before the sign-in was answered: the first page after shows it.
  await ready(page, "/cart");
  await expect(page.locator("#cart-lines")).toContainText("Espresso");
  await expect(page.locator("#cart-count")).toHaveText("1");
});

test("a user's cart is theirs in each of their sessions, live, and no one else's", async ({
  browser,
}, testInfo) => {
  const [laptop, phone, other] = await Promise.all([
    browser.newContext(),
    browser.newContext(),
    browser.newContext(),
  ]);
  const [a, b, c] = await Promise.all([laptop.newPage(), phone.newPage(), other.newPage()]);
  const ada = handle(testInfo, "ada");
  await signUp(a, ada);
  await signIn(b, ada);
  await signUp(c, handle(testInfo, "cy"));
  await ready(b, "/cart");
  await ready(c, "/cart");
  await expect(b.locator("#cart-count")).toHaveText("0");
  await ready(a, "/stores/47");
  await add(a, "Cortado");
  // The phone's open cart is told, without a reload.
  await expect(b.locator("#cart-count")).toHaveText("1");
  await expect(b.locator("#cart-lines")).toContainText("Cortado");
  // Another user's shows none of it.
  await expect(c.locator("#cart-count")).toHaveText("0");
  await ready(c, "/cart");
  await expect(c.locator("#cart-lines")).not.toContainText("Cortado");
  await Promise.all([laptop.close(), phone.close(), other.close()]);
});

test("an address saved goes to the store, whose estimate is the new address's as served", async ({
  page,
}) => {
  // No address yet: the kitchen's estimate alone.
  await ready(page, "/stores/47");
  await expect(delivery(page)).toHaveText(/Delivery in 25 to 35 min/);
  await expect(page.locator("#deliver-to")).toContainText("No delivery address yet");
  await page.locator("#change-address").click();
  await expect(page).toHaveURL(/\/stores\/47\/address$/);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await page.locator("#label").fill("Home");
  // What a load would lose: the window, and the layout's header.
  await page.evaluate(() => {
    window.__kept = true;
    document.querySelector("header").__kept = true;
  });
  // Every load of the store's document from the press on.
  const documents = [];
  page.on("request", (r) => {
    if (r.resourceType() === "document" && new URL(r.url()).pathname === "/stores/47") {
      documents.push(r.url());
    }
  });
  const saved = page.waitForResponse("**/command/store.page.add_address");
  await page.getByRole("button", { name: "Save Dolores Park, San Francisco" }).click();
  await saved;
  await page.waitForURL(/\/stores\/47$/);
  // Union Square to Dolores Park is 3.58 km, 15 minutes at four a kilometre
  // (places.rs): the kitchen's 25 to 35, and the courier's 15.
  await expect(delivery(page)).toHaveText(/Delivery in 40 to 50 min/);
  await expect(page.locator("#deliver-to")).toContainText("Delivering to Home");
  // No parameter to bust a cache, and no load of the document: the store's
  // page shown in the layout kept, its header not mounted again (ADR-0306).
  expect(new URL(page.url()).search).toBe("");
  await page.waitForTimeout(500);
  expect(documents).toEqual([]);
  expect(
    await page.evaluate(() => ({
      window: window.__kept === true,
      header: document.querySelector("header").__kept === true,
      soft: window.__pwSoftNavigations ?? 0,
    })),
    // Two: the link to the address page was followed softly too.
  ).toEqual({ window: true, header: true, soft: 2 });
});

test("a store out of reach of the chosen address says so, and refuses its Add", async ({
  page,
}) => {
  await ready(page, "/stores/48/address");
  await page.locator("#label").fill("Home");
  const saved = page.waitForResponse("**/command/store.page.add_address");
  await page.getByRole("button", { name: "Save Dolores Park, San Francisco" }).click();
  await saved;
  await page.waitForURL(/\/stores\/48$/);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#out-of-range")).toHaveText("Harbor Coffee doesn't deliver to Home.");
  await expect(delivery(page)).toHaveText(/Delivery estimate unavailable/);
  const refused = await add(page, "Drip Coffee");
  expect(refused.status()).toBe(202);
  await expect(page.locator("#cart-notice")).toHaveText(
    "This store doesn't deliver to your address.",
  );
  await expect(page.locator("#cart-count")).toHaveText("0");
});

test("an address chosen goes back to the store, its estimate the chosen one's", async ({
  page,
  browserName,
}) => {
  for (const [label, place] of [
    ["Home", "Dolores Park, San Francisco"],
    ["Work", "Ferry Building, San Francisco"],
  ]) {
    await ready(page, "/stores/47/address");
    await page.locator("#label").fill(label);
    const saved = page.waitForResponse("**/command/store.page.add_address");
    const store = await nextDocument(page, browserName);
    await page.getByRole("button", { name: `Save ${place}` }).click();
    await saved;
    await page.waitForURL(/\/stores\/47$/);
    // The store's page settled before the next `goto`, in WebKit: WebKitGTK's
    // network process can crash when a navigation cancels requests a page has
    // just started (docs/KNOWN_LIMITATIONS.md; ADR-0280, amended 2026-10-18).
    // Chromium and Firefox still navigate at once: such a `goto` found ADR-0306's
    // finding 2 (Firefox's NS_BINDING_ABORTED), which no mutant holds.
    await store.settled();
  }
  // The Ferry Building, the last saved, chosen: 1.47 km, 6 minutes.
  await expect(delivery(page)).toHaveText(/Delivery in 31 to 41 min/);
  await ready(page, "/stores/47/address");
  const chosen = page.waitForResponse("**/command/store.page.choose_address");
  await page.getByRole("button", { name: "Deliver to Home" }).click();
  await chosen;
  await page.waitForURL(/\/stores\/47$/);
  await expect(delivery(page)).toHaveText(/Delivery in 40 to 50 min/);
  await expect(page.locator("#deliver-to")).toContainText("Delivering to Home");
});

test("a tab whose reader signed in or out in another is refused at its next press, and told so", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext();
  const [here, there] = [await context.newPage(), await context.newPage()];
  const words = "You signed in or out in another tab. Reload this page to go on.";
  const told = (name) => here.locator(`button[aria-label="Add ${name}"] + .pw-refusal`);
  // Await an intercepted subscription BEFORE changing identity. Installing
  // a route alone leaves an earlier NDJSON response live: it can reload this
  // tab, making the next press a valid request from a new document instead.
  // subscribe() asks serially, after consuming the previous response. The
  // next interception therefore proves that old response has settled while
  // the reader is still current. Streams normally finish within 80 x 25ms;
  // the condition wait retains Playwright's normal assertion deadline.
  const shownDocument = (page = here) =>
    page.evaluate(() => JSON.parse(document.getElementById("pw-parts").textContent).cursor);
  const held = new Map();
  const hold = async (page = here) => {
    const routes = [];
    held.set(page, routes);
    await page.route("**/stream?*", (route) => routes.push(route));
    await expect.poll(() => routes.length).toBeGreaterThan(0);
    const document = Number(new URL(routes[0].request().url()).searchParams.get("doc"));
    expect(document).toBeGreaterThan(0);
    expect(await shownDocument(page)).toBe(document);
    return document;
  };
  const release = async (page = here) => {
    await page.unroute("**/stream?*");
    // A poll of a document already left was cancelled with it.
    for (const route of held.get(page) ?? []) await route.continue().catch(() => {});
    held.delete(page);
  };
  // A guest's page, and the reader signs up in another tab.
  await ready(here, "/stores/47");
  const guestDocument = await hold();
  await signUp(there, handle(testInfo, "tab"));
  expect(await shownDocument()).toBe(guestDocument);
  const refused = await add(here, "Espresso");
  expect(refused.request().headers()["pw-document"]).toBe(String(guestDocument));
  expect(refused.status()).toBe(403);
  // Told beside the button and by the page's announcer, the speculation
  // taken back, before the command ran.
  await expect(told("Espresso")).toHaveText(words);
  await expect(here.locator(".pw-announcer[role=status]")).toHaveText(words);
  await expect(here.getByRole("button", { name: "Add Espresso" })).toHaveAttribute(
    "data-pw-handler-error",
    "refused:another-reader",
  );
  await expect(here.locator("#cart-count")).toHaveText("0");
  // Read again, the page is the new reader's, and the press runs. Shown
  // before its subscription is let go: let go first, the old document's
  // next poll is told to read the page again, and that reload raced this
  // navigation (`net::ERR_ABORTED`, verify 38309784254).
  await ready(here, "/stores/47");
  await release();
  expect((await add(here, "Espresso")).status()).toBe(202);
  await expect(here.locator("#cart-count")).toHaveText("1");
  // Signed out in the other tab: refused again, the cart as it was.
  const accountDocument = await hold();
  // Its own subscription could otherwise reload the other tab during the
  // sign-out fetch's redirect, aborting that fetch before the stale press.
  await hold(there);
  await there.evaluate(() => fetch("/sign-out", { method: "POST" }));
  expect(await shownDocument()).toBe(accountDocument);
  const again = await add(here, "Cortado");
  expect(again.request().headers()["pw-document"]).toBe(String(accountDocument));
  expect(again.status()).toBe(403);
  await expect(told("Cortado")).toHaveText(words);
  await expect(here.locator("#cart-count")).toHaveText("1");
  await context.close();
});

test("a press made after a soft navigation is the shown page's, and runs", async ({ page }) => {
  // The runtime handed the next page names that page's document (ADR-0306's
  // hand-over), so the host answers its press as the reader's (Q3).
  await ready(page, "/");
  await page.evaluate(() => {
    window.__kept = true;
  });
  await page.getByRole("link", { name: "Blue Bottle" }).click();
  await expect(page).toHaveURL(/\/stores\/47$/);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
  expect(
    await page.evaluate(() => ({
      window: window.__kept === true,
      soft: window.__pwSoftNavigations ?? 0,
    })),
  ).toEqual({ window: true, soft: 1 });
  const answered = await add(page, "Espresso");
  expect(answered.status()).toBe(202);
  await expect(page.locator("#cart-count")).toHaveText("1");
  await expect(page.locator(".pw-refusal")).toHaveCount(0);
});
