// ADR-0175, charter §15.5: a one-shot network error the server makes, and a
// forced reconnect. The server closes the session's next command connection
// with no answer, before the command runs or after it has committed; and it
// ends the session's subscriptions, refusing new ones for a while. A press
// survives either drop as one mutation (ADR-0173), and a page cut off hears,
// once it is back, what changed meanwhile.
//
// Each fault is the session's own, so the suite shares its host.
import { expect, test } from "@playwright/test";

/** The store, ready. */
async function ready(page) {
  await page.goto("/stores/47");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

const logged = (page, prefix) =>
  page.evaluate((p) => (window.__pw?.log ?? []).filter((l) => l.startsWith(p)).length, prefix);

/** **WebKit's network process failing**, which a page hears as "WebKit
 * encountered an internal error". Relaunched, it holds none of the context's
 * session cookies, so the page asks as a new visitor and is rightly told to
 * reload, its cart a new session's (run 42772395298, WebKit, the cut-off
 * test). What follows says nothing of the server's, and the test is skipped
 * saying why, rather than failed as if the server had erred.
 *
 * Heard from the request as it fails, and from the console. The console can
 * say it late: in run 43044988596 (WebKit, the cut-off test) the press's
 * handler module failed so as it was clicked, and the console said so six
 * seconds later, after the expectation had failed and been judged a finding. */
function networkWatched(page) {
  const failed = [];
  const heard = (said) => {
    if (said.includes("WebKit encountered an internal error")) failed.push(said);
  };
  page.on("requestfailed", (r) => heard(`${r.url()}: ${r.failure()?.errorText ?? ""}`));
  page.on("console", (m) => {
    if (m.type() === "error") heard(m.text());
  });
  return failed;
}

/** **What the page and the server said of its document**, for a failure: the
 * runtime's log, and the server's trail of the document's frames with its
 * tellings (`/bench/records`, as `feed.spec.mjs` keeps them). Attached, and
 * the latest printed, where a job's log is all a reader has. Run 42785265911's
 * cut-off test failed with neither: its long poll was never answered, and
 * nothing said whether the change it waited for had been queued. */
async function recorded(page) {
  const said = await page
    .evaluate(() => ({
      log: window.__pw?.log ?? null,
      transport: window.__pw?.transport ?? null,
      reconnects: window.__pw?.reconnects ?? 0,
      doc: JSON.parse(document.getElementById("pw-parts")?.textContent ?? "{}").cursor ?? null,
    }))
    .catch((e) => ({ unavailable: String(e) }));
  const server = said.doc
    ? await page.request
        .get(`/bench/records?doc=${said.doc}`)
        .then((r) => r.json())
        .catch((e) => ({ unavailable: String(e) }))
    : null;
  await test.info().attach("records", { body: JSON.stringify({ said, server }, null, 2), contentType: "application/json" });
  const latest = (list, n) => (Array.isArray(list) ? list.slice(-n) : list);
  console.log(`pw-record ${JSON.stringify({ log: latest(said.log, 12), trail: latest(server?.trail, 24), tellings: latest(server?.tellings, 12) })}`);
}

/** `body`, failed as it fails, unless WebKit's network process failed
 * meanwhile; a failure records what the page and the server said. */
async function judged(page, failed, body) {
  try {
    await body();
  } catch (error) {
    test.skip(failed.length > 0, `WebKit's network process failed (${failed[0]}), and the session cookie with it`);
    await recorded(page).catch(() => {});
    throw error;
  }
}

for (const at of ["before", "after"]) {
  test(`a command whose connection is dropped ${at} it runs is sent again, and is one line`, async ({
    page,
  }) => {
    const failed = networkWatched(page);
    await ready(page);
    expect((await page.request.post(`/bench/drop?next=command&at=${at}`)).ok()).toBe(true);
    await judged(page, failed, async () => {
      await page.getByRole("button", { name: "Add Espresso" }).click();
      await expect.poll(() => logged(page, "resent store.page.add_to_cart")).toBe(1);
      await expect.poll(() => logged(page, "reconciled cart")).toBeGreaterThan(0);
      await expect(page.locator("#cart-count")).toHaveText("1");
      await expect(page.locator("[data-pw-handler-error]")).toHaveCount(0);
      // The server's: one line, though the command reached it twice.
      await ready(page);
      await expect(page.locator("#cart-count")).toHaveText("1");
    });
  });
}

test("a page cut off from its subscription hears, once it is back, what changed meanwhile", async ({
  page,
}) => {
  const failed = networkWatched(page);
  await ready(page);
  const reconnects = () => page.evaluate(() => window.__pw.reconnects ?? 0);
  expect((await page.request.post("/bench/reconnect?for=1500")).ok()).toBe(true);
  await judged(page, failed, async () => {
    // Cut off: its subscription ends, and asking again is refused.
    await expect.poll(reconnects).toBeGreaterThan(0);
    // A change while it is cut off: the press is answered, and its change is
    // queued for the page.
    await page.getByRole("button", { name: "Add Espresso" }).click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    // Back: the change reaches it, and the press is reconciled by it.
    await expect
      .poll(() => logged(page, "reconciled cart"), { timeout: 15_000 })
      .toBeGreaterThan(0);
    expect(await logged(page, "subscription failed")).toBeGreaterThan(0);
    await expect(page.locator("#cart-count")).toHaveText("1");
    await ready(page);
    await expect(page.locator("#cart-count")).toHaveText("1");
  });
});
