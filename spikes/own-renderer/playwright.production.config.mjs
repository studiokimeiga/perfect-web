// Track `deploy` (ADR-XXXX, a production host): the local production smoke
// test's browsers. `deploy/smoke.sh` starts the release host with
// `PW_DEPLOYMENT=production`, its database and its TLS front, then runs
// `production/production.spec.mjs` through the front, in each engine, once
// before the host restarts and once after (`PW_SMOKE_PHASE`). No web server
// here: the host's start, stop and restart are the smoke's to make.
import { defineConfig, devices } from "@playwright/test";

const port = process.env.PORT ?? "14241";
const device = { chromium: "Desktop Chrome", firefox: "Desktop Firefox", webkit: "Desktop Safari" };
const engines = (process.env.PW_SMOKE_ENGINES ?? "chromium,firefox,webkit").split(",");

export default defineConfig({
  testDir: "./production",
  forbidOnly: true,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: `https://localhost:${port}`,
    // The front's certificate is made for the run, by no authority a
    // browser trusts: the smoke holds the host's answers, not the PKI.
    ignoreHTTPSErrors: true,
    trace: "retain-on-failure",
  },
  projects: engines.map((name) => ({ name, use: { ...devices[device[name]] } })),
});
