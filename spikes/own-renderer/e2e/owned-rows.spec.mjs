import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { OWNED_PORTS } from "../host-metadata.mjs";

test.use({ baseURL: async ({}, use, info) => use(`http://127.0.0.1:${OWNED_PORTS[info.project.name]}`) });

async function signUp(page, who) {
  await page.goto("/");
  await page.getByRole("button", { name: "Sign up" }).click();
  await page.getByLabel("Handle").fill(`${who}${Date.now().toString(36)}`.slice(0, 15));
  await page.getByLabel("Name").fill(who);
  await page.getByLabel("Password").fill("correct horse battery");
  await page.getByRole("button", { name: "Sign up" }).click();
  await expect(page).toHaveURL(/\/$/);
  await page.goto("/notes");
  return page.locator("#reader").textContent();
}

async function command(page, name, args) {
  const response = await page.request.post(`/command/owned.app.${name}`, { data: args });
  return { status: response.status(), body: await response.json() };
}

async function keep(page, author, to, text, draft) {
  const result = await command(page, "keep", [{ author, id: "same-id", to, text, draft, added: 0 }]);
  expect(result.body.committed).toBe(true);
}

test("two readers own independent rows of the same key and cannot forge an author", async ({ browser }, info) => {
  const a = await browser.newContext(), b = await browser.newContext();
  try {
    const alice = await a.newPage(), bob = await b.newPage();
    const aid = await signUp(alice, `Alice${info.project.name.slice(0, 2)}`);
    const bid = await signUp(bob, `Bob${info.project.name.slice(0, 2)}`);
    expect(aid).not.toBe(bid);
    await keep(alice, aid, bid, "ALICE_TEXT", "ALICE_PRIVATE_DRAFT");
    await bob.reload();
    await expect(bob.locator("#mine li")).toHaveCount(0);
    await expect(bob.locator("#shared")).toHaveText("ALICE_TEXT");
    expect(await bob.content()).not.toContain("ALICE_PRIVATE_DRAFT");
    await keep(bob, bid, aid, "BOB_TEXT", "BOB_PRIVATE_DRAFT");
    const forged = await command(bob, "keep", [{ author: aid, id: "same-id", to: bid, text: "DEFACED", draft: "DEFACED", added: 0 }]);
    expect(forged.body.committed).toBe(false);
    await alice.reload();
    await expect(alice.locator("#mine .text")).toHaveText("ALICE_TEXT");
    await expect(alice.locator("#mine .draft")).toHaveText("ALICE_PRIVATE_DRAFT");
    await expect(alice.locator("#shared")).toHaveText("BOB_TEXT");
    expect(await alice.content()).not.toContain("BOB_PRIVATE_DRAFT");
    const removed = await command(bob, "remove", ["same-id", aid]);
    expect(removed.body.committed).toBe(true);
    await bob.reload();
    await expect(bob.locator("#mine li")).toHaveCount(0);
    await alice.reload();
    await expect(alice.locator("#mine .text")).toHaveText("ALICE_TEXT");
  } finally { await a.close(); await b.close(); }
});

test("a guarded query tells its predicate words and never another reader's note", async ({ browser }, info) => {
  const a = await browser.newContext(), b = await browser.newContext();
  try {
    const alice = await a.newPage(), bob = await b.newPage();
    const aid = await signUp(alice, `Ann${info.project.name.slice(0, 2)}`);
    const bid = await signUp(bob, `Ben${info.project.name.slice(0, 2)}`);
    await keep(alice, aid, bid, "GUARDED_ALICE_NOTE", "UNSHARED_DRAFT");
    const allowed = await alice.goto("/owned/same-id");
    expect(allowed.status()).toBe(200);
    await expect(alice.locator("#note")).toHaveText("GUARDED_ALICE_NOTE");
    const denied = await bob.goto("/owned/same-id");
    expect(denied.status()).toBe(403);
    await expect(bob.getByRole("main")).toHaveText("You can only change a note you wrote.");
    expect(await bob.content()).not.toContain("GUARDED_ALICE_NOTE");
    expect(await bob.content()).not.toContain("UNSHARED_DRAFT");
    await keep(bob, bid, aid, "GUARDED_BOB_NOTE", "BOB_DRAFT");
    const own = await bob.goto("/owned/same-id");
    expect(own.status()).toBe(200);
    await expect(bob.locator("#note")).toHaveText("GUARDED_BOB_NOTE");
  } finally { await a.close(); await b.close(); }
});
