// Values the reader's device keeps (track `device-store`, ADR-0366), in
// each engine.
//
// `examples/demo/device.pw` holds two: `notes`, a list at version 2 under
// `demo-notes@2`, which `notes_from_1` takes from version 1's `demo-notes`,
// and `presses`, a count at version 1 under `demo-count`. Each test is its
// own browser context, so its own empty storage. What this shows:
// - the server renders each declared default and hides what the device
//   decides until the device is read; with scripts off, the default shows;
// - a value kept survives a reload, and two tabs agree;
// - version 1's notes are taken to version 2, and the old key goes;
// - a page that finds a newer version writes nothing more, and says so;
// - an unreadable value is refused, kept aside, and never used;
// - a write the store refuses is said where it was made;
// - a value computed from the device's value and the page's is computed
//   again when either changes, and a row keeps its element as its list is
//   reordered (the block's morph, matched by key);
// - no request carries what the device keeps;
// - and the store's own limit, measured in each engine.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";

const PAGE = "/page/demo.device.DevicePage~ui";

async function ready(page) {
  await page.goto(PAGE);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** Every key the page's origin keeps, with its text. */
const stored = (page) =>
  page.evaluate(() =>
    Object.fromEntries(
      Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)).map((k) => [
        k,
        localStorage.getItem(k),
      ]),
    ),
  );

/** A required stored value is text before any content inspection. */
function keptText(values, key) {
  const value = values[key];
  expect(typeof value, `${key} is stored as text`).toBe("string");
  return value;
}

/** JSON validity is part of the persisted-data contract, not a parser panic. */
function keptJson(values, key) {
  const text = keptText(values, key);
  let value, valid = true;
  try { value = JSON.parse(text); } catch { valid = false; }
  expect(valid, `${key} contains valid JSON`).toBe(true);
  return value;
}

/** What the runtime says of each value it keeps. */
const state = (page) => page.evaluate(() => window.__pw.device?.state ?? {});

async function note(page, text) {
  await page.locator("#draft").fill(text);
  await page.locator("#draft").press("Enter");
}

test("the server renders the defaults, marks what the device decides, and names what it keeps", async ({
  request,
}) => {
  const response = await request.get(PAGE);
  const html = await response.text();
  expect(html).toContain('<html lang="en" data-pw-device="reading">');
  expect(html).toContain(
    "<style>html[data-pw-device=reading] [data-pw-device]{visibility:hidden}</style>",
  );
  expect(html).toContain(
    "<noscript><style>html[data-pw-device=reading] [data-pw-device]{visibility:visible}</style></noscript>",
  );
  const markup = html.split('<script type="application/json"')[0];
  // The defaults, each in an element marked as the device's.
  expect(markup).toMatch(/<p data-pw-device id="count"><!--pw:s\d+-->0 notes<!--pw:e\d+--><\/p>/);
  expect(markup).toMatch(/<p data-pw-device id="presses"><!--pw:s\d+-->0<!--pw:e\d+--><\/p>/);
  expect(markup).toContain('<section data-pw-device id="list"');
  // The field is the page's, not the device's.
  expect(markup).not.toMatch(/<input[^>]*data-pw-device/);
  const manifest = JSON.parse(
    html.match(/<script type="application\/json" id="pw-parts">(.*?)<\/script>/s)[1],
  );
  expect(manifest.device.map(({ signal, name, version, conflict }) => [signal, name, version, conflict])).toEqual([
    ["notes", "demo-notes", 2, "last_write_wins"],
    ["presses", "demo-count", 1, "last_write_wins"],
  ]);
  // The style the head holds is allowed by its hash, and no other inline
  // style or script is.
  const policy = response.headers()["content-security-policy"];
  expect(policy).toMatch(/style-src 'self'( 'sha256-[^']+')+/);
});

test("nothing the device decides is shown before the device is read, and the page's own is", async ({
  page,
}) => {
  await page.addInitScript(() => {
    if (!localStorage.getItem("demo-count")) localStorage.setItem("demo-count", "41");
  });
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/pw-runtime.mjs", async (route) => {
    await held;
    await route.continue();
  });
  await page.goto(PAGE, { waitUntil: "domcontentloaded" });
  await expect(page.locator("#draft")).toBeVisible();
  await expect(page.locator("#presses")).toBeHidden();
  await expect(page.locator("#count")).toBeHidden();
  expect(await page.evaluate(() => document.documentElement.dataset.pwDevice)).toBe("reading");
  release();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#presses")).toBeVisible();
  await expect(page.locator("#presses")).toHaveText("41");
  expect(await page.evaluate(() => document.documentElement.dataset.pwDevice)).toBe("read");
});

test("what the device decides stays hidden until it is rendered at the device's value", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem(
      "demo-notes@2",
      JSON.stringify([
        { id: "a", text: "kept", pinned: false },
        { id: "b", text: "kept too", pinned: false },
      ]),
    );
  });
  // The runtime runs; what computes the count from the notes is held back.
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/computed/demo.device.DevicePage~ui.mjs", async (route) => {
    await held;
    await route.continue();
  });
  try {
    await page.goto(PAGE);
    await expect.poll(() => page.evaluate(() =>
      (window.__pw?.log ?? []).some((l) => l === "device: demo-notes read"),
    )).toBe(true);
    // Read, not yet rendered: the default is not shown in its place.
    await expect(page.locator("#count")).toBeHidden();
    await expect(page.locator("#draft")).toBeVisible();
  } finally {
    release();
  }
  await expect(page.locator("#count")).toBeVisible();
  await expect(page.locator("#count")).toHaveText("2 notes");
});

test("with scripts off, the declared default is shown", async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto(PAGE);
  await expect(page.locator("#count")).toBeVisible();
  await expect(page.locator("#count")).toHaveText("0 notes");
  await context.close();
});

test("a value kept survives a reload, under its name and version", async ({ page }) => {
  await ready(page);
  expect(await state(page)).toEqual({ notes: "default", presses: "default" });
  await note(page, "buy some cheese");
  await note(page, "feed the cat");
  await page.locator("#press").click();
  await page.locator("#press").click();
  await expect(page.locator("#count")).toHaveText("2 notes");
  await expect(page.locator("#presses")).toHaveText("2");
  const kept = await stored(page);
  expect(keptJson(kept, "demo-notes@2")).toEqual([
    { id: "0-buy some cheese", text: "buy some cheese", pinned: false },
    { id: "1-feed the cat", text: "feed the cat", pinned: false },
  ]);
  expect(kept["demo-count"]).toBe("2");
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  expect(await state(page)).toEqual({ notes: "read", presses: "read" });
  await expect(page.locator("#count")).toHaveText("2 notes");
  await expect(page.locator("#notes li")).toHaveText(["buy some cheese", "feed the cat"]);
  await expect(page.locator("#presses")).toHaveText("2");
  // Control: a value cleared is kept cleared.
  await page.locator("#clear").click();
  await expect(page.locator("#count")).toHaveText("0 notes");
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#count")).toHaveText("0 notes");
  await expect(page.locator("#notes")).toHaveCount(0);
});

test("two tabs agree: each shows the other's writes and writes nothing back", async ({ context }) => {
  const one = await context.newPage();
  const two = await context.newPage();
  await ready(one);
  await ready(two);
  await note(one, "from the first tab");
  await expect(two.locator("#notes li")).toHaveText(["from the first tab"]);
  await two.locator("#press").click();
  await expect(one.locator("#presses")).toHaveText("1");
  // The other tab's write is shown and not written again.
  expect(await two.evaluate(() => window.__pw.device.writes)).toBe(1);
  expect(await one.evaluate(() => window.__pw.device.writes)).toBe(1);
  expect(await one.evaluate(() => window.__pw.log.filter((l) => l.includes("heard from another tab")).length)).toBeGreaterThan(0);
  // The last write is every tab's.
  await note(two, "from the second tab");
  await expect(one.locator("#notes li")).toHaveText(["from the first tab", "from the second tab"]);
});

test("a burst of writes from another tab is heard once, and what is held then is shown", async ({ browser }) => {
  const context = await browser.newContext();
  const one = await context.newPage();
  const two = await context.newPage();
  await ready(one);
  await ready(two);
  await note(one, "first");
  await expect(two.locator("#notes li")).toHaveText(["first"]);
  const key = "demo-notes@2";
  const heard = () => two.evaluate(() => window.__pw.log.filter((l) => l.includes("heard from another tab")).length);
  const before = await heard();
  // Three writes in one task, and an event that says what is no longer held:
  // the last write is shown, and shown once.
  await one.evaluate((k) => {
    const list = (...texts) => JSON.stringify(texts.map((text) => ({ id: text, text, pinned: false })));
    for (const value of [list("a"), list("a", "b"), list("a", "b", "c")]) localStorage.setItem(k, value);
  }, key);
  await expect(two.locator("#notes li")).toHaveText(["a", "b", "c"]);
  expect((await heard()) - before).toBe(1);
  await two.evaluate((k) => {
    window.dispatchEvent(new StorageEvent("storage", { key: k, newValue: '[{"id":"old","text":"old","pinned":false}]', storageArea: localStorage }));
  }, key);
  await two.waitForFunction((n) => window.__pw.log.filter((l) => l.includes("heard from another tab")).length > n, before + 1);
  await expect(two.locator("#notes li")).toHaveText(["a", "b", "c"]);
  await context.close();
});

test("version 1's notes are taken to version 2, and the old key goes", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      sessionStorage.setItem("seeded", "1");
      localStorage.setItem("demo-notes", JSON.stringify([{ text: "old" }, { text: "older" }]));
    }
  });
  await ready(page);
  expect((await state(page)).notes).toBe("migrated");
  await expect(page.locator("#notes li")).toHaveText(["old", "older"]);
  const kept = await stored(page);
  expect(kept["demo-notes"]).toBe(undefined);
  expect(keptJson(kept, "demo-notes@2")).toEqual([
    { id: "old", text: "old", pinned: false },
    { id: "older", text: "older", pinned: false },
  ]);
  // Read at version 2 from then on.
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  expect((await state(page)).notes).toBe("read");
  await expect(page.locator("#notes li")).toHaveText(["old", "older"]);
});

test("a page that finds a newer version writes nothing more, and says so", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("demo-notes@3", JSON.stringify({ shape: "a later build's" }));
  });
  await ready(page);
  expect((await state(page)).notes).toBe("stale");
  await note(page, "not kept");
  await expect(page.locator("#count")).toHaveText("1 note");
  const kept = await stored(page);
  expect(kept["demo-notes@2"]).toBe(undefined);
  await expect(page.locator(".pw-refusal")).toHaveText(
    "This page is out of date. Reload to keep your changes.",
  );
  // Control: the value it is current for is kept as ever.
  await page.locator("#press").click();
  await expect(page.locator("#presses")).toHaveText("1");
  expect((await stored(page))["demo-count"]).toBe("1");
});

test("a newer version kept by another tab stops this one writing", async ({ context }) => {
  const page = await context.newPage();
  await ready(page);
  await note(page, "kept");
  await expect(page.locator("#count")).toHaveText("1 note");
  expect(keptText(await stored(page), "demo-notes@2")).toContain("kept");
  const other = await context.newPage();
  await other.goto(PAGE);
  await other.evaluate(() => localStorage.setItem("demo-notes@3", "[]"));
  await expect.poll(async () => (await state(page)).notes).toBe("stale");
  await note(page, "not kept");
  await expect(page.locator("#count")).toHaveText("2 notes");
  expect(keptText(await stored(page), "demo-notes@2")).not.toContain("not kept");
});

test("an unreadable value is refused, never used, and kept aside before it is replaced", async ({
  page,
}) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("seeded")) {
      sessionStorage.setItem("seeded", "1");
      localStorage.setItem("demo-notes@2", JSON.stringify([{ id: "a", text: 7, pinned: false }]));
      localStorage.setItem("demo-count", "not a number");
    }
  });
  await ready(page);
  expect(await state(page)).toEqual({ notes: "unreadable", presses: "unreadable" });
  await expect(page.locator("#count")).toHaveText("0 notes");
  await expect(page.locator("#presses")).toHaveText("0");
  const said = await page.evaluate(() => window.__pw.log.filter((l) => l.includes("unreadable:")));
  expect(said.join("\n")).toContain("the value[0].text is no String");
  // Nothing is written until the reader writes, and then the unread text is
  // kept aside first.
  expect((await stored(page))["demo-notes@2!unreadable"]).toBe(undefined);
  await note(page, "a new start");
  await expect(page.locator("#count")).toHaveText("1 note");
  const kept = await stored(page);
  expect(keptJson(kept, "demo-notes@2!unreadable")).toEqual([{ id: "a", text: 7, pinned: false }]);
  expect(keptJson(kept, "demo-notes@2")).toEqual([
    { id: "0-a new start", text: "a new start", pinned: false },
  ]);
});

test("a write the store refuses is said where it was made, and what was kept stays", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("demo-count", "5");
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === "demo-count") throw new DOMException("The quota has been exceeded.", "QuotaExceededError");
      return setItem.call(this, key, value);
    };
  });
  await ready(page);
  await expect(page.locator("#presses")).toHaveText("5");
  await page.locator("#press").click();
  // The page holds what the person did, and says the device did not keep it.
  await expect(page.locator("#presses")).toHaveText("6");
  await expect(page.locator(".pw-refusal")).toHaveText("This device has no room to keep that.");
  expect((await stored(page))["demo-count"]).toBe("5");
  // Control: the other value is kept as ever.
  await note(page, "kept");
  await expect(page.locator("#count")).toHaveText("1 note");
  expect(keptText(await stored(page), "demo-notes@2")).toContain("kept");
});

test("a value computed from the device's value and the page's is computed again when either changes", async ({
  page,
}) => {
  await ready(page);
  await expect(page.locator("#summary")).toHaveText("0 notes");
  // The page's own value changes: what is typed.
  await page.locator("#draft").fill("milk");
  await expect(page.locator("#summary")).toHaveText("1 note once milk is added");
  // The device's: the note added, and the field emptied.
  await page.locator("#draft").press("Enter");
  await expect(page.locator("#summary")).toHaveText("1 note");
  await page.locator("#draft").fill("bread");
  await expect(page.locator("#summary")).toHaveText("2 notes once bread is added");
});

test("a row keeps its element as its list is reordered, matched by its note's key", async ({ page }) => {
  await ready(page);
  for (const text of ["first", "second", "third"]) await note(page, text);
  const rows = page.locator("#notes li");
  await expect(rows).toHaveText(["first", "second", "third"]);
  const held = await Promise.all([0, 1, 2].map((i) => rows.nth(i).elementHandle()));
  await page.locator("#newest").click();
  await expect(rows).toHaveText(["third", "second", "first"]);
  // Each row the element it was, moved: none made again.
  for (const [i, at] of [[0, 2], [1, 1], [2, 0]]) {
    expect(await rows.nth(at).evaluate((li, was) => li === was, held[i])).toBe(true);
  }
  await page.locator("#newest").click();
  await expect(rows).toHaveText(["first", "second", "third"]);
  expect(await rows.nth(0).evaluate((li, was) => li === was, held[0])).toBe(true);
});

test("no request carries what the device keeps", async ({ page }) => {
  const sent = [];
  page.on("request", (r) => sent.push(`${r.method()} ${r.url()} ${r.postData() ?? ""}`));
  await ready(page);
  const marker = `marker-${Date.now()}`;
  await note(page, marker);
  await expect(page.locator("#notes li")).toHaveText([marker]);
  expect(keptJson(await stored(page), "demo-notes@2")).toEqual([
    { id: "0-" + marker, text: marker, pinned: false },
  ]);
  await page.locator("#press").click();
  await expect(page.locator("#presses")).toHaveText("1");
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#notes li")).toHaveText([marker]);
  expect(sent.length).toBeGreaterThan(0);
  expect(sent.filter((s) => s.includes(marker))).toEqual([]);
});

test("the store's own limit, in this engine", async ({ page }, info) => {
  await ready(page);
  const bytes = await page.evaluate(() => {
    localStorage.clear();
    // Filled in chunks that halve each time the store refuses one, down to a
    // single code unit, so what it holds at the end is its limit.
    let n = 0;
    for (let size = 64 * 1024; size >= 1; size = Math.floor(size / 2)) {
      const chunk = "x".repeat(size);
      for (;;) {
        try {
          localStorage.setItem(`f${n}`, chunk);
          n++;
        } catch (error) {
          if (error.name !== "QuotaExceededError") throw error;
          break;
        }
      }
    }
    let kept = 0;
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      kept += k.length + localStorage.getItem(k).length;
    }
    localStorage.clear();
    return kept;
  });
  info.annotations.push({ type: "localStorage limit", description: `${bytes} UTF-16 code units` });
  // A line the evidence keeps (scripts/run_suite.sh keeps `EVIDENCE` lines).
  console.log(`EVIDENCE localStorage limit (${info.project.name}): ${bytes} UTF-16 code units`);
  expect(bytes).toBeGreaterThan(1024 * 1024);
});
