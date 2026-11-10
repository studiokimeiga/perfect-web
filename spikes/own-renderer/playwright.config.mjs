// The browser suite's configuration. Its hosts are the host map's
// (`host-metadata.mjs`, `host-registry.json`): before any host starts, the
// planner (`host-plan.mjs`) asks this same Playwright, with this same
// configuration and the caller's own arguments, which tests are selected
// (`--list`), and only their hosts are declared below. A run of one spec in
// one engine starts its one host, not the forty-odd every run started
// before. Playwright's `webServer` still starts, waits for and stops them.
import { defineConfig, devices } from "@playwright/test";
import { ENGINES, PORT, unservedSpecs } from "./host-metadata.mjs";
import { announce, selectedTests, serversForSelection } from "./host-plan.mjs";

// An optional build (the keyed store's, the feed's, kiokun's, the
// guestbook's) that is unbuilt or stale: its specs are not run, as before the
// host map, and a stale one says what to run. `dist` itself is required, and
// refused where a selected test needs it (`host-plan.mjs`).
const unserved = unservedSpecs();
const selection = await selectedTests();
const webServer = serversForSelection(selection);
announce(selection, webServer, unserved);
const device = { chromium: "Desktop Chrome", firefox: "Desktop Firefox", webkit: "Desktop Safari" };

export default defineConfig({
  testDir: "./e2e",
  // The planner lists the selected tests with `--list`, which does not apply
  // `.only`; a run does. Refused in both, so a focused test fails visibly
  // rather than run on a host set planned for other tests.
  forbidOnly: true,
  testIgnore: [
    // The performance gate is excluded from the parallel suite and run alone
    // by `just e7-performance`. Not a convenience: three engine families
    // times several workers saturates the machine, and a long-animation-frame
    // measurement taken under that load measures the load.
    ...(process.env.PW_PERFORMANCE ? [] : ["**/performance.spec.mjs"]),
    // Of a production build alone: `just e14-device-runtime` builds one.
    ...(process.env.PW_PRODUCTION_ASSETS ? [] : ["**/device-runtime.spec.mjs"]),
    ...unserved.map(({ file }) => `**/${file}`),
  ],
  fullyParallel: true,
  reporter: [["list"]],
  // A test that fails on CI keeps its trace, the network's frames among it,
  // in the browser job's `test-results` artifact: a failure seen once and not
  // here (WebKit's "Load more", run 37663299969) left only a page snapshot.
  // Not a retry: a test that fails once is a finding, and not to be run again
  // until it passes.
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: process.env.CI ? "retain-on-failure" : "off",
  },
  projects: ENGINES.map((name) => ({ name, use: { ...devices[device[name]] } })),
  webServer,
});
