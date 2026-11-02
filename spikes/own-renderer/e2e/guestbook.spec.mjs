// The guide's tutorial app (ADR-0343, a guide whose examples compile): the
// guestbook `docs/guide/tutorial.md` builds, in three engines, on hosts of
// its own (GUESTBOOK_PORTS), its sessions signed in through the development
// identity provider (`PW_IDENTITY=dev-accounts`). Its data is the tables it
// declares, served by the development server's generic memory adapter, with
// no data layer of its own (track `data-path`). What the tutorial says the
// app does, it does:
//   - a signed-out reader reads the entries, is asked to sign in, and is
//     given no composer and no Remove;
//   - signed up, a guest signs the guestbook: the entry is shown at once,
//     before the server answers, then as the server wrote it, under their
//     name, theirs to remove and no one else's;
//   - another reader's open page is told of the entry, and of its removal,
//     without a reload.
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { GUESTBOOK_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${GUESTBOOK_PORTS[testInfo.project.name]}`);
  },
});

test.describe.configure({ mode: "default" });

/** The guestbook, its handlers attached. */
async function home(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** A handle no other test or engine has: at most 15 of a-z, 0-9 and _. */
const handle = (testInfo, who) =>
  `${who}${testInfo.project.name.slice(0, 2)}${(Date.now() % 1e7).toString(36)}`.slice(0, 15);

/** The entry that says `text`. */
const entry = (page, text) => page.locator("main > ul > li").filter({ hasText: text });

const PASSWORD = "correct horse battery";

/** Signed up at the development provider, from the guestbook, and back. */
async function signUp(page, who, name) {
  await home(page);
  await expect(page.getByRole("button", { name: "Sign up" })).toBeVisible();
  await page.getByRole("button", { name: "Sign up" }).click();
  await expect(page.getByRole("note")).toContainText("not for production");
  await page.getByLabel("Handle").fill(who);
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign up" }).click();
  await expect(page).toHaveURL(/\/$/);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
}

/** `page`'s reader signs `words`, and the entry is the server's. */
async function sign(page, words, name) {
  await page.getByLabel("Your message").fill(words);
  await page.getByRole("button", { name: "Sign", exact: true }).click();
  await expect(entry(page, words).locator(".by")).toHaveText(name);
}

test("a signed-out reader reads the entries and is given no composer", async ({ browser }, testInfo) => {
  // Someone signs first, so there is an entry to read.
  const writer = await browser.newContext();
  const page = await writer.newPage();
  const name = `Ann ${testInfo.project.name}`;
  await signUp(page, handle(testInfo, "ann"), name);
  const words = `Welcome, from ${testInfo.project.name} ${Date.now()}`;
  await sign(page, words, name);
  await writer.close();
  const reader = await browser.newContext();
  const other = await reader.newPage();
  await home(other);
  await expect(other).toHaveTitle("Guestbook");
  await expect(entry(other, words)).toHaveCount(1);
  await expect(entry(other, words).locator(".by")).toHaveText(name);
  await expect(other.locator("#signed-out")).toHaveText("Sign in to sign the guestbook.");
  await expect(other.getByRole("button", { name: "Sign in" })).toBeVisible();
  await expect(other.getByLabel("Your message")).toBeHidden();
  await expect(other.getByRole("button", { name: "Remove" })).toHaveCount(0);
  await reader.close();
});

test("a guest signs up, signs, and the entry is theirs alone to remove", async ({ browser }, testInfo) => {
  const [mine, theirs] = [await browser.newContext(), await browser.newContext()];
  const [page, other] = [await mine.newPage(), await theirs.newPage()];
  const name = `Grace ${testInfo.project.name}`;
  await signUp(page, handle(testInfo, "grace"), name);
  const words = `Thanks for having us, from ${testInfo.project.name} ${Date.now()}`;
  await page.getByLabel("Your message").fill(words);
  await page.getByRole("button", { name: "Sign", exact: true }).click();
  // The draft is cleared, and the entry is there, first, by its author.
  await expect(page.getByLabel("Your message")).toHaveValue("");
  await expect(page.locator("main > ul > li").first()).toContainText(words);
  await expect(entry(page, words).locator(".by")).toHaveText(name);
  // Theirs to remove, once the server's row stands in the pending one's
  // place; and another signed-in reader is offered no Remove on it.
  await expect(entry(page, words).getByRole("button", { name: "Remove" })).toHaveCount(1);
  await signUp(other, handle(testInfo, "alan"), `Alan ${testInfo.project.name}`);
  await expect(entry(other, words)).toHaveCount(1);
  await expect(entry(other, words).getByRole("button", { name: "Remove" })).toHaveCount(0);
  await mine.close();
  await theirs.close();
});

test("an entry is shown before the server answers, and offers no Remove until it has", async ({ browser }, testInfo) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const name = `Joan ${testInfo.project.name}`;
  await signUp(page, handle(testInfo, "joan"), name);
  // The command's request is held at the network, so "before the server
  // answers" is observed, not inferred from timing.
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/command/guestbook.book.sign", async (route) => {
    await held;
    await route.continue();
  });
  const words = `Shown at once, from ${testInfo.project.name} ${Date.now()}`;
  await page.getByLabel("Your message").fill(words);
  await page.getByRole("button", { name: "Sign", exact: true }).click();
  // The server has not answered, and the page already shows the entry:
  // first, by "You", with no Remove button, since it is not the server's row.
  await expect(page.locator("main > ul > li").first()).toContainText(words);
  await expect(entry(page, words).locator(".by")).toHaveText("You");
  await expect(entry(page, words).getByRole("button", { name: "Remove" })).toHaveCount(0);
  release();
  // The server's row takes its place: its author named as the server names
  // it, one row, and theirs to remove.
  await expect(entry(page, words).locator(".by")).toHaveText(name);
  await expect(entry(page, words)).toHaveCount(1);
  await expect(entry(page, words).getByRole("button", { name: "Remove" })).toHaveCount(1);
  await context.close();
});

test("another reader's open page is told of an entry, and of its removal", async ({ browser }, testInfo) => {
  const [writer, reader] = [await browser.newContext(), await browser.newContext()];
  const [page, other] = [await writer.newPage(), await reader.newPage()];
  await signUp(page, handle(testInfo, "ada"), `Ada ${testInfo.project.name}`);
  await home(other);
  const words = `Live, from ${testInfo.project.name} ${Date.now()}`;
  await page.getByLabel("Your message").fill(words);
  await page.getByRole("button", { name: "Sign", exact: true }).click();
  // Told, with no reload: the command's event drops the reader's entries.
  await expect(entry(other, words)).toHaveCount(1);
  await expect(entry(other, words).getByRole("button", { name: "Remove" })).toHaveCount(0);
  await expect(entry(page, words).getByRole("button", { name: "Remove" })).toHaveCount(1);
  await entry(page, words).getByRole("button", { name: "Remove" }).click();
  await expect(entry(page, words)).toHaveCount(0);
  await expect(entry(other, words)).toHaveCount(0);
  await writer.close();
  await reader.close();
});
