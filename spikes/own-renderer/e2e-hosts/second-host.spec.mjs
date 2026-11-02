// Track `second-host` (ADR-0369): two hosts of the feed on one PostgreSQL
// schema, in three engines, as two servers of one deployment are. The spec
// starts each engine's two hosts, A and B, itself (`playwright.hosts.config.mjs`
// says why): each a development server of its own on a port of its own, both
// on a schema of the engine's own (`PW_FEED_SCHEMA`), their readers signing
// up through A's development identity provider (`PW_IDENTITY=dev-accounts`).
// A browser sends a cookie to every port of its host, so a reader's session
// reaches both hosts, as it reaches every host behind one origin.
//   - a session made on A holds on B, and a post through A is told to the
//     reader's page on B, live: B hears A's commit from the outbox;
//   - a post's request sent again to B, as a retrying proxy sends it once
//     A's answer was lost, is answered from what A committed, and posts once;
//   - a press sent to the wrong host (session affinity broken) is refused as
//     another host's page, in the platform's words, and nothing runs;
//   - B stopped while A commits, and started again: its page, told by the
//     new process to reload, shows what A committed meanwhile, its reader
//     signed in still, and is told A's next post live;
//   - a session signed out on B is no one on A;
//   - pages loading their runtime and modules while commands of their
//     session run are each answered (ADR-0389, amended 2026-11-02).
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, openSync, readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const PORT = Number(process.env.PORT ?? 3141);
const ENGINES = ["chromium", "firefox", "webkit"];
const DATABASE = process.env.PW_FEED_DATABASE_URL ?? "";
// The development server the recipe built, where the main suite's is.
const SERVER = "../../target/debug/pw-dev-server";

/** The engine's schema dropped, where `psql` can; a schema left behind holds
 * a run's posts and nothing else, and the next run of the engine drops it. */
function dropped(schema) {
  try {
    execFileSync("psql", [DATABASE, "-qAtc", `DROP SCHEMA IF EXISTS ${schema} CASCADE`], {
      stdio: "ignore",
    });
  } catch {
    console.warn(`second-host: the schema ${schema} was not dropped (psql)`);
  }
}

/** **One host**: a development server of the feed on `port`, on `schema`. */
class Host {
  constructor(name, port, schema) {
    this.name = name;
    this.port = port;
    this.schema = schema;
    this.origin = `http://127.0.0.1:${port}`;
    this.log = `test-results/second-host-${name}.log`;
  }

  /** Started, and answering; on the shared database, as it says at its start. */
  async start() {
    mkdirSync("test-results", { recursive: true });
    const out = openSync(this.log, "a");
    const env = {
      ...process.env,
      PORT: String(this.port),
      PW_FEED_DATABASE_URL: DATABASE,
      PW_FEED_SCHEMA: this.schema,
      PW_IDENTITY: "dev-accounts",
      // The store's layer, which the feed's build does not read, in memory.
      PW_STORE_DATABASE_URL: "",
    };
    // A deployment's kiokun data is none of this suite's.
    delete env.KIOKUN_DATA;
    delete env.KIOKUN_APP;
    this.child = spawn(SERVER, ["dist-feed"], { env, stdio: ["ignore", out, out] });
    this.exited = new Promise((resolve) => this.child.once("exit", resolve));
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      if (this.child.exitCode !== null) {
        throw new Error(`host ${this.name} exited (${this.child.exitCode}): see ${this.log}`);
      }
      try {
        if ((await fetch(`${this.origin}/`)).ok) {
          expect(readFileSync(this.log, "utf8")).toContain(
            `on a shared database, schema ${this.schema},`,
          );
          return;
        }
      } catch {
        // Not listening yet.
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    throw new Error(`host ${this.name} did not answer on ${this.port} within 60 s: see ${this.log}`);
  }

  /** Stopped, and its port let go. */
  async stop() {
    if (!this.child || this.child.exitCode !== null) return;
    this.child.kill("SIGTERM");
    await this.exited;
  }
}

test.describe.configure({ mode: "serial" });

let A;
let B;

test.beforeAll(async ({}, testInfo) => {
  if (!DATABASE) {
    throw new Error("PW_FEED_DATABASE_URL is not set: two hosts need a database to share");
  }
  const engine = testInfo.project.name;
  const i = ENGINES.indexOf(engine);
  const schema = `pw_e2e_hosts_${PORT}_${engine}`;
  dropped(schema);
  A = new Host(`${engine}-a`, PORT + 2 * i, schema);
  B = new Host(`${engine}-b`, PORT + 2 * i + 1, schema);
  await A.start();
  await B.start();
});

test.afterAll(async () => {
  await A?.stop();
  await B?.stop();
  if (A) dropped(A.schema);
});

/** `host`'s home page, its handlers attached. */
async function home(page, host) {
  await page.goto(`${host.origin}/`);
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** A handle no other test or engine has: at most 15 of a-z, 0-9 and _. */
const handle = (testInfo, who) =>
  `${who}${testInfo.project.name.slice(0, 2)}${(Date.now() % 1e7).toString(36)}`.slice(0, 15);

/** The timeline's post with `text`. */
const post = (page, text) => page.locator("main > ul > li").filter({ hasText: text });

const PASSWORD = "correct horse battery";

/** Signed up at `host`'s development provider, from its home page, and back. */
async function signUp(page, host, who, name) {
  await home(page, host);
  await page.getByRole("button", { name: "Sign up" }).click();
  await page.getByLabel("Handle").fill(who);
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign up" }).click();
  await expect(page).toHaveURL(new RegExp(`^${host.origin}/$`));
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
}

/** `text` posted from `page`, and shown on it. */
async function posted(page, text) {
  await page.getByLabel("What's happening?").fill(text);
  await page.getByRole("button", { name: "Post" }).click();
  await expect(post(page, text)).toHaveCount(1);
}

/** A mark on the page's window, which a reload of the page loses: what is
 * shown on a page still marked was told to it, not read again. */
const mark = (page) =>
  page.evaluate(() => {
    window.__secondHostMark = true;
  });
const marked = (page) => page.evaluate(() => window.__secondHostMark === true);

test("a session made on A holds on B, and a post through A is told live to its page on B", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext();
  const [onA, onB] = [await context.newPage(), await context.newPage()];
  const name = `Ada ${testInfo.project.name}`;
  await signUp(onA, A, handle(testInfo, "ada"), name);
  // B never saw the sign-in: it finds the session where the hosts keep it.
  await home(onB, B);
  await expect(onB.locator("#me")).toHaveText(`Signed in as ${name}`);
  await mark(onB);
  const text = `Through A, read on B, at ${Date.now()}`;
  await posted(onA, text);
  await expect(post(onB, text)).toHaveCount(1);
  await expect(post(onB, text).locator(".author")).toHaveText(name);
  expect(await marked(onB), "told to the page on B, not read again").toBe(true);
  await context.close();
});

test("a post's request sent again to B is answered from what A committed, and posts once", async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext();
  const onA = await context.newPage();
  await signUp(onA, A, handle(testInfo, "eve"), `Eve ${testInfo.project.name}`);
  // A commits the press, and its answer is lost on the way back; a proxy
  // sends the browser's request again, to B, as a load balancer may: the
  // same interaction, the same session, one origin's other host.
  const answers = [];
  await onA.route("**/command/feed.app.post", async (route) => {
    const request = route.request();
    const first = await route.fetch();
    const there = new URL(request.url());
    there.port = String(B.port);
    const headers = { ...(await request.allHeaders()), origin: B.origin };
    delete headers.host;
    const again = await route.fetch({ url: there.href, headers });
    answers.push(
      { status: first.status(), body: await first.json() },
      { status: again.status(), body: await again.json() },
    );
    await route.fulfill({ response: again });
  });
  const text = `Sent again to B at ${Date.now()}`;
  await posted(onA, text);
  // Shown on A by A's own telling, which may come before both answers do.
  await expect.poll(() => answers.length).toBe(2);
  expect(answers[0].body.committed).toBe(true);
  // What A committed, and its result: a kept answer carries no basis of
  // values, on one host as on two.
  const told = ({ status, body }) => ({ status, committed: body.committed, result: body.result });
  expect(told(answers[1]), "B answers what A committed").toEqual(told(answers[0]));
  await onA.unrouteAll({ behavior: "ignoreErrors" });
  // Posted once, as each host reads it.
  const onB = await context.newPage();
  await home(onB, B);
  await expect(post(onB, text)).toHaveCount(1);
  await home(onA, A);
  await expect(post(onA, text)).toHaveCount(1);
  await context.close();
});

test("a press sent to the wrong host is refused as another host's page, and nothing runs", async ({
  browser,
}, testInfo) => {
  // Session affinity broken (the integrator's rulings 1 and 3 of
  // 2026-10-20): a load balancer sends the press of a page A served to B,
  // which never served it. B answers that the page is another host's, in
  // the platform's words, told where the press was; nothing is posted, and
  // the reader is not told the page is out of date.
  const context = await browser.newContext();
  const onA = await context.newPage();
  await signUp(onA, A, handle(testInfo, "ivy"), `Ivy ${testInfo.project.name}`);
  await onA.route("**/command/feed.app.post", async (route) => {
    const request = route.request();
    const there = new URL(request.url());
    there.port = String(B.port);
    const headers = { ...(await request.allHeaders()), origin: B.origin };
    delete headers.host;
    await route.fulfill({ response: await route.fetch({ url: there.href, headers }) });
  });
  const text = `Sent to the wrong host at ${Date.now()}`;
  await onA.getByLabel("What's happening?").fill(text);
  const answered = onA.waitForResponse((r) => r.url().includes("/command/feed.app.post"));
  await onA.getByRole("button", { name: "Post" }).click();
  expect((await answered).status()).toBe(403);
  await expect(onA.locator("form + .pw-refusal").first()).toHaveText(
    "This page is served by another host. Reload it to go on.",
  );
  await expect(post(onA, text)).toHaveCount(0);
  await onA.unrouteAll({ behavior: "ignoreErrors" });
  // Nothing was posted, as either host reads it.
  for (const host of [A, B]) {
    const page = await context.newPage();
    await home(page, host);
    await expect(post(page, text)).toHaveCount(0);
  }
  await context.close();
});

test("B stopped while A commits, and started again, catches up", async ({ browser }, testInfo) => {
  const context = await browser.newContext();
  const [onA, onB] = [await context.newPage(), await context.newPage()];
  const name = `Lin ${testInfo.project.name}`;
  await signUp(onA, A, handle(testInfo, "lin"), name);
  await home(onB, B);
  await expect(onB.locator("#me")).toHaveText(`Signed in as ${name}`);
  await B.stop();
  const meanwhile = `While B was stopped, at ${Date.now()}`;
  await posted(onA, meanwhile);
  await B.start();
  // The page asks again (the subscription kept), and the new process, which
  // knows no document of it, tells it to reload: it shows what A committed
  // meanwhile, read from the database, its reader signed in still. Its asks
  // wait up to five seconds apart, and the host starts first.
  await expect(post(onB, meanwhile)).toHaveCount(1, { timeout: 30_000 });
  await expect(onB.locator("#me")).toHaveText(`Signed in as ${name}`);
  await onB.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
  // And the new process hears A's next commit, and tells it.
  await mark(onB);
  const after = `After B started again, at ${Date.now()}`;
  await posted(onA, after);
  await expect(post(onB, after)).toHaveCount(1);
  expect(await marked(onB), "told to the page on B, not read again").toBe(true);
  await context.close();
});

test("a session signed out on B is no one on A", async ({ browser }, testInfo) => {
  const context = await browser.newContext();
  const onA = await context.newPage();
  const name = `Kim ${testInfo.project.name}`;
  await signUp(onA, A, handle(testInfo, "kim"), name);
  const signedIn = (await context.cookies()).find((c) => c.name === "pw-session");
  const onB = await context.newPage();
  await home(onB, B);
  await expect(onB.locator("#me")).toHaveText(`Signed in as ${name}`);
  await onB.getByRole("button", { name: "Sign out" }).click();
  await expect(onB.locator("#signed-out")).toBeVisible();
  // The session B signed out, sent again to A, which signed it in and kept
  // it in its memory: no one there, since A reads each request's session
  // where the hosts keep it.
  const replayed = await browser.newContext();
  await replayed.addCookies([{ ...signedIn, sameSite: "Lax" }]);
  const old = await replayed.newPage();
  await home(old, A);
  await expect(old.locator("#signed-out")).toBeVisible();
  await replayed.close();
  await context.close();
});

test("pages loading their runtime and modules while commands run are each answered", async ({
  browser,
}, testInfo) => {
  // ADR-0389, amended 2026-11-02 (track session-overlap): a browser asks for
  // a page, its runtime, its handler manifest and its handler modules at
  // once, and its commands run meanwhile; each request is admitted, reading
  // its session where the hosts keep it, and each is answered. Until then,
  // of two admissions of one session that overlapped, one was refused: a
  // page or a file 503 "The session could not be confirmed", a command 403.
  // Each round opens new pages rather than navigating one again, so that no
  // navigation cancels what a page has just asked for (WebKitGTK's network
  // process, `e2e/next-document.mjs`).
  const context = await browser.newContext();
  const first = await context.newPage();
  await signUp(first, A, handle(testInfo, "ida"), `Ida ${testInfo.project.name}`);
  const logged = readFileSync(A.log, "utf8").length;
  const refused = [];
  context.on("response", (r) => {
    if (r.status() === 503 || r.status() === 403) {
      refused.push(`${r.status()} ${r.request().method()} ${r.url()}`);
    }
  });
  for (let n = 0; n < 4; n += 1) {
    // A page loads while a command of its session runs, sent as the
    // runtime sends one.
    const loading = await context.newPage();
    const text = `Posted while a page loaded, ${n}, at ${Date.now()}`;
    const [, answer] = await Promise.all([
      home(loading, A),
      context.request.post(`${A.origin}/command/feed.app.post`, {
        headers: {
          origin: A.origin,
          "content-type": "application/json",
          "pw-interaction": `overlap-${testInfo.project.name}-${n}-${Date.now()}`,
        },
        data: JSON.stringify([text]),
      }),
    ]);
    expect(answer.status(), `the command beside a page load: ${await answer.text()}`).toBe(202);
    expect((await answer.json()).committed).toBe(true);
    // And a press on that page, which asks for its handler's module and
    // sends its command, while another page loads.
    const another = await context.newPage();
    await Promise.all([
      home(another, A),
      posted(loading, `Pressed while a page loaded, ${n}, at ${Date.now()}`),
    ]);
  }
  expect(refused, "every request of the session answered").toEqual([]);
  expect(readFileSync(A.log, "utf8").slice(logged), "no admission refused").not.toContain(
    "session admission refused",
  );
  await context.close();
});
