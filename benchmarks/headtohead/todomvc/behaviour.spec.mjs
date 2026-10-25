// TodoMVC's specification (`app-spec.md`, tastejs/todomvc@ff43b02e), one
// test per behaviour it states, run on every stack the head-to-head measures
// in three engines (ADR-XXXX, "the head-to-head"): TodoMVC in Pleris and
// TodoMVC's own React and Svelte apps. The same tests for each; a test may
// know only the stack's address and how it says it is ready (Pleris's
// runtime sets `data-pw-ready`; a client-rendered app is ready when its field
// is in the document). Where an app does not do what the specification asks,
// its test fails, and `scripts/head_to_head_behaviour.py` reports it as that
// app's gap, against the expectations it holds (an app that stops doing what
// it did fails the run).
//
// TodoMVC in Pleris's own suite (`spikes/own-renderer/e2e/todomvc.spec.mjs`)
// asks more of it (its storage's shape, what no server is told); these are
// the specification's behaviours alone, written for any app.
import { expect, test as base } from "@playwright/test";

const test = base.extend({
  stack: ["", { option: true }],
  appPath: ["/", { option: true }],
});

const ONE = "buy some cheese";
const TWO = "feed the cat";
const THREE = "book a doctors appointment";

async function ready(page, stack) {
  if (stack === "pleris") {
    await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  } else {
    await page.locator(".new-todo").waitFor({ state: "visible" });
  }
}

async function open(page, stack, appPath, hash = "") {
  await page.goto(`${appPath}${hash}`);
  await ready(page, stack);
}

const items = (page) => page.locator(".todo-list li");
const field = (page) => page.locator(".new-todo");
const link = (page, name) => page.locator(".filters a", { hasText: name });
const label = (page, n) => items(page).nth(n).locator("label").evaluate((l) => l.textContent);

async function add(page, ...titles) {
  for (const title of titles) {
    const before = await items(page).count();
    await field(page).fill(title);
    await field(page).press("Enter");
    await expect(items(page)).toHaveCount(before + 1);
  }
}

test.beforeEach(async ({ page, stack, appPath }) => {
  await open(page, stack, appPath);
});

// --- No todos -----------------------------------------------------------------

test("no todos: .main and .footer are hidden", async ({ page }) => {
  await expect(items(page)).toHaveCount(0);
  await expect(page.locator(".main")).toBeHidden();
  await expect(page.locator(".footer")).toBeHidden();
  await expect(page.locator("h1")).toHaveText("todos");
});

// --- New todo -------------------------------------------------------------------

test("new todo: the field is focused when the page loads", async ({ page }) => {
  await expect(field(page)).toBeFocused();
});

test("new todo: Enter adds it, trimmed, at the bottom, and clears the field", async ({ page }) => {
  await add(page, ONE);
  await field(page).fill(`   ${TWO}   `);
  await field(page).press("Enter");
  await expect(items(page)).toHaveCount(2);
  expect(await label(page, 0)).toBe(ONE);
  expect(await label(page, 1)).toBe(TWO);
  await expect(field(page)).toHaveValue("");
  await expect(page.locator(".main")).toBeVisible();
  await expect(page.locator(".footer")).toBeVisible();
});

test("new todo: an empty or blank entry adds nothing", async ({ page }) => {
  await field(page).press("Enter");
  await field(page).fill("    ");
  await field(page).press("Enter");
  await expect(items(page)).toHaveCount(0);
});

// --- Mark all as complete ----------------------------------------------------------

test("mark all: completes every todo, and again clears them", async ({ page }) => {
  await add(page, ONE, TWO, THREE);
  await page.locator(".toggle-all").check({ force: true });
  await expect(page.locator(".todo-list li.completed")).toHaveCount(3);
  await page.locator(".toggle-all").uncheck({ force: true });
  await expect(page.locator(".todo-list li.completed")).toHaveCount(0);
});

test("mark all: its checkbox follows the todos", async ({ page }) => {
  await add(page, ONE, TWO);
  for (const n of [0, 1]) await items(page).nth(n).locator(".toggle").check();
  await expect(page.locator(".toggle-all")).toBeChecked();
  await items(page).nth(0).locator(".toggle").uncheck();
  await expect(page.locator(".toggle-all")).not.toBeChecked();
});

// --- Item --------------------------------------------------------------------------

test("item: its checkbox completes it, and again makes it active", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/\bcompleted\b/);
  await expect(items(page).nth(1)).not.toHaveClass(/\bcompleted\b/);
  await items(page).nth(0).locator(".toggle").uncheck();
  await expect(items(page).nth(0)).not.toHaveClass(/\bcompleted\b/);
});

test("item: a double press on its label edits it, the field holding its title, focused", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(1)).toHaveClass(/\bediting\b/);
  const edit = items(page).nth(1).locator(".edit");
  await expect(edit).toBeVisible();
  await expect(edit).toBeFocused();
  await expect(edit).toHaveValue(TWO);
});

test("item: its destroy button shows on hover and removes it", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).hover();
  await expect(items(page).nth(0).locator(".destroy")).toBeVisible();
  await items(page).nth(0).locator(".destroy").click();
  await expect(items(page)).toHaveCount(1);
  expect(await label(page, 0)).toBe(TWO);
});

// --- Editing -------------------------------------------------------------------------

test("editing: Enter saves the edit, trimmed", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await items(page).nth(0).locator(".edit").fill(`   ${THREE}   `);
  await items(page).nth(0).locator(".edit").press("Enter");
  await expect(items(page).nth(0)).not.toHaveClass(/\bediting\b/);
  expect(await label(page, 0)).toBe(THREE);
  expect(await label(page, 1)).toBe(TWO);
});

test("editing: leaving the field saves the edit", async ({ page }) => {
  await add(page, ONE);
  await items(page).nth(0).locator("label").dblclick();
  await items(page).nth(0).locator(".edit").fill(THREE);
  await items(page).nth(0).locator(".edit").blur();
  await expect(items(page).nth(0)).not.toHaveClass(/\bediting\b/);
  expect(await label(page, 0)).toBe(THREE);
});

test("editing: an edit left empty destroys the todo", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await items(page).nth(0).locator(".edit").fill("   ");
  await items(page).nth(0).locator(".edit").press("Enter");
  await expect(items(page)).toHaveCount(1);
  expect(await label(page, 0)).toBe(TWO);
});

test("editing: Escape discards the edit", async ({ page }) => {
  await add(page, ONE);
  await items(page).nth(0).locator("label").dblclick();
  await items(page).nth(0).locator(".edit").fill(THREE);
  await items(page).nth(0).locator(".edit").press("Escape");
  await expect(items(page).nth(0)).not.toHaveClass(/\bediting\b/);
  expect(await label(page, 0)).toBe(ONE);
});

test("editing: the todo's other controls are hidden while it is edited", async ({ page }) => {
  await add(page, ONE);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".toggle")).toBeHidden();
  await expect(items(page).nth(0).locator("label")).toBeHidden();
});

// --- Counter ---------------------------------------------------------------------------

test("counter: the number left in a <strong>, then 'item left' or 'items left'", async ({ page }) => {
  await add(page, ONE);
  await expect(page.locator(".todo-count")).toHaveText(/^\s*1\s+item left\s*$/);
  await expect(page.locator(".todo-count strong")).toHaveText("1");
  await add(page, TWO);
  await expect(page.locator(".todo-count")).toHaveText(/^\s*2\s+items left\s*$/);
  await expect(page.locator(".todo-count strong")).toHaveText("2");
});

// --- Clear completed -------------------------------------------------------------------

test("clear completed: shown only when a todo is completed, and removes them", async ({ page }) => {
  await add(page, ONE, TWO, THREE);
  await expect(page.locator(".clear-completed")).toBeHidden();
  await items(page).nth(1).locator(".toggle").check();
  await expect(page.locator(".clear-completed")).toBeVisible();
  await page.locator(".clear-completed").click();
  await expect(items(page)).toHaveCount(2);
  expect(await label(page, 0)).toBe(ONE);
  expect(await label(page, 1)).toBe(THREE);
  await expect(page.locator(".clear-completed")).toBeHidden();
});

// --- Persistence -------------------------------------------------------------------------

test("persistence: the todos and their states survive a reload", async ({ page, stack }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/\bcompleted\b/);
  await page.reload();
  await ready(page, stack);
  await expect(items(page)).toHaveCount(2);
  expect(await label(page, 0)).toBe(ONE);
  await expect(items(page).nth(0)).toHaveClass(/\bcompleted\b/);
  await expect(items(page).nth(1)).not.toHaveClass(/\bcompleted\b/);
});

// --- Routing -----------------------------------------------------------------------------

test("routing: #/active, #/completed and #/ filter the list, and select their link", async ({ page }) => {
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await link(page, "Active").click();
  await expect(items(page)).toHaveCount(2);
  await expect(link(page, "Active")).toHaveClass(/\bselected\b/);
  await link(page, "Completed").click();
  await expect(items(page)).toHaveCount(1);
  await expect(link(page, "Completed")).toHaveClass(/\bselected\b/);
  await link(page, "All").click();
  await expect(items(page)).toHaveCount(3);
  await expect(link(page, "All")).toHaveClass(/\bselected\b/);
});

test("routing: a page opened at #/completed shows that filter", async ({ page, stack, appPath }) => {
  // A load, not a change of the fragment in the page already open.
  await page.goto("about:blank");
  await open(page, stack, appPath, "#/completed");
  await expect(link(page, "Completed")).toHaveClass(/\bselected\b/);
  await field(page).fill(ONE);
  await field(page).press("Enter");
  // The todo was added (the counter counts it) and, being active, is not
  // shown among the completed ones.
  await expect(page.locator(".todo-count")).toContainText("1");
  await expect(items(page)).toHaveCount(0);
});

test("routing: the back button returns to the filter before", async ({ page }) => {
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await link(page, "Active").click();
  await expect(items(page)).toHaveCount(1);
  await link(page, "Completed").click();
  await expect(items(page)).toHaveCount(1);
  await expect(link(page, "Completed")).toHaveClass(/\bselected\b/);
  await page.goBack();
  await expect(link(page, "Active")).toHaveClass(/\bselected\b/);
  expect(await label(page, 0)).toBe(TWO);
});
