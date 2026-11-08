// E7-P — the protocol names no transport, and two adapters prove it.
//
// Architect ruling, 2026-08-07:
//
// > Multiplexed `StreamFrame` transport with long-poll as a fallback adapter.
//
// Two of them is the point. A protocol with exactly one transport is
// indistinguishable from a protocol that IS its transport: every claim about
// separation is unfalsifiable, because there is nothing to separate it from.
//
// So the same subscription runs twice — once over a held connection writing
// newline-delimited batches, once over a request-per-batch long poll — and the
// frames must be the same frames. `?transport=` pins the adapter so this is
// measured rather than left to whichever one the engine picked.
//
// # What "multiplexed" means here
//
// One subscription carries frames for MORE THAN ONE resource. The store page
// reads two: `store.page.Menu`, public and shared, and `store.page.Cart`,
// session-scoped and private. A transport that could only carry one would need
// a connection per resource, and the page would hold as many connections as it
// has data.

import { test, expect } from "@playwright/test";
import { MUTABLE_PORTS } from "../host-metadata.mjs";

// Mutates the menu, so it belongs on the isolated host — see
// `keyed-list.spec.mjs` for why that isolation is in the harness.
test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${MUTABLE_PORTS["transport"][testInfo.project.name]}`);
  },
});

// Serial. These tests share one host and one public menu, and a rename
// broadcast by one of them arrives in the middle of another's frame sequence —
// which showed up as the same frames in a different ORDER, a much harder
// symptom to read as interference than a wrong list would have been.
test.describe.configure({ mode: "serial" });

async function ready(page, transport) {
  await page.goto(`/StorePage.html?transport=${transport}`);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  await page.waitForFunction((t) => window.__pw.transport === t, transport);
}

/** Every frame this page applied, in order, as `kind@version` pairs. */
async function applied(page) {
  return page.evaluate(() =>
    window.__pw.log.filter((l) => /^(resource|updated|insert|remove|move|replace)/.test(l)),
  );
}

for (const transport of ["poll", "stream"]) {
  test(`${transport}: a change reaches the page`, async ({ page }) => {
    await ready(page, transport);
    await expect(page.locator("#cart-count")).toHaveText("0");
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");
  });

  test(`${transport}: several changes arrive in order`, async ({ page }) => {
    // Order matters and is the transport's job. Three clicks must land as
    // 1, 2, 3 — a transport that reordered would show a count that went
    // backwards, and the page's staleness guard would then refuse the real
    // value as old.
    await ready(page, transport);
    for (const n of ["1", "2", "3"]) {
      await page.locator("#menu button").first().click();
      await expect(page.locator("#cart-count")).toHaveText(n);
    }
  });
}

for (const transport of ["poll", "stream"]) {
  test(`${transport}: a page whose connection drops hears changes again`, async ({ page }) => {
    // Charter §15.5's forced reconnect. The page's first subscription
    // request fails, as a dropped connection does. Until 2026-10-03 that
    // ended its subscription for good, and it heard no change after.
    let dropped = 0;
    await page.route("**/stream?**", async (route) => {
      if (dropped === 0) {
        dropped += 1;
        return route.abort("connectionreset");
      }
      return route.continue();
    });
    await ready(page, transport);
    await expect.poll(() => dropped).toBe(1);
    // A change it hears only through its subscription: the menu, renamed
    // at the server.
    await page.request.post("/command/menu?op=rename&id=cortado&name=Gibraltar");
    await expect(page.locator("#menu")).toContainText("Gibraltar");
    await page.request.post("/command/menu?op=rename&id=cortado&name=Cortado");
    await expect(page.locator("#menu")).toContainText("Cortado");
    expect(await page.evaluate(() => window.__pw.reconnects)).toBeGreaterThan(0);
  });
}

for (const transport of ["poll", "stream"]) {
  test(`${transport}: a subscription asks at low priority, a command at its fetch's own`, async ({
    page,
    browserName,
  }, testInfo) => {
    // ADR-0399 (ADR-0392's finding 20): a browser shares six
    // connections to a host among all its pages over HTTP/1.1, and queues the
    // requests waiting for one by priority. A subscription holds one nearly
    // always; asked at low priority, it lets a press's command, at a fetch's
    // default, take the next one free, ahead of every other page's
    // subscription. The priority each request was asked at is Chromium's to
    // report (its DevTools protocol's `initialPriority`).
    test.skip(browserName !== "chromium", `${testInfo.project.name}: Chromium reports a request's priority`);
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Network.enable");
    const asked = [];
    cdp.on("Network.requestWillBeSent", (e) => {
      const path = new URL(e.request.url).pathname;
      if (path === "/stream" || path.startsWith("/command/")) asked.push([path, e.request.initialPriority]);
    });
    await ready(page, transport);
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).not.toHaveText("0");
    await expect.poll(() => asked.filter(([p]) => p === "/stream").length).toBeGreaterThan(0);
    const priorities = (path) => [...new Set(asked.filter(([p]) => p === path || (path === "/command/" && p.startsWith(path))).map(([, q]) => q))];
    expect(priorities("/stream"), "every subscription's request").toEqual(["Low"]);
    expect(priorities("/command/"), "the press's command").toEqual(["High"]);
  });
}

test("both adapters deliver the same frames", async ({ browser }) => {
  // The comparison the whole file exists for. Two pages, two adapters, one
  // sequence of commands, and the applied frames must match.
  const contexts = await Promise.all([browser.newContext(), browser.newContext()]);
  const pages = await Promise.all(contexts.map((c) => c.newPage()));
  await ready(pages[0], "poll");
  await ready(pages[1], "stream");

  for (const page of pages) {
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    // The count moves before the round trip (ADR-0122), so it no longer says
    // the server's patch has arrived; the runtime's log does.
    await expect
      .poll(() => page.evaluate(() => window.__pw.log.some((l) => l.startsWith("updated "))))
      .toBe(true);
  }

  // Normalized on the two things that are legitimately per-session: the cart's
  // `ResourceEntryId` and its version. Two sessions have two carts, and a
  // comparison that demanded those match would be asserting the opposite of
  // what `public-fragment.spec.mjs` proves. What must match is the SEQUENCE:
  // which frame kinds, at which addresses, in which order.
  const shape = (lines) =>
    lines.map((l) => l.replace(/\b[0-9a-f]{8,}\b/g, "<entry>").replace(/\d+/g, "N"));
  const [byPoll, byStream] = await Promise.all(pages.map(applied));
  expect(byPoll.length, "frames were applied").toBeGreaterThan(0);
  expect(
    shape(byStream),
    "the same frames, whichever connection carried them",
  ).toEqual(shape(byPoll));

  await Promise.all(contexts.map((c) => c.close()));
});

test("one subscription carries two different resources", async ({ page }) => {
  // Multiplexing, stated as what it buys: the page holds ONE subscription and
  // receives frames for a public entry and a private one. The two are
  // distinguishable because their `ResourceEntryId`s differ — the cart's is
  // per-session and the menu's is not.
  await ready(page, "stream");

  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.request.post("/command/menu?op=rename&id=cortado&name=Gibraltar");
  await expect(page.locator("#menu li").nth(1).locator("span")).toHaveText("Gibraltar");

  // Both entries, once both frames have come. The count shows the press's
  // speculation before the cart's frame does (ADR-0122), so that frame can
  // come after the menu's: read once, the entries were one in a full run
  // under load (found 2026-10-03).
  await expect
    .poll(
      async () => {
        const held = await page.evaluate(() => window.__pwHeld());
        const known = await page.evaluate(() => window.__pwKnown());
        return new Set([...Object.keys(held), ...Object.keys(known)]).size;
      },
      { message: "two resources, one subscription" },
    )
    .toBeGreaterThanOrEqual(2);

  await page.request.post("/command/menu?op=rename&id=cortado&name=Cortado");
});

test("the streaming adapter really holds one connection open", async ({ page }) => {
  // Otherwise "streaming" is a long poll with a different name, and the two
  // adapters are one adapter tested twice.
  const opened = [];
  await page.route("**/stream*", async (route) => {
    opened.push(route.request().url());
    await route.continue();
  });

  await ready(page, "stream");
  for (const n of ["1", "2", "3"]) {
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText(n);
  }

  const streaming = opened.filter((u) => u.includes("mode=stream"));
  expect(streaming.length, "the streaming adapter was used").toBeGreaterThan(0);
  expect(
    streaming.length,
    "three changes did not need three connections",
  ).toBeLessThan(3);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("the long poll opens a connection per batch", async ({ page }) => {
  // The control for the test above. If BOTH adapters held one connection, the
  // count above would prove nothing about streaming — it would be a property
  // of the server, not of the adapter under test.
  const opened = [];
  await page.route("**/stream*", async (route) => {
    opened.push(route.request().url());
    await route.continue();
  });

  await ready(page, "poll");
  for (const n of ["1", "2", "3"]) {
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText(n);
    // Each change's patch to the count, not only its speculation (ADR-0122):
    // the count moves before the round trip. The count's alone: a change
    // sets the subtotal too (ADR-0172).
    await expect
      .poll(() =>
        page.evaluate(() => {
          const { parts, schema } = window.__pw.parts;
          const count = parts.find((p) => p.value === "cart.line_count").id;
          return window.__pw.log.filter((l) => l.startsWith(`updated ${schema}/|${count} at`))
            .length;
        }),
      )
      .toBe(Number(n));
  }

  expect(opened.every((u) => !u.includes("mode=stream"))).toBe(true);
  expect(opened.length, "one request per batch, and then some").toBeGreaterThanOrEqual(3);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("the frames carry no trace of which adapter delivered them", async ({ page }) => {
  // The structural claim. A frame is a `StreamFrame`: a kind, a protocol
  // version, a causal basis, an address, an operation. If a transport could
  // add a field the browser reads, the separation would be nominal.
  const bodies = [];
  await page.route("**/stream*", async (route) => {
    const response = await route.fetch();
    bodies.push(await response.text());
    await route.fulfill({ response });
  });

  await ready(page, "poll");
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  // The count moves before the round trip (ADR-0122), so it no longer says
  // the server's patch has arrived; the runtime's log does.
  await expect
    .poll(() => page.evaluate(() => window.__pw.log.some((l) => l.startsWith("updated "))))
    .toBe(true);

  // `value` since ADR-0122: an `entry_value` frame's, a protocol field. And
  // `patches` since ADR-0145: a `patch_set`'s, each a target and an operation.
  // And `applied` since ADR-0172: the interactions an entry's value includes.
  const FRAME_FIELDS = new Set(["frame", "protocol", "basis", "target", "operation", "entry", "version", "reason", "value", "patches", "applied"]);
  let checked = 0;
  for (const body of bodies) {
    for (const line of body.split("\n").filter(Boolean)) {
      const batch = JSON.parse(line);
      // `cursor` is the batch's, not a frame's: transport bookkeeping stays
      // outside the frames it carries.
      expect(Object.keys(batch).sort()).toEqual(["cursor", "frames"]);
      for (const frame of batch.frames) {
        for (const field of Object.keys(frame)) {
          expect(FRAME_FIELDS.has(field), `unexpected frame field ${field}`).toBe(true);
        }
        checked++;
      }
    }
  }
  expect(checked, "frames were inspected").toBeGreaterThan(0);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});
