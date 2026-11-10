// E7-R — the store page, through the own renderer.
//
// The architect's vertical slice, run:
//
//   store page → own server renderer → HTML with only the required anchors
//   → parts manifest → decide() → Authorised → handler attaches → click Add
//   → command → only cart-related PartIds update
//
// Every assertion reads the LIVE DOCUMENT, not the markers the renderer
// emitted. E7-2's finding is the reason: source quotes survived into an
// `aria-label` and the bytes looked like escaping doing its job. Asking the
// browser what actually resulted is the only thing that found it.

import { test, expect } from "@playwright/test";

// Each test gets its own browser context, so each gets its own session cookie
// and its own cart. The server keys carts by session — which is what
// `examples/store/app.pw` declares — so six parallel workers do not observe
// each other's clicks.

async function ready(page) {
  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  return page.evaluate(() => window.__pw);
}

test.describe("the document the server produced", () => {
  test("no Marko code participates in this route", async ({ page }) => {
    const requested = [];
    page.on("request", (r) => requested.push(r.url()));
    await ready(page);

    const marko = requested.filter((u) => /marko/i.test(u));
    expect(marko, "the pw route must not fetch Marko").toEqual([]);

    // And nothing in the document mentions it either — a bundle can be inlined.
    const html = await page.content();
    expect(html).not.toMatch(/marko/i);
  });

  test("only dynamic regions carry identity markup", async ({ page }) => {
    await ready(page);
    const shape = await page.evaluate(() => {
      const anchors = [];
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);
      let n;
      while ((n = walker.nextNode())) {
        if (n.data.startsWith("pw:")) anchors.push(n.data);
      }
      return {
        anchors: anchors.length,
        anchoredElements: document.querySelectorAll("[data-pw]").length,
        // The store name, the menu items and the cart count are dynamic; the
        // headings and the section labels are not.
        totalElements: document.querySelectorAll("*").length,
      };
    });
    // store name 2, the loop's own range 2, three loop INSTANCE boundaries 6,
    // the item name inside each instance 6, the cart count 2, the cart's
    // notice 2 (ADR-0157) = 20. And the store's slots, filled (ADR-0165): the
    // estimate's range 2, and its least and most minutes 2 each (ADR-0180);
    // the recommendations' range 2, their loop's 2, two instances 4 and two
    // names 4 = 18. And what the store
    // and each item say of themselves (ADR-0166): 2, and 2 in each of three
    // instances = 8. And each item's price (ADR-0169): 2 in each of three
    // instances = 6. And the cart's own (ADR-0172): its lines' loop 2, its
    // subtotal 2, its fees note's block 2 = 6. And whether each item can be
    // ordered (ADR-0178): its block, 2 in each of three instances = 6. And
    // the menu grouped by category (ADR-0181): the coffee category's
    // instance 2, its heading 2, and its items' own loop 2 = 6. And the menu
    // counted, which the host keeps (ADR-0277): its line 2. And the count of
    // (ADR-0307's Clear block is not this store's: a cart is anyone's to
    // change, so its controls ask nothing; track `store-accounts`.)
    // the cart in the layout the page is shown in (ADR-0303): 2. And where
    // deliveries go (track `store-accounts`): whether an address is chosen,
    // its block 2, and whether the store reaches it, its block 2 = 4. And
    // the cart's link, "Checkout" with its subtotal while the cart holds a
    // line (ADR-0323): its block 2. And the store's cuisine (ADR-0308,
    // amended): 2. And whether it is open (track store-hours): 2. And its
    // ratings (ADR-0341): what they come to 2, and whether it has reviews,
    // its block 2 = 4. 88 in all.
    expect(shape.anchors).toBe(88);
    // Three Add buttons and one Clear button. The Clear button exists so that
    // E7-L has two handlers to tell apart — see `lazy-handler.spec.mjs`. And
    // the cart's empty message, whose `hidden` reads the cart (ADR-0172).
    // And the link to the address page, whose `href` reads the store's id
    // (track `store-accounts`).
    expect(
      shape.anchoredElements,
      "the Add buttons, Clear, the empty message and the address link",
    ).toBe(6);
    expect(shape.totalElements).toBeGreaterThan(20);
  });

  test("the page is readable with JavaScript disabled", async ({ browser }) => {
    // The document is complete before any script runs. That is what makes the
    // rest of this file about ATTACHING rather than about rendering.
    const context = await browser.newContext({ javaScriptEnabled: false });
    const page = await context.newPage();
    await page.goto("/StorePage.html");
    await expect(page.locator("#store-name")).toHaveText("Blue Bottle");
    await expect(page.locator("#menu li")).toHaveCount(3);
    // Each with its price (ADR-0169).
    await expect(page.locator("#menu li")).toContainText(["$3.50", "$4.25", "$4.75"]);
    await expect(page.locator("#cart-count")).toHaveText("0");
    await context.close();
  });
});

test.describe("decide() governs attachment", () => {
  test("a compatible manifest is authorised and the handler attaches", async ({ page }) => {
    const pw = await ready(page);
    expect(pw.log.join("\n")).toMatch(/attached \d+ to 3 instance\(s\)/);
  });

  test("clicking Add changes the cart", async ({ page }) => {
    await ready(page);
    await expect(page.locator("#cart-count")).toHaveText("0");
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");
  });

  test("an incompatible manifest is refused and nothing attaches", async ({ page }) => {
    // The refusal path, exercised by changing what the SERVER presents — the
    // runtime is not edited and has no test-only branch. Without this, "the
    // handler attached" is consistent with a runtime that attaches whatever it
    // is given.
    //
    // The document is answered with hash scheme 1, superseded, and refused as
    // INCOMPARABLE rather than as different (E7V). Until 2026-10-11 the page
    // was first loaded, its manifest edited and the page reloaded only to the
    // reload's commit: an edit the reload discards, and a navigation still
    // loading as the next began, whose load WebKit once never reached
    // (master's verify 38036676661).
    await page.route("**/StorePage.html", async (route) => {
      const response = await route.fetch();
      const body = (await response.text()).replace('"scheme":"2"', '"scheme":"1"');
      await route.fulfill({ response, body });
    });
    await page.goto("/StorePage.html");
    await page.waitForFunction(() => document.documentElement.dataset.pwReady);

    const pw = await page.evaluate(() => window.__pw);
    expect(pw.log.join("\n"), "the refusal is recorded, not silent").toMatch(/refused/);
    expect(pw.log.join("\n")).not.toMatch(/attached/);

    // And the button is inert rather than wrong.
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("0");
  });
});

test.describe("the update touches only what changed", () => {
  test("only cart-related part ids update", async ({ page }) => {
    await ready(page);
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    // The count moves before the round trip (ADR-0122), so it no longer says
    // the server's patch has arrived; the runtime's record of it does.
    await expect
      .poll(() => page.evaluate(() => window.__pw.updated.length))
      .toBeGreaterThan(0);

    const pw = await page.evaluate(() => window.__pw);
    const part = (kind, value) =>
      pw.parts.parts.find((p) => p.kind === kind && p.value === value).id;
    // ADDRESSES, not ids: each cart part is outside every loop, so its
    // instance path is empty and it has exactly one live instance. The address
    // is `PartAddress::key` — schema, path, part — because the browser index
    // and `pw_document` must spell it the same way or a patch finds nothing.
    const at = (id) => `${pw.parts.schema}/|${id}`;
    // Each block its lines decide: the fees, and the link to the cart's page,
    // which says "Checkout" with the subtotal once the cart holds a line
    // (ADR-0323). Each shows another arm, so each is rendered again whole.
    const blocks = pw.parts.parts
      .filter((p) => p.kind === "conditional" && p.value === "cart.lines")
      .map((p) => `${at(p.id)}:replace_range`);
    expect(blocks).toHaveLength(2);
    // The cart's parts, each what reads the cart (ADR-0172): its count, its
    // subtotal, its first line, its empty message hidden, its blocks; and
    // the count the page's layout shows of it (ADR-0303).
    expect([...pw.updated].sort(), "exactly the cart's parts").toEqual(
      [
        at(part("text", "cart~StoreLayout.line_count")),
        at(part("text", "cart.line_count")),
        at(part("text", "cart.subtotal.display")),
        `${at(part("each", "cart.lines"))}:insert_before`,
        `${at(part("boolean_attribute", "cart.lines"))}:hidden`,
        ...blocks,
      ].sort(),
    );
  });

  test("the menu's nodes keep their identity across the update", async ({ page }) => {
    // The property RQ-1 measured and Marko passed. Marked BEFORE the
    // interaction, because a node counted after it proves nothing about which
    // node it is.
    await ready(page);
    await page.evaluate(() => {
      document.querySelectorAll("#menu li").forEach((li, i) => {
        li.__pwIdentity = `item-${i}`;
      });
      document.querySelector("#store-name").__pwIdentity = "name";
    });

    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");

    const survived = await page.evaluate(() => ({
      items: [...document.querySelectorAll("#menu li")].map((li) => li.__pwIdentity),
      name: document.querySelector("#store-name").__pwIdentity,
    }));
    expect(survived.items, "no menu node was replaced").toEqual([
      "item-0",
      "item-1",
      "item-2",
    ]);
    expect(survived.name, "the store name was not re-rendered").toBe("name");
  });

  test("the cart's own element survives its content changing", async ({ page }) => {
    // The update replaces what is BETWEEN the anchors. The element holding them
    // is not touched, which is what lets the part be updated again.
    await ready(page);
    await page.evaluate(() => {
      document.querySelector("#cart-count").__pwIdentity = "cart";
    });
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("2");

    expect(
      await page.evaluate(() => document.querySelector("#cart-count").__pwIdentity),
    ).toBe("cart");
  });

  test("focus survives the update", async ({ page }) => {
    await ready(page);
    const button = page.locator("#menu button").first();
    await button.focus();
    const before = await page.evaluate(() => document.activeElement?.dataset.pw ?? "");
    expect(before).not.toBe("");

    await page.keyboard.press("Enter");
    await expect(page.locator("#cart-count")).toHaveText("1");
    const after = await page.evaluate(() => document.activeElement?.dataset.pw ?? "");
    expect(after, "the focused control is still focused").toBe(before);
  });
});

test.describe("the identity checks can fail", () => {
  test("replacing a menu node makes the identity assertion red", async ({ page }) => {
    // Architect ruling, standing: every favourable observation needs a mutation
    // that makes the exact measurement go red. Without this, "no menu node was
    // replaced" holds for a page whose menu is empty.
    await ready(page);
    await page.evaluate(() => {
      document.querySelectorAll("#menu li").forEach((li, i) => {
        li.__pwIdentity = `item-${i}`;
      });
    });

    // The mutation: one item is replaced with an equivalent node, which is
    // exactly what a re-rendering runtime would have done.
    await page.evaluate(() => {
      const li = document.querySelector("#menu li");
      const fresh = li.cloneNode(true);
      li.replaceWith(fresh);
    });

    const identities = await page.evaluate(() =>
      [...document.querySelectorAll("#menu li")].map((li) => li.__pwIdentity),
    );
    expect(
      identities,
      "the assertion must distinguish a replaced node from a preserved one",
    ).not.toEqual(["item-0", "item-1", "item-2"]);
  });
});

test.describe("what activation costs, now that E7-L has closed", () => {
  test("the runtime and the decision load eagerly; handlers do not", async ({ page }) => {
    // This test used to record E7-L as an open gap: "handler code is still
    // fetched before interaction". It is kept, inverted, because the
    // distinction it was drawing is the one worth holding on to.
    //
    // Two things load before any interaction and both must: the runtime, which
    // finds and addresses the parts, and the decision, which authorises
    // handlers. Neither is behaviour. Behaviour is what E7-L withholds — see
    // `lazy-handler.spec.mjs` for the twelve controls.
    const requested = [];
    page.on("request", (r) => requested.push(r.url()));
    await ready(page);
    await page.waitForTimeout(300);

    const eager = requested.filter((u) => u.endsWith(".mjs") || u.endsWith(".wasm"));
    expect(eager.some((u) => u.includes("pw-runtime")), "the runtime loads").toBe(true);
    expect(eager.filter((u) => u.endsWith(".wasm")), "no WebAssembly does").toEqual([]);
    expect(
      eager.filter((u) => u.includes("/handler/")),
      "and no handler does",
    ).toEqual([]);
  });
});

test.describe("instance identity — the three Add buttons", () => {
  test("each loop instance has a distinct address", async ({ page }) => {
    // RISK_QUEUE 25. All three buttons carry `data-pw="0"` because a
    // template-scoped id names a position in the TEMPLATE. What distinguishes
    // them is the instance path, and a runtime that keyed by id alone kept the
    // last one — so the third button worked and the first two were inert.
    const pw = await ready(page);
    const attached = pw.log.find((l) => l.startsWith("attached"));
    const addresses = attached.split(": ")[1].split(" ");
    expect(addresses).toHaveLength(3);
    expect(new Set(addresses).size, "three DISTINCT addresses").toBe(3);
  });

  test("every Add button works, not only the last one", async ({ page }) => {
    // The regression this exists for, stated as behaviour: each button in turn.
    await ready(page);
    const buttons = page.locator("#menu button");
    for (let i = 0; i < 3; i += 1) {
      await buttons.nth(i).click();
      await expect(page.locator("#cart-count")).toHaveText(String(i + 1));
    }
  });

  test("the instance token does not reveal the item key", async ({ page }) => {
    // Architect ruling: "Never put raw domain keys into identity markup merely
    // for renderer convenience." A comment is no more private than an
    // attribute — both are read by anything that can read the document.
    //
    // `data-pw-captures` is excluded by name, and only it. It is not identity
    // markup: it carries what the page's author declared the handler captures,
    // narrowed to the paths the compiled handler reads (E10, 2026-09-25), the
    // "only the metadata needed to resume interactions" of charter §8.5, whose
    // list of what a handler may capture includes resource keys. Every other
    // `data-pw*` attribute is still held to the ruling.
    await ready(page);
    const html = await page.content();
    const keys = ["espresso", "cortado", "cold-brew"];
    for (const key of keys) {
      expect(
        html,
        `\`${key}\` must not appear in identity markup`,
      ).not.toMatch(new RegExp(`pw:[se]\\d+@[^-]*${key}`));
      expect(html).not.toMatch(new RegExp(`data-pw(?!-captures=)[^=]*="[^"]*${key}`));
    }
    // The control: the tokens ARE there, so the assertion is about their
    // content rather than about their absence. The menu's, by where they are:
    // a part's number moves when the page above it changes (ADR-0166).
    expect(await page.locator("#menu").innerHTML()).toMatch(/<!--pw:s\d+@[A-Za-z0-9_-]{16}-->/);
  });

  test("the same document renders the same tokens twice", async ({ page, request }) => {
    // Determinism, at the identity layer. Two fetches of one document must
    // agree, or an address means nothing across a reload.
    const a = await (await request.get("/StorePage.html")).text();
    const b = await (await request.get("/StorePage.html")).text();
    // The menu's instances: a stream's region is its document's (ADR-0165),
    // and a part's number moves when the page above it changes (ADR-0166).
    const tokens = (html) => {
      const at = html.indexOf('<div id="menu">');
      const menu = html.slice(at, html.indexOf("</div>", at));
      return [...menu.matchAll(/pw:s\d+@([A-Za-z0-9_-]{16})/g)].map((m) => m[1]);
    };
    expect(tokens(a)).toEqual(tokens(b));
    // The coffee category's, and its three items' (ADR-0181).
    expect(tokens(a)).toHaveLength(4);
    void page;
  });
});
