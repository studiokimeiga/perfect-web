// Variants of the recorded run (the report test's fixture), each breaking the
// record in one way the record itself does not: a run that failed, an
// aborted request, a measurement that was not made, a server run that
// failed, a number an engine did not report, and the rest. The report of a
// variant differs from the record's by a few lines, which
// `variants.json` holds (made by the report as it was written when this was
// recorded); `report-branches.test.mjs` renders each and compares.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readSections } from "../../../head-to-head.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

/** The recorded run and its sections, freshly read, to be changed. */
export function recorded() {
  return { result: JSON.parse(fs.readFileSync(path.join(HERE, "runs.json"), "utf8")), sections: readSections(HERE) };
}

const tally = (lines) => lines.reduce((counts, l) => counts.set(l, (counts.get(l) ?? 0) + 1), new Map());

/** The lines of `lines` that `other` has no more of: a multiset difference, in order. */
function without(lines, other) {
  const left = tally(other);
  return lines.filter((l) => {
    const n = left.get(l) ?? 0;
    left.set(l, n - 1);
    return n <= 0;
  });
}

/** What changed from one report to another: the lines only the first has, and the lines only the second has. */
export function delta(before, after) {
  const a = before.split("\n");
  const b = after.split("\n");
  return { removed: without(a, b), added: without(b, a) };
}

const appOf = (r, name) => r.result.apps.find((a) => a.app === name);

const unshaped = (x, engine, condition) => x.engine === engine && x.condition === condition && !x.network;

/** An app's unshaped runs of a stack in an engine, cold or warm. */
const runsOf = (app, stack, engine, condition) => app.runs.filter((x) => x.stack === stack && unshaped(x, engine, condition));

const coldChromium = (r, name, stack) => runsOf(appOf(r, name), stack, "chromium", "cold");

/** A server measurement's runs of one stack in an app. */
const serverRuns = (r, name, key, stack) => appOf(r, name)[key].filter((x) => x.stack === stack);

const SERVER_KEYS = ["loads", "idle", "presses"];

const firstOfEach = (list) => list.filter((x, k, all) => all.findIndex((y) => y.stack === x.stack) === k);

/** The servers' measurements as a kept run from before 2026-11-02 holds them: one run a stack, by stack. */
const legacy = (list) => Object.fromEntries(firstOfEach(list).map(({ stack, ...rest }) => [stack, rest]));

const dropSizes = (q) => {
  delete q.type;
  delete q.raw;
  delete q.br;
  delete q.gzip;
};

const withPlaywrightBytes = (q) => {
  q.receivedBody = 100;
  q.receivedHeaders = 20;
};

const abort = (q) => {
  q.failed = "net::ERR_ABORTED";
};

const storePleris = (r) => coldChromium(r, "store", "pleris")[0];

export const VARIANTS = {
  "a run that failed": (r) => {
    const run = coldChromium(r, "todomvc", "pleris")[0];
    Object.assign(run, { error: "browser.newContext: Target page, context or browser has been closed", requests: [], interactions: [], timings: {} });
    delete run.received;
  },
  "a run kept with nothing in it": (r) => {
    const run = coldChromium(r, "todomvc", "pleris")[0];
    ["timings", "interactions", "requests", "received"].forEach((key) => delete run[key]);
  },
  "responses without their sizes or type": (r) => storePleris(r).requests.slice(0, 3).forEach(dropSizes),
  "Playwright's byte counts where Chromium's are absent": (r) => {
    coldChromium(r, "todomvc", "react").forEach((run) => {
      delete run.received;
      run.requests.forEach(withPlaywrightBytes);
    });
  },
  "requests aborted before their response ended": (r) => {
    const { requests } = storePleris(r);
    abort(requests.find((q) => q.phase === "load"));
    abort(requests.find((q) => q.phase === "interaction:add"));
  },
  "Chromium byte counts without their phases": (r) => {
    delete storePleris(r).received[0].byPhase;
  },
  "bytes that differ between the runs of a cell": (r) => {
    const run = coldChromium(r, "todomvc", "react")[1];
    run.received[0].byPhase = { load: 12345 };
    run.requests[0].raw += 5;
  },
  "servers measured once a stack, as a run from before 2026-11-02 keeps them": (r) => {
    r.result.apps.forEach((app) => SERVER_KEYS.forEach((key) => (app[key] = legacy(app[key]))));
  },
  "a server run that failed": (r) => {
    SERVER_KEYS.forEach((key) => (serverRuns(r, "store", key, "next-react")[0].error = "boom: the server went away"));
  },
  "every server run of a stack failed": (r) => {
    SERVER_KEYS.forEach((key) => serverRuns(r, "todomvc", key, "svelte").forEach((x) => (x.error = "boom: the server went away")));
  },
  "every server run of an app failed": (r) => {
    SERVER_KEYS.forEach((key) => appOf(r, "todomvc")[key].forEach((x) => (x.error = "boom: the server went away")));
  },
  "no server run of a stack": (r) => {
    SERVER_KEYS.forEach((key) => (appOf(r, "todomvc")[key] = appOf(r, "todomvc")[key].filter((x) => x.stack !== "svelte")));
  },
  "fixed loads that state no statuses": (r) => {
    r.result.apps.forEach((app) => app.loads.forEach((x) => delete x.statuses));
  },
  "server runs that state no resolution": (r) => {
    r.result.apps.forEach((app) => SERVER_KEYS.forEach((key) => app[key].forEach((x) => delete x.resolution)));
  },
  "servers not measured": (r) => {
    r.result.meta.args.load = false;
    r.result.apps.forEach((app) => SERVER_KEYS.forEach((key) => (app[key] = [])));
  },
  "shaping not recorded": (r) => {
    r.result.meta.network = null;
  },
  "no shaping": (r) => {
    r.result.meta.network = null;
    r.result.meta.args.shaped = false;
  },
  "uncommitted changes, no settling, no count of server runs": (r) => {
    const { meta } = r.result;
    meta.dirty = "uncommitted changes";
    delete meta.settled;
    delete meta.args.serverRuns;
    delete meta.loadAverageAfter;
  },
  "a store without Pleris": (r) => {
    const store = appOf(r, "store");
    store.targets = store.targets.filter((t) => t.stack !== "pleris");
    store.runs = store.runs.filter((x) => x.stack !== "pleris");
  },
  "no app measured": (r) => {
    r.result.apps = [];
  },
  "Pleris behind in the specification and the Cypress suite": (r) => {
    r.sections.behaviour.spec.stacks.pleris.passed = 10;
    r.sections.behaviour.cypress.pleris.passed = 5;
  },
  "feature changes the checkers did not judge": (r) => {
    const cells = r.sections.feature.rows.flatMap((row) => Object.values(row.stacks));
    cells.forEach((c) => {
      delete c.reference;
      delete c.unsafe;
    });
  },
  "unsafe versions with no list of new warnings": (r) => {
    const cells = r.sections.feature.rows.flatMap((row) => Object.values(row.stacks));
    cells.forEach((c) => delete c.unsafe.newWarnings);
  },
  "a timing reported by some runs of a cell only": (r) => {
    coldChromium(r, "todomvc", "pleris").slice(0, 2).forEach((run) => (run.timings.lcp = null));
  },
  "a timing reported by no run of a cell": (r) => {
    runsOf(appOf(r, "store"), "pleris", "firefox", "cold").forEach((run) => (run.timings.fcp = null));
  },
  "no sections": (r) => {
    r.sections = {};
  },
  "a stack with no cold Chromium run": (r) => {
    const todomvc = appOf(r, "todomvc");
    todomvc.runs = todomvc.runs.filter((x) => !(x.stack === "svelte" && unshaped(x, "chromium", "cold")));
  },
  "a stack with no warm Chromium run": (r) => {
    const todomvc = appOf(r, "todomvc");
    todomvc.runs = todomvc.runs.filter((x) => !(x.stack === "svelte" && unshaped(x, "chromium", "warm")));
  },
  "a stack with no interaction in a run": (r) => {
    coldChromium(r, "store", "next-react").forEach((run) => delete run.interactions);
  },
};
