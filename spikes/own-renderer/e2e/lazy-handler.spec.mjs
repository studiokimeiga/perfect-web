// E7-L — handler code arrives on interaction, and only the handler interacted
// with.
//
// Architect ruling, 2026-08-07:
//
// > Handler bytes absent before interaction; the exact handler fetched on first
// > interaction; E7V check → Authorised → attach → run; unrelated handler code
// > still absent.
//
// # Why the store page has two handlers
//
// With one, "the exact handler was fetched" is satisfied by any fetch at all,
// and "unrelated code stayed absent" is a claim about the empty set. So
// `examples/store/app.pw` declares `clear_cart` beside `add_to_cart`, and every
// test below watches BOTH.
//
// # What is being measured
//
// Network requests, by URL, recorded from before navigation. Not a bundle
// size, not a timing: `RISK_QUEUE` 22 is a detector that filtered request URLs
// by `/handler|interaction/` and matched a naming convention rather than a
// fact. A handler here is served at `/handler/<identity>.mjs` because the
// SERVER decided so and the runtime asks for it by identity — so the URLs are
// enumerable, and the test asserts on the exact set.

import { test, expect } from "@playwright/test";

/** Every handler module this page has fetched, by identity. */
function watch(page) {
  const fetched = [];
  page.on("request", (r) => {
    const m = /\/handler\/([^/?]+)\.mjs/.exec(r.url());
    if (m) fetched.push(m[1]);
  });
  return fetched;
}

/** The handler identities the document declares, by name. */
async function identities(page) {
  return page.evaluate(() =>
    Object.fromEntries(
      (window.__pw.parts.parts ?? [])
        .filter((p) => p.kind === "event")
        .map((p) => [p.name, p.value]),
    ),
  );
}

async function ready(page) {
  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
}

test("no handler code is fetched before an interaction", async ({ page }) => {
  const fetched = watch(page);
  await ready(page);

  // Settled, not merely "not yet": the page is fully booted, the decision wasm
  // has loaded, handlers are attached, and the subscription is running.
  await page.waitForTimeout(400);
  expect(fetched, "the document carries no behaviour").toEqual([]);

  // And the page is not inert for a different reason — it really did attach.
  const log = await page.evaluate(() => window.__pw.log.join("\n"));
  expect(log, "handlers were attached without their code").toMatch(/attached/);
});

test("the exact handler is fetched on first interaction", async ({ page }) => {
  const fetched = watch(page);
  await ready(page);
  const ids = await identities(page);
  // Add and Clear, and a cart line's three (ADR-0172), which no line is
  // rendered with yet.
  expect(Object.keys(ids).sort(), "five handlers on this page").toEqual([
    "add_to_cart",
    "clear_cart",
    "decrease_in_cart",
    "increase_in_cart",
    "remove_from_cart",
  ]);

  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");

  expect(fetched, "the one pressed, and nothing else").toEqual([ids.add_to_cart]);
});

test("the unrelated handler's code stays absent", async ({ page }) => {
  // The half that makes the test above mean something. A page that fetched
  // everything on first interaction would pass "the exact handler arrived".
  const fetched = watch(page);
  await ready(page);
  const ids = await identities(page);

  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.waitForTimeout(300);

  expect(fetched).not.toContain(ids.clear_cart);

  // And `clear_cart` is genuinely loadable — otherwise "absent" is a fact
  // about a URL that does not work.
  await page.locator("#clear-cart").click();
  await expect(page.locator("#cart-count")).toHaveText("0");
  expect(fetched, "now both, in the order they were pressed").toEqual([
    ids.add_to_cart,
    ids.clear_cart,
  ]);
});

test("two different handlers do two different things", async ({ page }) => {
  // A loader that fetched the right URL and ran the wrong module would pass
  // every request-counting test in this file.
  await ready(page);
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  await page.locator("#menu button").first().click();
  await expect(page.locator("#cart-count")).toHaveText("2");
  await page.locator("#clear-cart").click();
  await expect(page.locator("#cart-count")).toHaveText("0");
});

test("a repeated interaction does not refetch", async ({ page }) => {
  const fetched = watch(page);
  await ready(page);

  for (const n of ["1", "2", "3"]) {
    await page.locator("#menu button").first().click();
    await expect(page.locator("#cart-count")).toHaveText(n);
  }
  expect(fetched.length, "loaded once, pressed three times").toBe(1);
});

test("two fast presses are one fetch", async ({ page }) => {
  // In-flight, not merely loaded. A cache that only remembered FINISHED loads
  // would start a second fetch for a press that landed while the first was
  // still in the air — rare, invisible, and a double-run of the handler.
  const fetched = watch(page);
  await ready(page);

  const button = page.locator("#menu button").first();
  await Promise.all([button.click(), button.click()]);
  await expect(page.locator("#cart-count")).toHaveText("2");
  expect(fetched.length).toBe(1);
});

test("all three instances of one handler share one module", async ({ page }) => {
  // The three Add buttons are three INSTANCES of one template position, so
  // they are one handler. Fetching per instance would be three requests for
  // one behaviour — and would grow with the collection.
  const fetched = watch(page);
  await ready(page);

  for (const i of [0, 1, 2]) {
    await page.locator("#menu button").nth(i).click();
    await expect(page.locator("#cart-count")).toHaveText(String(i + 1));
  }
  expect(fetched.length, "one module for three buttons").toBe(1);
});

test("an unauthorised handler is never fetched", async ({ page }) => {
  // The order the ruling specifies: E7V check → Authorised → attach → run. A
  // runtime that fetched first and checked after would make the refusal a
  // formality the network had already ignored — the bytes would be on the
  // wire, in the cache, and in a profile.
  const fetched = watch(page);
  await page.route("**/StorePage.html", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    // An incompatible ABI. Served by rewriting what the SERVER sent, so the
    // refusal path is exercised through the real decision rather than by a
    // branch this file added.
    await route.fulfill({
      response,
      body: body.replace('"abi":"1"', '"abi":"99"'),
    });
  });

  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  await page.locator("#menu button").first().click();
  // The decision's word, not a pause of 400 ms that a loaded machine
  // outlasted: the runtime logs its refusal as it makes it, and the log is
  // the one of whichever document has one (a document between two, its
  // runtime not yet booted, has none: `window.__pw` is undefined there, which
  // a pause and a read of it met when the page was being read again). And a
  // turn of the page's event loop after it, for anything the refusal left
  // running to have asked.
  const log = await (
    await page.waitForFunction(() => {
      const lines = window.__pw?.log ?? [];
      return lines.some((line) => /refused \d+:/.test(line)) && lines.join("\n");
    })
  ).jsonValue();
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 0))).catch(() => {});

  expect(log, "the decision refused").toMatch(/refused \d+:/);
  expect(fetched, "and nothing was fetched").toEqual([]);
  await expect(page.locator("#cart-count"), "the button is inert").toHaveText("0");
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("a handler this build did not compile is refused, whatever the document says", async ({
  page,
}) => {
  // ADR-0132: what the decision knows comes from the build the runtime was
  // served with (`/pw-handlers`), never from the document. A document that
  // names a handler the build lacks, as one cached from another build would,
  // gets no listener and fetches no code, however consistently it names it.
  const fetched = watch(page);
  const warned = [];
  page.on("console", (m) => m.type() === "warning" && warned.push(m.text()));
  await page.route("**/StorePage.html", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    const manifest = JSON.parse(body.match(/id="pw-parts">(.*?)<\/script>/s)[1]);
    const add = manifest.parts.find((p) => p.kind === "event" && p.name === "add_to_cart").value;
    await route.fulfill({ response, body: body.replaceAll(add, "ffffffffffffffff") });
  });

  await page.goto("/StorePage.html");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady);
  await page.locator("#menu button").first().click();
  // The decision's word (see above): refused, as this build's code 4.
  const log = await (
    await page.waitForFunction(() => {
      const lines = window.__pw?.log ?? [];
      return lines.some((line) => /refused \d+: code 4 /.test(line)) && lines.join("\n");
    })
  ).jsonValue();
  await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 0))).catch(() => {});

  expect(log, "the build's handlers are known").toMatch(/knows \d+ handler\(s\)/);
  expect(log, "this one is not among them").toMatch(/refused \d+: code 4 /);
  // ADR-0400: a developer is told in the console, by the code
  // and the page that explains it.
  expect(warned, "the console names the refusal and its page").toContainEqual(
    expect.stringMatching(/^Pleris: handler \d+ not attached: PWR04, https:\/\/pleris\.dev\/e\/PWR04, recovery \S+$/),
  );
  expect(fetched, "and nothing was fetched").toEqual([]);
  await expect(page.locator("#cart-count"), "the button is inert").toHaveText("0");
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("a handler that fails to load is visible and recoverable", async ({ page }) => {
  // The failure a user would otherwise experience as "the button does
  // nothing". It must be visible on the element and it must not be permanent:
  // the next press retries.
  await ready(page);
  const ids = await identities(page);

  let failures = 0;
  await page.route(`**/handler/${ids.add_to_cart}.mjs*`, async (route) => {
    failures++;
    if (failures === 1) await route.abort("failed");
    else await route.continue();
  });

  const button = page.locator("#menu button").first();
  await button.click();
  await expect(button, "the failure is on the element").toHaveAttribute(
    "data-pw-handler-error",
    "failed",
  );
  // And told beside it (ADR-0302).
  await expect(page.locator("#menu .pw-refusal").first()).toHaveText(
    "This did not work. Try again.",
  );
  await expect(page.locator("#cart-count"), "and nothing happened").toHaveText("0");

  // Recoverable: the failed load was forgotten, so pressing again retries.
  //
  // The retry asks for a different specifier — `?attempt=1` — because the
  // browser's module map caches FAILURES, and re-importing the same specifier
  // returns the same rejected promise forever. Hence the `*` on the route
  // glob: without it the retry is invisible to this instrument and the test
  // reports "no refetch" for a page that refetched.
  await button.click();
  await expect(page.locator("#cart-count")).toHaveText("1");
  expect(failures, "the second press really did refetch").toBe(2);
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("focus survives a delayed attachment", async ({ page }) => {
  // Lazy loading introduces a wait between the press and the effect. If the
  // update moved focus, keyboard users would be thrown out of the control they
  // just used — every time, and only once loading was slow enough to notice.
  await ready(page);
  const ids = await identities(page);
  await page.route(`**/handler/${ids.add_to_cart}.mjs*`, async (route) => {
    await new Promise((r) => setTimeout(r, 300));
    await route.continue();
  });

  const button = page.locator("#menu button").first();
  await button.focus();
  await button.press("Enter");
  await expect(page.locator("#cart-count")).toHaveText("1");

  const focused = await page.evaluate(() => ({
    tag: document.activeElement?.tagName ?? null,
    text: document.activeElement?.textContent?.trim() ?? null,
  }));
  expect(focused, "focus stayed on the button that was pressed").toEqual({
    tag: "BUTTON",
    text: "Add",
  });
  await page.unrouteAll({ behavior: "ignoreErrors" }).catch(() => {});
});

test("the server refuses an identity this build does not have", async ({ page }) => {
  // Serving *some* handler for an unknown identity is how a stale document
  // ends up running new code. The 404 is the point.
  await ready(page);
  const response = await page.request.get("/handler/deadbeefdeadbeef.mjs");
  expect(response.status()).toBe(404);

  // The control: a known identity does resolve, so the 404 is about this
  // identity rather than about the route.
  const ids = await identities(page);
  const known = await page.request.get(`/handler/${ids.clear_cart}.mjs`);
  expect(known.status()).toBe(200);
  expect(await known.text()).toContain("clear_cart");
});

test("nothing is prefetched without a policy that says so", async ({ page }) => {
  // The default is that behaviour costs nothing until it is used. A runtime
  // that speculatively warmed handlers would make every measurement above pass
  // for a page that had already downloaded everything — so the absence of
  // prefetch is asserted rather than assumed.
  const fetched = watch(page);
  await ready(page);

  await page.hover("#menu button");
  await page.locator("#clear-cart").hover();
  await page.mouse.move(10, 10);
  await page.waitForTimeout(500);

  expect(fetched, "hovering is not pressing").toEqual([]);
  const links = await page.evaluate(() =>
    [...document.querySelectorAll("link")].map((l) => `${l.rel}:${l.href}`),
  );
  // What every page's runtime boots with is asked for by its head (the
  // first press's ADR): the runtime, the decision's code and this build's
  // handler table. Each is asked for by every boot, so none is speculative,
  // and none is a handler's code.
  const origin = new URL(page.url()).origin;
  const boot = [
    `modulepreload:${origin}/pw-runtime.mjs`,
    `preload:${origin}/pw-resume.wasm`,
    `preload:${origin}/pw-handlers`,
  ];
  expect(
    links.filter((l) => /preload|prefetch|modulepreload/.test(l)).sort(),
    "the document asks for what its runtime boots with",
  ).toEqual([...boot].sort());
  expect(
    links.filter((l) => l.includes("/handler/")),
    "and for no handler's code",
  ).toEqual([]);
});
