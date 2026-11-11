// A table of a thousand rows, in three engines, and on a production build on
// the device's runtime: whatever is pressed, in any order, the page shows what
// its rows say; a row the press did not change is left as it was; and what is
// heard at the document acts as it would on the element.
// `examples/rows/app.pw`, and the fixture `examples/rows/events.pw`, which
// is a build of its own (`row-events.sh`): the large list's is the app alone,
// as the head-to-head measures it, and a page alone in its build (ADR-0426).
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { ROWS_PORTS, ROW_EVENTS_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${ROWS_PORTS[testInfo.project.name]}`);
  },
});

async function open(page, path = "/") {
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  const events = `http://127.0.0.1:${ROW_EVENTS_PORTS[test.info().project.name]}`;
  await page.goto(path.startsWith("/events") ? `${events}${path}` : path);
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

/** Where the document first shows otherwise than the page's rows say, or ""
 * where it shows them all, and `size` of them: compared in the page, so a step
 * sends back one line rather than every row. */
const firstDifference = (page, size) =>
  page.evaluate((size) => {
    const { rows, selected } = window.__pw.signals;
    const shown = document.getElementById("tbody").children;
    if (shown.length !== size || rows.length !== size) return `${shown.length} rows shown, ${rows.length} held, ${size} made`;
    for (let i = 0; i < size; i++) {
      const [id, label] = shown[i].children;
      const now = [Number(id.textContent), label.textContent, shown[i].className];
      const said = [rows[i].id, rows[i].label, rows[i].id === selected ? "danger" : ""];
      if (now.some((v, k) => v !== said[k])) return `row ${i}: shown ${JSON.stringify(now)}, held ${JSON.stringify(said)}`;
    }
    return "";
  }, size);

async function counted(page, n) {
  await page.waitForFunction((n) => document.getElementById("tbody").children.length === n, n, { timeout: 30000 });
}

async function created(page) {
  await page.click("#run");
  await counted(page, 1000);
}

test("whatever is pressed, in any order, the page shows what its rows say", async ({ page }) => {
  // Sixty presses over as many as several thousand rows, each step compared
  // whole: under ten seconds on a workstation, several times that on a
  // shared runner with three engines at once.
  test.setTimeout(120_000);
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
    await expect.poll(() => firstDifference(page, size), { timeout: 30_000, intervals: [16, 50, 100, 250] }).toBe("");
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

test("a press that changes a row parses no row: its plan sets what changed", async ({ page }) => {
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
  await page.evaluate(() => (window.__parsed = []));
  await created(page);
  // A thousand rows, and one parsed: the plan's.
  expect(await page.evaluate(() => (window.__parsed.join("").match(/<tr/g) ?? []).length)).toBe(1);
  await page.evaluate(() => (window.__parsed = []));
  await page.locator("#tbody tr").nth(4).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  const parsed = await page.evaluate(() => window.__parsed.join(""));
  expect((parsed.match(/<tr/g) ?? []).length).toBe(0);
  // And what it set is what parsing the row would have made.
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  await page.locator("#tbody tr").nth(9).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("");
  await expect(page.locator("#tbody tr").nth(9)).toHaveClass("danger");
});

test("a row a plan made shows what parsing it shows, whatever its text holds", async ({ page }) => {
  await open(page, "/events");
  await page.click("#note");
  await expect(page.locator("#items .note").nth(0)).toHaveText("second");
  for (const name of ['a<b>&c"d\'e`f', "x\r\ny", "tab\there", "\u{1F600} wide", "&amp; written", "nul\u0000here"]) {
    await page.locator("#name").fill(name);
    await page.click("#rename-to");
    await expect(page.locator("#items .name").nth(1)).toHaveText(name.replace(/\r\n?/g, "\n").replaceAll("\0", ""));
    // An attribute it set reads as the parser reads one: a NUL the
    // replacement character. The row holds what the field took, which keeps
    // no line break.
    const held = await page.evaluate(() => window.__pw.signals.items[1].name);
    expect(await page.locator("#items .field").nth(1).getAttribute("value")).toBe(held.replaceAll("\0", "\uFFFD"));
    const html = await page.locator("#items li").nth(1).evaluate((li) => li.innerHTML);
    const parsed = await page.locator("#items li").nth(1).evaluate((li, html) => {
      const t = document.createElement("template");
      t.innerHTML = `<ul><li>${html}</li></ul>`;
      return t.content.querySelector("li").innerHTML;
    }, html);
    expect(parsed).toBe(html);
  }
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

/** The names and order of the event page's list. */
const names = (page) => page.locator("#items .name").allTextContents();

/** The event page, its list rendered once: what comes next is made by key. */
async function listRendered(page) {
  await open(page, "/events");
  await page.click("#note");
  await expect(page.locator("#items .note").nth(0)).toHaveText("second");
}

test("a row changed and moved shows its new text where it now is", async ({ page }) => {
  await listRendered(page);
  await page.click("#reverse-rename");
  await expect(page.locator("#items .name").nth(1)).toHaveText("renamed");
  expect(await names(page)).toEqual(["two", "renamed"]);
});

/** The event page, its list of rows with no handler rendered once on the
 * device, which writes them whole (ADR-0421, decision 4): what comes next is
 * made by key, and a row no handler captures from changes only where one of
 * its holes does. */
async function plainRendered(page) {
  await open(page, "/events");
  await page.click("#plain-first");
  await expect(page.locator("#plain li")).toHaveText(["one", "renamed"]);
}

test("a row with no handler shows its new text", async ({ page }) => {
  await plainRendered(page);
  await page.click("#plain-again");
  await expect(page.locator("#plain li")).toHaveText(["one", "again"]);
});

test("a row with no handler, changed and moved, shows its new text where it now is", async ({ page }) => {
  await plainRendered(page);
  await page.click("#plain-moved");
  await expect(page.locator("#plain li")).toHaveText(["renamed", "moved"]);
});

test("new rows before the others go before them", async ({ page }) => {
  await listRendered(page);
  await page.click("#prepend");
  await expect(page.locator("#items .name")).toHaveCount(4);
  expect(await names(page)).toEqual(["ten", "eleven", "one", "two"]);
});

test("new rows either side of the others go where their place is", async ({ page }) => {
  await listRendered(page);
  await page.click("#both-ends");
  await expect(page.locator("#items .name")).toHaveCount(4);
  expect(await names(page)).toEqual(["twenty", "one", "two", "twenty-one"]);
});

test("an event that stays on its element does not reach a handler around it", async ({ page }) => {
  await open(page, "/events");
  await page.locator("#inside").focus();
  await page.locator("#note").focus();
  await expect(page.locator("#counts")).toHaveText("0 0 0 0");
  await expect(page.locator("#wrapped")).toHaveText("0");
});

test("a press writes the text of the rows it changed and indexes nothing inside them", async ({ page }) => {
  await open(page);
  await created(page);
  const before = await page.evaluate(() => window.__pw.log.length);
  await page.locator("#tbody tr").nth(4).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  await page.waitForFunction((n) => window.__pw.log.slice(n).some((l) => l.endsWith("indexed")), before);
  const lines = await page.evaluate((n) => window.__pw.log.slice(n), before);
  expect(lines.some((l) => /wrote 1 of 1000 rows$/.test(l))).toBe(true);
  const indexed = Number(lines.find((l) => l.endsWith("indexed")).split(" ")[0]);
  expect(indexed < 100).toBe(true);
});

/** The rows of the fixture's list in a block, hidden and shown again, and
 * each template parse made as it was shown. */
async function shownAgain(page) {
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
  await open(page, "/events");
  await page.click("#hide-again");
  await expect(page.locator("#again li")).toHaveCount(0);
  const hidden = await page.evaluate(() => window.__pw.indexSize());
  await page.evaluate(() => (window.__parsed = []));
  await page.click("#show-again");
  await expect(page.locator("#again li")).toHaveCount(20);
  await expect(page.locator("#again li").nth(7)).toHaveText("7");
  return { hidden, parsed: await page.evaluate(() => window.__parsed.join("")) };
}

test("a row the document holds is set by its holes: renaming it parses no row", async ({ page }) => {
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
  await open(page, "/events");
  // Once the idle warming of the page's paths has run, which parses a copy,
  // and the list's first render on the device, which morphs the document's
  // rows whole, once (ADR-0421, decision 4).
  await page.waitForFunction(() => window.__pw.log.some((l) => l.startsWith("warmed the paths")));
  await page.click("#note");
  await expect(page.locator("#items .note").nth(0)).toHaveText("second");
  await page.evaluate(() => (window.__parsed = []));
  await page.click("#rename");
  await expect(page.locator("#items .name").nth(1)).toHaveText("renamed");
  const parsed = await page.evaluate(() => window.__parsed.join(""));
  expect(parsed.includes(">renamed<")).toBe(false);
});

test("a list a block shows again is made by its plan: no row of it is parsed", async ({ page }) => {
  const { parsed } = await shownAgain(page);
  expect(parsed.includes(">7</button>")).toBe(false);
});

test("a block shown again indexes nothing inside its list's rows", async ({ page }) => {
  const { hidden } = await shownAgain(page);
  const shown = await page.evaluate(() => window.__pw.indexSize());
  // The block's own anchors and its list's: none of the twenty rows' handlers.
  expect(shown - hidden < 10).toBe(true);
});

test("a row a plan made reads a return as a line, as the parser would", async ({ page }) => {
  await listRendered(page);
  await page.click("#rename-cr");
  await expect(page.locator("#items .name").nth(1)).toHaveText("x\ny\nz");
  // And an attribute's, as the parser reads one.
  expect(await page.locator("#items .field").nth(1).getAttribute("value")).toBe("x\ny\nz");
  const html = await page.locator("#items li").nth(1).evaluate((li) => li.innerHTML);
  const parsed = await page.evaluate((html) => {
    const t = document.createElement("template");
    t.innerHTML = `<ul><li>${html}</li></ul>`;
    return t.content.querySelector("li").innerHTML;
  }, html);
  expect(parsed).toBe(html);
});

test("a row's handler is given what its row holds, whatever its text holds", async ({ page }) => {
  await listRendered(page);
  await page.click("#rename-cr");
  await expect(page.locator("#items .name").nth(1)).toHaveText("x\ny\nz");
  // What the row was made with, its returns as they are: what the handler
  // captures is the value, not what its text shows.
  await page.locator("#items .pick").nth(1).click();
  await expect.poll(() => page.evaluate(() => window.__pw.signals.picked)).toBe("x\r\ny\rz");
  await page.locator("#items .pick").nth(0).click();
  await expect.poll(() => page.evaluate(() => window.__pw.signals.picked)).toBe("one");
});

test("a field a person typed in shows its row's value when that changes", async ({ page }) => {
  await listRendered(page);
  await page.locator("#items .field").nth(1).fill("typed");
  await page.click("#rename");
  await expect(page.locator("#items .name").nth(1)).toHaveText("renamed");
  await expect(page.locator("#items .field").nth(1)).toHaveValue("renamed");
});

test("a row whose value changed is written again", async ({ page }) => {
  await open(page, "/events");
  await page.click("#rename");
  await expect(page.locator("#items .name").nth(1)).toHaveText("renamed");
  await expect(page.locator("#items .name").nth(0)).toHaveText("one");
});

// --- what a row's handlers capture (ADR-0427) --------------------------------

test("a thousand rows a plan made hold no captured text, and a press on one reaches its row", async ({ page }) => {
  await open(page);
  await created(page);
  await expect(page.locator("#tbody [data-pw-captures]")).toHaveCount(0);
  await page.locator("#tbody tr").nth(6).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(6)).toHaveClass("danger");
  await page.locator("#tbody tr").nth(6).locator(".remove").click();
  await counted(page, 999);
  await expect(page.locator("#tbody tr").nth(6).locator(".id")).toHaveText("8");
});

/** The tags as the document holds them, once the list's first render on the
 * device has morphed them whole (ADR-0421, decision 4): each change after is
 * the plan's. */
async function tagsRendered(page) {
  await open(page, "/events");
  await page.click("#flip");
  await expect(page.locator("#tags .on").nth(0)).toHaveAttribute("hidden", "");
}

const picked = (page) => page.evaluate(() => window.__pw.signals.picked);

test("what a row's handler captures follows its row where nothing the row shows changed", async ({ page }) => {
  await tagsRendered(page);
  await page.click("#rehide");
  await expect.poll(() => page.evaluate(() => window.__pw.signals.tags[1].unseen)).toBe("second!");
  await page.locator("#tags .tag").nth(1).click();
  await expect.poll(() => picked(page)).toBe("second!");
  await page.locator("#tags .tag").nth(0).click();
  await expect.poll(() => picked(page)).toBe("first!");
});

test("a row the plan takes over from the document keeps no captured text", async ({ page }) => {
  await tagsRendered(page);
  await expect(page.locator("#tags .tag").nth(1)).toHaveAttribute("data-pw-captures", /second/);
  await page.click("#rehide");
  await expect.poll(() => page.evaluate(() => window.__pw.signals.tags[1].unseen)).toBe("second!");
  await expect(page.locator("#tags .tag").nth(1)).not.toHaveAttribute("data-pw-captures");
});

test("a row a plan made is pressed with what its row holds, then and after it changes", async ({ page }) => {
  await tagsRendered(page);
  await page.click("#add-tag");
  await expect(page.locator("#tags .shown").nth(0)).toHaveText("three");
  await expect(page.locator("#tags .tag").nth(0)).not.toHaveAttribute("data-pw-captures");
  await page.locator("#tags .tag").nth(0).click();
  await expect.poll(() => picked(page)).toBe("third");
  await page.click("#rehide");
  await expect.poll(() => page.evaluate(() => window.__pw.signals.tags[0].unseen)).toBe("third!");
  await page.locator("#tags .tag").nth(0).click();
  await expect.poll(() => picked(page)).toBe("third!");
});

test("a row's boolean attribute follows its row, on a row a plan made and one it took over", async ({ page }) => {
  await tagsRendered(page);
  // Flipped once by the list's first render: the first tag is off, the second on.
  await expect(page.locator("#tags .on").nth(1)).not.toHaveAttribute("hidden");
  await page.click("#add-tag");
  await expect(page.locator("#tags .shown").nth(0)).toHaveText("three");
  await expect(page.locator("#tags .on").nth(0)).not.toHaveAttribute("hidden");
  await page.click("#flip");
  await expect(page.locator("#tags .on").nth(0)).toHaveAttribute("hidden", "");
  await expect(page.locator("#tags .on").nth(1)).not.toHaveAttribute("hidden");
  await expect(page.locator("#tags .on").nth(2)).toHaveAttribute("hidden", "");
});
