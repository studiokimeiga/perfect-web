// TodoMVC in Pleris (track `todomvc`, docs/PARALLEL.md, W7), in three
// engines: every behaviour TodoMVC's own spec (`app-spec.md`) states, each
// with its test, and each test with a control where it has one.
//
// The program is `examples/todomvc`, built by `todomvc.sh` into
// `dist-todomvc` and served on a host per engine (`TODOMVC_PORTS`). The
// todos are the device's (track `device-store`): kept in the browser's own
// storage, and each test is its own browser context, so its own storage: the
// tests need no order and no clearing.
//
// The spec's sections, in its order:
// - No todos: `.main` and `.footer` are hidden.
// - New todo: Enter creates it, trimmed, never empty; the field is cleared
//   and focused when the page loads.
// - Mark all as complete.
// - Item: its checkbox, a double press on its label, hovering for `.destroy`.
// - Editing: Enter and blur save, trimmed; empty destroys; Escape discards.
// - Counter: "1 item left", "2 items left".
// - Clear completed: shown only where one is.
// - Persistence: `localStorage`, under `todos-pleris`, a list of
//   `{id, title, completed}`, which no server is told.
// - Routing: `#/`, `#/active`, `#/completed`, filtered by the model, the
//   `selected` class on the links, and a fragment kept on reload.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { TODOMVC_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${TODOMVC_PORTS[testInfo.project.name]}`);
  },
});

const ONE = "buy some cheese";
const TWO = "feed the cat";
const THREE = "book a doctors appointment";

async function open(page, hash = "") {
  await page.goto(`/${hash}`);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

const items = (page) => page.locator(".todo-list li");
const field = (page) => page.locator(".new-todo");
const link = (page, name) => page.locator(".filters a", { hasText: name });

/** Todos added by typing each and pressing Enter, as a person does. */
async function add(page, ...titles) {
  for (const title of titles) {
    const before = await items(page).count();
    await field(page).fill(title);
    await field(page).press("Enter");
    await expect(items(page)).toHaveCount(before + 1);
  }
}

/** A todo's label as the document has it, spaces and all. */
const written = (page, n) => items(page).nth(n).locator("label").evaluate((l) => l.textContent);

// --- No todos --------------------------------------------------------------

test("with no todos, main and footer are hidden, though they are there", async ({ page }) => {
  await open(page);
  await expect(page.locator(".main")).toHaveCount(1);
  await expect(page.locator(".footer")).toHaveCount(1);
  await expect(page.locator(".main")).toBeHidden();
  await expect(page.locator(".footer")).toBeHidden();
  await expect(items(page)).toHaveCount(0);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
  // The control: the header is not hidden.
  await expect(page.locator("h1")).toHaveText("todos");
  await expect(field(page)).toBeVisible();
});

test("with a todo, main and footer are shown; with none again, hidden again", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE);
  await expect(page.locator(".main")).toBeVisible();
  await expect(page.locator(".footer")).toBeVisible();
  await items(page).first().hover();
  await items(page).first().locator(".destroy").click();
  await expect(items(page)).toHaveCount(0);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
  await expect(page.locator(".main")).toBeHidden();
  await expect(page.locator(".footer")).toBeHidden();
});

test("served without script: the header, the field, and a hidden main and footer", async ({
  browser,
  baseURL,
}) => {
  const off = await browser.newContext({ baseURL, javaScriptEnabled: false });
  const page = await off.newPage();
  await page.goto("/");
  await expect(page.locator("h1")).toHaveText("todos");
  await expect(field(page)).toBeVisible();
  await expect(page.locator(".main")).toBeHidden();
  await expect(page.locator(".footer")).toBeHidden();
  await off.close();
});

// --- New todo --------------------------------------------------------------

test("the field is focused when the page is loaded", async ({ page }) => {
  await open(page);
  await expect(field(page)).toBeFocused();
});

test("Enter creates the todo, appends it, and clears the field", async ({ page }) => {
  await open(page);
  await add(page, ONE);
  await expect(field(page)).toHaveValue("");
  await expect(items(page).first().locator("label")).toHaveText(ONE);
  // The control: the field takes the next one, and it is typed in as before.
  await field(page).pressSequentially(TWO);
  await expect(field(page)).toHaveValue(TWO);
  await field(page).press("Enter");
  await expect(items(page)).toHaveCount(2);
});

test("new todos are appended to the bottom of the list", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
});

test("the text is trimmed", async ({ page }) => {
  await open(page);
  await add(page, `    ${ONE}    `);
  expect(await written(page, 0)).toBe(ONE);
});

test("a title that is nothing once trimmed creates no todo, and is left to be corrected", async ({
  page,
}) => {
  await open(page);
  await field(page).press("Enter");
  await field(page).fill("    ");
  await field(page).press("Enter");
  await page.waitForTimeout(300);
  await expect(items(page)).toHaveCount(0);
  await expect(field(page)).toHaveValue("    ");
  // The control: what follows the blanks is a title.
  await field(page).fill("   a   ");
  await field(page).press("Enter");
  await expect(items(page)).toHaveCount(1);
  expect(await written(page, 0)).toBe("a");
});

test("a title is at most a thousand characters", async ({ page }) => {
  await open(page);
  await field(page).fill("x".repeat(1001));
  await field(page).press("Enter");
  await page.waitForTimeout(300);
  await expect(items(page)).toHaveCount(0);
  await field(page).fill("x".repeat(1000));
  await field(page).press("Enter");
  await expect(items(page)).toHaveCount(1);
});

test("a title that is markup is shown as text", async ({ page }) => {
  await open(page);
  await add(page, "<b>bold</b> & <i>done</i>");
  await expect(items(page).first().locator("label")).toHaveText("<b>bold</b> & <i>done</i>");
  await expect(items(page).first().locator("b")).toHaveCount(0);
});

// --- Mark all as complete ---------------------------------------------------

test("the toggle-all completes every todo, and again clears every one", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await page.locator(".toggle-all").check({ force: true });
  for (const n of [0, 1, 2]) await expect(items(page).nth(n)).toHaveClass(/completed/);
  await expect(page.locator(".todo-count")).toHaveText("0 items left");
  await page.locator(".toggle-all").uncheck({ force: true });
  for (const n of [0, 1, 2]) await expect(items(page).nth(n)).not.toHaveClass(/completed/);
  await expect(page.locator(".todo-count")).toHaveText("3 items left");
});

test("the toggle-all completes the rest where some are completed", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  await expect(items(page).nth(2)).not.toHaveClass(/completed/);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
  await page.locator(".toggle-all").click({ force: true });
  for (const n of [0, 1, 2]) await expect(items(page).nth(n)).toHaveClass(/completed/);
  await expect(page.locator(".toggle-all")).toBeChecked();
});

// A checkbox a person has pressed holds its own state: the attribute the page
// patches no longer moves it. The page says what the box is, whoever pressed
// it last, so a patch sets the box's state as well as the attribute (the
// runtime's `setAttributeAt`). The official suite asks it of the toggle-all
// ("should update state when items are completed / cleared").
test("the toggle-all, pressed, still follows the todos: unchecked when one is cleared, checked when all are", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO);
  const all = page.locator(".toggle-all");
  await expect(all).not.toBeChecked();
  await all.check({ force: true });
  await expect(all).toBeChecked();
  // The press checks the box, and completes the todos, at once.
  for (const n of [0, 1]) await expect(items(page).nth(n)).toHaveClass(/completed/);
  await items(page).nth(0).locator(".toggle").uncheck();
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(all).not.toBeChecked();
  await items(page).nth(0).locator(".toggle").check();
  await expect(all).toBeChecked();
});

test("the toggle-all, pressed, is not checked for a todo added once the completed are cleared", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE);
  await page.locator(".toggle-all").check({ force: true });
  await expect(page.locator(".toggle-all")).toBeChecked();
  // The native checkbox changes before the device's completed state renders.
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  const clearCompleted = page.locator(".clear-completed");
  await expect(clearCompleted).toBeVisible();
  await clearCompleted.click();
  await expect(items(page)).toHaveCount(0);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
  await add(page, TWO);
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
});

// --- Item ------------------------------------------------------------------

test("a todo's checkbox completes it, toggling the class on its li", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(1)).not.toHaveClass(/completed/);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(1)).toHaveClass(/completed/);
});

test("the same checkbox, pressed again, un-completes it", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await items(page).nth(0).locator(".toggle").uncheck();
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(items(page).nth(1)).not.toHaveClass(/completed/);
});

test("a checkbox another tab cleared is shown cleared, though this tab had pressed it", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO);
  const second = await page.context().newPage();
  await open(second);
  await expect(items(second)).toHaveCount(2);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(second).nth(0)).toHaveClass(/completed/);
  await items(second).nth(0).locator(".toggle").uncheck();
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(items(page).nth(0).locator(".toggle")).not.toBeChecked();
  await expect(items(page).nth(1).locator(".toggle")).not.toBeChecked();
});

test("hovering over a todo shows its remove button, which removes it", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  const destroy = items(page).nth(0).locator(".destroy");
  await expect(destroy).toBeHidden();
  await items(page).nth(0).hover();
  await expect(destroy).toBeVisible();
  await destroy.click();
  await expect(items(page)).toHaveCount(1);
  await expect(items(page).first().locator("label")).toHaveText(TWO);
});

// --- Editing ---------------------------------------------------------------

test("a double press on the label begins editing: a field holds the title, and the other controls are hidden", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  const second = items(page).nth(1);
  await expect(second.locator(".edit")).toHaveCount(0);
  await second.locator("label").dblclick();
  await expect(second).toHaveClass(/editing/);
  await expect(second.locator(".edit")).toBeVisible();
  await expect(second.locator(".edit")).toHaveValue(TWO);
  await expect(second.locator(".toggle")).toBeHidden();
  await expect(second.locator("label")).toBeHidden();
  // The control: no other todo is being edited.
  await expect(items(page).nth(0)).not.toHaveClass(/editing/);
  await expect(items(page).nth(2)).not.toHaveClass(/editing/);
  await expect(page.locator(".edit")).toHaveCount(1);
});

test("the edit field is focused as it appears, and is the one typed in", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(1).locator(".edit")).toBeFocused();
  // Typed in with no click on it: at the end of the title, then Enter.
  await page.keyboard.press("End");
  await page.keyboard.type("!");
  await page.keyboard.press("Enter");
  await expect(items(page).nth(1).locator("label")).toHaveText(`${TWO}!`);
  // Each time a todo is edited, and whichever one.
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeFocused();
  await expect(items(page).nth(1).locator(".edit")).toHaveCount(0);
});

test("Enter saves the edit and leaves editing", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  const second = items(page).nth(1);
  await second.locator("label").dblclick();
  await expect(second.locator(".edit")).toBeVisible();
  await second.locator(".edit").fill("buy some sausages");
  await second.locator(".edit").press("Enter");
  await expect(second.locator("label")).toHaveText("buy some sausages");
  await expect(second).not.toHaveClass(/editing/);
  await expect(page.locator(".edit")).toHaveCount(0);
  await expect(items(page)).toHaveText([ONE, "buy some sausages", THREE]);
});

test("blur saves the edit and leaves editing", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  const second = items(page).nth(1);
  await second.locator("label").dblclick();
  await expect(second.locator(".edit")).toBeVisible();
  await second.locator(".edit").fill("buy some sausages");
  await second.locator(".edit").blur();
  await expect(second.locator("label")).toHaveText("buy some sausages");
  await expect(second).not.toHaveClass(/editing/);
  await expect(items(page)).toHaveText([ONE, "buy some sausages", THREE]);
});

test("clicking away saves the edit too", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").fill("changed");
  await page.locator("h1").click();
  await expect(items(page).nth(0).locator("label")).toHaveText("changed");
  await expect(page.locator(".edit")).toHaveCount(0);
});

test("Enter or blur with nothing changed keeps the todo as it is", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").press("Enter");
  await expect(items(page).nth(0)).not.toHaveClass(/editing/);
  await expect(items(page)).toHaveText([ONE, TWO]);
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(1).locator(".edit")).toBeFocused();
  await items(page).nth(1).locator(".edit").blur();
  await expect(items(page).nth(1)).not.toHaveClass(/editing/);
  await expect(items(page)).toHaveText([ONE, TWO]);
});

test("a completed todo being edited is shown completed, and stays so", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0)).toHaveClass(/editing/);
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").fill("changed");
  await items(page).nth(0).locator(".edit").press("Enter");
  await expect(items(page).nth(0).locator("label")).toHaveText("changed");
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(0)).not.toHaveClass(/editing/);
});

test("the edit is trimmed", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(1).locator(".edit")).toBeVisible();
  await items(page).nth(1).locator(".edit").fill("    buy some sausages    ");
  await items(page).nth(1).locator(".edit").press("Enter");
  await expect(items(page).nth(1).locator("label")).toHaveText("buy some sausages");
  expect(await written(page, 1)).toBe("buy some sausages");
});

test("an edit trimmed to nothing destroys the todo", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(1).locator(".edit")).toBeVisible();
  await items(page).nth(1).locator(".edit").fill("   ");
  await items(page).nth(1).locator(".edit").press("Enter");
  await expect(items(page)).toHaveCount(2);
  await expect(items(page)).toHaveText([ONE, THREE]);
});

test("an edit emptied and left by blur destroys the todo too", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").fill("");
  await items(page).nth(0).locator(".edit").blur();
  await expect(items(page)).toHaveText([TWO]);
});

test("Escape leaves editing and discards the changes", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  const second = items(page).nth(1);
  await second.locator("label").dblclick();
  await expect(second.locator(".edit")).toBeVisible();
  await second.locator(".edit").fill("foo");
  await second.locator(".edit").press("Escape");
  await expect(second).not.toHaveClass(/editing/);
  await expect(page.locator(".edit")).toHaveCount(0);
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
  // Nothing was saved: not now, and not after what leaving the field adds.
  await page.waitForTimeout(300);
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
  // The control: the same todo can be edited again, from what it is.
  await second.locator("label").dblclick();
  await expect(second.locator(".edit")).toHaveValue(TWO);
});

test("editing mode is not persisted: a reload shows no field", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await expect(page.locator(".edit")).toHaveCount(1);
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(page)).toHaveCount(2);
  await expect(page.locator(".edit")).toHaveCount(0);
  await expect(page.locator(".todo-list li.editing")).toHaveCount(0);
});

test("editing another todo saves the first", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").fill("first, changed");
  await items(page).nth(1).locator("label").dblclick();
  await expect(items(page).nth(0).locator("label")).toHaveText("first, changed");
  await expect(items(page).nth(1)).toHaveClass(/editing/);
  await expect(items(page).nth(1).locator(".edit")).toHaveValue(TWO);
  await expect(items(page).nth(0)).not.toHaveClass(/editing/);
});

// --- Counter ---------------------------------------------------------------

test("the counter says how many are left, the number in a strong", async ({ page }) => {
  await open(page);
  await expect(page.locator(".todo-count")).toHaveText("0 items left");
  await add(page, ONE);
  await expect(page.locator(".todo-count")).toHaveText("1 item left");
  await expect(page.locator(".todo-count strong")).toHaveText("1");
  await add(page, TWO);
  await expect(page.locator(".todo-count")).toHaveText("2 items left");
  await expect(page.locator(".todo-count strong")).toHaveText("2");
  // Completed todos are not counted.
  await items(page).nth(0).locator(".toggle").check();
  await expect(page.locator(".todo-count")).toHaveText("1 item left");
  await items(page).nth(1).locator(".toggle").check();
  await expect(page.locator(".todo-count")).toHaveText("0 items left");
});

// --- Clear completed --------------------------------------------------------

test("clear completed shows its text and removes the completed", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  const clearCompleted = page.locator(".clear-completed");
  await expect(clearCompleted).toHaveText("Clear completed");
  await expect(clearCompleted).toBeVisible();
  await clearCompleted.click();
  await expect(items(page)).toHaveText([ONE, THREE]);
});

test("clear completed is hidden where none is completed", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await expect(page.locator(".clear-completed")).toHaveCount(1);
  await expect(page.locator(".clear-completed")).toBeHidden();
  await items(page).nth(0).locator(".toggle").check();
  await expect(page.locator(".clear-completed")).toBeVisible();
  await page.locator(".clear-completed").click();
  await expect(page.locator(".clear-completed")).toBeHidden();
});

// --- Persistence ------------------------------------------------------------

test("the todos are the framework's to keep: they outlive a reload, as they were", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(page)).toHaveText([ONE, TWO]);
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(1)).not.toHaveClass(/completed/);
  await expect(page.locator(".todo-count")).toHaveText("1 item left");
});

test("the todos are kept in the browser's own storage, as TodoMVC's spec names them", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  const kept = await page.evaluate(() =>
    Object.fromEntries(
      Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)).map((k) => [
        k,
        localStorage.getItem(k),
      ]),
    ),
  );
  // One key, the spec's `todos-[framework]`; the spec's three keys an item.
  expect(Object.keys(kept)).toEqual(["todos-pleris"]);
  expect(JSON.parse(kept["todos-pleris"])).toEqual([
    { id: 1, title: ONE, completed: false },
    { id: 2, title: TWO, completed: true },
  ]);
  // Editing is not persisted (the spec): a field open keeps nothing more.
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeFocused();
  expect(await page.evaluate(() => localStorage.length)).toBe(1);
});

test("no request carries a todo: the server is told none", async ({ page }) => {
  const sent = [];
  page.on("request", (r) => sent.push(`${r.method()} ${r.url()} ${r.postData() ?? ""}`));
  await open(page);
  const marker = `todo-${Date.now()}`;
  await add(page, marker);
  await items(page).nth(0).locator(".toggle").check();
  await items(page).nth(0).locator("label").dblclick();
  await expect(items(page).nth(0).locator(".edit")).toBeVisible();
  await items(page).nth(0).locator(".edit").fill(`${marker} edited`);
  await items(page).nth(0).locator(".edit").press("Enter");
  await expect(items(page)).toHaveText([`${marker} edited`]);
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(page)).toHaveText([`${marker} edited`]);
  expect(sent.length).toBeGreaterThan(0);
  expect(sent.filter((r) => r.includes(marker))).toEqual([]);
});

test("a todo's row is one element across each change, matched by its key", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  const first = await items(page).nth(0).elementHandle();
  const second = await items(page).nth(1).elementHandle();
  // Toggled, the row is the same element, as TodoMVC's own tests hold it.
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  expect(await first.evaluate((li) => li.isConnected && li.classList.contains("completed"))).toBe(true);
  // A row before it destroyed, the second is still its own element, its todo's.
  await items(page).nth(0).hover();
  await items(page).nth(0).locator(".destroy").click();
  await expect(items(page)).toHaveText([TWO, THREE]);
  expect(await first.evaluate((li) => li.isConnected)).toBe(false);
  expect(await second.evaluate((li) => li.isConnected && li.textContent.includes("feed the cat"))).toBe(true);
  // A box a person pressed follows what the list says after: all completed,
  // then none.
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  await expect(items(page).nth(1)).not.toHaveClass(/completed/);
  await expect(page.locator(".toggle-all")).not.toBeChecked();
  await page.locator(".toggle-all").click({ force: true });
  await expect(items(page)).toHaveClass([/completed/, /completed/]);
  await expect(items(page).nth(0).locator(".toggle")).toBeChecked();
  await expect(items(page).nth(1).locator(".toggle")).toBeChecked();
  await expect(page.locator(".toggle-all")).toBeChecked();
  await page.locator(".toggle-all").click({ force: true });
  await expect(items(page).nth(0)).not.toHaveClass(/completed/);
  await expect(items(page).nth(1)).not.toHaveClass(/completed/);
  await expect(items(page).nth(0).locator(".toggle")).not.toBeChecked();
  await expect(items(page).nth(1).locator(".toggle")).not.toBeChecked();
  await expect(page.locator(".toggle-all")).not.toBeChecked();
});

test("another visitor has a list of their own", async ({ page, browser, baseURL }) => {
  await open(page);
  await add(page, ONE);
  const other = await browser.newContext({ baseURL });
  const stranger = await other.newPage();
  await open(stranger);
  await expect(items(stranger)).toHaveCount(0);
  await add(stranger, TWO);
  await expect(items(stranger)).toHaveText([TWO]);
  await expect(items(page)).toHaveText([ONE]);
  await other.close();
});

test("a second tab on the same device shows the same list, and each change reaches it", async ({
  page,
}) => {
  await open(page);
  const second = await page.context().newPage();
  await open(second);
  await add(page, ONE);
  await expect(items(second)).toHaveText([ONE]);
  await add(second, TWO);
  await expect(items(page)).toHaveText([ONE, TWO]);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(second).nth(0)).toHaveClass(/completed/);
});

// --- Routing ----------------------------------------------------------------

test("all is the default route, its link selected", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  await expect(link(page, "All")).toHaveClass(/selected/);
  await expect(link(page, "Active")).not.toHaveClass(/selected/);
  await expect(link(page, "Completed")).not.toHaveClass(/selected/);
  await expect(items(page)).toHaveCount(2);
});

test("#/active shows the active todos, #/completed the completed, and each link is selected", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  await link(page, "Active").click();
  await expect(items(page)).toHaveText([ONE, THREE]);
  await expect(link(page, "Active")).toHaveClass(/selected/);
  await expect(link(page, "All")).not.toHaveClass(/selected/);
  await link(page, "Completed").click();
  await expect(items(page)).toHaveText([TWO]);
  await expect(link(page, "Completed")).toHaveClass(/selected/);
  await expect(link(page, "Active")).not.toHaveClass(/selected/);
  await link(page, "All").click();
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
  await expect(link(page, "All")).toHaveClass(/selected/);
});

test("the counter counts every todo whatever the filter shows", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await link(page, "Completed").click();
  await expect(items(page)).toHaveCount(1);
  await expect(page.locator(".todo-count")).toHaveText("2 items left");
  await expect(page.locator(".clear-completed")).toBeVisible();
});

test("an item updated while filtered is updated accordingly", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  await link(page, "Active").click();
  // The fixture makes the boundary observable: only ONE and THREE are active.
  // Assert identities before using a row locator, so a filter fault cannot
  // make that action target a stale All-list row.
  await expect(items(page)).toHaveText([ONE, THREE]);
  // Checked, it is no longer active: it goes. Pressed, not `check()`ed:
  // the browser renders the list again as it is pressed (track
  // `device-store`), so the box leaves with its todo, and none is left to be
  // seen checked; what the list holds says what the press did.
  await items(page).nth(0).locator(".toggle").click();
  await expect(items(page)).toHaveText([THREE]);
  // The completed boundary now contains TWO and the just-completed ONE.
  // Assert exact rows before unchecking one of them.
  await link(page, "Completed").click();
  await expect(items(page)).toHaveText([ONE, TWO]);
  await items(page).nth(0).locator(".toggle").click();
  await expect(items(page)).toHaveText([TWO]);
  // Removing the last completed row still exercises the empty-filter boundary.
  await items(page).nth(0).locator(".toggle").click();
  await expect(items(page)).toHaveCount(0);
  await link(page, "All").click();
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
});

test("a todo added under a filter that shows it appears, and under one that does not, does not", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE);
  await link(page, "Active").click();
  await add(page, TWO);
  await expect(items(page)).toHaveText([ONE, TWO]);
  await link(page, "Completed").click();
  await expect(items(page)).toHaveCount(0);
  // The footer is the list's, not the filter's: it stays while the filter
  // shows none.
  await expect(page.locator(".main")).toBeVisible();
  await expect(page.locator(".footer")).toBeVisible();
  await field(page).fill(THREE);
  await field(page).press("Enter");
  await page.waitForTimeout(300);
  await expect(items(page)).toHaveCount(0);
  await link(page, "All").click();
  await expect(items(page)).toHaveText([ONE, TWO, THREE]);
});

test("the back button respects the routes", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO, THREE);
  await items(page).nth(1).locator(".toggle").check();
  await expect(items(page).nth(1)).toHaveClass(/completed/);
  await link(page, "All").click();
  await link(page, "Active").click();
  await expect(items(page)).toHaveCount(2);
  await link(page, "Completed").click();
  await expect(items(page)).toHaveCount(1);
  await page.goBack();
  await expect(items(page)).toHaveCount(2);
  await expect(link(page, "Active")).toHaveClass(/selected/);
  await page.goBack();
  await expect(link(page, "All")).toHaveClass(/selected/);
  await expect(items(page)).toHaveCount(3);
  await page.goForward();
  await expect(items(page)).toHaveCount(2);
});

test("a page opened at a route shows it, and the route is kept on reload", async ({
  page,
  browser,
  baseURL,
}) => {
  await open(page);
  await add(page, ONE, TWO);
  await items(page).nth(0).locator(".toggle").check();
  await expect(items(page).nth(0)).toHaveClass(/completed/);
  // A page opened at the route, in the same session.
  const second = await page.context().newPage();
  await open(second, "#/completed");
  await expect(items(second)).toHaveText([ONE]);
  await expect(link(second, "Completed")).toHaveClass(/selected/);
  await expect(link(second, "All")).not.toHaveClass(/selected/);
  // Reloaded, it is still at it.
  await second.reload();
  await second.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(items(second)).toHaveText([ONE]);
  await expect(link(second, "Completed")).toHaveClass(/selected/);
  const active = await page.context().newPage();
  await open(active, "#/active");
  await expect(items(active)).toHaveText([TWO]);
});

test("a fragment that is no route shows every todo", async ({ page }) => {
  await open(page);
  await add(page, ONE, TWO);
  const second = await page.context().newPage();
  await open(second, "#/nonsense");
  await expect(items(second)).toHaveCount(2);
  await expect(link(second, "All")).toHaveClass(/selected/);
});

// --- The template's markup ---------------------------------------------------

test("the markup is the template's, which the official tests' selectors are written over", async ({
  page,
}) => {
  await open(page);
  await add(page, ONE);
  for (const selector of [
    "section.todoapp",
    "header.header",
    "header.header h1",
    "input.new-todo",
    "section.main",
    "input.toggle-all#toggle-all",
    "label[for=toggle-all]",
    "ul.todo-list",
    "ul.todo-list > li > div.view > input.toggle[type=checkbox]",
    "ul.todo-list > li > div.view > label",
    "ul.todo-list > li > div.view > button.destroy",
    "footer.footer",
    "footer.footer > span.todo-count > strong",
    "footer.footer > ul.filters > li > a",
    "footer.footer > button.clear-completed",
    "footer.info",
  ]) {
    await expect(page.locator(selector).first(), selector).toBeAttached();
  }
  await expect(field(page)).toHaveAttribute("placeholder", "What needs to be done?");
  await expect(page.locator("label[for=toggle-all]")).toHaveText("Mark all as complete");
  await expect(page.locator("footer.info")).toContainText("Double-click to edit a todo");
});

test("the stylesheets are TodoMVC's own, and shown", async ({ page, request }) => {
  await open(page);
  for (const sheet of ["/todomvc-base.css", "/todomvc-app.css", "/todomvc-pleris.css"]) {
    const response = await request.get(sheet);
    expect(response.status(), sheet).toBe(200);
    expect(response.headers()["content-type"], sheet).toContain("text/css");
  }
  // The official sheet's own rules apply: the heading is the template's red,
  // set in 80px.
  const h1 = await page.locator("h1").evaluate((e) => {
    const style = getComputedStyle(e);
    return [style.color, style.fontSize];
  });
  expect(h1).toEqual(["rgb(184, 63, 69)", "80px"]);
  // The checkbox's drawings are served from this origin, as images.
  for (const drawing of ["/todomvc-circle.svg", "/todomvc-checked.svg"]) {
    const response = await request.get(drawing);
    expect(response.status(), drawing).toBe(200);
    expect(response.headers()["content-type"], drawing).toBe("image/svg+xml");
  }
  // And the licence of the one that asks for credit travels with it.
  const licence = await request.get("/todomvc-app-css.license.txt");
  expect(licence.status()).toBe(200);
  expect(await licence.text()).toContain("Creative Commons Attribution 4.0");
});
