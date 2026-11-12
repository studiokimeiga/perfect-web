// ADR-0382: the modules a page loads when it first needs them are loaded
// within the deadline, and a load that failed is not kept. A feed page
// computes what is left of a draft in the browser, by a module it loads with
// the draft's first change (ADR-0227). Until 2026-10-21 one failed load of it
// was kept, and nothing was computed again until the page was reloaded. It has
// no deadline: a press does not wait for it, and a boot that migrates a value
// the device keeps does, where a slow load is better waited for than failed.
// The press's modules are `press-modules.spec.mjs`'s.
//
// Each fault is the page's own, so the suite shares its host.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { FEED_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${FEED_PORTS[testInfo.project.name]}`);
  },
});

/** A page, ready to be typed in. Polled: WebKit runs no animation frame
 * before its first paint (`slots.spec.mjs`). */
async function ready(page, path) {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

const logged = (page, prefix) =>
  page.evaluate((p) => (window.__pw?.log ?? []).filter((l) => l.startsWith(p)).length, prefix);

/** The feed's home page, ready, and what is left of its draft. */
async function drafting(page, path) {
  await ready(page, path);
  const left = page.locator("#left");
  await expect(left).toHaveText("280 left");
  return { draft: page.getByLabel("What's happening?"), left };
}

test("a computed values module that failed to load is loaded again at the next change", async ({
  page,
}) => {
  let calls = 0;
  const uncaught = [];
  page.on("pageerror", (e) => uncaught.push(e.message));
  await page.route("**/computed/**", (route) => (++calls === 1 ? route.abort("failed") : route.continue()));
  try {
    const { draft, left } = await drafting(page, "/");
    await draft.fill("Hello");
    await expect.poll(() => logged(page, "computed values not loaded"), { timeout: 8_000 }).toBe(1);
    // And the developer is told, as of an error nothing caught (ADR-0431).
    await expect.poll(() => uncaught.length).toBe(1);
    // The next change asks the network again, and is computed.
    await draft.fill("Hello!");
    await expect(left).toHaveText("274 left");
    expect(calls).toBe(2);
    expect(uncaught.length, "and the change computed told nothing").toBe(1);
  } finally {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
});
