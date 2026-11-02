// ADR-0220: the feed reference app (ADR-0218) in three engines. A second
// program served by the same host as the store, with its own data layer,
// built by `feed.sh` and served on a host per engine: a post is every
// reader's, so one engine's posts are never another's, and the tests of a
// host run one at a time.
//   - the home page shows the timeline, a thread page a post's replies as
//     deep as they go, and a post that is not there is not found;
//   - a post reaches the author's timeline, and every other open reader's
//     without a reload (ADR-0219), and the draft is cleared;
//   - a like's count reaches every reader;
//   - a post shows before the server answers, and is the server's after;
//     one whose request fails is taken back (ADR-0222);
//   - a post's text is at most 280 code points: a longer draft is not sent
//     (ADR-0225);
//   - "Load more" shows the longer page, and a post after it is shown over
//     the longer page (ADR-0222, ADR-0224);
//   - a thread's counts are values its template computes, the host's, shown
//     with scripts off, and a like's reaches the open thread (ADR-0226);
//   - what is left of a draft, and whether there is a post to send, are
//     computed from it as it is typed, and the host's first (ADR-0227);
//   - a row's likes are counted in words, and a like shows before the
//     server answers, its count computed again in the browser (ADR-0228);
//   - a condition computed: the browser's for a draft too long, the host's
//     for a thread with no reply (ADR-0229);
//   - a reply reaches its thread and every reader of it, and no timeline:
//     the thread page's form passes the page's parameter (ADR-0231);
//   - a reply shows before the server answers, its count and the thread's
//     "No replies yet." with it, and is the server's after; one whose request
//     fails is taken back (ADR-0236, ruling 0122-d);
//   - a like on the thread page shows before the server answers, though
//     `like` speculates on the timeline too, which the page does not show
//     (ADR-0238);
//   - a post shown before the server answers waits: nothing on it links or
//     acts until the server's row stands in its place (ADR-0275).
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { FEED_PORTS } from "../host-metadata.mjs";
import { rendererWithTraps } from "../renderer-fault.mjs";

test.use({
  baseURL: async ({}, use, testInfo) => {
    await use(`http://127.0.0.1:${FEED_PORTS[testInfo.project.name]}`);
  },
});

// Keep this host sequential, but execute every independent case after a failure.
// Mutation evidence requires the complete inventory, including later cases.
test.describe.configure({ mode: "default" });

// What the network saw of the page's subscription and reads: when each
// `/stream` and `/pw-read` was asked, answered and ended, a stream's
// cursors and a read's answer. Recorded beside the page, not in it.
const network = new WeakMap();
/** Record what the network sees of `page`'s subscription and reads. */
function watch(page) {
  const seen = [];
  network.set(page, seen);
  const started = Date.now();
  const at = () => `${Date.now() - started} ms`;
  const named = (url) => url.match(/\/((?:stream|pw-read)\?.*)$/)?.[1].slice(0, 80);
  page.on("request", (r) => named(r.url()) && seen.push(`${at()} asked ${named(r.url())}`));
  page.on("response", (r) => {
    if (named(r.url())) seen.push(`${at()} answered ${r.status()} ${named(r.url())}`);
  });
  page.on("requestfailed", (r) => {
    if (named(r.url())) seen.push(`${at()} failed ${named(r.url())}: ${r.failure()?.errorText}`);
  });
  page.on("requestfinished", async (r) => {
    if (!named(r.url())) return;
    const ended = at();
    const body = await r
      .response()
      .then((answer) => answer?.text())
      .catch(() => null);
    const said =
      body == null
        ? "its body unread"
        : r.url().includes("/stream?")
          ? `${body.length} bytes, cursors ${[...body.matchAll(/"cursor":(\d+)/g)].map((m) => m[1]).join(",") || "none"}`
          : body.slice(0, 80);
    seen.push(`${ended} ended ${named(r.url())}: ${said}`);
  });
}
test.beforeEach(async ({ page }) => watch(page));

// What the page's runtime said, beside a failure and its trace: its log,
// its transport, how often it asked again, the versions it holds and
// knows, and what the network saw. WebKit's "Load more" failed on CI with
// none of it (runs 37663299969, 37681083688): the server answered the read,
// and the page kept its twenty rows. With the runtime's (run 37707617400),
// the read was applied, `{"applied":1}`, one stream was asked and none
// after it, and the runtime logged no failure.
/**
 * Attach what `page`'s runtime and the server said of its document, each
 * named with `name`: a test of several pages records each (run 37893301747:
 * a follow's second reader was told nothing, and only the first page's
 * record could be attached).
 */
async function record(page, testInfo, name = "") {
  const said = await page
    .evaluate(() => ({
      log: window.__pw?.log ?? null,
      transport: window.__pw?.transport ?? null,
      reconnects: window.__pw?.reconnects ?? 0,
      reads: window.__pw?.reads ?? 0,
      held: window.__pwHeld?.() ?? null,
      known: window.__pwKnown?.() ?? null,
    }))
    .catch((e) => ({ unavailable: String(e) }));
  said.network = network.get(page) ?? null;
  await testInfo.attach(`runtime${name}`, {
    body: JSON.stringify(said, null, 2),
    contentType: "application/json",
  });
  // And the server's: what happened to the document's frames, and each
  // long hold of its table. With the runtime's record alone (run
  // 37877464406), the read was applied and the stream never seen to end,
  // and nothing said where the frames were.
  const doc = [...(said.network ?? []).join("\n").matchAll(/stream\?doc=(\d+)/g)].at(-1)?.[1];
  if (doc) {
    const server = await page.request
      .get(`/bench/records?doc=${doc}`)
      .then((r) => r.json())
      .catch((e) => ({ unavailable: String(e) }));
    await testInfo.attach(`server${name}`, {
      body: JSON.stringify(server, null, 2),
      contentType: "application/json",
    });
    said.server = server;
  }
  // And printed, the latest of each, one line a page: a mutation script's
  // red baseline shows a failing test's records (`pw-record`), where a
  // recipe keeps no attachment. "a follow reaches another reader" failed in
  // five of them in run 37918808029, and in no browser job.
  const latest = (list, n) => (Array.isArray(list) ? list.slice(-n) : list);
  console.log(
    `pw-record${name} ${JSON.stringify({
      log: latest(said.log, 40),
      network: latest(said.network, 40),
      transport: said.transport,
      reconnects: said.reconnects,
      trail: latest(said.server?.trail, 80),
      slow: latest(said.server?.slow, 20),
      you: said.server?.you,
      tellings: latest(said.server?.tellings, 60),
      at: said.server?.at,
    })}`,
  );
}
test.afterEach(async ({ page }, testInfo) => {
  if (testInfo.status === testInfo.expectedStatus) return;
  await record(page, testInfo);
});

/** The home page, its handlers attached. */
async function home(page) {
  await page.goto("/");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  // A mark a reload would lose: what reaches the page is sent to it, never
  // read by reading the page again.
  await page.evaluate(() => {
    window.__unreloaded = true;
  });
}

const unreloaded = (page) => page.evaluate(() => window.__unreloaded === true);

/** The timeline's post with `text`. */
const post = (page, text) => page.locator("main > ul > li").filter({ hasText: text });

/** A post's like count, as its row says it in words (ADR-0228). */
async function likes(page, text) {
  const said = await post(page, text).locator(".likes").innerText();
  return Number(said.match(/^(\d+) like/)[1]);
}

/** A count in words, as the feed's `counted` writes it. */
const counted = (n, one, many) => (n === 1 ? `1 ${one}` : `${n} ${many}`);

test("the home page shows the timeline, and a thread its replies", async ({ page }) => {
  await home(page);
  await expect(post(page, "Hello, feed.")).toHaveCount(1);
  // As deep as they go (ADR-0203): a reply, and the reply to it.
  await page.goto("/post/p1");
  const thread = page.locator("main");
  await expect(thread).toContainText("Hello, feed.");
  await expect(thread).toContainText("Hello, Ada.");
  await expect(thread).toContainText("And a reply to the reply.");
  const missing = await page.goto("/post/none");
  expect(missing.status()).toBe(404);
});

test("a post reaches the author's timeline and every other reader's", async ({
  browser,
}, testInfo) => {
  const [a, b] = [await browser.newContext(), await browser.newContext()];
  const [author, reader] = [await a.newPage(), await b.newPage()];
  for (const page of [author, reader]) await home(page);
  const text = `Posted in ${testInfo.project.name} at ${Date.now()}`;
  const draft = author.getByLabel("What's happening?");
  await draft.fill(text);
  await expect(author.getByRole("button", { name: "Post" })).toBeEnabled();
  await author.getByRole("button", { name: "Post" }).click();
  await expect(post(author, text)).toHaveCount(1);
  await expect(draft).toHaveValue("");
  // Another reader's open timeline, told by the host (ADR-0219), and the
  // author named the same to it as to the author (ADR-0220).
  await expect(post(reader, text)).toHaveCount(1);
  // The author's own row is "You" until the server's arrives (ADR-0222):
  // waited for, not read at once. In one run of ADR-0229's, Firefox read
  // the pending row.
  await expect(post(author, text).locator(".author")).toHaveText(/^Guest /);
  const name = await post(author, text).locator(".author").innerText();
  await expect(post(reader, text).locator(".author")).toHaveText(name);
  expect(await unreloaded(author)).toBe(true);
  expect(await unreloaded(reader)).toBe(true);
  await a.close();
  await b.close();
});

test("a like's count reaches every reader", async ({ browser }) => {
  const [a, b] = [await browser.newContext(), await browser.newContext()];
  const [liker, reader] = [await a.newPage(), await b.newPage()];
  for (const page of [liker, reader]) await home(page);
  const before = await likes(reader, "Hello, feed.");
  await post(liker, "Hello, feed.").getByRole("button", { name: "Like" }).click();
  const after = counted(before + 1, "like", "likes");
  await expect(post(liker, "Hello, feed.").locator(".likes")).toHaveText(after);
  await expect(post(reader, "Hello, feed.").locator(".likes")).toHaveText(after);
  expect(await unreloaded(reader)).toBe(true);
  await a.close();
  await b.close();
});

test("what is left of a draft is computed as it is typed", async ({ browser }) => {
  // ADR-0227: `{280 - String.length(draft)} left`, and the button disabled
  // while there is no post to send. With scripts off, the host's first
  // values, from the draft's first value.
  const off = await browser.newContext({ javaScriptEnabled: false });
  const still = await off.newPage();
  await still.goto("/");
  await expect(still.locator("#left")).toHaveText("280 left");
  await expect(still.getByRole("button", { name: "Post" })).toBeDisabled();
  await off.close();
  // In the browser, as the draft changes: the page's module, loaded with
  // the first change, computes each again.
  const context = await browser.newContext();
  const page = await context.newPage();
  await home(page);
  const draft = page.getByLabel("What's happening?");
  const button = page.getByRole("button", { name: "Post" });
  await expect(page.locator("#left")).toHaveText("280 left");
  await expect(button).toBeDisabled();
  await draft.fill("Hello");
  await expect(page.locator("#left")).toHaveText("275 left");
  await expect(button).toBeEnabled();
  await draft.fill("");
  await expect(page.locator("#left")).toHaveText("280 left");
  await expect(button).toBeDisabled();
  expect(await unreloaded(page)).toBe(true);
  await context.close();
});

test("a condition is computed: a draft too long, and a thread with no reply", async ({
  page,
}) => {
  // ADR-0229. The home page's `{#if String.length(draft) > 280}` is the
  // browser's, which renders the block again as the draft changes; a
  // thread's `{#if List.length(thread.replies) == 0}` is the host's.
  await home(page);
  const draft = page.getByLabel("What's happening?");
  await expect(page.locator("#over")).toHaveCount(0);
  await draft.fill("x".repeat(281));
  await expect(page.getByRole("alert")).toHaveText("Too long to post.");
  await draft.fill("x".repeat(280));
  await expect(page.locator("#over")).toHaveCount(0);
  expect(await unreloaded(page)).toBe(true);
  await page.goto("/post/p3");
  await expect(page.locator("#quiet")).toHaveText("No replies yet.");
  await page.goto("/post/p1");
  await expect(page.locator("#counts")).toContainText("1 reply");
  await expect(page.locator("#quiet")).toHaveCount(0);
});

test("a thread's counts are computed by the host, and a like's reaches it", async ({
  browser,
}) => {
  // With scripts off: the host computed them into the page.
  const off = await browser.newContext({ javaScriptEnabled: false });
  const still = await off.newPage();
  await still.goto("/post/p3");
  await expect(still.locator("#counts")).toHaveText("0 replies · 1 like");
  await off.close();
  const [a, b] = [await browser.newContext(), await browser.newContext()];
  const [liker, reader] = [await a.newPage(), await b.newPage()];
  await home(liker);
  await reader.goto("/post/p1");
  await reader.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await reader.evaluate(() => {
    window.__unreloaded = true;
  });
  const before = await likes(liker, "Hello, feed.");
  const counts = reader.locator("#counts");
  await expect(counts).toHaveText(`1 reply · ${counted(before, "like", "likes")}`);
  // Another session's like: the thread is a new value, so a new count, sent
  // to the open page as its text.
  await post(liker, "Hello, feed.").getByRole("button", { name: "Like" }).click();
  await expect(counts).toHaveText(`1 reply · ${counted(before + 1, "like", "likes")}`);
  expect(await unreloaded(reader)).toBe(true);
  await a.close();
  await b.close();
});

test("a post shows before the server answers, and is the server's after", async ({
  page,
}, testInfo) => {
  await home(page);
  // The request held at the network: what shows meanwhile is the page's own.
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.post", async (route) => {
    await held;
    await route.continue();
  });
  const text = `Shown at once in ${testInfo.project.name}, ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  await expect(post(page, text)).toHaveCount(1);
  await expect(post(page, text).locator(".author")).toHaveText("You");
  await expect(page.locator("main > ul > li").first()).toContainText(text);
  release();
  // The server's: its author named as the server names it, and one row.
  await expect(post(page, text).locator(".author")).toHaveText(/^Guest /);
  await expect(post(page, text)).toHaveCount(1);
  await expect(page.getByLabel("What's happening?")).toHaveValue("");
  expect(await unreloaded(page)).toBe(true);
});

test("a post shown before the server answers waits, and acts once it is the server's", async ({
  page,
}, testInfo) => {
  // ADR-0275: its id is the page's own, `pending-..`, which no server has; a
  // like pressed on it found no such post (ADR-0274). Nothing on it links or
  // acts until the server's row stands in its place.
  await home(page);
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.post", async (route) => {
    await held;
    await route.continue();
  });
  const text = `Waiting in ${testInfo.project.name}, ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  const row = post(page, text);
  await expect(row.locator(".author")).toHaveText("You");
  for (const name of [".author", ".handle"]) {
    await expect(row.locator(name)).not.toHaveAttribute("href");
  }
  await expect(row.getByRole("button", { name: "Like" })).toBeDisabled();
  await expect(row.getByRole("button", { name: "Delete" })).toBeDisabled();
  release();
  // The server's: it links to the post and to its author, and a like
  // pressed on it is the server's.
  await expect(row.locator(".author")).toHaveAttribute("href", /^\/post\/p\d+$/);
  await expect(row.locator(".handle")).toHaveAttribute("href", /^\/user\/.+$/);
  await expect(row.getByRole("button", { name: "Delete" })).toBeEnabled();
  const liked = page.waitForResponse("**/command/feed.app.like");
  await row.getByRole("button", { name: "Like" }).click();
  await liked;
  await expect(row.locator(".likes")).toHaveText("1 like");
  expect(await unreloaded(page)).toBe(true);
  await page.reload();
  await expect(post(page, text).locator(".likes")).toHaveText("1 like");
});

test("a like shows before the server answers, and one that fails is taken back", async ({
  page,
}) => {
  // ADR-0228: the like's transition counts it in the timeline the page
  // holds, and each row's count in words is computed again from it, in
  // the browser, as the host computes it for the rows it renders.
  await home(page);
  const before = await likes(page, "Hello, feed.");
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.like", async (route) => {
    await held;
    await route.continue();
  });
  await post(page, "Hello, feed.").getByRole("button", { name: "Like" }).click();
  const count = post(page, "Hello, feed.").locator(".likes");
  await expect(count).toHaveText(counted(before + 1, "like", "likes"));
  // The server's, once it has answered: the same count, in one row.
  const answered = page.waitForResponse((r) => r.url().includes("/command/feed.app.like"));
  release();
  await answered;
  await expect(count).toHaveText(counted(before + 1, "like", "likes"));
  await expect(post(page, "Hello, feed.")).toHaveCount(1);
  await page.unroute("**/command/feed.app.like");
  // A like whose request fails is taken back.
  let fail;
  const failing = new Promise((r) => (fail = r));
  await page.route("**/command/feed.app.like", async (route) => {
    await failing;
    await route.abort("failed");
  });
  await post(page, "Hello, feed.").getByRole("button", { name: "Like" }).click();
  await expect(count).toHaveText(counted(before + 2, "like", "likes"));
  fail();
  await expect(count).toHaveText(counted(before + 1, "like", "likes"));
  expect(await unreloaded(page)).toBe(true);
});

test("a post whose request fails is taken back", async ({ page }, testInfo) => {
  await home(page);
  const before = await page.locator("main > ul > li").count();
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.post", async (route) => {
    await held;
    await route.abort("failed");
  });
  const text = `Never sent from ${testInfo.project.name}, ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  await expect(post(page, text)).toHaveCount(1);
  release();
  await expect(post(page, text)).toHaveCount(0);
  await expect(page.locator("main > ul > li")).toHaveCount(before);
  // The draft is kept, to send again.
  await expect(page.getByLabel("What's happening?")).toHaveValue(text);
});

test("a post longer than 280 characters is not sent, and 280 emoji are", async ({ page }) => {
  await home(page);
  const sent = [];
  page.on("request", (r) => {
    if (r.url().includes("/command/feed.app.post")) sent.push(r.postData());
  });
  const draft = page.getByLabel("What's happening?");
  const button = page.getByRole("button", { name: "Post" });
  // The page computes what is left, and whether there is a post to send,
  // from the draft (ADR-0227): one character over, and nothing to press.
  await draft.fill("x".repeat(281));
  await expect(page.locator("#left")).toHaveText("-1 left");
  await expect(button).toBeDisabled();
  // A submit that comes another way meets the page's own check,
  // `post_text`, which finds no post's text: nothing is sent, and the draft
  // is kept to shorten.
  // The post form, beside the attach form (track `uploads`).
  await page
    .locator("form")
    .filter({ has: draft })
    .evaluate((form) => form.requestSubmit());
  await expect(draft).toHaveValue("x".repeat(281));
  // 280 emoji are 280 code points, as `String.length` counts them, though
  // 560 UTF-16 units: none left, sent, and posted. Its row says the press
  // before it was answered too.
  const emoji = "\u{1F600}".repeat(280);
  await draft.fill(emoji);
  await expect(page.locator("#left")).toHaveText("0 left");
  await expect(button).toBeEnabled();
  await button.click();
  await expect(post(page, emoji)).toHaveCount(1);
  await expect(post(page, emoji).locator(".author")).toHaveText(/^Guest /);
  expect(sent).toHaveLength(1);
});

test("Load more shows the next page, and a post after it is shown over it", async ({
  browser,
  page,
}, testInfo) => {
  // Twenty-five posts by another session, through the command route as a
  // page sends them.
  const other = await browser.newContext();
  for (let i = 0; i < 25; i++) {
    const r = await other.request.post("/command/feed.app.post", {
      headers: { "content-type": "application/json", "pw-interaction": `seed-${Date.now()}-${i}` },
      data: [`Older ${testInfo.project.name} ${i}`],
    });
    expect(r.status()).toBe(202);
  }
  await other.close();
  await home(page);
  const rows = page.locator("main > ul > li");
  await expect(rows).toHaveCount(20);
  await page.getByRole("button", { name: "Load more" }).click();
  await expect.poll(async () => (await rows.count()) > 20).toBe(true);
  const longer = await rows.count();
  expect(longer).toBeLessThanOrEqual(40);
  // A post held at the network is shown over the page the browser holds now,
  // the longer one: one row more, the post first.
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.post", async (route) => {
    await held;
    await route.continue();
  });
  // Unique among repeats: two run at once wrote one text, and a test read
  // the other's post as its own.
  const text = `After more in ${testInfo.project.name}, ${Date.now()} ${testInfo.repeatEachIndex}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  await expect(rows.first()).toContainText(text);
  await expect(rows).toHaveCount(longer + 1);
  release();
  await expect(post(page, text).locator(".author")).toHaveText(/^Guest /);
  expect(await unreloaded(page)).toBe(true);
});

test("a reply reaches its thread and every reader of it, and no timeline", async ({
  browser,
}, testInfo) => {
  // ADR-0231. The thread page's form passes its `id`, the page's parameter,
  // which its handler captures. Until then a page that bound a query was
  // rendered without its parameters, and this one answered 503.
  const [a, b] = [await browser.newContext(), await browser.newContext()];
  const [author, reader] = [await a.newPage(), await b.newPage()];
  await home(author);
  const text = `A thread in ${testInfo.project.name} at ${Date.now()}`;
  await author.getByLabel("What's happening?").fill(text);
  await expect(author.getByRole("button", { name: "Post" })).toBeEnabled();
  await author.getByRole("button", { name: "Post" }).click();
  // The server's row, whose link is the post's address, not the pending
  // row's.
  const link = post(author, text).locator(".author");
  await expect(link).toHaveAttribute("href", /^\/post\/p\d+$/);
  const address = await link.getAttribute("href");
  for (const page of [author, reader]) {
    await page.goto(address);
    await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
    await page.evaluate(() => {
      window.__unreloaded = true;
    });
    await expect(page.locator("#quiet")).toHaveText("No replies yet.");
  }
  const reply = `A reply in ${testInfo.project.name} at ${Date.now()}`;
  const field = author.getByLabel("Your reply");
  await field.fill(reply);
  await author.getByRole("button", { name: "Reply" }).click();
  // In the thread, as its reply, to the author and to the other reader,
  // and counted.
  for (const page of [author, reader]) {
    await expect(page.locator("main article li").filter({ hasText: reply })).toHaveCount(1);
    await expect(page.locator("#counts")).toHaveText("1 reply · 0 likes");
    await expect(page.locator("#quiet")).toHaveCount(0);
    expect(await unreloaded(page)).toBe(true);
  }
  await expect(field).toHaveValue("");
  // And in no timeline: the post is there, its reply is not.
  await home(reader);
  await expect(post(reader, text)).toHaveCount(1);
  await expect(post(reader, reply)).toHaveCount(0);
  await a.close();
  await b.close();
});

/** A thread of its own, posted from the home page, open at its address. */
async function newThread(page, testInfo, what) {
  await home(page);
  const text = `${what} in ${testInfo.project.name} at ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  const link = post(page, text).locator(".author");
  await expect(link).toHaveAttribute("href", /^\/post\/p\d+$/);
  await page.goto(await link.getAttribute("href"));
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await page.evaluate(() => {
    window.__unreloaded = true;
  });
  await expect(page.locator("#quiet")).toHaveText("No replies yet.");
}

test("a reply shows before the server answers, and is the server's after", async ({
  page,
}, testInfo) => {
  // ADR-0236: the thread page binds `Thread(id)`, and its form passes `id`
  // to `reply`'s `to`, unchanged: the clause's `Thread(to)` is the thread the
  // page shows. Held at the network, the reply is the page's own meanwhile.
  await newThread(page, testInfo, "A held thread");
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.reply", async (route) => {
    await held;
    await route.continue();
  });
  const reply = `A held reply in ${testInfo.project.name} at ${Date.now()}`;
  await page.getByLabel("Your reply").fill(reply);
  await page.getByRole("button", { name: "Reply" }).click();
  const row = page.locator("main article li").filter({ hasText: reply });
  await expect(row).toHaveCount(1);
  await expect(row.getByRole("link")).toHaveText("You");
  await expect(page.locator("#counts")).toHaveText("1 reply · 0 likes");
  await expect(page.locator("#quiet")).toHaveCount(0);
  release();
  // The server's, by the session's guest.
  await expect(row.getByRole("link")).toHaveText(/^Guest /);
  await expect(page.locator("#counts")).toHaveText("1 reply · 0 likes");
  expect(await unreloaded(page)).toBe(true);
});

test("a reply whose request fails is taken back", async ({ page }, testInfo) => {
  await newThread(page, testInfo, "A failing thread");
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.reply", async (route) => {
    await held;
    await route.abort("failed");
  });
  const reply = `A failed reply in ${testInfo.project.name} at ${Date.now()}`;
  const field = page.getByLabel("Your reply");
  await field.fill(reply);
  await page.getByRole("button", { name: "Reply" }).click();
  const row = page.locator("main article li").filter({ hasText: reply });
  await expect(row).toHaveCount(1);
  release();
  // Taken back: the thread as the server has it, and the draft kept.
  await expect(row).toHaveCount(0);
  await expect(page.locator("#counts")).toHaveText("0 replies · 0 likes");
  await expect(page.locator("#quiet")).toHaveText("No replies yet.");
  await expect(field).toHaveValue(reply);
  expect(await unreloaded(page)).toBe(true);
});

test("a like on the thread page shows before the server answers", async ({ page }, testInfo) => {
  // ADR-0238: `like`'s arm on the timeline is the home page's; its arm on
  // `Thread(post)` is the thread page's, whose button passes `id`.
  await newThread(page, testInfo, "A liked thread");
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.like", async (route) => {
    await held;
    await route.continue();
  });
  await page.locator("#like").click();
  await expect(page.locator("#counts")).toHaveText("0 replies · 1 like");
  // Answered before the page is read again: a reload while the request is
  // in flight may cancel it, as WebKit once did here.
  const answered = page.waitForResponse("**/command/feed.app.like");
  release();
  await answered;
  await expect(page.locator("#counts")).toHaveText("0 replies · 1 like");
  // And it is the server's: the thread read again says so.
  await page.reload();
  await expect(page.locator("#counts")).toHaveText("0 replies · 1 like");
});

/** A user's page, its handlers attached. */
async function profile(page, id) {
  await page.goto(`/user/${id}`);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** A user's page's followers, as its counts say them. */
async function followers(page) {
  const said = await page.locator("#follow-counts").innerText();
  return Number(said.match(/^(\d+) follower/)[1]);
}

test("following someone shows their posts in Following, and unfollowing takes them out", async ({ page }) => {
  // ADR-0257: a new reader follows no one, and its timeline of those it
  // follows has nothing of Ada's; everyone's has.
  await page.goto("/following");
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(post(page, "Hello, feed.")).toHaveCount(0);
  await profile(page, "u-ada");
  await expect(page.locator("h1")).toHaveText("Ada");
  await expect(page.locator("#handle")).toHaveText("@ada");
  // Each answered before the page is left (ADR-0268): `#unfollow` is the
  // speculation's before the request has.
  const followed = page.waitForResponse("**/command/feed.app.follow");
  await page.locator("#follow").click();
  await followed;
  await expect(page.locator("#unfollow")).toHaveCount(1);
  await page.goto("/following");
  await expect(post(page, "Hello, feed.")).toHaveCount(1);
  await profile(page, "u-ada");
  const unfollowed = page.waitForResponse("**/command/feed.app.unfollow");
  await page.locator("#unfollow").click();
  await unfollowed;
  await expect(page.locator("#follow")).toHaveCount(1);
  await page.goto("/following");
  await expect(post(page, "Hello, feed.")).toHaveCount(0);
});

test("a follow shows before the server answers, and is the server's after", async ({ page }) => {
  await profile(page, "u-grace");
  const before = await followers(page);
  let release;
  const held = new Promise((r) => (release = r));
  await page.route("**/command/feed.app.follow", async (route) => {
    await held;
    await route.continue();
  });
  await page.locator("#follow").click();
  // Speculated: the button and the count, before the server answers.
  await expect(page.locator("#unfollow")).toHaveCount(1);
  expect(await followers(page)).toBe(before + 1);
  const answered = page.waitForResponse("**/command/feed.app.follow");
  release();
  await answered;
  await page.reload();
  await expect(page.locator("#unfollow")).toHaveCount(1);
  expect(await followers(page)).toBe(before + 1);
  // Left as it was found, for the next test's count, and answered before the
  // page is left (ADR-0268): `#follow` is the speculation's, before the
  // server has it. Not awaited, the unfollow could commit after the next
  // test read its count. Suspected, and not the cause, of the next test's
  // failures in CI's baselines (run 37893301747): awaited, it failed again
  // (run 37918808029).
  const unfollowed = page.waitForResponse("**/command/feed.app.unfollow");
  await page.locator("#unfollow").click();
  await unfollowed;
  await expect(page.locator("#follow")).toHaveCount(1);
});

test("a follow reaches another reader of the page without a reload", async ({ browser }, testInfo) => {
  // Two readers, each a session of its own.
  const [one, two] = await Promise.all([browser.newContext(), browser.newContext()]);
  const [a, b] = await Promise.all([one.newPage(), two.newPage()]);
  watch(a);
  watch(b);
  try {
    await profile(a, "u-grace");
    await profile(b, "u-grace");
    await b.evaluate(() => {
      window.__unreloaded = true;
    });
    const before = await followers(b);
    await a.locator("#follow").click();
    await expect(b.locator("#follow-counts")).toHaveText(new RegExp(`^${before + 1} follower`));
    expect(await unreloaded(b)).toBe(true);
    await a.locator("#unfollow").click();
    await expect(b.locator("#follow-counts")).toHaveText(new RegExp(`^${before} follower`));
  } catch (failed) {
    // Each reader's record: the default page is none of theirs.
    await record(a, testInfo, " (the reader who follows)");
    await record(b, testInfo, " (the reader told)");
    throw failed;
  } finally {
    await Promise.all([one.close(), two.close()]);
  }
});

test("your own page says it is yours", async ({ page }, testInfo) => {
  // A guest is someone once it posts: its handle leads to its page.
  await home(page);
  const text = `Mine in ${testInfo.project.name} at ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  await page.getByRole("button", { name: "Post" }).click();
  const row = post(page, text);
  // The server's row, by the session's guest, not the one shown before it
  // answered, by "You".
  await expect(row.locator(".author")).toHaveText(/^Guest /);
  await row.locator(".handle").click();
  await expect(page.locator("#you")).toHaveText("This is you.");
  await expect(page.locator("#follow")).toHaveCount(0);
  await expect(page.locator("main li").filter({ hasText: text })).toHaveCount(1);
});

test("a page of no one is not found", async ({ page }) => {
  const missing = await page.goto("/user/u-nobody");
  expect(missing.status()).toBe(404);
});

test("a post's request is kept alive, counted by its bytes", async ({ page }, testInfo) => {
  // ADR-0268: the Fetch standard's 64 KiB are bytes, and a post's text is
  // anyone's: an emoji is two of JavaScript's characters and four bytes.
  await home(page);
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/command/feed.app.post", async (route) => {
    await held;
    await route.continue();
  });
  const text = `\u{1F600} kept alive in ${testInfo.project.name} at ${Date.now()}`;
  await page.getByLabel("What's happening?").fill(text);
  await expect(page.getByRole("button", { name: "Post" })).toBeEnabled();
  const request = page.waitForRequest("**/command/feed.app.post");
  await page.getByRole("button", { name: "Post" }).click();
  const body = (await request).postData();
  const bytes = Buffer.byteLength(body, "utf8");
  expect(bytes).toBeGreaterThan(body.length);
  const inFlight = () => page.evaluate(() => window.__pw.keepalive().inFlight);
  await expect.poll(inFlight).toBe(bytes);
  release();
  await expect.poll(inFlight).toBe(0);
  await expect(post(page, text).locator(".author")).toHaveText(/^Guest /);
});

/** A real renderer export fails, rather than a fake of the page's runtime. */
async function failingReplyRenderer(page, rollbackFails) {
  let loads = 0;
  await page.route("**/pw-render.wasm", async (route) => {
    const ordinal = ++loads;
    if (ordinal === 2 && rollbackFails) {
      await route.abort("failed");
      return;
    }
    const response = await route.fetch();
    if (ordinal === 1) {
      const body = rendererWithTraps(await response.body(), ["render_part"]);
      await route.fulfill({ response, body: Buffer.from(body), contentType: "application/wasm" });
    } else {
      await route.fulfill({ response });
    }
  });
  return () => loads;
}

/** A later user press sends once, showing no transition from the failed one. */
async function retryReplyAfterRendererFailure(page, reply, sent, release) {
  await page.getByRole("button", { name: "Reply" }).click();
  const row = page.locator("main article li").filter({ hasText: reply });
  await expect(row).toHaveCount(1);
  await expect(page.locator("#counts")).toHaveText("1 reply · 0 likes");
  await expect.poll(sent).toBe(1);
  const answered = page.waitForResponse("**/command/feed.app.reply");
  release();
  await answered;
  await expect(row.getByRole("link")).toHaveText(/^Guest /);
  await page.reload();
  await expect(page.locator("main article li").filter({ hasText: reply })).toHaveCount(1);
  await expect(page.locator("#counts")).toHaveText("1 reply · 0 likes");
  expect(sent()).toBe(1);
}

test("a renderer trap discards an unsent reply before a later user press", async ({ page }, testInfo) => {
  await newThread(page, testInfo, "A renderer failure");
  const reply = `Not sent by the failed renderer in ${testInfo.project.name}`;
  const field = page.getByLabel("Your reply");
  await field.fill(reply);
  await expect(page.getByRole("button", { name: "Reply" })).toBeEnabled();
  const loads = await failingReplyRenderer(page, false);
  let sent = 0;
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/command/feed.app.reply", async (route) => {
    sent += 1;
    await held;
    await route.continue();
  });
  const logStart = await page.evaluate(() => window.__pw.log.length);
  try {
    await page.getByRole("button", { name: "Reply" }).click();
    await expect(page.getByRole("status")).toHaveText("This did not work. Try again.");
    await expect.poll(() => page.evaluate((start) => window.__pw.log.slice(start).some(
      (line) => /handler .*failed:.*unreachable/i.test(line),
    ), logStart)).toBe(true);
    expect(sent).toBe(0);
    await expect(page.locator("#counts")).toHaveText("0 replies · 0 likes");
    await expect(page.locator("#quiet")).toHaveText("No replies yet.");
    await expect(field).toHaveValue(reply);
    await retryReplyAfterRendererFailure(page, reply, () => sent, release);
    expect(loads()).toBe(2);
  } finally {
    release();
  }
});

test("a rollback that cannot load its renderer keeps no unsent reply", async ({ page }, testInfo) => {
  await newThread(page, testInfo, "A failed renderer rollback");
  const reply = `No ghost after rollback in ${testInfo.project.name}`;
  await page.getByLabel("Your reply").fill(reply);
  await expect(page.getByRole("button", { name: "Reply" })).toBeEnabled();
  const loads = await failingReplyRenderer(page, true);
  let sent = 0;
  let release;
  const held = new Promise((resolve) => (release = resolve));
  await page.route("**/command/feed.app.reply", async (route) => {
    sent += 1;
    await held;
    await route.continue();
  });
  const logStart = await page.evaluate(() => window.__pw.log.length);
  try {
    await page.getByRole("button", { name: "Reply" }).click();
    await expect(page.getByRole("status")).toHaveText("This did not work. Try again.");
    await expect.poll(() => page.evaluate((start) => window.__pw.log.slice(start).some(
      (line) => /handler .*failed:.*unreachable/i.test(line),
    ), logStart)).toBe(true);
    expect(sent).toBe(0);
    await expect(page.getByLabel("Your reply")).toHaveValue(reply);
    await retryReplyAfterRendererFailure(page, reply, () => sent, release);
    expect(loads()).toBe(3);
  } finally {
    release();
  }
});
