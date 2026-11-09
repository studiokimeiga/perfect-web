// E7-P — a keyed collection changes without the browser re-rendering it.
//
// Architect ruling, 2026-08-07:
//
// > Using the existing `IdentityDomain + InstancePath + LocalPartId`, support
// > `InsertBefore`, `InsertAfter`, `RemoveInstance`, `MoveInstance` […] DOM
// > identity should survive moves. A `MoveInstance` that deletes and recreates
// > the node is not equivalent.
//
// That last sentence is the whole milestone. Every operation here has a
// cheating implementation that produces an identical screenshot — re-render
// the list from the new order and the pixels agree, while focus, scroll
// position, form state, animation and every attached handler are destroyed.
// So no test below asserts on text alone. Each one marks the live nodes first
// and checks the marks survived.
//
// # How a node is marked
//
// `li.__pwIdentity = <unique>` — an expando property. It cannot survive
// serialization, cloning, or re-parsing, so a runtime that rebuilt the list
// from HTML loses it however faithful the HTML is. An attribute would not
// work: the server's markup could carry one.

import { test, expect } from "@playwright/test";

import { MUTABLE_PORTS } from "../host-metadata.mjs";

// Its own host, per engine. These tests mutate the shared public menu, and
// every other spec reasonably assumes the menu it was served — as does every
// other ENGINE running this same file at the same time. Isolation here rather
// than a per-session menu, because a per-session menu would delete the
// broadcast property the last test in this file proves.
test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${MUTABLE_PORTS["keyed-list"][testInfo.project.name]}`);
  },
});

const ITEMS = ["espresso", "cortado", "cold-brew"];

async function ready(page) {
  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  // Every test mutates one shared server-side list, so each starts by putting
  // it back. Done through the same commands under test — a reset endpoint
  // would be a second write path, and a second write path is a place for the
  // patches and the state to disagree.
  await restore(page);
  await mark(page);
}

/** Bring the server's list back to `ITEMS`, in order. */
async function restore(page) {
  const log = [];
  const run = async (q) => {
    const r = await command(page, q);
    log.push(`${q} → ${r.status}${r.body.refused ? ` ${r.body.refused}` : ""}`);
    return r;
  };
  const now = await ids(page);
  for (const extra of now.filter((i) => !ITEMS.includes(i))) {
    await run(`op=remove&id=${extra}`);
  }
  for (const missing of ITEMS.filter((i) => !now.includes(i))) {
    // Appended with no anchor. Anchoring to `ITEMS[0]` was wrong for exactly
    // the case that matters — restoring `ITEMS[0]` itself — and anchoring to
    // "whatever is first" cannot restore an EMPTY list at all. Both were found
    // by this fixture damaging its own server.
    await run(`op=insert_after&id=${missing}&name=${missing}`);
  }
  // Restore the order, then the names.
  for (let i = 1; i < ITEMS.length; i++) {
    await run(`op=move&id=${ITEMS[i]}&after=${ITEMS[i - 1]}`);
  }
  await run(`op=move&id=${ITEMS[0]}`);
  const names = { espresso: "Espresso", cortado: "Cortado", "cold-brew": "Cold Brew" };
  for (const [id, name] of Object.entries(names)) {
    await run(`op=rename&id=${id}&name=${name.replace(" ", "%20")}`);
  }
  // The fixture verifies itself. A restore that half-worked would surface as
  // a confusing failure in whatever test ran next — and one already did.
  const after = await ids(page);
  if (JSON.stringify(after) !== JSON.stringify(ITEMS)) {
    throw new Error(`restore left the list as ${JSON.stringify(after)} (log: ${log.join("; ")})`);
  }
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
}

async function command(page, query) {
  const r = await page.request.post(`/command/menu?${query}`);
  return { status: r.status(), body: await r.json() };
}

async function ids(page) {
  const r = await page.request.get("/menu");
  return (await r.json()).items;
}

/** Stamp every `<li>` with an identity no re-render can reproduce. */
async function mark(page) {
  await page.evaluate(() => {
    [...document.querySelectorAll("#menu li")].forEach((li, i) => {
      li.__pwIdentity = `mark-${i}`;
      li.dataset.mark = `mark-${i}`;
    });
  });
}

/**
 * Wait for the order a patch is expected to produce, then read the marks.
 *
 * A patch arrives over the transport, so the DOM after a command returns is
 * not yet the DOM the command described. Reading it immediately made the first
 * draft of this file fail for a reason that had nothing to do with what it
 * tests — and would equally have made it PASS by reading a stale DOM that
 * happened to match.
 *
 * The order is what is waited for; the identities are then asserted on the
 * settled document, and are never waited for. A retried identity assertion
 * would hide the failure it exists to catch, because a rebuilt list eventually
 * looks correct.
 */
async function settled(page, names) {
  try {
  await expect
    // Generous, because the suite runs three engine families in parallel and
    // a patch that takes four seconds to arrive under that load is slow rather
    // than absent. The assertion still fails if the change never comes.
    .poll(async () => (await shown(page)).map((r) => r.name), { timeout: 15000 })
    .toEqual(names);
  } catch (e) {
    console.log("DIAG:", await page.evaluate(() => window.__pw.log.join(" | ")));
    throw e;
  }
  return shown(page);
}

/** The visible order, and the surviving marks alongside it. */
async function shown(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll("#menu li")].map((li) => ({
      name: li.querySelector("span")?.textContent,
      identity: li.__pwIdentity ?? null,
    })),
  );
}

test.describe.configure({ mode: "serial" });

test("insert before an instance puts it before, and disturbs nothing else", async ({ page }) => {
  await ready(page);
  const { status } = await command(page, "op=insert_before&id=mocha&name=Mocha&at=cortado");
  expect(status).toBe(202);
  const rows = await settled(page, ["Espresso", "Mocha", "Cortado", "Cold Brew"]);
  expect(
    rows.map((r) => r.identity),
    "the three existing nodes are the same nodes",
  ).toEqual(["mark-0", null, "mark-1", "mark-2"]);
});

test("insert before the first instance puts it at the head, and disturbs nothing else", async ({
  page,
}) => {
  // ADR-0181: the server derives the change from the menu's values, and an
  // item new at the head goes before the first instance, by its address.
  await ready(page);
  const { status } = await command(page, "op=insert_before&id=mocha&name=Mocha&at=espresso");
  expect(status).toBe(202);
  const rows = await settled(page, ["Mocha", "Espresso", "Cortado", "Cold Brew"]);
  expect(rows.map((r) => r.identity)).toEqual([null, "mark-0", "mark-1", "mark-2"]);
});

test("insert after an instance puts it after", async ({ page }) => {
  // The control for the test above: if the runtime ignored `before`/`after`
  // and always did one of them, exactly one of these two would pass.
  await ready(page);
  await command(page, "op=insert_after&id=mocha&name=Mocha&at=cortado");

  const rows = await settled(page, ["Espresso", "Cortado", "Mocha", "Cold Brew"]);
  expect(rows.map((r) => r.identity)).toEqual(["mark-0", "mark-1", null, "mark-2"]);
});

for (const [position, id, expected] of [
  ["first", "espresso", ["Cortado", "Cold Brew"]],
  ["middle", "cortado", ["Espresso", "Cold Brew"]],
  ["last", "cold-brew", ["Espresso", "Cortado"]],
]) {
  test(`removing the ${position} instance removes that one`, async ({ page }) => {
    // Three positions because an off-by-one in the anchor arithmetic removes
    // the wrong neighbour, and with a single position under test the wrong
    // neighbour is often the right one by luck.
    await ready(page);
    await command(page, `op=remove&id=${id}`);

    const rows = await settled(page, expected);
    expect(
      rows.every((r) => r.identity !== null),
      "the survivors were not recreated",
    ).toBe(true);
  });
}

test("a reorder moves the nodes rather than rebuilding them", async ({ page }) => {
  // The claim the milestone is for. Note what is NOT asserted: the text order
  // alone. A rebuild satisfies that.
  await ready(page);
  await command(page, "op=move&id=cold-brew&after=espresso");

  const rows = await settled(page, ["Espresso", "Cold Brew", "Cortado"]);
  expect(
    rows.map((r) => r.identity),
    "every node kept the identity it had before the move",
  ).toEqual(["mark-0", "mark-2", "mark-1"]);
});

test("a move to the front is a move, not a recreation", async ({ page }) => {
  await ready(page);
  await command(page, "op=move&id=cold-brew");

  const rows = await settled(page, ["Cold Brew", "Espresso", "Cortado"]);
  expect(rows.map((r) => r.identity)).toEqual(["mark-2", "mark-0", "mark-1"]);
});

test("a move to where an instance already is moves nothing", async ({ page }) => {
  // ADR-0168's guard: an instance already at its anchor is left where it is.
  // Moved anyway, `moveBefore` put its nodes before its own start, and the
  // list's anchors no longer nested. No server of this repository sends such
  // a move since it derives a list's patches as their difference
  // (2026-10-04), so the press that once reached the guard sends none, and
  // nothing tested it (found by W5, 2026-10-09). The protocol allows the move,
  // so the runtime is given one: the real move of Cold Brew to the front,
  // delivered again once it is there, at the next version.
  let replay = null;
  let move = null;
  let armed = false;
  // A batch is added to as it is answered, so each must be answered briefly:
  // this page does not ask to be held long (the longer-holds ADR).
  await page.addInitScript(() => Object.defineProperty(navigator, "locks", { value: undefined }));
  await page.route("**/stream*", async (route) => {
    const response = await route.fetch();
    let body = await response.text();
    try {
      const batch = JSON.parse(body);
      for (const f of batch.frames ?? []) {
        if (armed && !move && JSON.stringify(f).includes('"op":"move_instance"')) move = f;
      }
      if (replay) {
        batch.frames = [...(batch.frames ?? []), replay];
        replay = null;
        body = JSON.stringify(batch);
      }
    } catch {
      /* an empty poll */
    }
    await route.fulfill({ response, body });
  });
  // The long poll, whose batches a test can read and add to; a held stream
  // has no body to read until it closes (protocol-boundary.spec.mjs).
  await page.goto("/StorePage.html?transport=poll");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  await restore(page);
  await mark(page);
  // Only this test's own move: putting the list back may move items too, and
  // a frame of the document `restore` reloaded is older than this one's
  // (on CI, the restore's moves at version 252, replayed into a page at 267,
  // were ignored as stale).
  armed = true;
  await command(page, "op=move&id=cold-brew");
  await settled(page, ["Cold Brew", "Espresso", "Cortado"]);
  await expect.poll(() => move !== null, { timeout: 15000 }).toBe(true);

  // The same move, one version on, every basis it carries advanced.
  const again = JSON.parse(JSON.stringify(move));
  const advance = (basis) => basis?.resources?.forEach((r) => (r.version += 1));
  advance(again.basis);
  (again.patches ?? []).forEach((p) => advance(p.basis));
  const before = await page.evaluate(() => {
    window.__pw.moveKind = null;
    return {
      log: window.__pw.log.length,
      ignored: window.__pw.ignored ?? 0,
      refused: window.__pw.refused ?? 0,
    };
  });
  replay = again;
  const version = again.basis.resources[0].version;
  await expect
    .poll(
      () =>
        page.evaluate(
          (v) => window.__pw.log.some((l) => l.startsWith("move_instance") && l.endsWith(` at version ${v}`)),
          version,
        ),
      { timeout: 15000 },
    )
    .toBe(true);

  const after = await page.evaluate(() => ({
    moved: window.__pw.moveKind,
    ignored: window.__pw.ignored ?? 0,
    refused: window.__pw.refused ?? 0,
  }));
  expect(after, "applied, and nothing moved").toEqual({
    moved: null,
    ignored: before.ignored,
    refused: before.refused,
  });
  const rows = await shown(page);
  expect(rows.map((r) => r.name)).toEqual(["Cold Brew", "Espresso", "Cortado"]);
  expect(rows.map((r) => r.identity)).toEqual(["mark-2", "mark-0", "mark-1"]);
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("a moved instance keeps focus", async ({ page }) => {
  // The consequence a user would feel, asserted directly. Focus lives on the
  // NODE; a recreated button is a different node and focus lands on the body.
  await ready(page);
  await page.locator("#menu li").first().locator("button").focus();
  const before = await page.evaluate(() => document.activeElement?.closest("li")?.dataset.mark);
  expect(before).toBe("mark-0");

  await command(page, "op=move&id=espresso&after=cold-brew");
  await expect(page.locator("#menu li").last()).toHaveAttribute("data-mark", "mark-0");

  const after = await page.evaluate(() => ({
    mark: document.activeElement?.closest("li")?.dataset.mark ?? null,
    tag: document.activeElement?.tagName ?? null,
  }));
  expect(after, "focus travelled with the node").toEqual({ mark: "mark-0", tag: "BUTTON" });
});

test("changing one item's field replaces no sibling", async ({ page }) => {
  // An ordinary field change inside a keyed instance. The failure this catches
  // is a runtime that treats any change to a collection as a collection
  // change and re-renders the lot.
  await ready(page);
  await command(page, "op=rename&id=cortado&name=Gibraltar");

  const rows = await settled(page, ["Espresso", "Gibraltar", "Cold Brew"]);
  expect(
    rows.map((r) => r.identity),
    "including the renamed item's own node",
  ).toEqual(["mark-0", "mark-1", "mark-2"]);
});

test("a duplicate key is refused, and nothing changes", async ({ page }) => {
  // Two instances with one key is one address for two places — `RISK_QUEUE` 25
  // arriving through the front door. The refusal must also leave no version
  // behind, or the page would be told to catch up to a change that never was.
  await ready(page);
  const before = await page.evaluate(() => window.__pw.log.length);

  const { status, body } = await command(page, "op=insert_after&id=cortado&name=Twin&at=espresso");
  expect(status).toBe(409);
  expect(body.refused).toMatch(/duplicate key/);

  await page.waitForTimeout(300);
  await expect(page.locator("#menu li")).toHaveCount(3);
  const rows = await shown(page);
  expect(rows.map((r) => r.identity)).toEqual(["mark-0", "mark-1", "mark-2"]);
  expect(await page.evaluate(() => window.__pw.log.length), "no frame was sent").toBe(before);
});

test("an item with no key is refused", async ({ page }) => {
  // An unaddressable instance. Rendering it would produce a row no patch can
  // ever reach again — visible, and permanently frozen.
  await ready(page);
  const { status, body } = await command(page, "op=insert_after&id=&name=Ghost&at=espresso");
  expect(status).toBe(409);
  expect(body.refused).toMatch(/no key/);
  await expect(page.locator("#menu li")).toHaveCount(3);
});

test("a patch aimed at an instance that does not exist is refused", async ({ page }) => {
  // The page and the server disagreeing about what the document contains. The
  // browser must say so rather than treat "addressed nothing" as "nothing to
  // do" — the two are indistinguishable from the outside and one of them is a
  // bug.
  await ready(page);
  await command(page, "op=insert_after&id=mocha&name=Mocha&at=espresso");
  await expect(page.locator("#menu li")).toHaveCount(4);

  const before = await page.evaluate(() => window.__pw.refused ?? 0);
  await page.request.post("/command/menu_ghost");
  await page.waitForFunction((n) => (window.__pw.refused ?? 0) > n, before);

  await expect(page.locator("#menu li"), "and it changed nothing").toHaveCount(4);
  const log = await page.evaluate(() => window.__pw.log.join("\n"));
  expect(log).toMatch(/refused remove_instance/);
});

test("the server refuses a move naming an item it does not have", async ({ page }) => {
  await ready(page);
  const { status, body } = await command(page, "op=move&id=espresso&after=nonesuch");
  expect(status).toBe(409);
  expect(body.refused).toMatch(/no item/);
});

test("two sessions see the same change at the same address", async ({ browser }) => {
  // ONE patch reaches both readers.
  //
  // The menu is `public … cache shared`, so it is materialized as a fragment
  // in its own PUBLIC identity domain — see `public-fragment.spec.mjs`. One
  // identity means one set of instance tokens, which means one address, which
  // means the server derives the patch once.
  //
  // The first version of this test asserted the opposite, because the fragment
  // then inherited each document's session-scoped domain: two readers of one
  // shared cache entry got different bytes. Both tests pass their own
  // rendering; only one of them describes a shared cache.
  const a = await browser.newContext();
  const b = await browser.newContext();
  const [pa, pb] = [await a.newPage(), await b.newPage()];
  for (const p of [pa, pb]) {
    await p.goto("/StorePage.html");
    await p.waitForFunction(() => document.documentElement.dataset.pwReady);
  }
  await restore(pa);
  await pb.reload();
  await pb.waitForFunction(() => document.documentElement.dataset.pwReady);
  for (const p of [pa, pb]) await mark(p);

  await command(pa, "op=move&id=cold-brew&after=espresso");

  for (const p of [pa, pb]) {
    const rows = await settled(p, ["Espresso", "Cold Brew", "Cortado"]);
    expect(rows.map((r) => r.identity), "each page moved its own nodes").toEqual([
      "mark-0",
      "mark-2",
      "mark-1",
    ]);
  }

  // And they moved at the SAME address, which is what makes one patch enough.
  // The menu's addresses: a stream's region is rendered for its document,
  // and its tokens are that document's (ADR-0165).
  const tokens = await Promise.all(
    [pa, pb].map((p) =>
      p.evaluate(() =>
        (function walk(node, out) {
          for (const n of node.childNodes) {
            if (n.nodeType === Node.COMMENT_NODE && /^pw:s\d+@/.test(n.data)) out.push(n.data);
            walk(n, out);
          }
          return out;
        })(document.getElementById("menu"), []),
      ),
    ),
  );
  expect(tokens[0].length).toBeGreaterThan(0);
  expect(tokens[0], "one public fragment, one set of instance tokens").toEqual(tokens[1]);

  await a.close();
  await b.close();
});
