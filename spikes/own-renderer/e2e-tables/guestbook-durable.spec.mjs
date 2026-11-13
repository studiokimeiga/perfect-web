// Track `durable-tables`: the guide's guestbook outlives the host that
// serves it, in three engines. Each engine's host is a development server of
// its own (`playwright.tables.config.mjs` says why), its tables in a
// PostgreSQL schema of the engine's own (`PW_TABLES_DATABASE_URL`,
// `PW_TABLES_SCHEMA`), its readers signed up through its development
// identity provider (`PW_IDENTITY=dev-accounts`), whose accounts are the
// process's and whose sessions are the database's:
//   - a reader signs up and signs; the host is killed (SIGKILL) while the
//     reader's page is open, and started again; the page, reloaded by its
//     reader while the new process may be telling it to reload itself (it
//     knows no document of the old one's), shows the entry, and the reader
//     still signed in, who signs again;
//   - the host is stopped (SIGTERM) and started again; reloaded the same
//     way, the page shows both entries, the reader's to remove, and another
//     reader's browser reads them too, offered no Remove.
// The reader's reload races the page's own: two requests of one session
// admitted at once are each answered (ADR-0389, amended by PR #67). Until
// then the reader's reload waited for the page's, as the later of the two
// was answered "The session could not be confirmed".
import { spawn } from "node:child_process";
import { mkdirSync, openSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "@playwright/test";
import { expect } from "../assertion-expect.mjs";
import { GUESTBOOK_PORTS } from "../host-metadata.mjs";
import { dropped } from "./schemas.mjs";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const DATABASE = process.env.PW_TABLES_DATABASE_URL ?? "";
// The development server the recipe built, where the main suite's is.
const SERVER = resolve(ROOT, "../../", process.env.CARGO_TARGET_DIR ?? "target", "debug", "pw-dev-server");

/** Whether a host answers its page at `origin`; not yet, while nothing
 * listens there. */
async function answers(origin) {
  try {
    return (await fetch(`${origin}/`)).ok;
  } catch {
    return false;
  }
}

/** **One host**: the guestbook's development server on `port`, its tables
 * in `schema`. */
class Host {
  constructor(name, port, schema) {
    this.name = name;
    this.port = port;
    this.schema = schema;
    this.origin = `http://127.0.0.1:${port}`;
    this.log = `test-results/guestbook-durable-${name}.log`;
  }

  /** Started, and answering; its tables on PostgreSQL, as it says at its
   * start. */
  async start() {
    mkdirSync("test-results", { recursive: true });
    const out = openSync(this.log, "a");
    const env = {
      ...process.env,
      PORT: String(this.port),
      PW_TABLES_DATABASE_URL: DATABASE,
      PW_TABLES_SCHEMA: this.schema,
      PW_IDENTITY: "dev-accounts",
      PW_FEED_DATABASE_URL: "",
      PW_STORE_DATABASE_URL: "",
    };
    // A deployment's kiokun data is none of this suite's.
    delete env.KIOKUN_DATA;
    delete env.KIOKUN_APP;
    this.child = spawn(SERVER, ["dist-guestbook"], { env, stdio: ["ignore", out, out] });
    this.exited = new Promise((resolve) => this.child.once("exit", resolve));
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      if (this.child.exitCode !== null) {
        throw new Error(`host ${this.name} exited (${this.child.exitCode}): see ${this.log}`);
      }
      if (await answers(this.origin)) {
        // Said at its start, before it answers anyone.
        expect(readFileSync(this.log, "utf8")).toContain(
          `pw dev server tables: on PostgreSQL, schema ${this.schema}`,
        );
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    throw new Error(`host ${this.name} did not answer on ${this.port} within 60 s: see ${this.log}`);
  }

  /** Ended by `signal`, and its port let go: SIGKILL as a crash ends it,
   * SIGTERM as a deployment stops it. */
  async stop(signal) {
    if (!this.child || this.child.exitCode !== null) return;
    this.child.kill(signal);
    await this.exited;
  }
}

test.describe.configure({ mode: "serial" });

let host;

test.beforeAll(async ({}, testInfo) => {
  if (!DATABASE) {
    throw new Error("PW_TABLES_DATABASE_URL is not set: the guestbook on PostgreSQL needs a database");
  }
  const engine = testInfo.project.name;
  // Beside the engine's guestbook host, which the config starts.
  const port = GUESTBOOK_PORTS[engine] + 10;
  host = new Host(engine, port, `pw_e2e_durable_${port}_${engine}_${Date.now()}`);
  await host.start();
});

test.afterAll(async () => {
  await host?.stop("SIGTERM");
  if (host) dropped(host.schema);
});

/** The guestbook, its handlers attached. */
async function home(page) {
  await page.goto(`${host.origin}/`);
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
  await page.getByRole("button", { name: "Sign up" }).click();
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
  await expect(entry(page, words).getByRole("button", { name: "Remove" })).toHaveCount(1);
}

/** `page` reloaded, its handlers attached again. */
async function reloaded(page) {
  await page.reload();
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

/** **`page` reloaded by its reader, as a host started again may be telling
 * it to reload itself**: its subscription asks again, and the new process,
 * which knows no document of it, tells it to. The two reloads race, each a
 * request of the reader's session, and each is answered (ADR-0389, as
 * amended); the reader's is let go only where the page's own navigation
 * replaced it, the browser saying so. Whichever lands last, its handlers are
 * attached again. */
async function reloadedRacing(page) {
  try {
    await page.reload();
  } catch (error) {
    if (!/interrupted by another navigation|ERR_ABORTED|NS_BINDING_ABORTED|cancelled/i.test(String(error))) {
      throw error;
    }
  }
  await page.waitForFunction(() => document.documentElement.dataset.pwReady === "1");
}

test("the guestbook outlives its host: killed, stopped, and reloaded", async ({ browser }, testInfo) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  const name = `Ida ${testInfo.project.name}`;
  await signUp(page, handle(testInfo, "ida"), name);
  const before = `Before the crash, from ${testInfo.project.name} ${Date.now()}`;
  await sign(page, before, name);
  // Killed while the page is open: no warning, no shutdown.
  await host.stop("SIGKILL");
  await host.start();
  await reloadedRacing(page);
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
  await expect(entry(page, before)).toHaveCount(1);
  await reloaded(page);
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
  await expect(entry(page, before)).toHaveCount(1);
  await expect(entry(page, before).locator(".by")).toHaveText(name);
  const after = `After it, from ${testInfo.project.name} ${Date.now()}`;
  await sign(page, after, name);
  // Stopped as a deployment stops it, and started again.
  await host.stop("SIGTERM");
  await host.start();
  await reloadedRacing(page);
  await expect(entry(page, after)).toHaveCount(1);
  await reloaded(page);
  await expect(page.locator("#me")).toHaveText(`Signed in as ${name}`);
  await expect(entry(page, before)).toHaveCount(1);
  await expect(entry(page, after)).toHaveCount(1);
  await expect(page.locator("main > ul > li").first()).toContainText(after);
  await expect(entry(page, after).getByRole("button", { name: "Remove" })).toHaveCount(1);
  // Another reader, signed out: both entries, and no Remove.
  const other = await (await browser.newContext()).newPage();
  await home(other);
  await expect(other.locator("#signed-out")).toHaveText("Sign in to sign the guestbook.");
  await expect(entry(other, before)).toHaveCount(1);
  await expect(entry(other, after)).toHaveCount(1);
  await expect(other.getByRole("button", { name: "Remove" })).toHaveCount(0);
  await other.context().close();
  await context.close();
});
