// The host map's own tests that need no browser and start no host: the
// registry, the map held to what each spec's source reaches, the build
// policy, the hosts a selection declares, and the planner's receipts.
// `just ci` runs them (scripts/tests/test_browser_host_map.py); the planner
// against the real Playwright CLI is `host-plan.native.test.mjs`'s.
import assert from "node:assert/strict";
import test from "node:test";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  REGISTRY, STRIDE, ENGINES, REQUIRED_BUILD, MUTABLE_PORTS, STORE_PAYMENTS_PORTS, SCHEDULED_ORDERS_PORTS, TODOMVC_PORTS,
  validateRegistry, listenersForSlots, portsForRole, rolesForTest, roleOfURL, buildProblem, buildState, servable,
  unservedSpecs, hostsReachedBy, hostAnnotationsIn,
} from "./host-metadata.mjs";
import { collectionArguments, ownedCommand, serversForSelection, validatePlan } from "./host-plan.mjs";
import HostPlanReporter from "./host-plan-reporter.mjs";

const clone = () => structuredClone(REGISTRY);
const SPECS = new URL("./e2e/", import.meta.url);
const specFiles = () => readdirSync(SPECS).filter((name) => name.endsWith(".spec.mjs")).sort();

// A private copy of the renderer's builds, each whole and current unless a
// test makes it otherwise.
function builds(dists = ["dist"]) {
  const root = mkdtempSync(join(tmpdir(), "pw-host-builds-"));
  mkdirSync(join(root, "public"));
  writeFileSync(join(root, "public/pw-runtime.mjs"), "private test runtime");
  for (const dist of dists) whole(root, dist);
  return root;
}
function whole(root, dist) {
  mkdirSync(join(root, dist, "build"), { recursive: true });
  writeFileSync(join(root, dist, "pw-runtime.mjs"), "private test runtime");
  writeFileSync(join(root, dist, "sources.json"), '{"files":[]}');
}
// Run `body` with these environment variables, and give the rest back.
function withEnvironment(values, body) {
  const before = Object.fromEntries(Object.keys(values).map((key) => [key, process.env[key]]));
  try {
    for (const [key, value] of Object.entries(values)) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
    return body();
  } finally {
    for (const [key, value] of Object.entries(before)) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
  }
}
const selected = (...tests) => ({ collectionOnly: false, receipt: { tests } });
const one = (file, project, annotations = []) => ({ file, project, roles: rolesForTest(file, project, annotations) });

test("registry covers the exact current spec inventory", () => {
  assert.deepEqual(Object.keys(REGISTRY.specs).sort(), specFiles());
  assert.equal(STRIDE, 256);
  assert.deepEqual(ENGINES, ["chromium", "firefox", "webkit"]);
});

// **The map is held to the specs.** A spec reaches a host only through its
// `baseURL` (`hostsReachedBy`); its registered roles, or for a spec whose
// tests differ the roles its annotations name, are exactly those hosts.
// A spec whose host changed and whose entry did not fails here, in `just ci`.
test("every spec's registered hosts are exactly the hosts its source reaches", () => {
  const wrong = [];
  for (const file of specFiles()) {
    const source = readFileSync(new URL(file, SPECS), "utf8");
    const spec = REGISTRY.specs[file];
    const registered = spec.annotations === true ? hostAnnotationsIn(source) : [...spec.roles].sort();
    const reached = hostsReachedBy(source);
    if (JSON.stringify(registered) !== JSON.stringify(reached)) wrong.push(`${file}: registered ${registered}, reaches ${reached}`);
  }
  assert.deepEqual(wrong, []);
  // Some spec reaches each role a spec can name, so no role is planned for nothing.
  const reached = new Set(specFiles().flatMap((file) => hostsReachedBy(readFileSync(new URL(file, SPECS), "utf8"))));
  const roles = Object.entries(REGISTRY.roles).filter(([, role]) => role.auxiliary !== true && role.reserved !== true).map(([name]) => name);
  assert.deepEqual(roles.filter((role) => !reached.has(role)), []);
});

test("a spec's hosts are read from its baseURL overrides, and an unnamed loopback port is refused", () => {
  const override = (expression, indent = "") => `${indent}test.use({\n${indent}  baseURL: async ({}, use, testInfo) => {\n${indent}    await use(\`http://127.0.0.1:\${${expression}}\`);\n${indent}  },\n${indent}});\n`;
  assert.deepEqual(hostsReachedBy('import { test } from "@playwright/test";\ntest("a", async ({ page }) => page.goto("/"));\n'), ["base"]);
  assert.deepEqual(hostsReachedBy(override("FEED_PORTS[testInfo.project.name]")), ["feed"]);
  assert.deepEqual(hostsReachedBy(override("MUTABLE_PORTS.cart[testInfo.project.name]")), ["mutable:cart"]);
  assert.deepEqual(hostsReachedBy(override('MUTABLE_PORTS["keyed-list"][testInfo.project.name]')), ["mutable:keyed-list"]);
  assert.deepEqual(hostsReachedBy(override("STORE_PAYMENTS_PORTS.webkit")), ["payments"]);
  assert.deepEqual(hostsReachedBy("test.use({ baseURL: `http://127.0.0.1:${KIOKUN_PORT}` });\n"), ["legacy-kiokun"]);
  // An override inside a describe leaves the file's other tests on base.
  assert.deepEqual(hostsReachedBy(`test("a", () => {});\ntest.describe("b", () => {\n${override("IDENTITY_PORTS[testInfo.project.name]", "  ")}});\n`), ["base", "identity"]);
  assert.deepEqual(hostsReachedBy('test.use({ javaScriptEnabled: false });\n'), ["base"]);
  // The address of a page's content, without a port, reaches nothing.
  assert.deepEqual(hostsReachedBy('const canonical = "http://127.0.0.1/stores";\n'), ["base"]);
  for (const source of [
    'await page.goto("http://127.0.0.1:3141/stores/47");',
    "await page.goto(`http://localhost:${port}/`);",
    'const url = "http://[::1]:3141";',
    override("OTHER_PORTS[testInfo.project.name]"),
    override("MUTABLE_PORTS.unknown[testInfo.project.name]"),
    override("FEED_PORTS.edge"),
    override("PORT + 60"),
  ]) assert.throws(() => hostsReachedBy(source), /browser host registry: a (host named|loopback address)/, source);
  assert.deepEqual(hostAnnotationsIn('test("a", { annotation: { type: "pw-host", description: "feed" } }, f);\ntest.describe("b", { annotation: { type: "pw-host", description: "base" } }, f);'), ["base", "feed"]);
});

test("all host and provider listeners are disjoint across adjacent and multiple slots", () => {
  for (const slots of [2, 8, 31]) {
    const listeners = listenersForSlots(REGISTRY, 13201, slots);
    assert.equal(new Set(listeners.map(({ port }) => port)).size, listeners.length);
    assert.equal(listeners.filter(({ role }) => role.endsWith("provider")).length, 6 * slots);
    assert.equal(listeners.filter(({ role }) => role === "todomvc").length, 3 * slots);
  }
  assert.throws(() => { const bad = clone(); bad.stride = 200; validateRegistry(bad); }, /escapes stride/);
});

test("TCP bound includes the final slot and largest concrete listener", () => {
  const max = Math.max(...listenersForSlots(REGISTRY, 1).map(({ port }) => port - 1));
  assert.equal(max, 212);
  assert.equal(Math.max(...listenersForSlots(REGISTRY, 65535 - max - REGISTRY.stride, 2).map(({ port }) => port)), 65535);
  assert.throws(() => listenersForSlots(REGISTRY, 65536 - max - REGISTRY.stride, 2), /TCP port/);
  for (const base of [0, -1, 1.5, NaN, Infinity, "13201", 65536]) assert.throws(() => listenersForSlots(REGISTRY, base), /PORT/);
  for (const count of [0, -1, 1.5, NaN, Infinity, "2", 65536]) assert.throws(() => listenersForSlots(REGISTRY, 13201, count), /slot count/);
});

test("duplicate, fractional, negative and escaping concrete offsets refuse", () => {
  for (const offset of [0, -1, 1.5, NaN, Infinity, "200", 256, 65536]) {
    const bad = clone(); bad.roles.reviews.offsets.chromium = offset;
    assert.throws(() => validateRegistry(bad), /offset/);
  }
});

test("provider dependencies are declared and engine compatible", () => {
  const missing = clone(); delete missing.roles["payment-provider"];
  assert.throws(() => validateRegistry(missing), /unknown auxiliary dependency/);
  const omitted = clone(); delete omitted.roles.payments.depends;
  assert.throws(() => validateRegistry(omitted), /provider is not a declared dependency/);
  const engine = clone(); delete engine.roles["payment-provider"].offsets.webkit;
  assert.throws(() => validateRegistry(engine), /incompatible engines|every engine/);
  assert.deepEqual(rolesForTest("payments.spec.mjs", "webkit"), ["payment-provider", "payments"]);
  assert.deepEqual(rolesForTest("scheduled-orders.spec.mjs", "chromium"), ["scheduled-orders", "scheduled-provider"]);
});

test("pure legacy port exports preserve every accepted within-slot offset", () => {
  assert.equal(MUTABLE_PORTS["keyed-list"].chromium - portsForRole("base").shared, 1);
  assert.equal(MUTABLE_PORTS.materialized.webkit - portsForRole("base").shared, 36);
  assert.equal(STORE_PAYMENTS_PORTS.chromium - portsForRole("base").shared, 180);
  assert.equal(SCHEDULED_ORDERS_PORTS.webkit - portsForRole("base").shared, 185);
  assert.equal(TODOMVC_PORTS.chromium - portsForRole("base").shared, 140);
  assert.equal(portsForRole("payment-provider").chromium - portsForRole("base").shared, 190);
  assert.equal(portsForRole("scheduled-provider").webkit - portsForRole("base").shared, 195);
  assert.equal(portsForRole("reviews").webkit - portsForRole("base").shared, 202);
  assert.equal(portsForRole("guestbook").webkit - portsForRole("base").shared, 212);
  assert.throws(() => { REGISTRY.roles.base.offsets.shared = 9; }, TypeError);
});

test("a test's baseURL names the one registered host of its engine", () => {
  assert.equal(roleOfURL("http://127.0.0.1:3141", "chromium", 3141), "base");
  assert.equal(roleOfURL("http://127.0.0.1:3141/stores/47", "webkit", 3141), "base");
  assert.equal(roleOfURL("http://127.0.0.1:3142", "chromium", 3141), "mutable:keyed-list");
  assert.equal(roleOfURL("http://127.0.0.1:3202", "firefox", 3141), "feed");
  assert.equal(roleOfURL("http://127.0.0.1:3181", "chromium", 3141), "legacy-kiokun");
  // A port of another engine's host, or of none, is no host of this engine.
  assert.throws(() => roleOfURL("http://127.0.0.1:3201", "webkit", 3141), /no registered host of webkit/);
  assert.throws(() => roleOfURL("http://127.0.0.1:3140", "chromium", 3141), /no registered host/);
});

test("unknown selected spec, project, role and conflicting declarations refuse", () => {
  assert.throws(() => rolesForTest("unknown.spec.mjs", "chromium"), /unregistered selected spec/);
  assert.throws(() => rolesForTest("signals.spec.mjs", "chrome"), /unknown project/);
  assert.throws(() => rolesForTest("signals.spec.mjs", "chromium", [{ type: "pw-host", description: "base" }]), /conflicting/);
  for (const annotations of [[], [{ type: "pw-host", description: "unknown" }], [{ type: "pw-host", description: "base" }, { type: "pw-host", description: "feed" }]]) {
    assert.throws(() => rolesForTest("page-policy.spec.mjs", "chromium", annotations), /host annotation|host role/);
  }
  // A future app may reserve its role even when every current app is present.
  // Both fixed spec mappings and explicit annotations obey that same flag.
  const reserved = clone();
  reserved.roles.base.reserved = true;
  assert.doesNotThrow(() => validateRegistry(reserved));
  const selections = [
    ["signals.spec.mjs", []],
    ["page-policy.spec.mjs", [{ type: "pw-host", description: "base" }]],
  ];
  for (const engine of ENGINES) {
    for (const [file, annotations] of selections) {
      assert.throws(() => rolesForTest(file, engine, annotations, reserved), /selected host role base is reserved/);
    }
  }
  for (const present of [false, undefined]) {
    if (present === undefined) delete reserved.roles.base.reserved;
    else reserved.roles.base.reserved = present;
    assert.doesNotThrow(() => validateRegistry(reserved));
    for (const engine of ENGINES) {
      for (const [file, annotations] of selections) {
        assert.deepEqual(rolesForTest(file, engine, annotations, reserved), ["base"]);
      }
    }
  }
  // TodoMVC's hosts serve its spec, its app on this branch (track `todomvc`).
  assert.deepEqual(rolesForTest("todomvc.spec.mjs", "firefox"), ["todomvc"]);
  assert.deepEqual(rolesForTest("signals.spec.mjs", "chromium"), ["base"]);
  assert.deepEqual(rolesForTest("page-policy.spec.mjs", "webkit", [{ type: "pw-host", description: "payments" }]), ["payment-provider", "payments"]);
});

test("schema, engines, role builds and spec mappings fail closed", () => {
  for (const mutate of [
    (bad) => bad.schema = 2,
    (bad) => bad.engines.push("chromium"),
    (bad) => bad.roles.base.build = "absent",
    (bad) => bad.mutable.cart = "absent",
    (bad) => bad.portExports.FEED_PORTS = "absent",
    (bad) => bad.specs["signals.spec.mjs"].roles = ["absent"],
    (bad) => bad.specs["signals.spec.mjs"].annotations = true,
    (bad) => bad.roles["payment-provider"].depends = ["scheduled-provider"],
  ]) {
    const bad = clone(); mutate(bad); assert.throws(() => validateRegistry(bad), /browser host registry/);
  }
});

test("build checks preserve whole-build runtime and source freshness with concrete prerequisites", () => {
  const root = mkdtempSync(join(tmpdir(), "pw-host-build-"));
  try {
    assert.match(buildProblem("feed", root), /dist-feed is no whole build.*run feed.sh/);
    mkdirSync(join(root, "dist-feed/build"), { recursive: true });
    assert.match(buildProblem("feed", root), /whole build/);
    writeFileSync(join(root, "dist-feed/sources.json"), '{"files":[]}');
    assert.match(buildProblem("feed", root), /another pw-runtime/);
    mkdirSync(join(root, "public"));
    writeFileSync(join(root, "public/pw-runtime.mjs"), "runtime");
    writeFileSync(join(root, "dist-feed/pw-runtime.mjs"), "runtime");
    assert.equal(buildProblem("feed", root), null);
    writeFileSync(join(root, "source.pw"), "before");
    const sha256 = createHash("sha256").update("before").digest("hex");
    writeFileSync(join(root, "dist-feed/sources.json"), JSON.stringify({ files: [{ path: "source.pw", sha256 }] }));
    assert.equal(buildProblem("feed", root), null);
    writeFileSync(join(root, "source.pw"), "after");
    assert.match(buildProblem("feed", root), /source.pw changed/);
    rmSync(join(root, "source.pw"));
    assert.match(buildProblem("feed", root), /missing required source source.pw/);
    writeFileSync(join(root, "dist-feed/sources.json"), '{"files":false}');
    assert.match(buildProblem("feed", root), /invalid source records/);
    writeFileSync(join(root, "dist-feed/sources.json"), '{');
    assert.match(buildProblem("feed", root), /cannot be read/);
    writeFileSync(join(root, "dist-feed/sources.json"), '{"files":[]}');
    rmSync(join(root, "dist-feed/build"), { recursive: true });
    assert.match(buildProblem("feed", root), /dist-feed is not built/);
    assert.match(buildProblem("todomvc", root), /dist-todomvc is no whole build/);
    assert.equal(buildProblem("legacy-kiokun", root), null);
    assert.throws(() => buildProblem("absent", root), /unknown build/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("only keyed external generated copies may be absent and existing copies remain hash checked", () => {
  const privateRoot = mkdtempSync(join(tmpdir(), "pw-host-generated-"));
  const repository = join(privateRoot, "repository");
  const root = join(repository, "spikes/own-renderer");
  try {
    mkdirSync(join(root, "public"), { recursive: true });
    writeFileSync(join(root, "public/pw-runtime.mjs"), "runtime");
    const required = join(repository, "permanent.pw");
    const external = join(privateRoot, "removed-generated.pw");
    const sha256 = createHash("sha256").update("source").digest("hex");
    for (const dist of ["dist-feed", "dist-keyed"]) {
      mkdirSync(join(root, dist, "build"), { recursive: true });
      writeFileSync(join(root, dist, "pw-runtime.mjs"), "runtime");
      writeFileSync(join(root, dist, "sources.json"), JSON.stringify({ files: [{ path: external, sha256 }] }));
    }
    assert.match(buildProblem("feed", root), /missing required source/);
    assert.equal(buildProblem("keyed", root), null);
    writeFileSync(external, "different");
    assert.match(buildProblem("keyed", root), /changed since/);
    rmSync(external);
    writeFileSync(join(root, "dist-keyed/sources.json"), JSON.stringify({ files: [{ path: required, sha256 }] }));
    assert.match(buildProblem("keyed", root), /missing required source/);
    symlinkSync(repository, join(privateRoot, "looks-external"), "dir");
    writeFileSync(join(root, "dist-keyed/sources.json"), JSON.stringify({ files: [{ path: join(privateRoot, "looks-external/permanent.pw"), sha256 }] }));
    assert.match(buildProblem("keyed", root), /missing required source/);
    assert.deepEqual(Object.entries(REGISTRY.builds).filter(([, build]) => build.externalGeneratedSources).map(([name]) => name), ["keyed"]);
    const invalid = clone(); invalid.builds.keyed.externalGeneratedSources = "yes";
    assert.throws(() => validateRegistry(invalid), /externalGeneratedSources/);
  } finally {
    rmSync(privateRoot, { recursive: true, force: true });
  }
});

// **The build policy, as before the host map**: `dist` is required; an
// optional build that is unbuilt or stale leaves its specs out, and never
// silently (the integrator's ruling of 2026-10-19): each spec left out is
// said, with the build it lacks and what to run.
test("specs of an optional build that cannot be served are left out, each said with the build it lacks, and never dist's", () => {
  const root = builds(["dist"]);
  try {
    assert.equal(REQUIRED_BUILD, "store");
    const optional = (root) => Object.fromEntries(unservedSpecs(root).map(({ file, warning }) => [file, warning]));
    const feed = ["computed-modules.spec.mjs", "feed.spec.mjs", "identity.spec.mjs", "messages.spec.mjs", "notifications.spec.mjs", "upload-metadata.spec.mjs", "uploads.spec.mjs"];
    const unbuilt = optional(root);
    assert.deepEqual(Object.keys(unbuilt).sort(), [...feed, "guestbook.spec.mjs", "keyed.spec.mjs", "kiokun-site.spec.mjs", "todomvc.spec.mjs"].sort());
    const lacking = { "guestbook.spec.mjs": ["dist-guestbook", "guestbook.sh"], "keyed.spec.mjs": ["dist-keyed", "keyed-store.sh"], "kiokun-site.spec.mjs": ["dist-kiokun", "kiokun-site.sh"], "todomvc.spec.mjs": ["dist-todomvc", "todomvc.sh"] };
    for (const file of feed) lacking[file] = ["dist-feed", "feed.sh"];
    for (const [file, [dist, prerequisite]] of Object.entries(lacking)) {
      assert.equal(unbuilt[file], `e2e/${file} is not run: ${dist} is not built: run ${prerequisite}`);
    }
    assert.deepEqual(buildState("feed", root), { built: false, problem: buildProblem("feed", root) });
    mkdirSync(join(root, "dist-feed/build"), { recursive: true });
    writeFileSync(join(root, "dist-feed/sources.json"), '{"files":[]}');
    writeFileSync(join(root, "dist-feed/pw-runtime.mjs"), "another runtime");
    const stale = optional(root);
    for (const file of feed) assert.match(stale[file], new RegExp(`^e2e/${file.replace(".", "\\.")} is not run: dist-feed was built with another pw-runtime.mjs: run feed.sh$`));
    assert.equal(servable("feed", root), false);
    whole(root, "dist-feed");
    assert.equal(servable("feed", root), true);
    assert.deepEqual(feed.filter((file) => Object.hasOwn(optional(root), file)), []);
    // dist's own problem is the planner's to refuse, never a quiet skip.
    rmSync(join(root, "dist/sources.json"));
    const unserved = optional(root);
    assert.ok(!Object.hasOwn(unserved, "signals.spec.mjs") && !Object.hasOwn(unserved, "payments.spec.mjs") && !Object.hasOwn(unserved, "page-policy.spec.mjs"));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("a selection declares only its own hosts, refuses an unservable dist and drops an optional host only for tests that skip themselves", () => {
  const root = builds(["dist"]);
  const declared = (selection) => withEnvironment({ PW_PERFORMANCE: undefined, CARGO_TARGET_DIR: undefined, PW_REUSE: undefined },
    () => serversForSelection(selection, { root, port: 13201 }).map((host) => host.port).sort());
  try {
    assert.deepEqual(declared({ collectionOnly: true, receipt: null }), []);
    assert.deepEqual(declared(selected(one("signals.spec.mjs", "chromium"), one("views.spec.mjs", "webkit"))), [13201]);
    assert.deepEqual(declared(selected(one("cart.spec.mjs", "firefox"), one("cart.spec.mjs", "firefox"))), [13227]);
    assert.deepEqual(declared(selected(one("payments.spec.mjs", "webkit"))), [13383]);
    assert.deepEqual(declared(selected(one("kiokun.spec.mjs", "firefox"))), [13241]);
    // The feed's build is not built: page-policy's feed test skips itself, so
    // its host is not started; any other test of it should never be selected.
    const policy = (role) => one("page-policy.spec.mjs", "chromium", [{ type: "pw-host", description: role }]);
    assert.deepEqual(declared(selected(policy("base"), policy("feed"), policy("payments"))), [13201, 13381]);
    assert.throws(() => declared(selected(one("feed.spec.mjs", "chromium"))), /browser host plan: feed.spec.mjs needs feed, whose build cannot be served \(dist-feed is no whole build/);
    whole(root, "dist-feed");
    assert.deepEqual(declared(selected(policy("base"), policy("feed"), policy("payments"))), [13201, 13261, 13381]);
    assert.deepEqual(declared(selected(one("feed.spec.mjs", "chromium"))), [13261]);
    // dist itself refuses, wherever a selected test needs it, and only there.
    writeFileSync(join(root, "dist/pw-runtime.mjs"), "another runtime");
    assert.throws(() => declared(selected(one("signals.spec.mjs", "chromium"))), /browser host plan: dist was built with another pw-runtime.mjs: run run.sh/);
    assert.deepEqual(declared(selected(one("feed.spec.mjs", "chromium"), one("kiokun.spec.mjs", "chromium"))), [13241, 13261]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("selected descriptors preserve memory identity blobs provider ports target and reuse policy", () => {
  const root = builds(["dist", "dist-feed", "dist-kiokun", "dist-guestbook"]);
  try {
    const tests = ["signals.spec.mjs", "identity.spec.mjs", "uploads.spec.mjs", "payments.spec.mjs", "scheduled-orders.spec.mjs", "reviews.spec.mjs", "guestbook.spec.mjs", "kiokun.spec.mjs"]
      .map((file) => one(file, "webkit"));
    // A deployment's kiokun data where the caller's environment names it,
    // as the PC's does: no host may read it.
    const hosts = withEnvironment({
      CARGO_TARGET_DIR: join(root, "private-target"), PW_REUSE: "1", PW_PERFORMANCE: undefined,
      KIOKUN_DATA: join(root, "deployment-data"), KIOKUN_APP: join(root, "deployment-app"),
      PW_FEED_DATABASE_URL: "postgresql://deployment/feed", PW_STORE_DATABASE_URL: "postgresql://deployment/store",
    }, () => serversForSelection(selected(...tests), { root, port: 13201 }));
    assert.equal(hosts.length, 8);
    // Every host keeps its data in memory (ADR-0257), whatever the caller's
    // environment says, and never reads a deployment's kiokun data.
    for (const host of hosts) {
      assert.equal(host.env.PW_FEED_DATABASE_URL, "");
      assert.equal(host.env.PW_STORE_DATABASE_URL, "");
      for (const key of ["KIOKUN_DATA", "KIOKUN_APP"]) assert.ok(Object.hasOwn(host.env, key) && host.env[key] === undefined, key);
      assert.equal(host.reuseExistingServer, true);
      assert.equal(host.env.PORT, String(host.port));
    }
    const at = (port) => hosts.find((host) => host.port === port);
    // Each host is run by the launcher that ends it with its owner.
    assert.equal(at(13241).command, ownedCommand([`${root}/private-target/debug/kiokun-server`]));
    assert.ok(hosts.filter((host) => host.port !== 13241).every((host) => host.command.startsWith(`${ownedCommand([`${root}/private-target/debug/pw-dev-server`])} '`)));
    for (const port of [13273, 13403, 13413]) assert.equal(at(port).env.PW_IDENTITY, "dev-accounts");
    assert.ok(hosts.filter((host) => ![13273, 13403, 13413].includes(host.port)).every((host) => host.env.PW_IDENTITY === undefined));
    assert.equal(at(13283).env.PW_BLOB_DIR, "dist-feed/blobs-13283");
    assert.equal(at(13383).env.PW_PAYMENTS_PORT, "13393");
    assert.equal(at(13386).env.PW_PAYMENTS_PORT, "13396");
    assert.equal(at(13383).env.PW_PAYMENTS, "dev");
    assert.ok(hosts.filter((host) => ![13383, 13386].includes(host.port)).every((host) => host.env.PW_PAYMENTS_PORT === undefined && host.env.PW_PAYMENTS === undefined));
    assert.equal(at(13413).command.endsWith(" 'dist-guestbook'"), true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("PW_PERFORMANCE admits only the Chromium performance host", () => {
  const root = builds(["dist"]);
  const declared = (...tests) => withEnvironment({ PW_PERFORMANCE: "1", CARGO_TARGET_DIR: undefined },
    () => serversForSelection(selected(...tests), { root, port: 13201 }).map((host) => host.port));
  try {
    assert.deepEqual(declared(one("runtime-size.spec.mjs", "chromium"), one("performance.spec.mjs", "chromium")), [13208]);
    for (const extra of [one("runtime-size.spec.mjs", "firefox"), one("signals.spec.mjs", "chromium")]) {
      assert.throws(() => declared(one("performance.spec.mjs", "chromium"), extra), /PW_PERFORMANCE requires only the Chromium/);
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("native collection framing preserves every opaque argument before and after separator", () => {
  const argv = ["test", "--config", "custom.config.mjs", "--grep", "--ui", "--project=fire*", "--project=chromium", "--reporter=line", "--", "signals.spec.mjs"];
  assert.deepEqual(collectionArguments(argv), ["test", "--list", ...argv.slice(1)]);
  assert.deepEqual(collectionArguments(["test", "--grep", "--", "--", "signals.spec.mjs"]), ["test", "--list", "--grep", "--", "--", "signals.spec.mjs"]);
  assert.deepEqual(collectionArguments(["test", "signals.spec.mjs"]), ["test", "--list", "signals.spec.mjs"]);
  assert.throws(() => collectionArguments(["show-report"]), /native playwright test/);
});

test("collection receipts require exact issuer selection config completion and role identities", () => {
  const request = { nonce: "fresh", issuerPid: 12, cwd: "/private", root: "/private", argv: ["test"], collectionArgv: ["test", "--list"], configFile: "/private/custom.config.mjs" };
  const value = { schema: 1, complete: true, ...request, collectorPid: 13, forbidOnly: true, tests: [{ id: "selected", file: "signals.spec.mjs", project: "chromium", roles: ["base"] }] };
  assert.equal(validatePlan(value, request, 13), value);
  for (const mutate of [
    (bad) => bad.schema = 2,
    (bad) => bad.complete = false,
    (bad) => bad.nonce = "stale",
    (bad) => bad.issuerPid++,
    (bad) => bad.collectorPid++,
    (bad) => bad.cwd = "/elsewhere",
    (bad) => bad.root = "/elsewhere",
    (bad) => bad.argv.push("other.spec.mjs"),
    (bad) => bad.collectionArgv.push("other.spec.mjs"),
    (bad) => bad.configFile = "/elsewhere/config.mjs",
    (bad) => bad.forbidOnly = false,
    (bad) => bad.tests.push(bad.tests[0]),
    (bad) => bad.tests[0].roles = ["feed"],
    (bad) => bad.tests[0].project = "unknown",
    (bad) => bad.tests[0].file = "unknown.spec.mjs",
  ]) {
    const bad = structuredClone(value); mutate(bad);
    assert.throws(() => validatePlan(bad, request, 13), /browser host/);
  }
});

// Playwright swallows a reporter's exceptions, so the reporter publishes a
// receipt only after exactly one successful, complete collection.
test("public collection reporter publishes only one successful complete callback sequence", () => {
  const root = mkdtempSync(join(tmpdir(), "pw-host-reporter-"));
  try {
    const rows = [];
    for (const mode of ["complete", "no-end", "two-begins", "two-ends", "error", "test-begin", "failed"]) {
      const report = join(root, `${mode}-report.json`);
      const path = join(root, `${mode}-request.json`);
      writeFileSync(path, JSON.stringify({ schema: 1, nonce: mode, issuerPid: process.ppid, cwd: process.cwd(), root, report, argv: [], collectionArgv: process.argv.slice(2) }));
      withEnvironment({ PW_HOST_COLLECTION_REQUEST: path }, () => {
        const reporter = new HostPlanReporter();
        const config = { forbidOnly: true, configFile: join(root, "custom.config.mjs") };
        const suite = { allTests: () => [{ id: "known", location: { file: join(root, "e2e/signals.spec.mjs") }, parent: { project: () => ({ name: "chromium" }) }, annotations: [] }] };
        const quiet = console.error;
        console.error = () => {};
        try {
          reporter.onBegin(config, suite);
          if (mode === "two-begins") reporter.onBegin(config, suite);
          if (mode === "error") reporter.onError({ message: "controlled callback error" });
          if (mode === "test-begin") reporter.onTestBegin();
          if (mode !== "no-end") reporter.onEnd({ status: mode === "failed" ? "failed" : "passed" });
          if (mode === "two-ends") reporter.onEnd({ status: "passed" });
          reporter.onExit();
        } finally { console.error = quiet; }
      });
      rows.push({ mode, published: existsSync(report), receipt: existsSync(report) ? JSON.parse(readFileSync(report, "utf8")) : null });
    }
    assert.deepEqual(rows.map((row) => [row.mode, row.published]), [["complete", true], ["no-end", false], ["two-begins", false], ["two-ends", false], ["error", false], ["test-begin", false], ["failed", false]]);
    assert.equal(rows[0].receipt.complete, true);
    assert.deepEqual(rows[0].receipt.tests, [{ id: "known", file: "signals.spec.mjs", project: "chromium", roles: ["base"] }]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("spec metadata imports cannot recurse through the Playwright config", () => {
  for (const file of specFiles()) {
    assert.doesNotMatch(readFileSync(new URL(file, SPECS), "utf8"), /from ["']\.\.\/playwright\.config\.mjs["']/);
  }
});

// **A host ends with the process that started it** (`serve-while-owned.sh`).
// Playwright stops its hosts as it ends, but a runner killed outright (a
// recipe's bound) stops none, and each host, the leader of a process group of
// its own, lived on holding its port. These tests run the launcher as
// Playwright does, `sh -c` of the host's command with a pipe for its standard
// input that the owner holds; the events they wait for are the host's own
// word that it listens, its port refusing a connection, its exit.
const NODE = process.execPath;
// A host: listens on a port of the system's choosing, and says which.
const LISTENER = 's=require("net").createServer(c=>c.end()).listen(0,"127.0.0.1",()=>console.log("listening "+s.address().port+" "+process.pid));';
const CAP = 15000;

// The host's command run as Playwright runs it, its input a pipe held here.
function launched(command, options = {}) {
  const child = spawn("/bin/sh", ["-c", command], { stdio: ["pipe", "pipe", "inherit"], ...options });
  let text = "";
  const heard = [];
  child.stdout.on("data", (data) => { text += data; for (const waiting of heard) waiting(); });
  const said = (pattern, what) => new Promise((accept, reject) => {
    const timer = setTimeout(() => reject(new assert.AssertionError({ message: `${what}: never said ${pattern} (it said ${JSON.stringify(text)})` })), CAP);
    const look = () => { const found = text.match(pattern); if (found) { clearTimeout(timer); accept(found); } };
    heard.push(look);
    look();
  });
  const exit = new Promise((accept) => child.once("exit", (code, signal) => accept({ code, signal })));
  // Its exit, with a cap on a hang: a launcher that never ends fails here,
  // and does not hold the run.
  const exited = (what) => Promise.race([exit, new Promise((accept, reject) => {
    setTimeout(() => reject(new assert.AssertionError({ message: `${what}: did not end within ${CAP} ms` })), CAP).unref();
  })]);
  return { child, said, exited, text: () => text };
}
const connects = (port) => new Promise((accept) => {
  const socket = net.connect(port, "127.0.0.1");
  socket.once("connect", () => { socket.destroy(); accept(true); });
  socket.once("error", () => accept(false));
});
// Until `check` holds: the event itself, with a cap on a hang.
async function until(check, what) {
  const deadline = Date.now() + CAP;
  while (!(await check())) {
    if (Date.now() > deadline) assert.fail(`${what}: not within ${CAP} ms`);
    await new Promise((accept) => setTimeout(accept, 10));
  }
}
const gone = (pid) => { try { process.kill(pid, "SIGKILL"); } catch {} };

test("a host's command passes each word to its host as it is", async () => {
  const words = ["a b", "it's", "$HOME", "`id`", "*", ";echo no", "", "--flag=\"x\""];
  const run = launched(ownedCommand([NODE, "-e", "console.log(JSON.stringify(process.argv.slice(1)))", ...words]));
  try {
    const seen = await run.said(/^(\[.*\])$/m, "the host");
    assert.deepEqual(JSON.parse(seen[1]), words);
    assert.equal((await run.exited("the launcher")).code, 0);
  } finally { run.child.stdin.destroy(); }
});

test("a host's launcher keeps its host while its input is open and ends it when the input ends", async () => {
  const run = launched(ownedCommand([NODE, "-e", LISTENER]));
  let pid;
  try {
    // Up: the launcher did not end it as it started, with its input open.
    const [, port, hostPid] = await run.said(/listening (\d+) (\d+)/, "the host");
    pid = Number(hostPid);
    assert.equal(await connects(Number(port)), true, "the host answers while its input is open");
    run.child.stdin.end();
    await run.exited("the launcher, its input ended");
    await until(async () => !(await connects(Number(port))), "the host's port closing");
  } finally { if (pid) gone(pid); }
});

test("a host's launcher ends with its host's status, whatever its input", async () => {
  const run = launched(ownedCommand([NODE, "-e", "process.exit(7)"]));
  try {
    // The input stays open: the launcher does not wait for its end.
    assert.equal((await run.exited("the launcher")).code, 7);
  } finally { run.child.stdin.destroy(); }
});

test("a host's owner killed outright leaves its host listening nowhere", async () => {
  // The owner is a process of its own that starts the host as Playwright
  // does (a group of its own, a pipe for its input) and is then killed, as a
  // recipe's bound kills a runner.
  const owner = `const {spawn}=require("child_process");
const host=spawn("/bin/sh",["-c",process.argv[1]],{detached:true,stdio:["pipe","pipe","inherit"]});
host.stdout.on("data",(data)=>process.stdout.write(data));
setInterval(()=>{},1e6);`;
  let pid;
  let owning;
  try {
    owning = spawn(NODE, ["-e", owner, ownedCommand([NODE, "-e", LISTENER])], { stdio: ["ignore", "pipe", "inherit"] });
    let text = "";
    const [, port, hostPid] = await new Promise((accept, reject) => {
      const timer = setTimeout(() => reject(new assert.AssertionError({ message: `the host never said it listens (it said ${JSON.stringify(text)})` })), CAP);
      owning.stdout.on("data", (data) => {
        text += data;
        const found = text.match(/listening (\d+) (\d+)/);
        if (found) { clearTimeout(timer); accept(found); }
      });
    });
    pid = Number(hostPid);
    assert.equal(await connects(Number(port)), true, "the host listens while its owner lives");
    owning.kill("SIGKILL");
    await until(async () => !(await connects(Number(port))), "the host's port closing after its owner was killed");
  } finally {
    owning?.kill("SIGKILL");
    if (pid) gone(pid);
  }
});
