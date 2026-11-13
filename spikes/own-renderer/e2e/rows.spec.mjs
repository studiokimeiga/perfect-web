// A table of a thousand rows, in three engines, and on a production build on
// the device's runtime: whatever is pressed, in any order, the page shows what
// its rows say; a row the press did not change is left as it was; and what is
// heard at the document acts as it would on the element.
// `examples/rows/app.pw`, and the fixture `examples/rows/events.pw`, which
// is a build of its own (`row-events.sh`): the large list's is the app alone,
// as the head-to-head measures it, and a page alone in its build (ADR-0426).
// A test that reads what the page reports about itself, its signals, its log
// or its index, is tagged `@report`, and runs on a build that reports: a
// development build, or a production one built with `--report`. Every other
// test runs on the build as it is served too, which reports nothing
// (ADR-0430): `just e14-device-runtime` runs them so.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { ROWS_PORTS, ROW_EVENTS_PORTS } from "../host-metadata.mjs";
import { rowElement } from "../own-rows.mjs";

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
  if (test.info().tags.includes("@report")) {
    const reports = await page.evaluate(() => "__pw" in window);
    expect(reports, "a test that reads the page's reports needs a build that keeps them: PW_REPORT=1 (pw build --report)").toBe(true);
  }
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

test("whatever is pressed, in any order, the page shows what its rows say", { tag: "@report" }, async ({ page }) => {
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

test("a press on a row reaches the handler of that row", { tag: "@report" }, async ({ page }) => {
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

test("a row a plan made shows what parsing it shows, whatever its text holds", { tag: "@report" }, async ({ page }) => {
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

/** What the page says it did with its rows for `press` (ADR-0432): each
 * `rows ...` line its log gains, once `n` rows show and one has been said. */
async function madeBy(page, press, n) {
  const before = await page.evaluate(() => window.__pw.log.length);
  await press();
  await counted(page, n);
  await page.waitForFunction((b) => window.__pw.log.slice(b).some((l) => l.startsWith("rows ")), before);
  return page.evaluate((b) => window.__pw.log.slice(b).filter((l) => l.startsWith("rows ")), before);
}

test("rows where they were are changed in place and new ones go in after them, and a list replaced goes at once", { tag: "@report" }, async ({ page }) => {
  await open(page);
  const row = (i) => page.locator("#tbody tr").nth(i);
  // Made from none, and added to at its end: no row is looked for or placed.
  expect(await madeBy(page, () => page.click("#run"), 1000)).toEqual(["rows in order: 0 changed, 1000 added"]);
  expect(await madeBy(page, () => page.click("#add"), 2000)).toEqual(["rows in order: 0 changed, 1000 added"]);
  // Changed where they are: every tenth row, then the one selected.
  expect(await madeBy(page, () => page.click("#update"), 2000)).toEqual(["rows in order: 200 changed, 0 added"]);
  expect(await madeBy(page, () => row(4).locator(".select").click(), 2000)).toEqual(["rows in order: 1 changed, 0 added"]);
  // Replaced: every row goes at once, and the new ones go in as one.
  expect(await madeBy(page, () => page.click("#run"), 1000)).toEqual(["rows replaced: 2000 gone", "rows in order: 0 changed, 1000 added"]);
  // Moved, and one taken out: the rows are placed.
  expect(await madeBy(page, () => page.click("#swaprows"), 1000)).toEqual(["rows placed: 0 gone, 0 changed, 0 added"]);
  expect(await madeBy(page, () => row(4).locator(".remove").click(), 999)).toEqual(["rows placed: 1 gone, 0 changed, 0 added"]);
  expect(await madeBy(page, () => page.click("#clear"), 0)).toEqual(["rows replaced: 999 gone", "rows in order: 0 changed, 0 added"]);
  expect(page.errors).toEqual([]);
});

/** Each plan's word on which walk reaches its holes (ADR-0437), as its log
 * says it once the plan is made: `plan <part>: its own walk` or `... the
 * general walk`. */
const walks = (page) => page.evaluate(() => window.__pw.log.filter((l) => /^plan \d+: /.test(l)));

test("the large list's rows are made by its own code, its holes reached by the walk the packaging wrote", { tag: "@report" }, async ({ page }) => {
  await open(page);
  await created(page);
  expect(await firstDifference(page, 1000)).toBe("");
  expect(await walks(page)).toEqual(["plan 6: its own walk"]);
  // Each of the thousand rows by that walk.
  expect(await page.evaluate(() => window.__pw.walked)).toBe(1000);
  await page.click("#update");
  await page.locator("#tbody tr").nth(4).locator(".select").click();
  await expect(page.locator("#tbody tr").nth(4)).toHaveClass("danger");
  expect(await firstDifference(page, 1000)).toBe("");
  expect(page.errors).toEqual([]);
});

test("a row whose markup the parser reads otherwise is reached by the general walk, and shows what its row says", { tag: "@report" }, async ({ page }) => {
  await open(page, "/events");
  const cells = () => page.locator("#cells .cell").allTextContents();
  await page.click("#add-cell");
  await expect(page.locator("#cells .cell")).toHaveCount(3);
  // The parser puts the row's table row in a `<tbody>` the markup does not
  // write: the packaging's reading of it is not the browser's.
  expect((await walks(page)).filter((l) => l.endsWith("the general walk"))).toHaveLength(1);
  expect(await page.evaluate(() => window.__pw.walked ?? 0)).toBe(0);
  expect(await page.locator("#cells li").nth(2).evaluate((li) => li.querySelector("table > tbody > tr > td.cell") !== null)).toBe(true);
  expect(await cells()).toEqual(["one", "two", "three"]);
  await page.click("#rename-cell");
  await expect(page.locator("#cells .cell").nth(0)).toHaveText("renamed");
  expect(await cells()).toEqual(["renamed", "two", "three"]);
  expect(page.errors).toEqual([]);
});

test("a list inside a case's arm is kept by the general code, and shows what its rows say", async ({ page }) => {
  // Its row reads the name the arm binds, which the page's own code does not
  // (ADR-0437): the general code makes its rows by their plan, each kind of
  // hole there is, and its handler is pressed with what its row holds.
  await open(page, "/events");
  const row = (i) => page.locator("#matched .matched").nth(i);
  const names = () => page.locator("#matched .mname").allTextContents();
  expect(await names()).toEqual(["one 1", "two 1"]);
  await page.click("#next-maybe");
  await expect(row(0).locator(".mname")).toHaveText("one 2");
  await page.click("#rename");
  await expect(row(1).locator(".mname")).toHaveText("renamed 2");
  expect(await names()).toEqual(["one 2", "renamed 2"]);
  // An attribute and a boolean attribute follow the row.
  expect(await row(1).getAttribute("title")).toBe("renamed");
  expect(await row(1).locator(".moff").evaluate((s) => s.hidden)).toBe(true);
  expect(await row(0).locator(".moff").evaluate((s) => s.hidden)).toBe(false);
  // And its handler is given what its row holds now.
  await row(1).locator(".mpick").click();
  await expect(page.locator("#picked")).toHaveText("renamed");
  await row(0).locator(".mpick").click();
  await expect(page.locator("#picked")).toHaveText("one");
  expect(page.errors).toEqual([]);
});

test("a list the general code keeps leaves a row where it was where what it shows and computes is the same", async ({ page }) => {
  // The case's arm's list again (ADR-0437): each row's class is what it
  // computes from the page's `picked`, and its button has two handlers, one
  // hole. A press that changes one row's class leaves the other's node the
  // one it was, as what the other computes comes out the same.
  await open(page, "/events");
  const row = (i) => page.locator("#matched .matched").nth(i);
  await page.click("#next-maybe");
  await expect(row(1).locator(".mname")).toHaveText("two 2");
  await expect(row(0)).toHaveAttribute("class", "matched");
  await row(1).evaluate((li) => (li.__kept = true));
  await row(0).locator(".mpick").click();
  await expect(row(0)).toHaveAttribute("class", "matched chosen");
  await expect(row(1)).toHaveAttribute("class", "matched");
  expect(await row(1).evaluate((li) => li.__kept === true)).toBe(true);
  expect(await row(1).locator(".moff").evaluate((s) => s.hidden)).toBe(false);
  expect(await row(0).getAttribute("title")).toBe("one");
  expect(page.errors).toEqual([]);
});

test("a press writes the one row of a list the general code keeps that it changed, by what the row computes", { tag: "@report" }, async ({ page }) => {
  // What the other row computes reads the page too, and comes out the same:
  // it is kept, not written again (the large list's own code is held so by
  // "a press writes the text of the rows it changed").
  await open(page, "/events");
  const row = (i) => page.locator("#matched .matched").nth(i);
  await page.click("#next-maybe");
  await expect(row(1).locator(".mname")).toHaveText("two 2");
  const before = await page.evaluate(() => window.__pw.log.length);
  await row(0).locator(".mpick").click();
  await expect(row(0)).toHaveAttribute("class", "matched chosen");
  await page.waitForFunction((n) => window.__pw.log.slice(n).some((l) => / of 2 rows$/.test(l)), before);
  const lines = await page.evaluate((n) => window.__pw.log.slice(n), before);
  expect(lines.filter((l) => / of 2 rows$/.test(l)).map((l) => l.replace(/^block \d+ /, ""))).toEqual(["wrote 1 of 2 rows"]);
});

test("a press writes the text of the rows it changed and indexes nothing inside them", { tag: "@report" }, async ({ page }) => {
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

test("a row the document holds is set by its holes: renaming it parses no row", { tag: "@report" }, async ({ page }) => {
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

test("a list a block shows again is made by its plan: no row of it is parsed", { tag: "@report" }, async ({ page }) => {
  const { parsed } = await shownAgain(page);
  expect(parsed.includes(">7</button>")).toBe(false);
});

test("a block shown again indexes nothing inside its list's rows", { tag: "@report" }, async ({ page }) => {
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

test("a row's handler is given what its row holds, whatever its text holds", { tag: "@report" }, async ({ page }) => {
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

test("a thousand rows a plan made hold no captured text, and a press on one reaches its row", { tag: "@report" }, async ({ page }) => {
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

test("what a row's handler captures follows its row where nothing the row shows changed", { tag: "@report" }, async ({ page }) => {
  await tagsRendered(page);
  await page.click("#rehide");
  await expect.poll(() => page.evaluate(() => window.__pw.signals.tags[1].unseen)).toBe("second!");
  await page.locator("#tags .tag").nth(1).click();
  await expect.poll(() => picked(page)).toBe("second!");
  await page.locator("#tags .tag").nth(0).click();
  await expect.poll(() => picked(page)).toBe("first!");
});

test("a row the plan takes over from the document keeps no captured text", { tag: "@report" }, async ({ page }) => {
  await tagsRendered(page);
  await expect(page.locator("#tags .tag").nth(1)).toHaveAttribute("data-pw-captures", /second/);
  await page.click("#rehide");
  await expect.poll(() => page.evaluate(() => window.__pw.signals.tags[1].unseen)).toBe("second!");
  await expect(page.locator("#tags .tag").nth(1)).not.toHaveAttribute("data-pw-captures");
});

test("a row a plan made is pressed with what its row holds, then and after it changes", { tag: "@report" }, async ({ page }) => {
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

// --- a planned row that is one element is its own anchor (ADR-0438) ---------

/** The nodes of the element `id` holds, each by its kind: an element by its
 * tag, a comment by its text. */
const childShape = (page, id) =>
  page.evaluate((id) => [...document.getElementById(id).childNodes].map((n) => (n.nodeType === 1 ? n.tagName : n.nodeType === 8 ? `<!--${n.data}-->` : "text")), id);

/** The comments `id` holds, none of a row's: its list's two anchors. */
const listAnchors = (shape) => shape.filter((s) => s.startsWith("<!--"));

test("a thousand rows are a thousand elements, each its own anchor, between their list's two anchors", async ({ page }) => {
  await open(page);
  const only = (shape, rows) => {
    expect(shape.length).toBe(rows + 2);
    expect(listAnchors(shape)).toEqual(["<!--pw:s6-->", "<!--pw:e6-->"]);
    expect(shape.filter((s) => s === "TR").length).toBe(rows);
  };
  await created(page);
  only(await childShape(page, "tbody"), 1000);
  // As each way a list changes leaves it: added to, moved, one taken out,
  // changed in place, replaced, cleared.
  await page.click("#add");
  await counted(page, 2000);
  await page.click("#swaprows");
  await page.waitForFunction(() => document.getElementById("tbody").children[1].querySelector(".id").textContent === "999");
  await page.locator("#tbody tr").nth(3).locator(".remove").click();
  await counted(page, 1999);
  await page.click("#update");
  await expect(page.locator("#tbody tr").nth(0).locator(".select")).toContainText("!!!");
  only(await childShape(page, "tbody"), 1999);
  await page.click("#run");
  await page.waitForFunction(() => document.getElementById("tbody").children.length === 1000 && document.getElementById("tbody").children[0].querySelector(".id").textContent === "2001");
  only(await childShape(page, "tbody"), 1000);
  await page.click("#clear");
  await counted(page, 0);
  only(await childShape(page, "tbody"), 0);
  expect(page.errors).toEqual([]);
});

/** The names the fixture's table of cards shows, in order. */
const cards = (page) => page.locator("#cards .cname").allTextContents();

test("a document's rows that are their own anchors carry their keys on their elements", async ({ page }) => {
  await open(page, "/events");
  const shape = await childShape(page, "cards");
  expect(shape.filter((s) => s === "TR").length).toBe(3);
  expect(listAnchors(shape).length).toBe(2);
  expect(listAnchors(shape).every((s) => /^<!--pw:[se]\d+-->$/.test(s))).toBe(true);
  // Each its key, a host's token, its list's part before it.
  const keys = await page.evaluate(() => [...document.getElementById("cards").children].map((tr) => tr.getAttribute("data-pw-row")));
  expect(keys.every((k) => /^\d+@[A-Za-z0-9_-]{16}$/.test(k))).toBe(true);
  expect(new Set(keys).size).toBe(3);
  expect(await cards(page)).toEqual(["one", "two", "three"]);
});

/** The fixture's cards, their block's first render on the device done: the
 * document's rows made again by the plan, once (ADR-0421, decision 4). */
async function cardsRendered(page) {
  await open(page, "/events");
  await page.click("#rename-card");
  await expect(page.locator("#cards .cname").nth(1)).toHaveText("renamed");
}

/** Each card's element marked with its name, on the element itself. */
const markCards = (page) => page.evaluate(() => [...document.getElementById("cards").children].forEach((tr) => (tr.mark = tr.querySelector(".cname").textContent)));
const cardMarks = (page) => page.evaluate(() => [...document.getElementById("cards").children].map((tr) => tr.mark ?? ""));

test("rows that are their own anchors, morphed with the block around them, keep their elements by their keys", async ({ page }) => {
  await cardsRendered(page);
  await markCards(page);
  // The cards moved, and the text around them changed since the block last
  // rendered (the deck, set in place): the block is morphed whole, and each
  // row is the element it was, by its key.
  await page.click("#shuffle");
  await expect(page.locator("#deck")).toHaveText("1");
  expect(await cards(page)).toEqual(["three", "renamed", "one"]);
  expect(await cardMarks(page)).toEqual(["three", "renamed", "one"]);
  await page.click("#deal");
  await expect(page.locator("#deck")).toHaveText("2");
  await page.click("#shuffle");
  await expect(page.locator("#deck")).toHaveText("3");
  expect(await cards(page)).toEqual(["one", "renamed", "three"]);
  expect(await cardMarks(page)).toEqual(["one", "renamed", "three"]);
  expect(listAnchors(await childShape(page, "cards")).length).toBe(2);
  expect(page.errors).toEqual([]);
});

test("rows a morph kept as their own anchors are set by their plan after: renaming one parses no row", async ({ page }) => {
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
  await cardsRendered(page);
  await page.click("#shuffle");
  await expect(page.locator("#deck")).toHaveText("1");
  await markCards(page);
  await page.evaluate(() => (window.__parsed = []));
  // Its rows now the document's as the morph left them: found by their keys
  // on their elements, and each set by its holes.
  await page.click("#rename-first-card");
  await expect(page.locator("#cards .cname").nth(2)).toHaveText("renamed");
  const parsed = await page.evaluate(() => window.__parsed.join(""));
  expect(parsed.includes("<tr")).toBe(false);
  expect(await cards(page)).toEqual(["three", "renamed", "renamed"]);
  expect(await cardMarks(page)).toEqual(["three", "renamed", "one"]);
  await expect(page.locator("#cards tr").nth(2)).toHaveClass("renamed");
  // And pressed, its handler is given what its row holds now, and the row
  // beside it what it holds.
  await page.locator("#cards .cpick").nth(2).click();
  await expect(page.locator("#picked")).toHaveText("renamed");
  await page.locator("#cards .cpick").nth(0).click();
  await expect(page.locator("#picked")).toHaveText("three");
  expect(page.errors).toEqual([]);
});

test("rows that are their own anchors and whose plan a parser cannot make are parsed and morphed as text", async ({ page }) => {
  // Each row writes a `<form>` in a `<form>`, whose start tag the parser
  // drops, its hole with it: no plan holds that row, and its rows are
  // written as text, each its one element all the same.
  await open(page, "/events");
  const names = () => page.locator("#dropped .dname").allTextContents();
  const rows = async (n) => {
    const shape = await childShape(page, "dropped");
    expect(shape.length).toBe(n + 2);
    expect(shape.filter((s) => s === "TR").length).toBe(n);
  };
  await rows(2);
  // The list's first render on the device: the document's rows morphed.
  await page.click("#add-dropped");
  await expect(page.locator("#dropped .dname")).toHaveCount(3);
  await rows(3);
  // A row added after, parsed.
  await page.click("#add-dropped");
  await expect(page.locator("#dropped .dname")).toHaveCount(4);
  await rows(4);
  expect(await names()).toEqual(["one", "two", "more", "more"]);
  // A row changed, morphed where it is.
  await page.evaluate(() => [...document.getElementById("dropped").children].forEach((tr, i) => (tr.mark = String(i))));
  await page.click("#rename-dropped");
  await expect(page.locator("#dropped .dname").nth(1)).toHaveText("renamed");
  expect(await names()).toEqual(["one", "renamed", "more", "more"]);
  expect(await page.evaluate(() => [...document.getElementById("dropped").children].map((tr) => tr.mark))).toEqual(["0", "1", "2", "3"]);
  await rows(4);
  expect(page.errors).toEqual([]);
});

test("a block shown again indexes nothing inside its rows that are their own anchors", { tag: "@report" }, async ({ page }) => {
  await open(page, "/events");
  await page.click("#hide-cards");
  await expect(page.locator("#cards")).toHaveCount(0);
  const hidden = await page.evaluate(() => window.__pw.indexSize());
  await page.click("#show-cards");
  await expect(page.locator("#cards tr")).toHaveCount(3);
  const shown = await page.evaluate(() => window.__pw.indexSize());
  // The ranges of the block's text and of its list, and none of its rows'
  // parts: a row its own anchor is passed over whole.
  expect(shown - hidden).toBe(2);
});

/** Rows of every shape a row's own anchor is read for, as the compiler
 * writes a row (ADR-0438): an element with its holes in its tag, its
 * children and its end, of many kinds, a hole's value filled in. */
function shapes(count) {
  let s = 20261112;
  const next = () => ((s = (s * 16807) % 2147483647) - 1) / 2147483646;
  const pick = (xs) => xs[Math.floor(next() * xs.length)];
  const ROOTS = ["tr", "tr", "li", "div", "span", "p", "a", "button", "td", "th", "h2", "dd", "dt", "section", "ul", "label", "img", "input", "em"];
  const INSIDE = ["span", "b", "div", "p", "a", "button", "li", "td", "th", "tr", "tbody", "table", "input", "img", "br", "h3", "ul", "dd", "dt", "em", "form", "label", "section", "caption", "col", "colgroup", "thead", "tfoot"];
  const VOIDS = new Set(["img", "input", "br", "col"]);
  const out = [];
  for (let i = 0; i < count; i++) {
    const body = [];
    let html = "";
    const st = (text) => {
      body.push({ chunk: "static", value: text });
      html += text;
    };
    const element = (tag, depth) => {
      st(`<${tag} data-pw="${depth}"`);
      if (next() < 0.4) {
        st(" ");
        body.push({ chunk: "dynamic", value: { part: "attribute", id: i, owner: 0, name: "class", value: "row.name", context: "attribute" } });
        html += 'class="v"';
      }
      st(">");
      if (VOIDS.has(tag)) return;
      for (let k = Math.floor(next() * 3); k >= 0; k--) {
        const roll = next();
        if (roll < 0.3) {
          body.push({ chunk: "dynamic", value: { part: "text", id: i, value: "row.name", context: "text" } });
          html += "v";
        } else if (roll < 0.4) st(pick(["x", " "]));
        else if (depth < 2) element(tag === "tr" && next() < 0.8 ? pick(["td", "th"]) : pick(INSIDE), depth + 1);
      }
      st(`</${tag}>`);
    };
    element(pick(ROOTS), 0);
    out.push({ body, html });
  }
  return out;
}

test("each row the packaging says is its own anchor is one element as this browser parses it", async ({ page }) => {
  // The packaging's reading is the host's and the device's word for what a
  // row is (`rowElement`); the browser's parse is what the document holds.
  const all = shapes(3000);
  const one = all.filter(({ body }) => rowElement(body) === null).map(({ html }) => html);
  const not = all.filter(({ body }) => rowElement(body) !== null).map(({ html }) => html);
  expect(one.length > 500 && not.length > 500).toBe(true);
  await page.setContent("<!doctype html><title>rows</title>");
  const parsedAs = (rows) =>
    page.evaluate((rows) => rows.map((html) => {
      const t = document.createElement("template");
      t.innerHTML = html;
      const c = t.content;
      return c.childNodes.length === 1 && c.firstChild.nodeType === 1;
    }), rows);
  const ones = await parsedAs(one);
  expect(one.filter((_, i) => !ones[i])).toEqual([]);
  // And of the rows it says no to, the parser makes some more than one node:
  // what it refuses is what a parser breaks, among others it keeps whole.
  const nots = await parsedAs(not);
  expect(nots.some((x) => !x)).toBe(true);
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

// --- rows the page's own code found where they were (ADR-0439) -------------

/** The fixture's list it reorders every way a press does, as it starts. */
const FIRST_ORDER = ["one", "two", "three", "four", "five", "six"].map((name, i) => ({ id: i + 1, name }));

/** **Each press of the fixture's reordered list** (`examples/rows/events.pw`):
 * what it makes of the list, as its handler does, and whether it is pressed
 * on a list of `n` rows, where its handler's places are inside the list. */
const ORDER_PRESSES = {
  reverse: { can: () => true, made: (xs) => [...xs].reverse() },
  turn: { can: (n) => n > 0, made: (xs) => [...xs.slice(1), ...xs.slice(0, 1)] },
  "turn-back": { can: (n) => n > 0, made: (xs) => [...xs.slice(-1), ...xs.slice(0, -1)] },
  "turn-three": { can: (n) => n >= 3, made: (xs) => [...xs.slice(3), ...xs.slice(0, 3)] },
  swap: {
    can: (n) => n >= 3,
    made: (xs) => {
      const out = [...xs];
      [out[1], out[xs.length - 2]] = [out[xs.length - 2], out[1]];
      return out;
    },
  },
  move: { can: (n) => n >= 5, made: (xs) => [...xs.slice(0, 1), ...xs.slice(2, 5), ...xs.slice(1, 2), ...xs.slice(5)] },
  drop: { can: (n) => n >= 3, made: (xs) => [...xs.slice(0, 2), ...xs.slice(3)] },
  insert: { can: (n) => n >= 3, made: (xs, fresh) => [...xs.slice(0, 3), fresh, ...xs.slice(3)] },
  sort: { can: () => true, made: (xs) => [...xs].sort((a, b) => a.id - b.id) },
  rename: { can: () => true, made: (xs) => xs.map((x) => (x.id === 3 ? { id: 3, name: "renamed" } : x)) },
  clear: { can: () => true, made: () => [] },
  refill: { can: () => true, made: () => FIRST_ORDER },
};

/** The reordered list as the document shows it: the table's rows, `[id,
 * name, its element's mark]`, the list's items, `[name, mark]`, and the
 * general code's, `[text, mark]`. */
const orderShown = (page) =>
  page.evaluate(() => ({
    rows: [...document.querySelectorAll("#order tr")].map((tr) => [tr.querySelector(".oid").textContent, tr.querySelector(".oname").textContent, tr.mark ?? null]),
    items: [...document.querySelectorAll("#order-items li")].map((li) => [li.textContent, li.mark ?? null]),
    kept: [...document.querySelectorAll("#order-kept li")].map((li) => [li.textContent, li.mark ?? null]),
  }));

/** Each row element of the reordered list marked, on itself, with its id:
 * a row kept or moved is the element it was, and a new one has no mark. */
const orderMarked = (page) =>
  page.evaluate(() => {
    for (const tr of document.querySelectorAll("#order tr")) tr.mark = tr.querySelector(".oid").textContent;
    const ids = [...document.querySelectorAll("#order tr")].map((tr) => tr.mark);
    document.querySelectorAll("#order-items li").forEach((li, i) => (li.mark = ids[i]));
    document.querySelectorAll("#order-kept li").forEach((li, i) => (li.mark = ids[i]));
  });

/** What the document should show of `order` after a press, each row that
 * was before it its element marked with its id. */
const orderSaid = (order, was) => {
  const before = new Set(was.map((o) => String(o.id)));
  const mark = (o) => (before.has(String(o.id)) ? String(o.id) : null);
  return {
    rows: order.map((o) => [String(o.id), o.name, mark(o)]),
    items: order.map((o) => [o.name, mark(o)]),
    kept: order.map((o) => [`${o.name} 1`, mark(o)]),
  };
};

test("rows reordered every way a press does are where their list says, each the element it was", async ({ page }) => {
  // Eighty presses of a seeded walk, the same in every engine: each compared
  // whole, a table's rows each their own anchor and a list's between their
  // comments, every row that was before the press the element it was. The
  // first press after the document morphs each block whole, its rows made
  // again: the host's tokens are not the device's.
  test.setTimeout(120_000);
  await open(page, "/events");
  await page.click("#order-reverse");
  let order = ORDER_PRESSES.reverse.made(FIRST_ORDER);
  const ids = (shown) => JSON.stringify(shown.rows.map(([id]) => id));
  await expect.poll(async () => ids(await orderShown(page))).toBe(JSON.stringify(order.map((o) => String(o.id))));
  let seed = 4321;
  const next = (n) => (seed = (seed * 16807) % 2147483647) % n;
  const names = Object.keys(ORDER_PRESSES);
  let fresh = 100;
  for (let step = 0; step < 80; step++) {
    const can = names.filter((name) => ORDER_PRESSES[name].can(order.length));
    const name = can[next(can.length)];
    await orderMarked(page);
    const was = order;
    order = ORDER_PRESSES[name].made(order, { id: fresh, name: `new ${fresh}` });
    if (name === "insert") fresh += 1;
    await page.click(`#order-${name}`);
    const said = orderSaid(order, was);
    await expect.poll(async () => JSON.stringify(await orderShown(page)), { timeout: 10_000, intervals: [16, 50, 100, 250], message: `after ${name}, press ${step}` }).toBe(JSON.stringify(said));
  }
  expect(page.errors).toEqual([]);
});

test("a list its own code keeps is placed by what that code found, each press after the first", { tag: "@report" }, async ({ page }) => {
  // The first press after the document morphs the block whole; each one
  // after places each of the two lists by what its own code found: no key
  // is looked up to say which rows stay, moved, went or came (ADR-0439).
  await open(page, "/events");
  await page.click("#order-reverse");
  await page.waitForFunction(() => window.__pw.log.some((l) => /^signal order -> block/.test(l)));
  let order = ORDER_PRESSES.reverse.made(FIRST_ORDER);
  let fresh = 100;
  for (const name of ["turn", "swap", "move", "drop", "insert", "sort", "rename", "turn-three", "clear", "refill", "turn-back", "reverse"]) {
    const before = await page.evaluate(() => window.__pw.found ?? 0);
    await orderMarked(page);
    const was = order;
    order = ORDER_PRESSES[name].made(order, { id: fresh, name: `new ${fresh}` });
    if (name === "insert") fresh += 1;
    await page.click(`#order-${name}`);
    await page.waitForFunction((b) => (window.__pw.found ?? 0) >= b + 2, before);
    expect(await page.evaluate(() => window.__pw.found), `found after ${name}`).toBe(before + 2);
    await expect.poll(async () => JSON.stringify(await orderShown(page)), { timeout: 10_000, message: `after ${name}` }).toBe(JSON.stringify(orderSaid(order, was)));
  }
  expect(page.errors).toEqual([]);
});

/** The fewest rows of `was` that must move for it to read `now`, of the rows
 * both hold, by their ids: those outside a longest run of them in order. */
function fewestMoved(was, now) {
  const at = new Map(was.map((o, i) => [o.id, i]));
  const places = now.map((o) => at.get(o.id) ?? -1).filter((p) => p >= 0);
  const best = places.map(() => 1);
  places.forEach((v, i) => {
    for (let j = 0; j < i; j++) if (places[j] < v) best[i] = Math.max(best[i], best[j] + 1);
  });
  return places.length - Math.max(0, ...best);
}

test("rows its list's own code found out of their order are as few moved as leave the rest in it", { tag: "@report" }, async ({ page }) => {
  // Each press of the fixture's reordered list, after the one that morphs
  // its blocks: its two lists its own code keeps each move the fewest rows
  // that leave the rest in order, and a row put in or gone moves none
  // (ADR-0439). And the large list's swap of two rows moves those two.
  await open(page, "/events");
  await page.click("#order-reverse");
  await page.waitForFunction(() => window.__pw.log.some((l) => /^signal order -> block/.test(l)));
  let order = ORDER_PRESSES.reverse.made(FIRST_ORDER);
  let fresh = 100;
  // Each press one that changes the list: a renamed row while it is there.
  for (const name of ["swap", "turn", "rename", "reverse", "move", "drop", "insert", "turn-three", "sort", "turn-back", "swap"]) {
    const before = await page.evaluate(() => [window.__pw.found ?? 0, window.__pw.moves ?? 0]);
    const was = order;
    order = ORDER_PRESSES[name].made(order, { id: fresh, name: `new ${fresh}` });
    if (name === "insert") fresh += 1;
    await page.click(`#order-${name}`);
    await page.waitForFunction((b) => (window.__pw.found ?? 0) >= b + 2, before[0]);
    const moved = (await page.evaluate(() => window.__pw.moves ?? 0)) - before[1];
    expect(moved, `rows moved by ${name}, in each of two lists`).toBe(2 * fewestMoved(was, order));
  }
  await open(page);
  await created(page);
  await page.click("#update");
  await expect(page.locator("#tbody tr").nth(0).locator(".select")).toContainText("!!!");
  const before = await page.evaluate(() => window.__pw.moves ?? 0);
  await page.click("#swaprows");
  await page.waitForFunction(() => document.getElementById("tbody").children[1].querySelector(".id").textContent === "999");
  expect((await page.evaluate(() => window.__pw.moves ?? 0)) - before, "rows moved by a swap of two").toBe(2);
  expect(page.errors).toEqual([]);
});
