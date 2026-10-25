// The head-to-head's behaviour checks (ADR-XXXX): TodoMVC's specification,
// `todomvc/behaviour.spec.mjs`, on each stack in three engines. Each stack is
// served as the measurement serves it: TodoMVC in Pleris by the Pleris host's
// release build, TodoMVC's own React and Svelte apps by the head-to-head's
// static host. Ports: PORT + 232..234, in the head-to-head's range.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../..");
const PORT = Number(process.env.PORT ?? 3141);
const SERVER = path.join(process.env.CARGO_TARGET_DIR ?? path.join(REPO, "target"), "release/pw-dev-server");

const STACKS = {
  pleris: { port: PORT + 232, appPath: "/" },
  react: { port: PORT + 233, appPath: "/react/dist/" },
  svelte: { port: PORT + 234, appPath: "/svelte/dist/" },
};
const ENGINES = { chromium: devices["Desktop Chrome"], firefox: devices["Desktop Firefox"], webkit: devices["Desktop Safari"] };
const only = process.env.H2H_STACKS?.split(",");
const stacks = Object.entries(STACKS).filter(([name]) => !only || only.includes(name));

const command = {
  pleris: `${SERVER} dist-todomvc`,
  react: `node ${path.join(HERE, "static-host.mjs")} --port ${STACKS.react.port} --mount /react/dist=${path.join(HERE, "todomvc/react/dist")}`,
  svelte: `node ${path.join(HERE, "static-host.mjs")} --port ${STACKS.svelte.port} --mount /svelte/dist=${path.join(HERE, "todomvc/svelte/dist")}`,
};

export default defineConfig({
  testDir: "./todomvc",
  testMatch: "behaviour.spec.mjs",
  fullyParallel: true,
  retries: 0,
  reporter: [["list"]],
  projects: stacks.flatMap(([name, s]) =>
    Object.entries(ENGINES).map(([engine, device]) => ({
      name: `${name}-${engine}`,
      use: { ...device, baseURL: `http://127.0.0.1:${s.port}`, stack: name, appPath: s.appPath },
    })),
  ),
  webServer: stacks.map(([name, s]) => ({
    command: command[name],
    cwd: name === "pleris" ? path.join(REPO, "spikes/own-renderer") : REPO,
    env: { PORT: String(s.port), PW_FEED_DATABASE_URL: "", PW_STORE_DATABASE_URL: "" },
    url: `http://127.0.0.1:${s.port}${s.appPath}`,
    reuseExistingServer: false,
    timeout: 60_000,
  })),
});
