// **The browser suite's hosts, as data** (the host map, ADR-0356). Every host
// a spec can reach, its port, its build and what it depends on are read from
// `host-registry.json`; the specs import their ports from here, and the
// config (`playwright.config.mjs`) starts only the hosts the selected tests
// need (`host-plan.mjs`). Data and projections only: importing a spec must
// never load Playwright's config, which plans hosts as it loads.
//
// The own renderer's pages come from a Rust development server that owns
// `pw-materialize`, `pw-resource`, `pw-render` and `pw-protocol`. The browser
// sees only the protocol. Not E8's production host: its deletion condition is
// that E8 replaces it with the capability-constrained one while preserving
// the `pw-protocol` boundary.
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { basename, dirname, isAbsolute, relative, resolve } from "node:path";

const own = (value, key) => Object.hasOwn(value, key);
const object = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const fail = (message) => { throw new Error(`browser host registry: ${message}`); };
const integer = (value, label, minimum = 0) => {
  if (!Number.isSafeInteger(value) || value < minimum || value > 65535) fail(`${label} must be an integer in ${minimum}..65535`);
};
const names = (value, label) => {
  if (!Array.isArray(value) || value.some((name) => typeof name !== "string" || !name)) fail(`${label} must contain names`);
  if (new Set(value).size !== value.length) fail(`${label} contains duplicate names`);
};

// **The registry**, checked before anything reads it: every host's offset is
// its own and inside one slot's stride (a mutation slot's ports are
// `PORT + k * stride + offset`, scripts/mutation_slots.py), every dependency
// is an auxiliary listener with an offset per engine, and every spec names
// known roles. The old stride, 200, put slot 0's reviews and guestbook hosts
// on slot 1's base and public-fragment hosts.
export function validateRegistry(value) {
  if (!object(value) || value.schema !== 1) fail("unsupported schema");
  integer(value.stride, "stride", 1);
  names(value.engines, "engines");
  if (!value.engines.length) fail("engines is empty");
  for (const field of ["builds", "roles", "mutable", "portExports", "specs"]) {
    if (!object(value[field])) fail(`${field} must be an object`);
  }
  for (const [name, build] of Object.entries(value.builds)) {
    if (build?.externalGeneratedSources !== undefined && typeof build.externalGeneratedSources !== "boolean") fail(`${name}.externalGeneratedSources must be boolean`);
    if (!object(build) || typeof build.prerequisite !== "string" || !build.prerequisite ||
        (build.dist !== null && (typeof build.dist !== "string" || !/^dist(?:-[a-z0-9-]+)?$/.test(build.dist)))) fail(`invalid build ${name}`);
  }
  const seen = new Map();
  for (const [name, role] of Object.entries(value.roles)) {
    if (!object(role) || !object(role.offsets)) fail(`invalid role ${name}`);
    const keys = Object.keys(role.offsets);
    if (!(keys.length === 1 && keys[0] === "shared") &&
        (keys.length !== value.engines.length || value.engines.some((engine) => !own(role.offsets, engine)))) fail(`${name} must declare shared or every engine offset`);
    for (const [engine, offset] of Object.entries(role.offsets)) {
      integer(offset, `${name}/${engine} offset`);
      if (offset >= value.stride) fail(`${name}/${engine} offset ${offset} escapes stride ${value.stride}`);
      if (seen.has(offset)) fail(`duplicate offset ${offset}: ${seen.get(offset)} and ${name}/${engine}`);
      seen.set(offset, `${name}/${engine}`);
    }
    if (role.auxiliary === true) {
      if (role.build !== null || role.provider !== undefined || role.depends !== undefined) fail(`invalid auxiliary role ${name}`);
    } else if (typeof role.build !== "string" || !own(value.builds, role.build)) fail(`${name} names an unknown build`);
    for (const flag of ["auxiliary", "reserved", "identity", "blobs"]) {
      if (role[flag] !== undefined && typeof role[flag] !== "boolean") fail(`${name}.${flag} must be boolean`);
    }
    if (role.depends !== undefined) {
      names(role.depends, `${name}.depends`);
      for (const dependency of role.depends) {
        if (!own(value.roles, dependency) || value.roles[dependency].auxiliary !== true) fail(`${name} names unknown auxiliary dependency ${dependency}`);
        const offsets = value.roles[dependency].offsets;
        if (!object(offsets) || keys.some((engine) => !own(offsets, engine))) fail(`${name} dependency ${dependency} has incompatible engines`);
      }
    }
    if (role.provider !== undefined && (typeof role.provider !== "string" || !role.depends?.includes(role.provider))) fail(`${name} provider is not a declared dependency`);
  }
  if (!seen.size) fail("roles is empty");
  for (const field of ["mutable", "portExports"]) {
    for (const [name, role] of Object.entries(value[field])) {
      if (typeof role !== "string" || !own(value.roles, role) || value.roles[role].auxiliary) fail(`${field}.${name} names unknown host role ${role}`);
    }
  }
  for (const [name, spec] of Object.entries(value.specs)) {
    if (!/^[a-z0-9-]+\.spec\.mjs$/.test(name) || !object(spec)) fail(`invalid spec ${name}`);
    if (spec.annotations === true && spec.roles === undefined) continue;
    names(spec.roles, `${name}.roles`);
    if (!spec.roles.length || spec.annotations !== undefined) fail(`conflicting roles for ${name}`);
    for (const role of spec.roles) if (!own(value.roles, role) || value.roles[role].auxiliary) fail(`${name} names unknown host role ${role}`);
  }
  return value;
}

function freeze(value) {
  if (value && typeof value === "object") {
    Object.values(value).forEach(freeze);
    Object.freeze(value);
  }
  return value;
}

export const REGISTRY = freeze(validateRegistry(JSON.parse(readFileSync(new URL("./host-registry.json", import.meta.url), "utf8"))));
export const ENGINES = REGISTRY.engines;
export const STRIDE = REGISTRY.stride;
export const BUILDS = REGISTRY.builds;
// A spec whose tests run on different hosts (`page-policy.spec.mjs`) names
// each test's host with this annotation, `{ type: "pw-host", description:
// "<role>" }`, on the test or its describe.
export const HOST_ANNOTATION = "pw-host";
export const PORT = Number(process.env.PORT ?? 3141);
const ROOT = fileURLToPath(new URL(".", import.meta.url));

export function listenersForSlots(registry, base, count = 1) {
  validateRegistry(registry);
  integer(base, "PORT", 1);
  integer(count, "slot count", 1);
  const max = Math.max(...Object.values(registry.roles).flatMap((role) => Object.values(role.offsets)));
  if (base + (count - 1) * registry.stride + max > 65535) fail("PORT and slot count exceed TCP port 65535");
  return Array.from({ length: count }, (_, slot) => Object.entries(registry.roles).flatMap(([role, definition]) =>
    Object.entries(definition.offsets).map(([engine, offset]) => ({ slot, role, engine, port: base + slot * registry.stride + offset })))).flat();
}

export function portsForRole(role, base = PORT, registry = REGISTRY) {
  validateRegistry(registry);
  listenersForSlots(registry, base);
  if (!own(registry.roles, role)) fail(`unknown role ${role}`);
  return Object.fromEntries(Object.entries(registry.roles[role].offsets).map(([engine, offset]) => [engine, base + offset]));
}

// The roles one selected test needs, with the auxiliary listeners they
// depend on: its spec's, or for a spec whose tests differ, the one its
// annotation names. An unregistered spec is refused, never given every host.
export function rolesForTest(file, engine, annotations = [], registry = REGISTRY) {
  validateRegistry(registry);
  if (!registry.engines.includes(engine)) fail(`unknown project/engine ${engine}`);
  if (!own(registry.specs, file)) fail(`unregistered selected spec ${file}`);
  const spec = registry.specs[file];
  const written = annotations.filter((annotation) => annotation.type === HOST_ANNOTATION).map((annotation) => annotation.description);
  if (spec.annotations !== true && written.length) fail(`conflicting host annotation in ${file}`);
  const roles = spec.annotations === true ? written : spec.roles;
  names(roles, `${file} selected roles`);
  if (spec.annotations === true && roles.length !== 1) fail(`${file} requires exactly one static host annotation`);
  const closure = new Set();
  for (const role of roles) {
    if (!own(registry.roles, role) || registry.roles[role].auxiliary) fail(`unknown selected host role ${role}`);
    // A reserved role keeps its ports for an app not on this branch.
    if (registry.roles[role].reserved === true) fail(`selected host role ${role} is reserved: its app is not on this branch`);
    closure.add(role);
    for (const dependency of registry.roles[role].depends ?? []) closure.add(dependency);
  }
  return [...closure].sort();
}

// The role whose host a test's `baseURL` is, for the engine it runs in: what
// `page-policy.spec.mjs` holds each test's annotation to.
export function roleOfURL(url, engine, base = PORT, registry = REGISTRY) {
  const port = Number(new URL(url).port);
  const found = Object.entries(registry.roles).filter(([, role]) =>
    Object.entries(role.offsets).some(([key, offset]) => (key === "shared" || key === engine) && base + offset === port));
  if (found.length !== 1) fail(`${url} is no registered host of ${engine}`);
  return found[0][0];
}

const portExport = (name) => freeze(portsForRole(REGISTRY.portExports[name]));

// **The suites that mutate shared state** (`REGISTRY.mutable`), each with a
// host per engine. The menu is public data: one list, broadcast to every
// subscriber, which is the property `keyed-list.spec.mjs` exists to prove.
// That makes it hostile to a parallel suite: a reorder committed by one run is
// visible to every other, and `store.spec.mjs` reasonably assumes three items
// in a known order. Isolation in the HARNESS rather than in the server: a
// per-session menu would delete the property under test. One host per engine,
// because the three engine projects run concurrently and a single extra host
// merely moved the interference somewhere less obvious. And per SUITE: two
// mutating files on one host interfere exactly as two engines did, and the
// second time it presented as frames arriving in a different order rather
// than as a wrong list.
// - `public-fragment` since 2026-09-25: it renames a menu item and back, which
//   broadcasts a patch to every page on its host; on the shared host a store
//   page read another test's rename as its latest batch (1 in 15 runs).
// - `stream` (ADR-0148): its recommender is one per server.
// - `availability` (ADR-0157), `cart` (ADR-0172), `accessibility` (ADR-0182):
//   an item sold out is sold out for every page on the host.
// - `stores` (ADR-0162): store 47's menu, which a test renames.
// - `slots` (ADR-0165): the recommender, which a test slows.
// - `controls` (ADR-0174): the store's delay, which a test sets.
// - `materialized` (ADR-0277): the menu a test adds to and takes from.
// - `performance`: the performance run needs ONE host, not eleven. Playwright
//   starts every declared `webServer` before the first test, and eleven
//   processes coming up while the first navigation happens is itself the load
//   a long-animation-frame measurement is trying not to see (gate 8 once
//   reported a long frame on a cold start and none on a warm one).
export const MUTABLE_PORTS = freeze(Object.fromEntries(Object.entries(REGISTRY.mutable).map(([name, role]) => [name, portsForRole(role)])));
// The kiokun slice's host (E10): a second application, its own server, and
// read-only, so every engine shares it.
export const KIOKUN_PORT = portsForRole("legacy-kiokun").shared;
// The keyed store's hosts (ADR-0152): the store with T07's category tabs,
// built into `dist-keyed` by `keyed-store.sh`. One per engine, because which
// category is slow is one per server.
export const KEYED_PORTS = portExport("KEYED_PORTS");
// The feed reference app's hosts (ADR-0220), built into `dist-feed` by
// `feed.sh`. One per engine, because a post is every reader's.
export const FEED_PORTS = portExport("FEED_PORTS");
// The identity track's hosts (ADR-0258): the feed's build, its sessions
// signed in through the development identity provider (`PW_IDENTITY`).
export const IDENTITY_PORTS = portExport("IDENTITY_PORTS");
// The uploads track's hosts (ADR-0260): the feed's build, each with a blob
// storage of its own (`PW_BLOB_DIR`), so one engine's images are never
// another's.
export const UPLOADS_PORTS = portExport("UPLOADS_PORTS");
// The notifications track's hosts (ADR-0274) and the messages track's
// (ADR-0279): the feed's build, signed in, one per engine, because a
// notification or a message is its users' on a host.
export const NOTIFICATIONS_PORTS = portExport("NOTIFICATIONS_PORTS");
export const MESSAGES_PORTS = portExport("MESSAGES_PORTS");
// kiokun.com in Pleris (track `kiokun`), one per engine, beside the slice's
// single host.
export const KIOKUN_PORTS = portExport("KIOKUN_PORTS");
// TodoMVC's hosts (track `todomvc`), one per engine.
export const TODOMVC_PORTS = portExport("TODOMVC_PORTS");
// The large list's hosts, one per engine.
export const ROWS_PORTS = portExport("ROWS_PORTS");
// The large list's fixture of presses, a build of its own, one per engine.
export const ROW_EVENTS_PORTS = portExport("ROW_EVENTS_PORTS");
// The store-accounts track's hosts: the store's build, signed in, one per
// engine, because an account and its cart are every reader's on a host.
export const STORE_ACCOUNTS_PORTS = portExport("STORE_ACCOUNTS_PORTS");
// The store's search hosts (W9): a test adds a dish to store 47's menu.
export const STORE_SEARCH_PORTS = portExport("STORE_SEARCH_PORTS");
// The store's build taking payments from the development provider
// (`PW_PAYMENTS=dev`), one per engine, because an order awaiting its payment
// is its reader's on a host. Each host's provider listens on an origin of its
// own, the registry's `payment-provider` port (`PW_PAYMENTS_PORT`), which the
// order page frames; it runs in the host's own process.
export const STORE_PAYMENTS_PORTS = portExport("STORE_PAYMENTS_PORTS");
// The scheduled orders' hosts: as the payments' hosts, on hosts of their own,
// because a test of an order for a time moves its host's clock forward, which
// would sweep another suite's order awaiting its payment.
export const SCHEDULED_ORDERS_PORTS = portExport("SCHEDULED_ORDERS_PORTS");
// The reviews' hosts: the store's build, signed in, because a store's ratings
// are every reader's on a host.
export const REVIEWS_PORTS = portExport("REVIEWS_PORTS");
// The guide's tutorial app (ADR-0343): the guestbook, built into
// `dist-guestbook` by `guestbook.sh`, signed in.
export const GUESTBOOK_PORTS = portExport("GUESTBOOK_PORTS");

// **The hosts a spec's source reaches**, which the host map's holding test
// (`host-plan.test.mjs`) compares with the registry for every spec. A spec
// reaches a host only through its `baseURL`: the config's, which is `PORT`'s
// `base` host, unless the file overrides it at its top level; or an override
// that names a port this module exports, `http://127.0.0.1:${FEED_PORTS[..]}`.
// A loopback address written any other way is refused: the map could not see
// which host it is.
export function hostsReachedBy(source, registry = REGISTRY) {
  const roles = new Set();
  const named = (expression) => {
    const m = /^([A-Z_]+)(?:\.([a-z][a-z-]*)|\["([a-z][a-z-]*)"\])?(?:\[[^\]]+\])?$/.exec(expression.trim());
    if (!m) fail(`a host named by \`${expression}\`, which names no registered role`);
    const [, name, dotted, quoted] = m;
    const member = dotted ?? quoted;
    if (name === "MUTABLE_PORTS" && member && own(registry.mutable, member)) return registry.mutable[member];
    if (name === "KIOKUN_PORT" && !member) return "legacy-kiokun";
    if (name === "PORT" && !member) return "base";
    if (own(registry.portExports, name) && (!member || registry.engines.includes(member))) return registry.portExports[name];
    fail(`a host named by \`${expression}\`, which names no registered role`);
  };
  for (const m of source.matchAll(/(?:127\.0\.0\.1|localhost|\[::1\]):(\$\{|\d)?/g)) {
    if (m[1] !== "${") fail(`a loopback address without a registered port (${source.slice(m.index, m.index + 30)})`);
    let depth = 1;
    let end = m.index + m[0].length;
    for (; end < source.length && depth; end++) depth += source[end] === "{" ? 1 : source[end] === "}" ? -1 : 0;
    roles.add(named(source.slice(m.index + m[0].length, end - 1)));
  }
  // An override of the whole file: a `test.use(..)` at its top level that
  // gives `baseURL`. Without one, its tests (or some of them) run on `base`.
  const topLevel = [...source.matchAll(/^test\.use\(/gm)].some((m) => {
    let depth = 0;
    let end = m.index + "test.use".length;
    for (; end < source.length; end++) {
      depth += source[end] === "(" ? 1 : source[end] === ")" ? -1 : 0;
      if (!depth) break;
    }
    return /\bbaseURL\b/.test(source.slice(m.index, end));
  });
  if (!topLevel) roles.add("base");
  return [...roles].sort();
}

// The host annotations a spec's source writes, for a spec whose tests differ.
export function hostAnnotationsIn(source) {
  return [...new Set([...source.matchAll(/\{\s*type:\s*"pw-host",\s*description:\s*"([^"]+)"\s*\}/g)].map((m) => m[1]))].sort();
}

// **Which builds a run may lack.** `dist`, the shared demo's build (the
// `base` role's), is required: `run.sh` makes it before every suite and
// every mutation control's run, and a selected test that needs it refuses a
// build that is not whole or not current (ADR-0161, ADR-0166; a suite once
// ran on a build stopped part-way, track evidence-integrity's finding 5).
// Every other build is optional, as it was before the host map: while one is
// unbuilt or stale its specs are not run (ADR-0152, ADR-0220). Refusing them
// would fail every recipe that builds only what its specs need, and CI's
// browser jobs, which build neither kiokun's nor the guestbook's (the
// integrator's ruling of 2026-10-19). But never silently: each spec left out
// is said, with each build it lacks and what to run (`unservedSpecs`), and a
// test in `just ci` holds that each spec an optional build can leave out runs
// in at least one recipe that makes that build (`scripts/skipped_specs.py`).
export const REQUIRED_BUILD = REGISTRY.roles.base.build;

// Availability is information, never a reason for the collection phase to
// skip a required build: the planner checks the builds its selection needs.
function canonicalMissingSource(path) {
  let ancestor = path;
  const suffix = [];
  while (!existsSync(ancestor)) {
    // A dangling link cannot prove that a generated input lies outside the
    // repository. Refuse rather than applying an exemption by written path.
    try { if (lstatSync(ancestor).isSymbolicLink()) throw new Error("dangling generated-source link"); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    suffix.unshift(basename(ancestor));
    const parent = dirname(ancestor);
    if (parent === ancestor) throw new Error("generated-source location cannot be resolved");
    ancestor = parent;
  }
  return resolve(realpathSync(ancestor), ...suffix);
}

// Why a build cannot be served, or null: it is not whole (`run.sh` records
// its sources last, so a build stopped part-way has no `sources.json`), it
// was built with another `pw-runtime.mjs` (a build serves the runtime it was
// built with: `dist-keyed` once ran the old runtime against the new server;
// a production build's is packed, and its `delivery.json` says what from),
// a source it recorded changed or is gone since (ADR-0166), or it is not
// built. Only `keyed-store.sh`'s copies, made outside the repository and
// removed after its build, may be gone.
function isBuiltFrom(runtime, directory) {
  if (runtime.equals(readFileSync(resolve(directory, "pw-runtime.mjs")))) return true;
  // A production build packs its runtime, and says what it packed.
  try {
    const { runtime: packed } = JSON.parse(readFileSync(resolve(directory, "delivery.json"), "utf8"));
    return packed === createHash("sha256").update(runtime).digest("hex");
  } catch {
    return false;
  }
}

export function buildProblem(name, root = ROOT) {
  const build = BUILDS[name];
  if (!build) fail(`unknown build ${name}`);
  if (build.dist === null) return null;
  const directory = resolve(root, build.dist);
  const runtime = resolve(root, "public/pw-runtime.mjs");
  const problem = (why) => `${why}: run ${build.prerequisite}`;
  if (!existsSync(resolve(directory, "sources.json"))) return problem(`${build.dist} is no whole build (sources.json is missing)`);
  if (!existsSync(runtime) || !existsSync(resolve(directory, "pw-runtime.mjs")) || !isBuiltFrom(readFileSync(runtime), directory)) return problem(`${build.dist} was built with another pw-runtime.mjs`);
  try {
    const { files } = JSON.parse(readFileSync(resolve(directory, "sources.json"), "utf8"));
    if (!Array.isArray(files) || files.some((file) => !object(file) || typeof file.path !== "string" || typeof file.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(file.sha256))) return problem(`${build.dist} has invalid source records`);
    for (const { path, sha256 } of files) {
      const source = resolve(root, path);
      if (!existsSync(source)) {
        if (build.externalGeneratedSources === true) {
          const repository = realpathSync(resolve(root, "../.."));
          const outside = relative(repository, canonicalMissingSource(source));
          if (outside === ".." || outside.startsWith("../") || outside.startsWith("..\\") || isAbsolute(outside)) continue;
        }
        return problem(`missing required source ${path}`);
      }
      if (createHash("sha256").update(readFileSync(source)).digest("hex") !== sha256) return problem(`${path} changed since ${build.dist} was built`);
    }
  } catch (error) {
    return problem(`${build.dist} source records cannot be read (${error.message})`);
  }
  if (!existsSync(resolve(directory, "build"))) return problem(`${build.dist} is not built`);
  return null;
}

// An optional build is built when its `build` directory exists; one that is
// built and has a problem is stale. Read afresh at each call: a recipe builds
// between runs, and a test changes a build between its checks.
export function buildState(name, root = ROOT) {
  const build = BUILDS[name];
  if (!build) fail(`unknown build ${name}`);
  const built = build.dist === null || existsSync(resolve(root, build.dist, "build"));
  return { built, problem: buildProblem(name, root) };
}

// Whether a build can be served: what an optional build's tests ask before
// they run (`test.skip(!servable("feed"), ..)` in a spec whose tests differ).
export const servable = (name, root = ROOT) => buildProblem(name, root) === null;

// The specs not run because an optional build they need cannot be served,
// each with the warning to print: never a silent skip (the integrator's
// ruling of 2026-10-19). The warning names the spec and each build it lacks,
// with why it cannot be served and what to run (`buildProblem`), or, for a
// build not made at all, that it is not built and what to run.
export function unservedSpecs(root = ROOT, registry = REGISTRY) {
  const states = new Map();
  const state = (build) => {
    if (!states.has(build)) states.set(build, buildState(build, root));
    return states.get(build);
  };
  const out = [];
  for (const [file, spec] of Object.entries(registry.specs)) {
    if (spec.annotations === true) continue;
    const builds = [...new Set(spec.roles.map((role) => registry.roles[role].build))];
    const missing = builds.filter((build) => build !== REQUIRED_BUILD && state(build).problem !== null);
    if (missing.length) {
      const why = missing.map((build) => {
        const { built, problem } = state(build);
        return built ? problem : `${BUILDS[build].dist} is not built: run ${BUILDS[build].prerequisite}`;
      });
      out.push({ file, warning: `e2e/${file} is not run: ${why.join("; ")}` });
    }
  }
  return out;
}
