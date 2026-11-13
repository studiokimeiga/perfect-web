// **The host plan** (the host map, ADR-0356). Playwright owns selection and
// the servers' lifecycle. This module frames one native collection
// invocation, `playwright test --list` with the caller's own arguments and
// configuration, reads the selected tests from a reporter of its own
// (`host-plan-reporter.mjs`), and declares only their hosts, with the
// auxiliary listeners those depend on. It interprets no selection itself:
// file and line filters, projects, `--grep`, shards, `--test-list` and
// `--last-failed` all mean what Playwright says they mean.
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { closeSync, existsSync, lstatSync, mkdtempSync, openSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { REGISTRY, PORT, REQUIRED_BUILD, rolesForTest, portsForRole, buildProblem, servable } from "./host-metadata.mjs";

export const PLAN_SCHEMA = 1;
const ROOT = fileURLToPath(new URL(".", import.meta.url));
const REPORTER = fileURLToPath(new URL("./host-plan-reporter.mjs", import.meta.url));
const LAUNCHER = fileURLToPath(new URL("./serve-while-owned.sh", import.meta.url));
const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;

// **A host's command, run so that it ends with the process that started it**
// (`serve-while-owned.sh`). Playwright stops its hosts as it ends, but a
// runner that is killed outright (a recipe's bound, the out-of-memory
// killer) stops none: each host, the leader of a group of its own, lived on
// holding its port, and every later run on the port failed to start.
export const ownedCommand = (argv) => ["sh", quote(LAUNCHER), ...argv.map(quote)].join(" ");
const REQUEST = "PW_HOST_COLLECTION_REQUEST";
const CONTEXT = "PW_HOST_INVOCATION";
const own = (value, key) => Object.hasOwn(value, key);
const fail = (message) => { throw new Error(`browser host plan: ${message}`); };
const hash = (value) => createHash("sha256").update(JSON.stringify(value)).digest("hex");
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const localRequire = createRequire(import.meta.url);
let issuedPlan;

function installed() {
  const testRequire = createRequire(localRequire.resolve("@playwright/test/package.json"));
  const packageFile = testRequire.resolve("playwright/package.json");
  const packageRoot = dirname(packageFile);
  if (JSON.parse(readFileSync(packageFile, "utf8")).version !== "1.58.0") fail("requires the verified Playwright 1.58.0 CLI");
  return {
    cli: testRequire.resolve("@playwright/test/cli"),
    entries: [testRequire.resolve("@playwright/test/cli"), join(packageRoot, "cli.js")].map((path) => realpathSync(path)),
    worker: realpathSync(join(packageRoot, "lib/common/process.js")),
    program: testRequire("playwright/lib/program").program,
    resolveConfigLocation: testRequire("playwright/lib/common/configLoader").resolveConfigLocation,
  };
}

export function collectionArguments(argv) {
  if (!Array.isArray(argv) || argv.some((arg) => typeof arg !== "string") || argv[0] !== "test") fail("requires a native playwright test invocation");
  return ["test", "--list", ...argv.slice(1)];
}

function nativeOptions(library) {
  if (!process.argv[1] || !library.entries.includes(realpathSync(process.argv[1]))) fail("requires the installed native Playwright CLI");
  const commands = library.program.commands.filter((command) => command.name() === "test");
  if (commands.length !== 1 || process.argv[2] !== "test") fail("requires the canonical native test command");
  return commands[0].opts();
}

export function collectionRequest() {
  const path = process.env[REQUEST];
  if (!path) return null;
  try {
    const request = JSON.parse(readFileSync(path, "utf8"));
    if (request.schema !== PLAN_SCHEMA || request.issuerPid !== process.ppid || typeof request.nonce !== "string" ||
        !same(request.collectionArgv, process.argv.slice(2)) || request.cwd !== process.cwd() ||
        typeof request.root !== "string" || typeof request.report !== "string" || !Array.isArray(request.argv)) return null;
    return request;
  } catch {
    return null;
  }
}

function selectedSpec(file, root) {
  const local = relative(join(root, "e2e"), resolve(file));
  if (local !== basename(local)) fail(`selected spec lies outside registered e2e inventory: ${file}`);
  return local;
}

export function selectionFromSuite(config, suite, request) {
  if (config.forbidOnly !== true) fail("forbidOnly must be true in collection and execution");
  if (typeof config.configFile !== "string") fail("native collection reported no config file");
  const tests = suite.allTests().map((test) => {
    const project = test.parent.project()?.name;
    const file = selectedSpec(test.location.file, request.root);
    return { id: test.id, file, project, roles: rolesForTest(file, project, test.annotations) };
  });
  return { configFile: resolve(config.configFile), tests };
}

export function validatePlan(value, request, collectorPid) {
  if (!value || value.schema !== PLAN_SCHEMA || value.complete !== true || value.nonce !== request.nonce ||
      value.issuerPid !== request.issuerPid || value.collectorPid !== collectorPid || value.cwd !== request.cwd ||
      value.root !== request.root || !same(value.argv, request.argv) || !same(value.collectionArgv, request.collectionArgv) ||
      value.forbidOnly !== true || value.configFile !== request.configFile || !Array.isArray(value.tests)) fail("invalid or incomplete collection receipt");
  const ids = new Set();
  for (const test of value.tests) {
    if (!test || typeof test.id !== "string" || !test.id || ids.has(test.id) || typeof test.file !== "string" || typeof test.project !== "string" || !Array.isArray(test.roles)) fail("invalid or duplicate selected test identity");
    ids.add(test.id);
    const annotations = REGISTRY.specs[test.file]?.annotations === true
      ? test.roles.filter((role) => REGISTRY.roles[role]?.auxiliary !== true).map((role) => ({ type: "pw-host", description: role })) : [];
    if (!same(rolesForTest(test.file, test.project, annotations), test.roles)) fail("selected test role closure differs from registry");
  }
  return value;
}

function readReceipt(path) {
  if (!existsSync(path) || !lstatSync(path).isFile() || statSync(path).size > 8 * 1024 * 1024) fail("missing or oversized collection receipt");
  try { return JSON.parse(readFileSync(path, "utf8")); } catch { fail("malformed collection receipt"); }
}

async function runCollection(library, request, directory, timeoutMs) {
  const output = join(directory, "collection.log");
  const descriptor = openSync(output, "wx", 0o600);
  let child;
  let timer;
  let escalation;
  let finalBound;
  let monitor;
  let primary;
  let receipt;
  let exited = false;
  const errors = [];
  const stop = (error) => {
    primary ??= error;
    if (!child || exited || escalation) return;
    try { child.kill("SIGTERM"); } catch (error) { errors.push(error); }
    escalation = setTimeout(() => {
      try { if (!exited) child.kill("SIGKILL"); } catch (error) { errors.push(error); }
    }, 1000);
  };
  const interrupted = (signal) => stop(new Error(`collection interrupted by ${signal}`));
  const onInt = () => interrupted("SIGINT");
  const onTerm = () => interrupted("SIGTERM");
  process.on("SIGINT", onInt);
  process.on("SIGTERM", onTerm);
  try {
    const env = { ...process.env, [REQUEST]: join(directory, "request.json") };
    delete env[CONTEXT];
    delete env.NODE_OPTIONS;
    child = spawn(process.execPath, ["--import", import.meta.url, library.cli, ...request.collectionArgv], { cwd: request.cwd, env, detached: false, stdio: ["ignore", descriptor, descriptor] });
    const result = await new Promise((accept, reject) => {
      child.once("error", reject);
      child.once("exit", (status, signal) => { exited = true; accept({ status, signal }); });
      timer = setTimeout(() => stop(new Error(`collection timed out after ${timeoutMs}ms`)), timeoutMs);
      monitor = setInterval(() => {
        try { if (statSync(output).size > 1024 * 1024) stop(new Error("collection output exceeds 1 MiB")); } catch (error) { stop(error); }
      }, 100);
      finalBound = setTimeout(() => reject(new Error("collection did not exit after bounded termination; cleanup unproved")), timeoutMs + 5000);
    });
    if (primary) throw primary;
    if (result.status !== 0 || result.signal !== null) fail(`native collection failed (status ${result.status}, signal ${result.signal}):\n${readFileSync(output, "utf8").slice(-8000)}`);
    receipt = validatePlan(readReceipt(request.report), request, child.pid);
  } catch (error) {
    primary ??= error;
  } finally {
    for (const handle of [timer, escalation, finalBound, monitor]) clearTimeout(handle);
    try { closeSync(descriptor); } catch (error) { errors.push(error); }
    process.off("SIGINT", onInt);
    process.off("SIGTERM", onTerm);
  }
  if (errors.length) primary = new AggregateError([...(primary ? [primary] : []), ...errors], primary?.message ?? "collection cleanup failed");
  if (primary) throw primary;
  return receipt;
}

function reused(library, root) {
  if (!process.env[CONTEXT]) return null;
  let context;
  try { context = JSON.parse(process.env[CONTEXT]); } catch { return null; }
  const nativeWorker = typeof process.send === "function" && process.argv[1] && realpathSync(process.argv[1]) === library.worker && process.ppid === context.issuerPid;
  if (!nativeWorker) return null;
  if (context.root !== root || context.cwd !== process.cwd() || typeof context.report !== "string") fail("worker invocation identity mismatch");
  const receipt = readReceipt(context.report);
  if (hash(receipt) !== context.digest || receipt.nonce !== context.nonce || receipt.issuerPid !== context.issuerPid || receipt.root !== root) fail("worker receipt differs from validated invocation");
  validatePlan(receipt, context.request, context.collectorPid);
  return receipt;
}

// The collection is bounded so that a spec that hangs as it loads fails the
// run rather than hold it: at five minutes it is hung, not slow (it takes
// seconds, more on a machine many runs share).
export async function selectedTests({ root = ROOT, timeoutMs = 300000 } = {}) {
  root = resolve(root);
  const library = installed();
  if (issuedPlan) {
    if (issuedPlan.root !== root || !same(issuedPlan.receipt.argv, process.argv.slice(2))) fail("issuing invocation changed during config reload");
    return { ...issuedPlan, reused: true };
  }
  const previous = reused(library, root);
  if (previous) return { receipt: previous, collectionOnly: false, reused: true };
  const options = nativeOptions(library);
  if (options.ui || options.uiHost || options.uiPort || process.env.PWTEST_WATCH) fail("UI/watch mode is unsupported by this finite host plan; use playwright test or --debug");
  const request = collectionRequest();
  if (request) {
    if (options.list !== true || request.root !== root) fail("private collection request is not an explicit matching --list invocation");
    return { receipt: null, collectionOnly: true, reused: false };
  }
  const reporter = options.reporter;
  if ((Array.isArray(reporter) ? reporter : String(reporter ?? "").split(",")).includes(REPORTER)) fail("invalid private collection context");
  const directory = mkdtempSync(join(tmpdir(), "pw-host-plan-"));
  const next = {
    schema: PLAN_SCHEMA, issuerPid: process.pid, nonce: randomUUID(), cwd: process.cwd(), root,
    argv: process.argv.slice(2), collectionArgv: collectionArguments(process.argv.slice(2)), report: join(directory, "receipt.json"),
    configFile: library.resolveConfigLocation(options.config).resolvedConfigFile,
  };
  writeFileSync(join(directory, "request.json"), JSON.stringify(next), { flag: "wx", mode: 0o600 });
  try {
    const receipt = await runCollection(library, next, directory, timeoutMs);
    process.env[CONTEXT] = JSON.stringify({ ...next, request: next, report: next.report, collectorPid: receipt.collectorPid, digest: hash(receipt) });
    // Keep the validated receipt for actual native worker config reloads only.
    process.once("exit", () => rmSync(directory, { recursive: true, force: true }));
    issuedPlan = { root, receipt, collectionOnly: options.list === true, reused: false };
    return issuedPlan;
  } catch (error) {
    try { rmSync(directory, { recursive: true, force: true }); }
    catch (cleanup) { throw new AggregateError([error, cleanup], error.message); }
    throw error;
  }
}

export function serversForSelection(selection, { root = ROOT, port = PORT } = {}) {
  if (selection.collectionOnly) return [];
  const chosen = new Map();
  for (const test of selection.receipt.tests) {
    for (const role of test.roles) {
      const definition = REGISTRY.roles[role];
      const engine = own(definition.offsets, "shared") ? "shared" : test.project;
      chosen.set(`${role}/${engine}`, { role, engine });
    }
  }
  // `dist` must be servable where a selected test needs it. An optional
  // build that is not servable has no selected tests but those of a spec whose
  // tests differ, which skip themselves (`page-policy.spec.mjs`): its host is
  // not started. Any other selected test of one is a test the config should
  // have left out, and refused.
  const hosts = [];
  for (const host of chosen.values()) {
    const { auxiliary, build } = REGISTRY.roles[host.role];
    if (auxiliary === true) continue;
    if (build === REQUIRED_BUILD) {
      const problem = buildProblem(build, root);
      if (problem) fail(problem);
    } else if (!servable(build, root)) {
      const unskipped = selection.receipt.tests.find((test) => test.roles.includes(host.role) && REGISTRY.specs[test.file]?.annotations !== true);
      if (unskipped) fail(`${unskipped.file} needs ${host.role}, whose build cannot be served (${buildProblem(build, root)})`);
      continue;
    }
    hosts.push(host);
  }
  if (process.env.PW_PERFORMANCE && selection.receipt.tests.some((test) => test.project !== "chromium" || !same(test.roles, ["mutable:performance"]))) fail("PW_PERFORMANCE requires only the Chromium performance/runtime-size selection");
  const target = resolve(root, "../../", process.env.CARGO_TARGET_DIR ?? "target", "debug");
  return hosts.map(({ role, engine }) => {
    const definition = REGISTRY.roles[role];
    const hostPort = portsForRole(role, port)[engine];
    const env = {
      PORT: String(hostPort), PW_FEED_DATABASE_URL: "", PW_STORE_DATABASE_URL: "",
      // A program's declared tables too (track durable-tables): in memory,
      // whatever the caller's environment names for them.
      PW_TABLES_DATABASE_URL: "", PW_TABLES_SCHEMA: undefined,
      KIOKUN_DATA: undefined, KIOKUN_APP: undefined,
      PW_IDENTITY: undefined, PW_BLOB_DIR: undefined, PW_PAYMENTS: undefined, PW_PAYMENTS_PORT: undefined,
    };
    if (definition.identity) env.PW_IDENTITY = "dev-accounts";
    if (definition.blobs) env.PW_BLOB_DIR = `${REGISTRY.builds[definition.build].dist}/blobs-${hostPort}`;
    if (definition.provider) {
      env.PW_PAYMENTS = "dev";
      env.PW_PAYMENTS_PORT = String(portsForRole(definition.provider, port)[engine]);
    }
    const executable = definition.build === "legacy-kiokun" ? "kiokun-server" : "pw-dev-server";
    const dist = REGISTRY.builds[definition.build].dist;
    return { command: ownedCommand([join(target, executable), ...(dist ? [dist] : [])]), env, port: hostPort, reuseExistingServer: !!process.env.PW_REUSE, timeout: 60000 };
  });
}

// What the invocation that planned says before its hosts start: each stale
// optional build, whose specs are not run, and how many hosts it starts.
export function announce(selection, servers, unserved = []) {
  if (!selection.receipt || selection.reused || selection.collectionOnly) return;
  for (const { warning } of unserved) if (warning) console.warn(warning);
  const all = Object.values(REGISTRY.roles).filter((role) => role.auxiliary !== true && role.reserved !== true)
    .reduce((count, role) => count + Object.keys(role.offsets).length, 0);
  console.warn(`browser host map: ${servers.length} of ${all} hosts for ${selection.receipt.tests.length} selected tests`);
}

// This explicit child-only preload runs before the native CLI parses argv.
// Its canonical preAction hook sees every original option already parsed and
// installs only the private collection reporter before cliOverrides is built.
if (same(process.execArgv, ["--import", import.meta.url])) {
  const request = collectionRequest();
  if (!request) fail("explicit collection preload requires a validated private request");
  const library = installed();
  nativeOptions(library);
  const command = library.program.commands.find((candidate) => candidate.name() === "test");
  command.hook("preAction", (actual) => {
    if (actual !== command || !collectionRequest() || actual.opts().list !== true) fail("private collection preAction identity/list mismatch");
    actual.setOptionValue("reporter", REPORTER);
  });
}
