// Track `identity` (ADR-0258): accounts and sign-in on the feed, in three
// engines. The feed is served on hosts of its own (IDENTITY_PORTS), one per
// engine, its sessions signed in through the development identity provider
// (`PW_IDENTITY=dev-accounts`), which this server serves at /dev-idp as a
// deployment's provider would be served at its own origin.
//   - a signed-out reader reads the timeline and is shown how to sign in,
//     where the post form's guard is, and is given neither a composer nor a
//     Like (ADR-0302, and the ADR of a control shown where its command's
//     predicates hold); a like pressed on a page whose reader signed out in
//     another tab is refused by the server (`requires SignedIn`, 403), taken
//     back, and told beside the button, in the feed's words for the
//     predicate, and said by the page's announcer; a tab whose reader signed
//     out in another is told so at its next post; a press no answer comes for
//     is told so;
//   - sign-up at the provider, back signed in, the session cookie HttpOnly
//     and SameSite=Lax and a new one;
//   - a post is its author's: named for them to every reader, and a Delete
//     button on their row alone; deleting it removes it from every open
//     timeline; another user has no Delete for it;
//   - sign-out forgets the session, and sign-in takes the password.
import { expect, test } from "@playwright/test";
import { IDENTITY_PORTS } from "../host-metadata.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${IDENTITY_PORTS[testInfo.project.name]}`);
  },
});

test.describe.configure({ mode: "serial" });

/** The home page, its handlers attached. */
async function home(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** A handle no other test or engine has: at most 15 of a-z, 0-9 and _. */
const handle = (testInfo, who) =>
  `${who}${testInfo.project.name.slice(0, 2)}${(Date.now() % 1e7).toString(36)}`.slice(0, 15);

/** The timeline's post with `text`. */
const post = (page, text) => page.locator("main > ul > li").filter({ hasText: text });

const PASSWORD = "correct horse battery";

/** Signed up at the development provider, from the home page, and back. */
async function signUp(page, who, name) {
  await home(page);
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

/** The session cookie's value, and the cookie. */
async function sessionCookie(context) {
  const cookies = await context.cookies();
  return cookies.find((c) => c.name === "pw-session");
}

/** What the page's announcer says, as a screen reader would hear it. */
const announced = (page) => page.locator(".pw-announcer[role=status]");

/**
 * The page's stream held from now: what tells it of a sign-out in another
 * tab is its next press. Its next stream request would carry the new
 * session, whose document it is not, and load it again, signed out, which
 * also tells the reader; which comes first is a race (found 2026-10-11 in
 * Firefox), and a page told at once of a principal's change is W8's.
 */
const holdStream = (page) =>
  page.route((url) => new URL(url).pathname === "/stream", () => {});

test("a signed-out reader reads, and is given neither a composer nor a Like", async ({ page }) => {
  await home(page);
  await expect(post(page, "Hello, feed.")).toHaveCount(1);
  await expect(page.locator("#signed-out")).toHaveText("Sign in to post, reply or like.");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign out" })).toHaveCount(0);
  // No composer to be refused at (ADR-0302), and no Like: the page asks
  // `SignedIn` of its reader, and shows a control where it holds.
  await expect(page.getByLabel("What's happening?")).toBeHidden();
  await expect(page.getByRole("button", { name: "Post" })).toBeHidden();
  await expect(post(page, "Hello, feed.").getByRole("button", { name: "Like" })).toHaveCount(0);
});

test("a like pressed on a page whose reader signed out in another tab is refused where it was pressed", async ({
  browser,
}, testInfo) => {
  // Signed in when the page was shown, so its Like is there; signed out in
  // another tab since: the press is refused before the command runs, and
  // told where it was pressed. The sign-out gave the browser a new session,
  // and the page was shown to the old one, so the host refuses it as a page
  // another reader's, before `requires` is read (track `store-accounts`,
  // Q3), in the platform's words.
  const context = await browser.newContext();
  const [page, there] = [await context.newPage(), await context.newPage()];
  const uncaught = [];
  page.on("pageerror", (e) => uncaught.push(e.message));
  await signUp(page, handle(testInfo, "lee"), `Lee ${testInfo.project.name}`);
  await holdStream(page);
  await home(there);
  await there.getByRole("button", { name: "Sign out" }).click();
  await expect(there.locator("#signed-out")).toBeVisible();
  const like = post(page, "Hello, feed.").getByRole("button", { name: "Like" });
  const likes = await post(page, "Hello, feed.").locator(".likes").innerText();
  const refused = page.waitForResponse((r) => r.url().includes("/command/feed.app.like"));
  // From the keyboard, where the focus is the button's in every engine.
  await like.focus();
  await like.press("Enter");
  expect((await refused).status()).toBe(403);
  const words = "You signed in or out in another tab. Reload this page to go on.";
  const message = post(page, "Hello, feed.").locator(".pw-refusal");
  await expect(message).toHaveText(words);
  // Named by the button, and said without the focus moving.
  const id = await message.getAttribute("id");
  await expect(like).toHaveAttribute("aria-describedby", new RegExp(`\\b${id}\\b`));
  await expect(like).toHaveAttribute("data-pw-handler-error", "refused:another-reader");
  await expect(announced(page)).toHaveText(words);
  // The focus stays where the press was.
  await expect(like).toBeFocused();
  // A refusal is the host's answer, told the reader: no developer's error
  // (ADR-0431). What the press raised has reached the test by now.
  await page.evaluate(() => 0);
  expect(uncaught).toEqual([]);
  // Taken back: the count is the server's.
  await expect(post(page, "Hello, feed.").locator(".likes")).toHaveText(likes);
  // Pressed again: cleared as it is pressed, and told again; the announcer
  // emptied before it is written, so the same words are said again.
  await page.evaluate(() => (window.__heard = { message: [], said: [] }));
  for (const [node, into] of [
    [message, "message"],
    [announced(page), "said"],
  ]) {
    await node.evaluate(
      (el, into) =>
        new MutationObserver(() => window.__heard[into].push(el.textContent)).observe(el, {
          childList: true,
          characterData: true,
          subtree: true,
        }),
      into,
    );
  }
  const again = page.waitForResponse((r) => r.url().includes("/command/feed.app.like"));
  await like.click();
  await again;
  await expect(message).toHaveText(words);
  await expect
    .poll(() => page.evaluate(() => window.__heard))
    .toEqual({ message: ["", words], said: ["", words] });
  // What the server answers a press from no page, signed out: the predicate
  // and the feed's words for it, nothing else.
  const answer = await page.request.post("/command/feed.app.like", {
    data: ["p1"],
    headers: { "pw-interaction": `refused-${Date.now()}` },
  });
  expect(answer.status()).toBe(403);
  expect(await answer.json()).toEqual({
    committed: false,
    refused: "SignedIn",
    says: "Sign in to post, reply, like or follow.",
  });
  await context.close();
});

test("the page's announcer is in it from the first byte, empty", async ({ request }) => {
  // Served by the host, before the runtime: a live region added with its
  // words is not reliably said (ADR-0182).
  const served = await (await request.get("/")).text();
  const found = served.match(/<div role="status" class="pw-announcer"[^>]*>(.*?)<\/div>/gs) ?? [];
  expect(found).toHaveLength(1);
  expect(found[0]).toMatch(/><\/div>$/);
  expect(served.indexOf(found[0])).toBeLessThan(served.indexOf('id="pw-parts"'));
});

test("a tab whose reader signed out in another is told so at its next post", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext();
  const [here, there] = [await context.newPage(), await context.newPage()];
  await signUp(here, handle(testInfo, "eve"), `Eve ${testInfo.project.name}`);
  await holdStream(here);
  await home(there);
  await there.getByRole("button", { name: "Sign out" }).click();
  await expect(there.locator("#signed-out")).toBeVisible();
  // Still showing a signed-in reader's page: its composer is there.
  const text = `Stale at ${Date.now()}`;
  await here.getByLabel("What's happening?").fill(text);
  const refused = here.waitForResponse((r) => r.url().includes("/command/feed.app.post"));
  await here.getByRole("button", { name: "Post" }).click();
  expect((await refused).status()).toBe(403);
  // Told beside the form, the post taken back, what was typed kept. The
  // sign-out gave the browser a new session, and the page was shown to the
  // old one: the host refuses the press as a page another reader's, before
  // `requires` is read (track `store-accounts`, Q3).
  await expect(here.locator("form + .pw-refusal").first()).toHaveText(
    "You signed in or out in another tab. Reload this page to go on.",
  );
  await expect(post(here, text)).toHaveCount(0);
  await expect(here.getByLabel("What's happening?")).toHaveValue(text);
  await context.close();
});

test("a press no answer comes for is told so", async ({ page }, testInfo) => {
  // A signed-in reader's Like, which the page shows them.
  await signUp(page, handle(testInfo, "ned"), `Ned ${testInfo.project.name}`);
  const uncaught = [];
  page.on("pageerror", (e) => uncaught.push(e.message));
  await page.route("**/command/feed.app.like", (route) => route.abort("failed"));
  const like = post(page, "Hello, feed.").getByRole("button", { name: "Like" });
  await like.click();
  await expect(post(page, "Hello, feed.").locator(".pw-refusal")).toHaveText(
    "This could not be sent. Check the connection and try again.",
  );
  await expect(like).toHaveAttribute("data-pw-handler-error", "unreachable");
  // No answer at all is the network's, told the reader: no developer's error
  // (ADR-0431). What the press raised has reached the test by now.
  await page.evaluate(() => 0);
  expect(uncaught).toEqual([]);
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("a post is its author's, and theirs alone to delete", async ({ browser }, testInfo) => {
  const [a, b] = [await browser.newContext(), await browser.newContext()];
  const [author, reader] = [await a.newPage(), await b.newPage()];
  await home(author);
  const before = await sessionCookie(a);
  const name = `Ada ${testInfo.project.name}`;
  await signUp(author, handle(testInfo, "ada"), name);
  // A new session, which no script reads, sent cross-site only on a
  // top-level navigation.
  const after = await sessionCookie(a);
  expect(after.value).not.toBe(before.value);
  expect(after.httpOnly).toBe(true);
  expect(after.sameSite).toBe("Lax");

  const text = `By ${name} at ${Date.now()}`;
  await author.getByLabel("What's happening?").fill(text);
  await author.getByRole("button", { name: "Post" }).click();
  await expect(post(author, text).locator(".author")).toHaveText(name);
  await expect(post(author, text).getByRole("button", { name: "Delete" })).toHaveCount(1);

  // Another reader, signed out, then signed in as someone else: the post
  // is named for its author, and has no Delete for them.
  await home(reader);
  await expect(post(reader, text).locator(".author")).toHaveText(name);
  await expect(post(reader, text).getByRole("button", { name: "Delete" })).toHaveCount(0);
  await signUp(reader, handle(testInfo, "grace"), `Grace ${testInfo.project.name}`);
  await expect(post(reader, text)).toHaveCount(1);
  await expect(post(reader, text).getByRole("button", { name: "Delete" })).toHaveCount(0);

  // Its author deletes it, and it is gone from every open timeline.
  await post(author, text).getByRole("button", { name: "Delete" }).click();
  await expect(post(author, text)).toHaveCount(0);
  await expect(post(reader, text)).toHaveCount(0);
  await a.close();
  await b.close();
});

test("signing out forgets the session, and signing in takes the password", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const who = handle(testInfo, "hopper");
  const name = `Grace Hopper ${testInfo.project.name}`;
  await signUp(page, who, name);
  const signedIn = await sessionCookie(context);
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.locator("#signed-out")).toBeVisible();
  const signedOut = await sessionCookie(context);
  expect(signedOut.value).not.toBe(signedIn.value);
  // The old session, sent again, is no one.
  const replayed = await browser.newContext();
  await replayed.addCookies([{ ...signedIn, sameSite: "Lax" }]);
  const old = await replayed.newPage();
  await home(old);
  await expect(old.locator("#signed-out")).toBeVisible();
  await replayed.close();

  // A wrong password is said so, at the provider.
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByLabel("Handle").fill(who);
  await page.getByLabel("Password").fill("not the password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "That handle and password do not match an account.",
  );
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
  await context.close();
});
