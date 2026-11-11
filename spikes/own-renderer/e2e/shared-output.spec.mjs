// ADR-0184, charter §15.6 tests 2, 12 and 13: what a cache may keep holds
// nothing of a session's. The store's page is a session's, so no cache keeps
// it, and the browser asks for it again rather than show a copy; a file of
// the build is every reader's, and names no session. What the server's own
// shared caches keep is the development server's tests'.
import { expect, test } from "@playwright/test";

async function ready(page, path = "/stores/47") {
  await page.goto(path);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
}

test("the store's page is kept by no cache, and a build's file names no session", async ({
  playwright,
  baseURL,
}) => {
  // A fresh visitor each time: no cookie, so the server names a session.
  const fresh = await playwright.request.newContext({ baseURL });
  try {
    const page = await fresh.get("/stores/47");
    expect(page.status()).toBe(200);
    expect(page.headers()["cache-control"]).toBe("private, no-store");
    expect(page.headers()["set-cookie"]).toMatch(/^pw-session=/);
  } finally {
    await fresh.dispose();
  }
  const first = await playwright.request.newContext({ baseURL });
  try {
    // The runtime, asked for before any page: every reader's, so no session.
    const runtime = await first.get("/pw-runtime.mjs");
    expect(runtime.status()).toBe(200);
    expect(runtime.headers()["set-cookie"]).toBeUndefined();
    expect(runtime.headers()["cache-control"] ?? "").not.toContain("private");
  } finally {
    await first.dispose();
  }
});

test("a shared build asset revalidates without persisting or disclosing a session cookie", async ({ playwright, baseURL }) => {
  const client = await playwright.request.newContext({ baseURL });
  try {
    const headers = { "accept-encoding": "identity" };
    const first = await client.get("/pw-runtime.mjs", { headers });
    expect(first.status()).toBe(200);
    expect(first.headers()["cache-control"]).toBe("public, no-cache");
    expect(first.headers()["set-cookie"]).toBeUndefined();
    const etag = first.headers().etag;
    expect(etag).toMatch(/^"[A-Za-z0-9_-]{22}"$/);
    const original = await first.body();
    expect(original.length).toBeGreaterThan(0);
    const second = await client.get("/pw-runtime.mjs", {
      headers: { ...headers, "if-none-match": etag },
    });
    expect(second.status()).toBe(304);
    expect(second.headers().etag).toBe(etag);
    expect(second.headers()["set-cookie"]).toBeUndefined();
    expect((await second.body()).length).toBe(0);
    const weak = await client.get("/pw-runtime.mjs", {
      headers: { ...headers, "if-none-match": `W/${etag}` },
    });
    // The server uses a conservative strong-validator check.
    // RFC 9110 §13.1.2: `If-None-Match` compares weakly.
    expect(weak.status()).toBe(304);
    const staticPage = await client.get("/HelloStatic.html");
    expect(staticPage.status()).toBe(200);
    expect(staticPage.headers()["content-security-policy"]).toContain("default-src 'none'");
  } finally {
    await client.dispose();
  }
});

// Read-your-writes across the history. These engines ask for the page again
// here whatever its response says; `no-store` is what makes a browser that
// keeps pages for going back ask again too, and the test above holds it.
test("back to the store, its cart is as it is now, not as it was left", async ({ page, context }) => {
  await ready(page);
  // Answered before the page is left (ADR-0268).
  const added = page.waitForResponse("**/command/store.page.add_to_cart");
  await page.getByRole("button", { name: "Add Espresso", exact: true }).click();
  await added;
  await expect(page.locator("#cart-count")).toHaveText("1");
  await ready(page, "/stores/48");
  // The same session adds again, in another tab, while the first is away.
  const other = await context.newPage();
  await ready(other);
  await other.getByRole("button", { name: "Add Cortado", exact: true }).click();
  await expect(other.locator("#cart-count")).toHaveText("2");
  await other.close();
  // A copy of the page as it was left would say 1.
  await page.goBack();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1", null, {
    polling: 50,
  });
  await expect(page.locator("#cart-count")).toHaveText("2");
  await page.getByRole("button", { name: "Add Cold Brew", exact: true }).click();
  await expect(page.locator("#cart-count")).toHaveText("3");
});
