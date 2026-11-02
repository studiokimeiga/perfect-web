// Track `deploy` (ADR-XXXX, a production host): **the store, served by the
// release host with `PW_DEPLOYMENT=production`, through a TLS front, asked
// as a browser asks it.** `deploy/smoke.sh` runs this file twice in each
// engine: `PW_SMOKE_PHASE=first`, then the host is stopped (SIGTERM) and
// started again on the same database, then `PW_SMOKE_PHASE=again`, with the
// browser's cookies the first phase kept (`PW_SMOKE_STATE`).
//
// Sign-in is not here: production refuses the development provider and has
// no other until the owner's OIDC client exists (the ADR's gap). A guest
// browses, fills a cart and is told an order needs a sign-in.
import { expect, test } from "@playwright/test";

const phase = process.env.PW_SMOKE_PHASE ?? "first";
const stateOf = (engine) => `${process.env.PW_SMOKE_STATE ?? "/tmp/pw-smoke-state"}-${engine}.json`;
const token = process.env.PW_SMOKE_METRICS_TOKEN ?? "";

test.describe.configure({ mode: "serial" });

/** The store's page, its runtime started: the bootstrap its policy hashes
 * ran, the minified runtime loaded, and its WebAssembly. */
async function ready(page) {
  const answer = await page.goto("/stores/47");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
    timeout: 30_000,
  });
  return answer;
}

if (phase === "first") {
  test("the store's page is served over TLS with its policy, nosniff and HSTS, uncompressed", async ({
    page,
  }) => {
    const answer = await ready(page);
    expect(answer.status()).toBe(200);
    const headers = answer.headers();
    expect(headers["strict-transport-security"]).toBe("max-age=31536000");
    expect(headers["x-content-type-options"]).toBe("nosniff");
    expect(headers["cache-control"]).toBe("private, no-store");
    const policy = headers["content-security-policy"];
    expect(policy).toContain("script-src 'self' 'wasm-unsafe-eval' 'sha256-");
    expect(policy).toContain("frame-ancestors 'none'");
    // A session's answer is never compressed (BREACH).
    expect(headers["content-encoding"]).toBeUndefined();
    // Its session's cookie: HttpOnly, SameSite=Lax, Secure.
    const cookies = await page.context().cookies();
    const session = cookies.find((c) => c.name === "pw-session");
    expect(session?.httpOnly).toBe(true);
    expect(session?.secure).toBe(true);
    expect(session?.sameSite).toBe("Lax");
    // The runtime is started at the build's version.
    const html = await answer.text();
    expect(html).toMatch(/<script>import\("\/pw-runtime\.mjs\?v=[0-9a-f]{16}"\)<\/script>/);
    expect(html).toContain("Blue Bottle");
  });

  test("its assets come at the build's version, compressed and kept for good", async ({ page }) => {
    const assets = new Map();
    page.on("response", (response) => {
      const url = new URL(response.url());
      if (/^\/(pw-runtime\.mjs|pw-resume\.wasm|pw-render\.wasm|handler\/)/.test(url.pathname)) {
        assets.set(url.pathname, { query: url.search, headers: response.headers() });
      }
    });
    await ready(page);
    // A handler's module is loaded when its control is pressed.
    await page.getByRole("button", { name: "Add Espresso" }).click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    const runtime = assets.get("/pw-runtime.mjs");
    expect(runtime?.query).toMatch(/^\?v=[0-9a-f]{16}$/);
    const version = runtime.query;
    for (const [path, { query, headers }] of assets) {
      expect(query, path).toContain(version.slice(1));
      expect(headers["cache-control"], path).toBe("public, max-age=31536000, immutable");
      expect(headers["etag"], path).toMatch(/^"[0-9a-f]{32}(-gz)?"$/);
      expect(headers["x-content-type-options"], path).toBe("nosniff");
    }
    expect(assets.get("/pw-runtime.mjs").headers["content-encoding"]).toBe("gzip");
    expect(assets.get("/pw-resume.wasm").headers["content-encoding"]).toBe("gzip");
    expect([...assets.keys()].some((p) => p.startsWith("/handler/"))).toBe(true);
  });

  test("a guest's cart is kept, and an order needs a sign-in this deployment does not have", async ({
    page,
  }, testInfo) => {
    await ready(page);
    await page.getByRole("button", { name: "Add Cortado" }).click();
    await expect(page.locator("#cart-count")).toHaveText("1");
    await page.goto("/cart");
    await expect(page.locator("#cart-lines li")).toHaveCount(1);
    await expect(page.locator("#cart-lines li").first()).toContainText("Cortado");
    // Placing an order requires `SignedIn`, which holds for no one here.
    await expect(page.locator("#place-order")).toHaveCount(0);
    const signIn = await page.goto("/sign-in");
    expect(signIn.status()).toBe(404);
    await expect(page.getByRole("alert")).toContainText("no sign-in");
    await page.context().storageState({ path: stateOf(testInfo.project.name) });
  });

  test("what production does not serve, and what it says of itself", async ({ request }) => {
    for (const path of [
      "/bench/clock",
      "/menu-entry",
      "/StorePage.html?items=40",
      "/build-info.json",
      "/HelloStatic.html",
      "/build/templates.json",
      "/metrics",
      "/dev-idp/authorize",
    ]) {
      const answer = await request.get(path);
      expect(answer.status(), path).toBe(404);
    }
    for (const path of ["/bench/clock?advance=60000", "/command/menu_changed"]) {
      const answer = await request.post(path);
      expect(answer.status(), path).toBe(404);
    }
    const metrics = await request.get("/metrics", { headers: { authorization: `Bearer ${token}` } });
    expect(metrics.status()).toBe(200);
    expect(Object.keys((await metrics.json()).calls).length).toBeGreaterThan(0);

    const health = await request.get("/_pw/health");
    expect(health.status()).toBe(200);
    expect(await health.json()).toEqual({ status: "live" });
    expect(health.headers()["strict-transport-security"]).toBe("max-age=31536000");
    const readiness = await request.get("/_pw/ready");
    expect(readiness.status()).toBe(200);
    expect(await readiness.json()).toEqual({ status: "ready" });

    // An address of no version is asked again, and answered with nothing
    // where the browser holds it.
    const plain = await request.get("/pw-runtime.mjs");
    expect(plain.headers()["cache-control"]).toBe("no-cache");
    const tag = plain.headers()["etag"];
    const again = await request.get("/pw-runtime.mjs", { headers: { "if-none-match": tag } });
    expect(again.status()).toBe(304);
  });
} else {
  test("after a restart, the guest's cart is still theirs, and the host is ready again", async ({
    browser,
    request,
  }, testInfo) => {
    const context = await browser.newContext({
      storageState: stateOf(testInfo.project.name),
      ignoreHTTPSErrors: true,
    });
    const page = await context.newPage();
    await ready(page);
    // The Cortado the first phase's guest added, kept in PostgreSQL, found
    // by the session its cookie names.
    await expect(page.locator("#cart-count")).toHaveText("1");
    await page.goto("/cart");
    await expect(page.locator("#cart-lines li")).toHaveCount(1);
    await expect(page.locator("#cart-lines li").first()).toContainText("Cortado");
    await context.close();
    const readiness = await request.get("/_pw/ready");
    expect(await readiness.json()).toEqual({ status: "ready" });
  });
}
