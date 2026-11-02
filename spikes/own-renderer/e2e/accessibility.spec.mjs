// ADR-0182, charter §15.6 test 14 and §17.4: keyboard and screen-reader
// semantics remain valid, as the store is served and after each kind of
// change that reaches it: a press, its speculation and its answer, a
// refusal, a slot streamed in, and a change to the menu every page is told.
//
// The rules are `accessibility-rules.mjs`'s. A test does not prove a page
// accessible (charter §17.4): what a person still checks with a screen
// reader is `docs/research/screen-reader-smoke-test.md`.
import { writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { MUTABLE_PORTS } from "../host-metadata.mjs";
import { tabStops, violations } from "./accessibility-rules.mjs";
import { nextDocument } from "./next-document.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${MUTABLE_PORTS.accessibility[testInfo.project.name]}`);
  },
});

// An item sold out, and the menu's changes, are one per server: one test at
// a time, each leaving them as it found them.
test.describe.configure({ mode: "serial" });

test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status === testInfo.expectedStatus) return;
  const log = await page.evaluate(() => (window.__pw?.log ?? []).join("\n")).catch(() => "");
  const path = testInfo.outputPath("runtime-log.txt");
  writeFileSync(path, log);
  await testInfo.attach("runtime log", { path, contentType: "text/plain" });
});

/** The store, ready to be pressed. Polled: WebKit runs no animation frame
 * before its first paint (`slots.spec.mjs`). */
async function ready(page, path = "/stores/47", options) {
  await page.goto(path, options);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

/** What the page breaks, by `accessibility-rules.mjs`. */
const audit = (page) => page.evaluate(violations);

const delivery = (page) => page.getByRole("region", { name: "Delivery" });
const recommendations = (page) => page.getByRole("region", { name: "Recommendations" });
const lines = (page) => page.locator("#cart-lines li");

/** The slots come: the page as a person reads it, once nothing is coming. */
async function slots(page, recommended = 2) {
  await expect(delivery(page)).toHaveText(/^\s*Delivery in \d+ to \d+ min\s*$/);
  await expect(recommendations(page).getByRole("listitem")).toHaveCount(recommended);
}

/** Presses answered: a speculation reconciled with the server's answer, or
 * restored where the server refused it. */
const answered = (page) =>
  page.evaluate(
    () => window.__pw.log.filter((l) => /^(reconciled|restored) cart/.test(l)).length,
  );

/** A control pressed, by a pointer or by a key, and its answer come. */
async function press(page, name, key) {
  const before = await answered(page);
  const control = page.getByRole("button", { name, exact: true });
  if (key) await control.press(key);
  else await control.click();
  await expect.poll(() => answered(page)).toBeGreaterThan(before);
}

/** Whether an item can be ordered, told to every page or not. */
async function stock(request, item, available, told = false) {
  const r = await request.post(
    `/bench/stock?item=${item}&available=${available}${told ? "&tell=true" : ""}`,
  );
  expect(r.ok()).toBe(true);
}

/** One of E7-P's operations on the menu, which every page is told. */
async function menu(request, query) {
  const r = await request.post(`/command/menu?${query}`);
  expect(r.status(), query).toBe(202);
}

/** Store 47's menu as the tests found it. */
async function restored(request) {
  const r = await request.get("/menu");
  expect((await r.json()).items).toEqual(["espresso", "cortado", "cold-brew"]);
}

/** The element focus is on, by its name, and whether focus is shown there:
 * an outline or a shadow, where the browser shows focus for a keyboard. */
const focused = (page) =>
  page.evaluate(() => {
    const el = document.activeElement;
    const s = getComputedStyle(el);
    const shown =
      el.matches(":focus-visible") &&
      ((s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== "none");
    const name = el.getAttribute("aria-label") ?? el.textContent.replace(/\s+/g, " ").trim();
    return shown ? name : `${name} (focus not shown)`;
  });

test("the store keeps every rule as it is served, and after each kind of change", async ({
  page,
  request,
}) => {
  await ready(page);
  await slots(page);
  expect(await audit(page), "as served").toEqual([]);

  await press(page, "Add Espresso");
  expect(await audit(page), "a line added").toEqual([]);
  await press(page, "Increase quantity of Espresso");
  expect(await audit(page), "a line's quantity increased").toEqual([]);

  // Sold out, and the page not told: it still offers the item, and the
  // press is refused.
  await stock(request, "cold-brew", false);
  try {
    await press(page, "Add Cold Brew");
    await expect(page.locator("#cart-notice")).toHaveText("That item just sold out.");
    expect(await audit(page), "an add refused").toEqual([]);
  } finally {
    await stock(request, "cold-brew", true);
  }

  // The menu, changed for every page.
  try {
    await menu(request, "op=rename&id=cortado&name=Cortado%20Doppio");
    await expect(page.getByRole("button", { name: "Add Cortado Doppio", exact: true })).toBeVisible();
    expect(await audit(page), "an item renamed").toEqual([]);

    await menu(request, "op=insert_before&id=flat-white&name=Flat%20White&at=espresso");
    await expect(page.getByRole("button", { name: "Add Flat White", exact: true })).toBeVisible();
    expect(await audit(page), "an item new at the head").toEqual([]);

    await menu(request, "op=move&id=cold-brew&after=flat-white");
    await expect(page.locator("#menu li span")).toHaveText([
      "Flat White",
      "Cold Brew",
      "Espresso",
      "Cortado Doppio",
    ]);
    expect(await audit(page), "an item moved").toEqual([]);

    await stock(request, "cortado", false, true);
    await expect(page.locator("#menu li").filter({ hasText: "Cortado Doppio" })).toContainText(
      "Sold out",
    );
    expect(await audit(page), "an item sold out, and every page told").toEqual([]);

    await menu(request, "op=remove&id=flat-white");
    await expect(page.locator("#menu li span")).toHaveText(["Cold Brew", "Espresso", "Cortado Doppio"]);
    expect(await audit(page), "an item removed").toEqual([]);
  } finally {
    await stock(request, "cortado", true, true);
    await request.post("/command/menu?op=remove&id=flat-white");
    await request.post("/command/menu?op=move&id=cold-brew&after=cortado");
    await request.post("/command/menu?op=rename&id=cortado&name=Cortado");
  }
  await restored(request);

  await press(page, "Remove Espresso");
  await expect(lines(page)).toHaveCount(0);
  expect(await audit(page), "the last line removed").toEqual([]);

  // Clearing is not speculated: what it changes is the server's patch.
  await press(page, "Add Cold Brew");
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(page.locator("#cart-count")).toHaveText("0");
  await expect(lines(page)).toHaveCount(0);
  expect(await audit(page), "the cart cleared").toEqual([]);
});

test("another store, and a store that is not there, keep every rule", async ({ page }) => {
  await ready(page, "/stores/48");
  await slots(page);
  expect(await audit(page), "store 48").toEqual([]);
  const absent = await page.goto("/stores/999");
  expect(absent.status()).toBe(404);
  expect(await audit(page), "not found").toEqual([]);
});

test("the order's page keeps every rule, as the store moves it along", async ({
  page,
  browserName,
}) => {
  // ADR-0193: a status said to a screen reader as it changes.
  await ready(page, "/order");
  expect(await audit(page), "no order").toEqual([]);
  await ready(page, "/stores/47");
  // Answered before the cart is read (ADR-0268).
  const added = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await added;
  await expect(page.locator("#cart-count")).toHaveText("1");
  await ready(page, "/cart");
  // Placed, the cart goes to the order's page (ADR-0280); back on the cart,
  // it links there.
  const order = await nextDocument(page, browserName);
  await page.getByRole("button", { name: "Place order" }).click();
  await page.waitForURL(/\/order$/);
  // The order's page settled before the next `goto`, in WebKit: WebKitGTK's
  // network process can crash when a navigation cancels requests a page has
  // just started (docs/KNOWN_LIMITATIONS.md; ADR-0280, amended 2026-10-18).
  // Chromium and Firefox still navigate at once: this `goto` found ADR-0306's
  // finding 2 (Firefox's NS_BINDING_ABORTED), which no mutant holds.
  await order.settled();
  await ready(page, "/cart");
  await expect(page.locator("#order-link")).toBeVisible();
  expect(await audit(page), "the cart, its order placed").toEqual([]);
  await ready(page, "/order");
  await expect(page.locator("#order-status")).toHaveText("Placed: the store has your order.");
  expect(await audit(page), "placed").toEqual([]);
  await page.request.post("/bench/order?status=preparing");
  await expect(page.locator("#order-status")).toHaveText("Preparing: the store is making it.");
  expect(await audit(page), "preparing").toEqual([]);
});

test("the home page keeps every rule", async ({ context }) => {
  // ADR-0192: the stores, and the session's cart beside them.
  const home = await context.newPage();
  await ready(home, "/");
  expect(await audit(home), "as served").toEqual([]);
  const store = await context.newPage();
  await ready(store, "/stores/47");
  await store.getByRole("button", { name: "Add Espresso" }).click();
  await expect(home.locator("#cart-count")).toHaveText("1");
  expect(await audit(home), "its cart counted").toEqual([]);
});

test("the cart's own page keeps every rule, empty and with lines", async ({ context }) => {
  // ADR-0190: a second page that binds a query, audited as the store's is.
  const store = await context.newPage();
  const cart = await context.newPage();
  await ready(cart, "/cart");
  expect(await audit(cart), "empty").toEqual([]);
  await ready(store, "/stores/47");
  await store.getByRole("button", { name: "Add Espresso" }).click();
  await expect(lines(cart)).toHaveCount(1);
  expect(await audit(cart), "a line added on the store's page").toEqual([]);
  await cart.getByRole("button", { name: "Increase quantity of Espresso" }).click();
  await expect(cart.locator("#cart-count")).toHaveText("2");
  expect(await audit(cart), "its quantity increased here").toEqual([]);
  await cart.locator("#clear-cart").click();
  await expect(lines(cart)).toHaveCount(0);
  expect(await audit(cart), "cleared").toEqual([]);
});

test("each page is titled by what it is, and a change to its title is shown once", async ({
  page,
}) => {
  // ADR-0183: the page states its title, from its values. Until 2026-10-04
  // every store's page was "Store" (WCAG 2.4.2, F25).
  await ready(page);
  await expect(page).toHaveTitle("Blue Bottle");
  // A change to what the title reads is a patch to the title's part, which
  // the runtime sets as the document's title; one that changes nothing
  // writes nothing (ADR-0182).
  const { schema, title } = await page.evaluate(() => ({
    schema: window.__pw.parts.schema,
    title: window.__pw.parts.parts.find((p) => p.kind === "title"),
  }));
  expect(title).toMatchObject({ anchor: "document", value: "store.name" });
  await page.evaluate(() => {
    window.__titled = 0;
    new MutationObserver(() => (window.__titled += 1)).observe(document.head, {
      childList: true,
      subtree: true,
      characterData: true,
    });
  });
  const patch = (version, text) =>
    page.evaluate(
      ({ schema, part, version, text }) =>
        window.__pwTestApply({
          frame: "patch",
          protocol: 1,
          basis: { resources: [{ entry: "a-test-title", version }] },
          target: { template: schema, instances: [], part },
          operation: { op: "replace_text", text },
        }),
      { schema, part: title.id, version, text },
    );
  await patch(1, "Blue Bottle Coffee");
  await expect(page).toHaveTitle("Blue Bottle Coffee");
  await patch(2, "Blue Bottle Coffee");
  await expect.poll(() => page.evaluate(() => window.__titled)).toBe(1);
  // Each store's its own, and the page that is not found says so.
  await ready(page, "/stores/48");
  await expect(page).toHaveTitle("Harbor Coffee");
  await page.goto("/stores/999");
  await expect(page).toHaveTitle("Not found");
});

test("the tree a screen reader reads is the page's", async ({ page, request }) => {
  // Read in each engine by Playwright's reading of WAI-ARIA and accname:
  // roles, names, levels and text, every node of them.
  await ready(page);
  await slots(page);
  await expect(page.getByRole("main")).toMatchAriaSnapshot(`
    - main:
      - /children: deep-equal
      - heading "Blue Bottle" [level=1]
      - paragraph: Coffee
      - paragraph: Open 24 hours
      - paragraph: No ratings yet
      - paragraph: Small-batch coffee, served at the bar or carried out. The espresso changes with the season, and the pastries come in every morning.
      - paragraph: 3 items in 1 section
      - paragraph:
        - text: No delivery address yet
        - link "Change the address":
          - /url: /stores/47/address
      - region "Delivery":
        - paragraph: Delivery in 25 to 35 min
      - region "Menu":
        - heading "Coffee" [level=2]
        - list:
          - listitem:
            - text: Espresso
            - paragraph: A double shot, pulled short.
            - paragraph: $3.50
            - button "Add Espresso": Add
          - listitem:
            - text: Cortado
            - paragraph: Espresso cut with an equal part of warm milk.
            - paragraph: $4.25
            - button "Add Cortado": Add
          - listitem:
            - text: Cold Brew
            - paragraph: Steeped for eighteen hours and served over ice.
            - paragraph: $4.75
            - button "Add Cold Brew": Add
      - region "Cart":
        - heading "Cart" [level=2]
        - paragraph: "Items in cart: 0"
        - paragraph: Your cart is empty.
        - list
        - paragraph: "Subtotal: $0.00"
        - status
        - button "Clear"
        - paragraph:
          - link "Go to your cart":
            - /url: /cart
      - region "Recommendations":
        - list:
          - listitem: Cortado
          - listitem: Cold Brew
      - region "What customers said":
        - heading "What customers said" [level=2]
        - paragraph: No reviews yet.
  `);

  // A line, and a refusal said in the status region.
  await press(page, "Add Espresso");
  await stock(request, "cold-brew", false);
  try {
    await press(page, "Add Cold Brew");
    await expect(page.locator("#cart-notice")).toHaveText("That item just sold out.");
  } finally {
    await stock(request, "cold-brew", true);
  }
  await expect(page.getByRole("region", { name: "Cart" })).toMatchAriaSnapshot(`
    - region "Cart":
      - /children: deep-equal
      - heading "Cart" [level=2]
      - paragraph: "Items in cart: 1"
      - list:
        - listitem:
          - text: Espresso
          - button "Decrease quantity of Espresso": −
          - text: "1"
          - button "Increase quantity of Espresso": +
          - text: $3.50
          - button "Remove Espresso": Remove
      - paragraph: "Subtotal: $3.50"
      - paragraph: Delivery fee and taxes are added at checkout.
      - status: That item just sold out.
      - button "Clear"
      - paragraph:
        - link "Checkout $3.50":
          - /url: /cart
  `);
});

test("every control is reached from the keyboard, in the order it is read, and shows its focus", async ({
  page,
  browserName,
}) => {
  await ready(page);
  await press(page, "Add Espresso");
  // From the top of the page, as a person arriving with a keyboard does.
  await ready(page);
  // Safari moves focus to a button with Option-Tab: Tab alone reaches
  // fields and links, unless a person has set it to reach everything.
  const [forward, back] = browserName === "webkit" ? ["Alt+Tab", "Alt+Shift+Tab"] : ["Tab", "Shift+Tab"];
  const stops = await page.evaluate(tabStops);
  expect(stops).toEqual([
    // The layout the page is shown in, read first (ADR-0303): the way home,
    // and the cart, counted.
    "Stores",
    "Cart 1",
    // Where deliveries go (track `store-accounts`), above the menu.
    "Change the address",
    "Add Espresso",
    "Add Cortado",
    "Add Cold Brew",
    "Decrease quantity of Espresso",
    "Increase quantity of Espresso",
    "Remove Espresso",
    "Clear",
    // The cart's own page (ADR-0190), "Checkout" with its subtotal while
    // the cart holds a line.
    "Checkout $3.50",
  ]);
  const reached = [];
  for (let i = 0; i < stops.length; i += 1) {
    await page.keyboard.press(forward);
    reached.push(await focused(page));
  }
  expect(reached).toEqual(stops);
  const returned = [];
  for (let i = 1; i < stops.length; i += 1) {
    await page.keyboard.press(back);
    returned.push(await focused(page));
  }
  expect(returned).toEqual(stops.slice(0, -1).reverse());
});

test("each control is pressed with Enter and with Space", async ({ page }) => {
  await ready(page);
  await press(page, "Add Espresso", "Enter");
  await expect(page.locator("#cart-count")).toHaveText("1");
  await press(page, "Increase quantity of Espresso", "Space");
  await expect(page.locator("#cart-count")).toHaveText("2");
  await press(page, "Decrease quantity of Espresso", "Enter");
  await expect(page.locator("#cart-count")).toHaveText("1");
  // The last line taken away: focus goes to the cart's heading (ADR-0172).
  await press(page, "Remove Espresso", "Space");
  await expect(lines(page)).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Cart" })).toBeFocused();
  await press(page, "Add Cortado", "Space");
  await page.getByRole("button", { name: "Clear", exact: true }).press("Enter");
  await expect(page.locator("#cart-count")).toHaveText("0");
  await expect(lines(page)).toHaveCount(0);
});

test("the page's announcer is in it from the first byte, empty", async ({ request }) => {
  // Where a failed press is said (ADR-0302), served before the runtime, in
  // the page a build renders and in the page a host does.
  for (const path of ["/StorePage.html", "/stores/47"]) {
    const served = await (await request.get(path)).text();
    const found = served.match(/<div role="status" class="pw-announcer"[^>]*>(.*?)<\/div>/gs) ?? [];
    expect(found, path).toHaveLength(1);
    expect(found[0], path).toMatch(/><\/div>$/);
    expect(served.indexOf(found[0]), path).toBeLessThan(served.indexOf('id="pw-parts"'));
  }
});

test("a live region is the same node from the start, and says each change once", async ({
  page,
  request,
}) => {
  // The estimate comes after the page is ready: this session's estimator
  // takes its time.
  await ready(page);
  const slow = await page.request.post("/bench/estimate?delay=1500");
  expect(slow.ok()).toBe(true);
  await ready(page, "/stores/47", { waitUntil: "commit" });
  await expect(delivery(page)).toHaveText("Estimating delivery");
  const regions = await page.evaluate(() => {
    const live = '[aria-live]:not([aria-live="off"]), [role="status"], [role="alert"], [role="log"]';
    const found = [...document.querySelectorAll(live)];
    const said = found.map(() => []);
    found.forEach((region, i) =>
      new MutationObserver(() => said[i].push(region.textContent.replace(/\s+/g, " ").trim())).observe(
        region,
        { childList: true, subtree: true, characterData: true },
      ),
    );
    window.__live = { live, found, said };
    return found.map((r) => r.getAttribute("aria-label") || r.id || r.localName);
  });
  // The estimate, the cart's count, what the cart last had to say, and the
  // page's announcer, where a failed press is said (ADR-0302).
  expect(regions).toHaveLength(4);

  await slots(page);
  await press(page, "Add Espresso");
  await stock(request, "cold-brew", false);
  try {
    await press(page, "Add Cold Brew");
    await expect(page.locator("#cart-notice")).toHaveText("That item just sold out.");
  } finally {
    await stock(request, "cold-brew", true);
  }
  // A change to the menu is no live region's.
  try {
    await menu(request, "op=rename&id=espresso&name=Ristretto");
    await expect(page.getByRole("button", { name: "Add Ristretto", exact: true })).toBeVisible();
  } finally {
    await request.post("/command/menu?op=rename&id=espresso&name=Espresso");
  }
  await expect(page.getByRole("button", { name: "Add Espresso", exact: true })).toBeVisible();
  await restored(request);

  const after = await page.evaluate(() => {
    const { live, found, said } = window.__live;
    const now = [...document.querySelectorAll(live)];
    return {
      same: now.length === found.length && now.every((r, i) => r === found[i]),
      said,
    };
  });
  // Each region the node it was when the page was served: a screen reader
  // listens to that node, and one put in its place is a region it has not
  // heard of.
  expect(after.same).toBe(true);
  // And each change said once, as it was shown: an Add's speculation is
  // the count's change, and its answer is not another (ADR-0182). The
  // refused Add is shown, and taken back.
  expect(after.said).toEqual([
    ["Delivery in 25 to 35 min"],
    ["Items in cart: 1", "Items in cart: 2", "Items in cart: 1"],
    ["That item just sold out."],
    // The announcer: no press failed, nothing said. The refused Add is the
    // store's own notice to tell, as its program declares it (ADR-0157).
    [],
  ]);
});

test("a change writes only what it changes", async ({ page, request }) => {
  // An attribute set again, the same, is a change all the same: a control's
  // name set again is said again where it has focus (ADR-0182).
  await page.addInitScript(() => {
    const same = [];
    window.__same = same;
    const set = Element.prototype.setAttribute;
    Element.prototype.setAttribute = function (name, value) {
      if (this.isConnected && this.getAttribute(name) === String(value)) {
        same.push(`${name}="${value}" on <${this.localName}${this.id ? `#${this.id}` : ""}>`);
      }
      return set.call(this, name, value);
    };
  });
  await ready(page);
  await slots(page);
  await press(page, "Add Espresso");
  await press(page, "Increase quantity of Espresso");
  await press(page, "Decrease quantity of Espresso");
  await stock(request, "cold-brew", false);
  try {
    await press(page, "Add Cold Brew");
    await expect(page.locator("#cart-notice")).toHaveText("That item just sold out.");
  } finally {
    await stock(request, "cold-brew", true);
  }
  try {
    await menu(request, "op=rename&id=espresso&name=Ristretto");
    await expect(page.getByRole("button", { name: "Decrease quantity of Espresso", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Add Ristretto", exact: true })).toBeVisible();
  } finally {
    await request.post("/command/menu?op=rename&id=espresso&name=Espresso");
  }
  await expect(page.getByRole("button", { name: "Add Espresso", exact: true })).toBeVisible();
  await restored(request);
  await press(page, "Remove Espresso");
  await expect(lines(page)).toHaveCount(0);
  expect(await page.evaluate(() => window.__same)).toEqual([]);
});

test("at 320 CSS pixels the page reflows, and keeps every rule", async ({ page }) => {
  // WCAG 1.4.10: a page 1280 pixels wide zoomed to 400%.
  await page.setViewportSize({ width: 320, height: 640 });
  await ready(page);
  await slots(page);
  await press(page, "Add Espresso");
  const [scrolled, wide] = await page.evaluate(() => [
    document.documentElement.scrollWidth,
    document.documentElement.clientWidth,
  ]);
  expect(scrolled).toBeLessThanOrEqual(wide);
  expect(await audit(page)).toEqual([]);
});

test("on a phone the page is laid out at the phone's width", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.skip(browserName === "firefox", "Playwright's Firefox lays out no phone's viewport (isMobile)");
  const context = await browser.newContext({
    baseURL,
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 3,
    isMobile: true,
    hasTouch: true,
  });
  try {
    const page = await context.newPage();
    await ready(page);
    await slots(page);
    // Without the page's viewport a phone lays it out 980 pixels wide, and
    // shows it shrunk.
    expect(
      await page.evaluate(() => [
        window.innerWidth,
        document.documentElement.clientWidth,
        document.documentElement.scrollWidth,
      ]),
    ).toEqual([390, 390, 390]);
    await page.getByRole("button", { name: "Add Espresso", exact: true }).tap();
    await expect(page.locator("#cart-count")).toHaveText("1");
    expect(await audit(page)).toEqual([]);
  } finally {
    await context.close();
  }
});

test("when motion is reduced, nothing on the page moves", async ({ page, request }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(() => {
    const moved = [];
    window.__moved = moved;
    for (const type of ["animationstart", "transitionrun"]) {
      document.addEventListener(type, (e) => moved.push(`${type} on <${e.target.localName}>`), true);
    }
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (...args) {
      moved.push(`animate on <${this.localName}>`);
      return animate.apply(this, args);
    };
  });
  await ready(page);
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-motion: reduce)").matches)).toBe(true);
  await slots(page);
  await press(page, "Add Espresso");
  await press(page, "Remove Espresso");
  try {
    await menu(request, "op=rename&id=cortado&name=Cortado%20Doppio");
    await expect(page.getByRole("button", { name: "Add Cortado Doppio", exact: true })).toBeVisible();
  } finally {
    await request.post("/command/menu?op=rename&id=cortado&name=Cortado");
  }
  await expect(page.getByRole("button", { name: "Add Cortado", exact: true })).toBeVisible();
  expect(
    await page.evaluate(() => [
      ...window.__moved,
      ...document.getAnimations().map((a) => `running ${a.constructor.name}`),
    ]),
  ).toEqual([]);
});
