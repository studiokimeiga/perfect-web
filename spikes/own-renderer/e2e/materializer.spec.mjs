// ADR-0176, charter §15.5: a materializer failure. The session's next
// regeneration of its cart's entry fails, once. The change it was for must
// still reach the page, without a reload, and so must every change after it.
//
// The fault is the session's own, so the suite shares its host.
import { expect, test } from "@playwright/test";

/** The store, ready, counting each time the page is read again. Its page is
 * held briefly: a page held long has its failed regeneration tried again
 * within the hold (ADR-0402), and it is the next request's trying
 * that this holds to. */
async function ready(page) {
  await page.addInitScript(() => Object.defineProperty(navigator, "locks", { value: undefined }));
  const loads = { count: 0 };
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) loads.count += 1;
  });
  await page.goto("/stores/47");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  return loads;
}

const logged = (page, prefix) =>
  page.evaluate((p) => (window.__pw?.log ?? []).filter((l) => l.startsWith(p)).length, prefix);
const refused = (page) => page.evaluate(() => window.__pw.refused ?? 0);

test("a regeneration that fails is tried again, and the page hears the change, and the next", async ({
  page,
}) => {
  const loads = await ready(page);
  expect((await page.request.post("/bench/materializer?fail=next")).ok()).toBe(true);
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  // The server's value reaches the page and reconciles the press.
  // The page's own binding of it, counted alone: its layout's count is
  // another binding of the cart, reconciled beside it (ADR-0303).
  await expect.poll(() => logged(page, "reconciled cart at"), { timeout: 10_000 }).toBe(1);
  // And the next change, as any change does.
  await page.getByRole("button", { name: "Add Cortado" }).click();
  await expect.poll(() => logged(page, "reconciled cart at"), { timeout: 10_000 }).toBe(2);
  await expect(page.locator("#cart-lines li")).toHaveCount(2);
  await expect(page.locator("#cart-count")).toHaveText("2");
  expect(await refused(page)).toBe(0);
  expect(loads.count, "never read again").toBe(1);
});
