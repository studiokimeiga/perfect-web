// A table of a thousand rows, in three engines, and on a production build on
// the device's runtime: whatever is pressed, in any order, the page shows what
// its rows say; a row the press did not change is left as it was; and what is
// heard at the document acts as it would on the element.
// `examples/rows/app.pw`, `examples/rows/events.pw`.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { ROWS_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${ROWS_PORTS[testInfo.project.name]}`);
  },
});

async function open(page, path = "/") {
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** What the page's signals say each row is, beside what the document shows. */
const bothViews = (page) =>
  page.evaluate(() => {
    const { rows, selected } = window.__pw.signals;
    const said = rows.map((r) => [r.id, r.label, r.id === selected ? "danger" : ""]);
    const shown = [...document.getElementById("tbody").children].map((tr) => [
      Number(tr.querySelector(".id").textContent),
      tr.querySelector(".select").textContent,
      tr.className,
    ]);
    return { said, shown };
  });

/** Each row element marked with its place, on the element itself: a morph
 * takes away what its rendering does not hold, and a new element has none. */
const marked = (page) =>
  page.evaluate(() => {
    [...document.getElementById("tbody").children].forEach((tr, i) => (tr.mark = String(i)));
  });

/** Every row as the document holds it, by its mark, in order. */
const marks = (page) => page.evaluate(() => [...document.getElementById("tbody").children].map((tr) => tr.mark ?? ""));

async function counted(page, n) {
  await page.waitForFunction((n) => document.getElementById("tbody").children.length === n, n, { timeout: 30000 });
}

async function created(page) {
  await page.click("#run");
  await counted(page, 1000);
}

test("whatever is pressed, in any order, the page shows what its rows say", async ({ page }) => {
  await open(page);
  // A seeded walk, the same in every engine.
  let seed = 1234;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  let size = 0;
  for (let step = 0; step < 60; step++) {
    const op = size === 0 ? 0 : next(8);
    if (op === 0) {
      await page.click("#run");
      size = 1000;
    } else if (op === 1) {
      await page.click("#add");
      size += 1000;
    } else if (op === 2) {
      await page.click("#update");
      await page.waitForFunction(() => {
        const first = [...document.querySelectorAll("#tbody .select")].find((b, i) => i % 10 === 0);
        return !first || first.textContent.endsWith("!!!") || window.__pw.signals.rows[0]?.id % 10 !== 1;
      });
    } else if (op === 3 && size >= 999) {
      await page.click("#swaprows");
    } else if (op === 4) {
      await page.locator("#tbody tr").nth(next(size)).locator(".remove").click();
      size -= 1;
    } else if (op === 5) {
      await page.locator("#tbody tr").nth(next(size)).locator(".select").click();
    } else if (op === 6 && next(4) === 0) {
      await page.click("#clear");
      size = 0;
    }
    await counted(page, size);
    await page.waitForFunction(() => {
      const { rows, selected } = window.__pw.signals;
      return [...document.getElementById("tbody").children].every(
        (tr, i) => Number(tr.querySelector(".id").textContent) === rows[i].id && tr.className === (rows[i].id === selected ? "danger" : ""),
      );
    });
    const { said, shown } = await bothViews(page);
    expect(shown).toEqual(said);
  }
  expect(page.errors).toEqual([]);
});

test("a press leaves the rows it did not change as they were", async ({ page }) => {
  await open(page);
  await created(page);
  await marked(page);
  const before = await marks(page);
  await page.locator("#tbody tr").nth(4).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  expect(await marks(page)).toEqual(before);
  await page.click("#update");
  await expect(page.locator("#tbody tr").nth(0).locator(".select")).toContainText("!!!");
  expect(await marks(page)).toEqual(before);
});

test("two rows swapped are the same two rows, moved", async ({ page }) => {
  await open(page);
  await created(page);
  await marked(page);
  const before = await marks(page);
  await page.click("#swaprows");
  await page.waitForFunction(() => document.getElementById("tbody").children[1].mark === "998");
  const expected = [...before];
  [expected[1], expected[998]] = [expected[998], expected[1]];
  expect(await marks(page)).toEqual(expected);
});

test("a press on a row reaches the handler of that row", async ({ page }) => {
  await open(page);
  await created(page);
  await page.click("#swaprows");
  await page.waitForFunction(() => Number(document.getElementById("tbody").children[1].querySelector(".id").textContent) === 999);
  await page.locator("#tbody tr").nth(1).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(1)).toHaveClass("danger");
  await page.locator("#tbody tr").nth(1).locator(".remove").click();
  await counted(page, 999);
  const { said, shown } = await bothViews(page);
  expect(shown).toEqual(said);
  expect(said.some(([id]) => id === 999)).toBe(false);
});

test("a row's text is written again only for the rows that changed", async ({ page }) => {
  await page.addInitScript(() => {
    window.__parsed = [];
    const set = Object.getOwnPropertyDescriptor(Element.prototype, "innerHTML").set;
    Object.defineProperty(HTMLTemplateElement.prototype, "innerHTML", {
      set(html) {
        window.__parsed.push(html);
        set.call(this, html);
      },
    });
  });
  await open(page);
  await created(page);
  await page.evaluate(() => (window.__parsed = []));
  await page.locator("#tbody tr").nth(4).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  const parsed = await page.evaluate(() => window.__parsed.join(""));
  expect((parsed.match(/<tr/g) ?? []).length).toBe(1);
});

test("a handler that stops its event is the last to hear it", async ({ page }) => {
  await open(page, "/events");
  await page.click("#stops");
  await expect(page.locator("#counts")).toHaveText("0 1 0 0");
  await page.click("#bubbles");
  await expect(page.locator("#counts")).toHaveText("1 2 0 0");
  expect(page.errors).toEqual([]);
});

test("an event that stays on its element is heard there alone", async ({ page }) => {
  await open(page, "/events");
  await page.locator("#field").focus();
  await page.keyboard.press("a");
  await expect(page.locator("#counts")).toHaveText("0 0 0 1");
  await page.locator("#note").focus();
  await expect(page.locator("#counts")).toHaveText("0 0 1 1");
});

test("a row that reads the page is written again when the page changes", async ({ page }) => {
  await open(page, "/events");
  await page.click("#note");
  await expect(page.locator("#items .note").nth(0)).toHaveText("second");
  await expect(page.locator("#items .note").nth(1)).toHaveText("second");
});

test("a row whose value changed is written again", async ({ page }) => {
  await open(page, "/events");
  await page.click("#rename");
  await expect(page.locator("#items .name").nth(1)).toHaveText("renamed");
  await expect(page.locator("#items .name").nth(0)).toHaveText("one");
});
