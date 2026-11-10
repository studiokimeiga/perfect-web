// ADR-0195's ruling 13 (the track `page-csp`): every HTML answer the host
// writes carries a Content-Security-Policy generated from its build and its
// deployment. Each page the suite serves runs under it with nothing refused:
// the runtime starts, a press reaches its handler, and the order page frames
// the payment provider. The controls: what the policy refuses is refused, and
// said, in each engine; and without the provider among its frames, the page
// does not work, so the policy's sources are each what something needs.
import { expect, test } from "@playwright/test";
import {
  FEED_PORTS,
  HOST_ANNOTATION,
  IDENTITY_PORTS,
  KIOKUN_PORTS,
  STORE_PAYMENTS_PORTS,
  roleOfURL,
  servable,
} from "../host-metadata.mjs";

// This file's tests run on five hosts, so each names its own to the host map
// (`host-metadata.mjs`), which starts only the selected tests' hosts; and
// each is held here to run on the host it names.
test.beforeEach(async ({ baseURL }, testInfo) => {
  const named = testInfo.annotations.filter((a) => a.type === HOST_ANNOTATION).map((a) => a.description);
  expect(named, "the host this test names to the host map is the one it runs on").toEqual([
    roleOfURL(baseURL, testInfo.project.name),
  ]);
});

/** **What the page's policy refused**, as its browser says it: each
 * `securitypolicyviolation` its documents raise, by the directive and what
 * it blocked, and each console message that names the policy. Installed
 * before any of the page's own script. */
async function watching(page) {
  const said = [];
  page.on("console", (m) => {
    if (/Content[- ]Security[- ]Policy|Refused to/i.test(m.text())) said.push(m.text());
  });
  await page.addInitScript(() => {
    window.__refused = [];
    window.addEventListener(
      "securitypolicyviolation",
      (e) => window.__refused.push(`${e.effectiveDirective} ${e.blockedURI}`),
      true,
    );
  });
  return async () => [...(await page.evaluate(() => window.__refused ?? [])), ...said];
}

/** The runtime started, or failed to: polled, as WebKit runs no animation
 * frame before its first paint (`slots.spec.mjs`). */
async function booted(page) {
  await page.waitForFunction(
    () => ["1", "failed"].includes(document.documentElement.dataset.pwReady),
    null,
    { polling: 50 },
  );
  return page.evaluate(() => document.documentElement.dataset.pwReady);
}

/** Visit `path`, and say what it was answered with. */
async function visit(page, path) {
  const response = await page.goto(path);
  return response.headers()["content-security-policy"] ?? null;
}

// The store's pages, and the host's own: each with the runtime started,
// where it has one, and nothing refused.
const RUNS = ["/", "/stores/47", "/StorePage.html", "/cart", "/search?q=pie"];
const RUNS_NOTHING = ["/stores/999", "/HelloStatic.html", "/Tricky.html"];

for (const path of RUNS) {
  test(`${path} runs under its policy, nothing refused`, { annotation: { type: "pw-host", description: "base" } }, async ({ page }) => {
    const refused = await watching(page);
    const policy = await visit(page, path);
    expect(policy, "a policy is sent").toContain("script-src 'self' 'sha256-");
    expect(policy, "and nothing compiled on the page").not.toContain("wasm-unsafe-eval");
    expect(await booted(page), "the runtime started").toBe("1");
    expect(await refused()).toEqual([]);
  });
}

for (const path of RUNS_NOTHING) {
  test(`${path} runs nothing, and loads what it shows`, { annotation: { type: "pw-host", description: "base" } }, async ({ page }) => {
    const refused = await watching(page);
    const policy = await visit(page, path);
    expect(policy).toBe(
      "default-src 'none'; img-src 'self'; style-src 'self'; form-action 'self'; " +
        "frame-ancestors 'none'; base-uri 'none'",
    );
    await page.waitForLoadState("load");
    expect(await refused()).toEqual([]);
  });
}

test("a press reaches its handler under the policy", { annotation: { type: "pw-host", description: "base" } }, async ({ page }) => {
  const refused = await watching(page);
  await visit(page, "/stores/47");
  expect(await booted(page)).toBe("1");
  const before = Number(await page.locator("#cart-count").textContent());
  await page.getByRole("button", { name: "Add Espresso" }).click();
  await expect(page.locator("#cart-count")).toHaveText(String(before + 1));
  expect(await refused()).toEqual([]);
});

test("the announcer, served unstyled, is seen by no one when it speaks", { annotation: { type: "pw-host", description: "base" } }, async ({ page }) => {
  // The host writes no style attribute (`pw_render::ANNOUNCER`); the runtime
  // hides the announcer through the CSSOM, which the policy allows, before
  // it says what a press that could not be sent is told.
  const refused = await watching(page);
  await visit(page, "/stores/47");
  expect(await booted(page)).toBe("1");
  await page.route("**/command/store.page.add_to_cart", (route) => route.abort("failed"));
  await page.getByRole("button", { name: "Add Espresso" }).click();
  const announcer = page.locator(".pw-announcer[role=status]");
  await expect(announcer).toHaveText(/could not be sent/);
  const box = await announcer.boundingBox();
  expect(box.width, "one pixel wide at most").toBeLessThanOrEqual(1);
  expect(box.height, "and one high").toBeLessThanOrEqual(1);
  expect(await announcer.evaluate((el) => getComputedStyle(el).clipPath)).toBe("inset(50%)");
  expect(await refused()).toEqual([]);
});

test("the control: what the policy refuses is refused, and said", { annotation: { type: "pw-host", description: "base" } }, async ({ page }) => {
  const refused = await watching(page);
  await visit(page, "/stores/47");
  expect(await booted(page)).toBe("1");
  // A script inline, which no hash allows: not run.
  await page.evaluate(() => {
    const script = document.createElement("script");
    script.textContent = "window.__ran = true";
    document.body.append(script);
  });
  // A style attribute, which the store's build writes none of: not applied.
  await page.evaluate(() =>
    document.querySelector("h1").setAttribute("style", "color: rgb(1, 2, 3)"),
  );
  await expect.poll(async () => (await refused()).join("\n")).toMatch(/^script-src/m);
  await expect.poll(async () => (await refused()).join("\n")).toMatch(/^style-src/m);
  expect(await page.evaluate(() => window.__ran)).toBeUndefined();
  expect(
    await page.evaluate(() => getComputedStyle(document.querySelector("h1")).color),
  ).not.toBe("rgb(1, 2, 3)");
});

test.describe("a deployment that takes payments", { annotation: { type: "pw-host", description: "payments" } }, () => {
  test.use({
    baseURL: async ({}, use, testInfo) => {
      await use(`http://127.0.0.1:${STORE_PAYMENTS_PORTS[testInfo.project.name]}`);
    },
  });

  /** An espresso in the reader's cart, its order placed, and the order
   * page framing the provider's page. */
  async function awaiting(page) {
    await visit(page, "/stores/47");
    expect(await booted(page)).toBe("1");
    const answered = page.waitForResponse("**/command/store.page.add_to_cart");
    await page.getByRole("button", { name: "Add Espresso" }).click();
    await answered;
    await visit(page, "/cart");
    expect(await booted(page)).toBe("1");
    await page.getByRole("button", { name: "Place order" }).click();
    await page.waitForURL(/\/order$/);
  }

  test("the order page frames the provider's page, nothing refused", async ({ page }) => {
    const refused = await watching(page);
    await awaiting(page);
    await expect(page.frameLocator("#payment").getByLabel("Card number")).toBeVisible();
    expect(await refused()).toEqual([]);
  });

  test("the control: a policy that names no provider frames nothing", async ({ page }) => {
    const refused = await watching(page);
    for (const path of ["/stores/47", "/cart", "/order"]) {
      await page.route(
        (url) => new URL(url).pathname === path,
        async (route) => {
          const response = await route.fetch();
          const headers = response.headers();
          headers["content-security-policy"] = headers["content-security-policy"].replace(
            /; frame-src [^;]+/,
            "",
          );
          await route.fulfill({ response, headers });
        },
      );
    }
    await awaiting(page);
    await expect.poll(async () => (await refused()).join("\n")).toMatch(/^frame-src/m);
  });
});

test.describe("the feed", { annotation: { type: "pw-host", description: "feed" } }, () => {
  test.skip(!servable("feed"), "dist-feed is not built: run feed.sh");
  test.use({
    baseURL: async ({}, use, testInfo) => {
      await use(`http://127.0.0.1:${FEED_PORTS[testInfo.project.name]}`);
    },
  });

  for (const path of ["/", "/post/p1"]) {
    test(`${path} runs under its policy, nothing refused`, async ({ page }) => {
      const refused = await watching(page);
      expect(await visit(page, path)).not.toContain("'wasm-unsafe-eval'");
      expect(await booted(page)).toBe("1");
      expect(await refused()).toEqual([]);
    });
  }
});

test.describe("a sign-in, at the development provider", { annotation: { type: "pw-host", description: "identity" } }, () => {
  test.skip(!servable("feed"), "dist-feed is not built: run feed.sh");
  test.use({
    baseURL: async ({}, use, testInfo) => {
      await use(`http://127.0.0.1:${IDENTITY_PORTS[testInfo.project.name]}`);
    },
  });

  test("the provider's page runs nothing, nothing refused", async ({ page }) => {
    const refused = await watching(page);
    await visit(page, "/");
    expect(await booted(page)).toBe("1");
    // A form's `GET /sign-in`, answered with a redirect to the provider,
    // which this server serves.
    await page.getByRole("button", { name: "Sign in" }).click();
    await page.waitForURL(/\/dev-idp\/authorize/);
    await page.waitForLoadState("load");
    expect(await refused()).toEqual([]);
  });
});

test.describe("kiokun", { annotation: { type: "pw-host", description: "kiokun" } }, () => {
  test.skip(!servable("kiokun"), "dist-kiokun is not built: run kiokun-site.sh");
  test.use({
    baseURL: async ({}, use, testInfo) => {
      await use(`http://127.0.0.1:${KIOKUN_PORTS[testInfo.project.name]}`);
    },
  });

  test("a word's page runs under its policy, nothing refused", async ({ page }) => {
    const refused = await watching(page);
    expect(await visit(page, `/word/${encodeURIComponent("人")}`)).not.toContain("'wasm-unsafe-eval'");
    expect(await booted(page)).toBe("1");
    expect(await refused()).toEqual([]);
  });
});
