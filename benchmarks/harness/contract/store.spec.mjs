// The canonical store's behavioural contract (charter §15.3), as all three
// stacks implement it today. It is the floor every benchmark task starts
// from: a task's hidden tests add to it, and a change that breaks it fails.
//
// What the contract does NOT include, and why (ADR-0120):
// - an optimistic count before the round trip, and its rollback;
// - one add per interaction under a retried request.
// The Pleris store declares both (`optimistic`, `idempotent_by`) and, since
// ADR-0121 and ADR-0122, performs both; the other two stacks perform neither,
// so neither is part of the shared floor. They are what T01 and T08 ask for.

import { test, expect } from "./fixtures.mjs";

const MENU = ["Espresso", "Cortado", "Cold Brew"];

test("the page names the store, lists the menu, and shows an empty cart", async ({ page, store }) => {
  await store.open();
  await expect(page.locator("#store-name")).toHaveText("Blue Bottle");
  await expect(page.locator("#menu li")).toHaveCount(3);
  for (const [i, name] of MENU.entries()) {
    await expect(page.locator("#menu li").nth(i)).toContainText(name);
    await expect(page.locator("#menu li").nth(i).getByRole("button")).toHaveText("Add");
  }
  await expect(store.count()).toHaveText("0");
  await expect(page.locator("#clear-cart")).toHaveText("Clear");
});

test("the page is complete with JavaScript disabled", async ({ browser, storePath, baseURL }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL });
  const page = await context.newPage();
  await page.goto(storePath);
  await expect(page.locator("#store-name")).toHaveText("Blue Bottle");
  await expect(page.locator("#menu li")).toHaveCount(3);
  await expect(page.locator("#cart-count")).toHaveText("0");
  await context.close();
});

test("the menu and cart are labelled regions", async ({ page, store }) => {
  await store.open();
  await expect(page.getByRole("region", { name: "Menu" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Cart" })).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Blue Bottle");
});

test("each Add adds one item", async ({ store }) => {
  await store.open();
  for (const n of ["1", "2", "3"]) {
    await store.add();
    await expect(store.count()).toHaveText(n);
  }
});

test("different items add up", async ({ store }) => {
  await store.open();
  await store.add(undefined, 0);
  await expect(store.count()).toHaveText("1");
  await store.add(undefined, 1);
  await expect(store.count()).toHaveText("2");
  await store.add(undefined, 2);
  await expect(store.count()).toHaveText("3");
});

test("Clear empties the cart", async ({ store }) => {
  await store.open();
  await store.add();
  await store.add();
  await expect(store.count()).toHaveText("2");
  await store.clear();
  await expect(store.count()).toHaveText("0");
});

test("the cart survives a reload", async ({ page, store }) => {
  await store.open();
  await store.add();
  await expect(store.count()).toHaveText("1");
  await page.reload();
  await expect(store.count()).toHaveText("1");
});

test("two presses in quick succession are two adds", async ({ store }) => {
  await store.open();
  await Promise.all([store.add(), store.add()]);
  await expect(store.count()).toHaveText("2");
});

test("one session's cart is not another's", async ({ browser, baseURL, store }) => {
  const a = await browser.newContext({ baseURL });
  const b = await browser.newContext({ baseURL });
  const pa = await a.newPage();
  const pb = await b.newPage();
  await store.open(pa);
  await store.open(pb);

  await store.add(pa);
  await store.add(pa);
  await expect(store.count(pa)).toHaveText("2");

  await store.open(pb);
  await expect(store.count(pb)).toHaveText("0");
  await store.add(pb);
  await expect(store.count(pb)).toHaveText("1");

  await pa.reload();
  await expect(store.count(pa)).toHaveText("2");
  await a.close();
  await b.close();
});
