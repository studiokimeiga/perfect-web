// A block a signal decides, inside another: what the page shows follows the
// signals, however the two have been pressed. `examples/demo/nested.pw`.
//
// What this shows, in each engine, and on a production build on the device's
// runtime, which keeps what each block last rendered:
// - an inner block taken away and shown again, with its signal changed while
//   it was gone, shows what its signal says on its next render;
// - one press that changes both blocks is rendered without an error.
import { expect, test } from "@playwright/test";

const PAGE = "/nested";

async function open(page) {
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(String(e)));
  await page.goto(PAGE);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

const shown = (page) => page.locator("#shown");

/** Presses `id` and waits for the page's next frame to have applied it. */
async function press(page, id, text) {
  await page.locator(`#${id}`).click();
  await expect(shown(page)).toHaveText(text);
}

test("an inner block shown again renders what its signal says", async ({ page }) => {
  await open(page);
  await expect(shown(page)).toHaveText("on");
  await press(page, "flag", "off");
  await press(page, "outer", "");
  // Changed while the block is gone: nothing renders it.
  await page.locator("#flag").click();
  await press(page, "outer", "on");
  // What it last rendered is "off", and it shows "on".
  await press(page, "flag", "off");
  expect(page.errors).toEqual([]);
});

test("one press that changes both blocks is rendered", async ({ page }) => {
  await open(page);
  await press(page, "flag", "off");
  await press(page, "both", "");
  await press(page, "both", "off");
  await press(page, "flag", "on");
  expect(page.errors).toEqual([]);
});
