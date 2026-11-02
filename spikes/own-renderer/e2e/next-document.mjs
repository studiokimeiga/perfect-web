// The wait a spec asks for when it navigates (`goto`) as soon as a press's
// in-page navigation has changed the address: one definition, shared by every
// such spec (ADR-0280, amended 2026-10-18 and 2026-11-02).

/**
 * The document an in-page navigation is about to show, and a wait for it to
 * settle: asked before the press that navigates, `settled` awaited once the
 * address has changed. Settled is the next document's runtime booted and its
 * subscription answered, so that nothing the page has just asked for is in
 * flight when the test navigates again. A navigation that cancels requests a
 * page started a few milliseconds before can crash WebKitGTK's network
 * process (a use-after-free in libsoup 3.x), and the navigation then fails or
 * hangs: found by train-10's verify, reproduced on master and outside this
 * program (docs/KNOWN_LIMITATIONS.md; ADR-0280, amended 2026-10-18). A person
 * cannot navigate that soon after a page appears. In WebKit only: Chromium
 * and Firefox still navigate at once, as when such a `goto` found ADR-0306's
 * rule that the browser's own load wins (Firefox's NS_BINDING_ABORTED).
 *
 * For a page that subscribes: the next document's `/stream` answer is what
 * `settled` waits for, and a page whose manifest says `"listens": false`
 * never asks, so `settled` would wait to the test's timeout there. Each page
 * the specs navigate to with it reads a query, and subscribes.
 */
export async function nextDocument(page, browserName) {
  if (browserName !== "webkit") return { settled: async () => {} };
  const shown = await page.evaluate(
    () => JSON.parse(document.getElementById("pw-parts").textContent).cursor,
  );
  // A later document's subscription: the one shown now asks with its own.
  const subscribed = page.waitForResponse((response) => {
    const doc = /\/stream\?doc=(\d+)/.exec(response.url())?.[1];
    return doc !== undefined && Number(doc) > shown;
  });
  subscribed.catch(() => {});
  return {
    settled: async () => {
      await subscribed;
      await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
        polling: 50,
      });
    },
  };
}
