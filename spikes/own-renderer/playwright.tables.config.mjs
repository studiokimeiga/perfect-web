// Track `durable-tables`: the guide's guestbook on PostgreSQL, in three
// engines, apart from the main suite (`playwright.config.mjs`), whose hosts
// keep their tables in memory (ADR-0257). Two parts:
//   - the main suite's own guestbook spec (`e2e/guestbook.spec.mjs`), as
//     written, against hosts whose tables are a PostgreSQL schema of each
//     engine's own: one host per engine at the spec's own ports
//     (GUESTBOOK_PORTS, from PORT), started and stopped by Playwright;
//   - `e2e-tables/guestbook-durable.spec.mjs`, which starts a host of its own
//     per engine, stops it (SIGKILL, then SIGTERM) and starts it again while a
//     reader's page is open, and reloads it: Playwright's `webServer` cannot
//     stop one host while a test runs.
// Needs the guestbook's build (`guestbook.sh`), the development server
// (`cargo build -p pw-dev-server`) and PW_TABLES_DATABASE_URL, a throwaway
// database; `just e14-durable-tables` runs it.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";
import { ENGINES, GUESTBOOK_PORTS, buildProblem } from "./host-metadata.mjs";
import { ownedCommand } from "./host-plan.mjs";
import { schemaOf } from "./e2e-tables/schemas.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const DATABASE = process.env.PW_TABLES_DATABASE_URL ?? "";
if (!DATABASE) {
  throw new Error("PW_TABLES_DATABASE_URL is not set: the guestbook on PostgreSQL needs a throwaway database");
}
// The guestbook's build, held as the main suite holds it (ADR-0161,
// ADR-0166): built, with the runtime the repository has, from the sources it
// records. A stale build is refused here rather than served.
const problem = buildProblem("guestbook", ROOT);
if (problem) throw new Error(problem);

// **This run's schemas**, one per engine: named once, in the process that
// starts the hosts, and handed to its workers and its teardown through the
// environment, so that every evaluation of this file names the same ones
// (`e2e-tables/schemas.mjs`).
process.env.PW_E2E_TABLES_RUN ??= `${Date.now()}`;

const target = resolve(ROOT, "../../", process.env.CARGO_TARGET_DIR ?? "target", "debug");
const webServer = ENGINES.map((engine) => ({
  command: ownedCommand([resolve(target, "pw-dev-server"), "dist-guestbook"]),
  env: {
    PORT: String(GUESTBOOK_PORTS[engine]),
    PW_TABLES_DATABASE_URL: DATABASE,
    PW_TABLES_SCHEMA: schemaOf(engine),
    PW_IDENTITY: "dev-accounts",
    PW_FEED_DATABASE_URL: "",
    PW_STORE_DATABASE_URL: "",
    KIOKUN_DATA: undefined,
    KIOKUN_APP: undefined,
    PW_BLOB_DIR: undefined,
    PW_PAYMENTS: undefined,
    PW_PAYMENTS_PORT: undefined,
  },
  port: GUESTBOOK_PORTS[engine],
  reuseExistingServer: false,
  timeout: 60_000,
}));

export default defineConfig({
  testDir: ".",
  testMatch: ["e2e/guestbook.spec.mjs", "e2e-tables/*.spec.mjs"],
  forbidOnly: true,
  fullyParallel: false,
  workers: ENGINES.length,
  reporter: [["list"]],
  // A test that stops a host and starts it again waits for the new process
  // to be ready, as a deployment's restart does.
  timeout: 90_000,
  // A test that fails on CI keeps its trace, as the main suite's does. Not a
  // retry: a test that fails once is a finding.
  use: {
    trace: process.env.CI ? "retain-on-failure" : "off",
  },
  projects: ENGINES.map((name) => ({
    name,
    use: { ...devices[{ chromium: "Desktop Chrome", firefox: "Desktop Firefox", webkit: "Desktop Safari" }[name]] },
  })),
  webServer,
  globalTeardown: fileURLToPath(new URL("./e2e-tables/teardown.mjs", import.meta.url)),
});
