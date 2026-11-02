// The head-to-head's report (ADR-0392): every table read from the runs, and
// where Pleris loses, first, in the same tables' terms.
//
// Bytes are the same in every cold run of a stack (a build is a build), and
// the report says so or shows the spread; they are read from Chromium's runs
// (ADR-0342 §4's instrument), and the bytes each engine received are shown
// beside. Times are medians with their quartiles and extremes, per engine,
// condition and network. A difference in time is reported as a loss or a win
// only where the two spreads are apart by at least the engines' timer
// resolution, a millisecond (one's first quartile that much above the other's
// third); otherwise it is "within the spread". The servers' measurements are
// repeated too (`head-to-head.mjs`'s `serverRuns`) and judged by the same
// rule, apart by what their instrument can tell apart; and a stack whose server is the
// instrument's stand-in for any static host is never one Pleris's host "wins"
// against.

import { summary } from "./lib.mjs";

const fmt = (n) => (n == null ? "-" : Math.round(n).toLocaleString("en-US"));
const ms = (n) => (n == null ? "-" : n < 10 ? n.toFixed(1) : Math.round(n).toString());
/** A server's number, to the precision its row is judged at and the ADR cites. */
const fine = (n) => (n == null ? "-" : n < 10 ? n.toFixed(2) : n < 100 ? n.toFixed(1) : Math.round(n).toLocaleString("en-US"));
const cls = (n) => (n == null ? "-" : n.toFixed(3));
const pad = (s, w) => String(s).padEnd(w);
const rpad = (s, w) => String(s).padStart(w);

function isCommon(url, common) {
  const p = new URL(url).pathname;
  return common.some((src) => new RegExp(src.slice(1, src.lastIndexOf("/")), src.slice(src.lastIndexOf("/") + 1)).test(p));
}

const KINDS = [
  ["document", (q) => q.kind === "document"],
  ["script", (q) => /javascript/.test(q.type ?? "")],
  ["wasm", (q) => /wasm/.test(q.type ?? "")],
  ["style", (q) => /css/.test(q.type ?? "")],
  ["other", () => true],
];

function kindOf(q) {
  return KINDS.find(([, test]) => test(q))[0];
}

// --- bytes ------------------------------------------------------------------

/** A phase's name tested: "interactions" is every interaction's. */
const phaseMatcher = (phase) => (p) => (phase === "interactions" ? p.startsWith("interaction") : p === phase);

/** Whether a response or a byte count is counted: TodoMVC's common files only where asked for. */
const counted = (url, common, withCommon) => withCommon || !isCommon(url, common);

const emptyBytes = (run) => ({
  requests: 0,
  aborted: 0,
  raw: 0,
  br: 0,
  gzip: 0,
  received: 0,
  byKind: {},
  encodings: new Set(),
  notModified: 0,
  source: run.received ? "chromium" : "playwright",
});

/** A response's sizes into `to`: each count named, none where it has none. */
const addSizes = (to, q, keys) => keys.forEach((key) => (to[key] += q[key] ?? 0));

/** What the engine says it received of a response (Playwright's), headers included. */
const receivedBy = (q) => (q.receivedBody ?? 0) + (q.receivedHeaders ?? 0);

function addKind(byKind, q) {
  const kind = (byKind[kindOf(q)] ??= { requests: 0, raw: 0, br: 0 });
  kind.requests++;
  addSizes(kind, q, ["raw", "br"]);
}

function addResponse(out, q, fromPlaywright) {
  out.requests++;
  if (q.status === 304) out.notModified++;
  addSizes(out, q, ["raw", "br", "gzip"]);
  if (fromPlaywright) out.received += receivedBy(q);
  if (q.encoding) out.encodings.add(q.encoding);
  addKind(out.byKind, q);
}

/** The responses into `out`; a request that ended without one is counted apart. */
function addRequests(out, qs, fromPlaywright) {
  for (const q of qs) {
    if (q.failed) out.aborted++;
    else addResponse(out, q, fromPlaywright);
  }
}

/** What Chromium's DevTools protocol counted of one request in the phase. */
const phaseBytes = (c, inPhase) =>
  Object.entries(c.byPhase ?? {})
    .filter(([p]) => inPhase(p))
    .reduce((sum, [, n]) => sum + n, 0);

function addChromium(out, received, inPhase, common, withCommon) {
  for (const c of received.filter((x) => counted(x.url, common, withCommon))) out.received += phaseBytes(c, inPhase);
}

/**
 * One phase's requests in one run: the responses' count and bytes (decoded,
 * and as a static host compresses them), and the bytes received: on
 * Chromium, the DevTools protocol's count of every byte that arrived in the
 * phase (headers, bodies, streams and aborted requests alike); elsewhere,
 * Playwright's `request.sizes()` of each response. A request that ended
 * without a response (an aborted stream) is counted apart, not as a response.
 */
export function bytesOf(run, phase, common = [], { withCommon = true } = {}) {
  const inPhase = phaseMatcher(phase);
  const out = emptyBytes(run);
  const qs = (run.requests ?? []).filter((q) => inPhase(q.phase) && counted(q.url, common, withCommon));
  addRequests(out, qs, !run.received);
  if (run.received) addChromium(out, run.received, inPhase, common, withCommon);
  out.encodings = [...out.encodings].sort().join("+") || "none";
  return out;
}

// --- runs and their spreads ---------------------------------------------------

const sameCell = (r, { stack, engine, condition }) => r.stack === stack && r.engine === engine && r.condition === condition;

function runsOf(app, cell) {
  const network = cell.network ?? null;
  return app.runs.filter((r) => sameCell(r, cell) && (r.network ?? null) === network);
}

/** The same numbers in every run, or their range. */
function same(values) {
  const s = [...new Set(values)];
  return s.length === 1 ? { value: s[0], varies: false } : { value: Math.max(...s), min: Math.min(...s), varies: true };
}

const missingNote = (s) => (s?.missing ? `not reported (${s.missing} runs)` : "-");

function spread(s, f = ms) {
  if (!s || s.n === 0) return missingNote(s);
  return `${f(s.median)} [${f(s.q1)}-${f(s.q3)}] (${f(s.min)}-${f(s.max)}, n=${s.n}${s.missing ? `, ${s.missing} not reported` : ""})`;
}

/** The servers' measurements as lists of runs (a kept run of before
 * 2026-11-02 holds one run a stack, by stack). */
const asList = (x) => (Array.isArray(x) ? x : Object.entries(x ?? {}).map(([stack, r]) => ({ stack, i: 0, ...r })));

/** Timing summaries for a cell: first contentful paint, the app's first
 * frame, interactive, and each interaction to its frame. */
function timings(app, cell, steps) {
  const rs = runsOf(app, cell);
  const t = (k) => summary(rs.map((r) => r.timings?.[k] ?? null));
  const out = { fcp: t("fcp"), lcp: t("lcp"), cls: t("cls"), appFrame: t("appFrame"), ready: t("ready"), ttfb: t("ttfb"), load: t("load") };
  for (const s of steps) {
    out[`step:${s}`] = summary(rs.map((r) => r.interactions?.find((x) => x.name === s)?.toFrame ?? null));
  }
  return out;
}

/** A difference in time smaller than the engines' timer resolution is no
 * difference: Firefox and WebKit give `performance.now()` to a millisecond. */
const RESOLUTION_MS = 1;

/**
 * A server row's resolution: what its instrument can tell apart, as each run
 * reports it (`load.mjs`), the coarsest of its runs and never under a
 * hundredth of the row's unit: the CPU clock's tick over a run's requests
 * (0.01 ms a request for a thousand) or over an idle window (1 ms a
 * second for ten seconds), one request over that window, a press's polling
 * (10 ms). Two servers' numbers differ only where their spreads are that far
 * apart, as two pages' times differ only a millisecond apart.
 */
function serverResolution(runs, key) {
  return Math.max(0.01, ...runs.map((r) => r.resolution?.[key] ?? 0));
}

/** Layout shift, a score: a difference under a tenth of the score web.dev
 * calls good (0.1) is no difference. */
export const CLS_RESOLUTION = 0.01;

const noRuns = (s) => !s || s.n === 0;

/** Either has no run to judge by. */
const unknown = (a, b) => noRuns(a) || noRuns(b);

/** The first quartile of `x` is at least `resolution` above the third of `y`. */
const apart = (x, y, resolution) => x.q1 - y.q3 >= resolution;

/** Whether `a` (Pleris) is worse than `b`: a median above, with spreads apart. */
export function compareSpread(a, b, resolution = RESOLUTION_MS) {
  if (unknown(a, b)) return "unknown";
  if (apart(a, b, resolution)) return "loses";
  if (apart(b, a, resolution)) return "wins";
  return "within the spread";
}

// --- the rows of where Pleris loses and is best -------------------------------

const STACK_LABEL = { pleris: "Pleris", react: "React", svelte: "Svelte", "next-react": "Next.js", sveltekit: "SvelteKit" };

const byValue = (a, b) => (a[1] > b[1] ? "loses" : a[1] < b[1] ? "wins" : "within the spread");
const byMore = (a, b) => (a[1] < b[1] ? "loses" : a[1] > b[1] ? "wins" : "within the spread");
const bySpread = (a, b) => compareSpread(a[1], b[1]);

/** A row: what is compared, every stack's [label, value, shown], and how
 * Pleris's value is judged against another's (`better`). A stand-in is a
 * stack whose server is the instrument's static host. */
const rowOf = (app, what, values, better, standIns = new Set()) => ({ app, what, values, better, standIns });

const plerisIn = (values) => values.find(([l]) => l === "Pleris");

/** A stand-in (the instrument's static host, serving a client-rendered app's
 * files as any static host or CDN would) is not its stack's cost: a static
 * file needs no application server. Pleris's host doing better than it is no
 * win; doing worse is a loss all the same. */
function verdictAgainst(r, mine, v) {
  const verdict = r.better(mine, v);
  return [v[0], verdict === "wins" && r.standIns.has(v[0]) ? "not compared: a stand-in" : verdict];
}

const rowText = (r) => `${r.app.app ?? r.app}: ${r.what}: ${r.values.map(([l, , shown]) => `${l} ${shown}`).join(" | ")}`;

/** Where a row leaves Pleris: beaten by some stack (a loss), best of all
 * (a win), or neither. */
function classify(r) {
  const mine = plerisIn(r.values);
  const verdicts = r.values.filter(([l]) => l !== "Pleris").map((v) => verdictAgainst(r, mine, v));
  const beaten = verdicts.filter(([, v]) => v === "loses").map(([l]) => l);
  if (beaten.length) return { kind: "loss", text: `${rowText(r)}  (better: ${beaten.join(", ")})` };
  const best = verdicts.length > 0 && verdicts.every(([, v]) => v === "wins");
  return best ? { kind: "win", text: rowText(r) } : { kind: "neither" };
}

const whenPresent = (data, make) => (data ? make(data) : []);

const passed = (counts) => Object.entries(counts).map(([s, v]) => [STACK_LABEL[s], v.passed, `${v.passed}/${v.total}`]);

const behaviourRows = (behaviour) => [
  ...whenPresent(behaviour?.spec, (spec) => [
    rowOf("todomvc", "TodoMVC's specification, tests passed in Chromium, Firefox and WebKit together (more is better)", passed(spec.stacks), byMore),
  ]),
  ...whenPresent(behaviour?.cypress, (cypress) => [rowOf("todomvc", "TodoMVC's official Cypress suite, tests passed (more is better)", passed(cypress), byMore)]),
];

const SOURCE_ROWS = [
  ["app lines", (c) => c.app.lines],
  ["app lines plus the code it needs in its server in another language", (c) => c.app.lines + c.host.lines],
  ["app files", (c) => c.app.files.length],
  ["languages (app, and server code it needs)", (c) => new Set([...Object.keys(c.app.languages), ...Object.keys(c.host.languages)]).size],
  ["build and configuration lines", (c) => c.build.lines],
];

const sourceValues = (stacks, f) => Object.entries(stacks).map(([s, c]) => [STACK_LABEL[s], f(c), String(f(c))]);

const sourceRows = (loc) =>
  Object.entries(loc?.counts ?? {}).flatMap(([app, stacks]) =>
    SOURCE_ROWS.map(([what, f]) => rowOf(app, `source the developer writes: ${what}`, sourceValues(stacks, f), byValue)),
  );

const changeShown = (pr) => `+${pr.added}/-${pr.removed} in ${pr.files} file(s), ${pr.languages.join("+")}`;

/** One of the twelve changes: the lines each stack's pull request changes. */
const changeRow = (r) =>
  rowOf(
    "store",
    `the same change, ${r.id} (${r.title}): lines changed`,
    Object.entries(r.stacks).map(([s, c]) => [STACK_LABEL[s], c.pr.added + c.pr.removed, changeShown(c.pr)]),
    byValue,
  );

const newChangeTotals = () => ({ lines: 0, files: 0, languages: new Set(), refused: 0, n: 0 });

/** The checker's catch: a refusal, or a warning its reference lacks. */
const checkerCaught = (c) => !!c.unsafe && (!c.unsafe.clean || (c.unsafe.newWarnings ?? []).length > 0);

function addChange(totals, stack, c) {
  const t = (totals[stack] ??= newChangeTotals());
  t.lines += c.pr.added + c.pr.removed;
  t.files += c.pr.files;
  c.pr.languages.forEach((l) => t.languages.add(l));
  if (checkerCaught(c)) t.refused++;
  t.n++;
}

function changeTotals(rows) {
  const totals = {};
  for (const r of rows) {
    for (const [stack, c] of Object.entries(r.stacks)) addChange(totals, stack, c);
  }
  return totals;
}

const hasUnsafe = (r) => Object.values(r.stacks).some((c) => c.unsafe);

function totalRows(totals, judged) {
  const v = (f, show) => Object.entries(totals).map(([s, t]) => [STACK_LABEL[s], f(t), show(t)]);
  return [
    rowOf("store", "the twelve changes: lines changed in all", v((t) => t.lines, (t) => String(t.lines)), byValue),
    rowOf("store", "the twelve changes: files touched in all", v((t) => t.files, (t) => String(t.files)), byValue),
    rowOf("store", "the twelve changes: languages touched", v((t) => t.languages.size, (t) => `${t.languages.size} (${[...t.languages].sort().join("+")})`), byValue),
    ...(judged
      ? [rowOf("store", "the twelve changes: unsafe versions the stack's own checker refuses or warns of (more is better)", v((t) => t.refused, (t) => `${t.refused}/${t.n}`), byMore)]
      : []),
  ];
}

const featureRows = (feature) =>
  whenPresent(feature?.rows, (rows) => [...rows.map(changeRow), ...totalRows(changeTotals(rows), rows.some(hasUnsafe))]);

function shownValue(exhibits, stack, n) {
  const shown = exhibits.filter((e) => e.stack === stack && e.shown).length;
  return [STACK_LABEL[stack], shown, `${shown}/${n} (shown in it)`];
}

function museumValues(exhibits) {
  const n = exhibits.length;
  const open = exhibits.filter((e) => !e.refused).length;
  return [["Pleris", open, `${open}/${n}`], ...["next-react", "sveltekit"].map((s) => shownValue(exhibits, s, n))];
}

const museumRows = (museum) =>
  whenPresent(museum?.exhibits, (exhibits) => [
    rowOf("store", "the museum's bug classes the stack lets ship (fewer is better)", museumValues(exhibits), byValue),
  ]);

/** The rows the other sections give: behaviour, source, the feature
 * changes, the museum. Each in the same terms as the measured rows. */
const sectionRows = ({ behaviour, loc, feature, museum }) => [
  ...behaviourRows(behaviour),
  ...sourceRows(loc),
  ...featureRows(feature),
  ...museumRows(museum),
];

const stepNames = (app) => app.runs[0]?.interactions?.map((x) => x.name) ?? [];

const byteValues = (app, f) =>
  app.targets
    .map((t) => [t.label, runsOf(app, { stack: t.stack, engine: "chromium", condition: "cold" })[0], t.common])
    .filter(([, r]) => r)
    .map(([l, r, c]) => {
      const v = f(r, c);
      return [l, v, fmt(v)];
    });

/** The bytes of the first load and of each interaction, cold, in Chromium. */
function byteRows(app) {
  const aside = app.targets.some((t) => t.common?.length) ? ", TodoMVC's common files aside" : "";
  const specs = [
    ["first load, bytes received as each host sent them (Chromium, cold)", (r, c) => bytesOf(r, "load", c).received],
    ["first load, bytes decoded (Chromium, cold)", (r, c) => bytesOf(r, "load", c).raw],
    [`first load, bytes as a static host compresses them, Brotli 11${aside} (Chromium, cold)`, (r, c) => bytesOf(r, "load", c, { withCommon: false }).br],
    ["first load, requests (Chromium, cold)", (r, c) => bytesOf(r, "load", c).requests],
    ...stepNames(app).map((s) => [`interaction "${s}", bytes received (Chromium, cold)`, (r, c) => bytesOf(r, `interaction:${s}`, c).received]),
  ];
  return specs.map(([what, f]) => rowOf(app, what, byteValues(app, f), byValue));
}

/** The engine, condition and network each of an app's runs was made in, once each. */
const blocksOf = (app) => [...new Set(app.runs.map((r) => `${r.engine}|${r.condition}|${r.network ?? ""}`))];

const TIME_WHAT = new Map([
  ["fcp", "first contentful paint"],
  ["lcp", "largest contentful paint"],
  ["ready", "interactive"],
]);

const timeWhat = (k) => TIME_WHAT.get(k) ?? `interaction "${k.slice(5)}" to its frame`;

const timeRow = (app, where, tm, k) =>
  rowOf(
    app,
    `${timeWhat(k)}, ms, median ${where}`,
    Object.entries(tm).map(([l, t]) => [l, t[k], `${ms(t[k].median)} ms`]),
    bySpread,
  );

const shiftRow = (app, where, tm) =>
  rowOf(
    app,
    `cumulative layout shift, median ${where}`,
    Object.entries(tm).map(([l, t]) => [l, t.cls, cls(t.cls.median)]),
    (a, b) => compareSpread(a[1], b[1], CLS_RESOLUTION),
  );

/** The times of one block (an engine, a condition, a network): paints, interactive, each interaction, layout shift. */
function blockRows(app, steps, block) {
  const [engine, condition, network] = block.split("|");
  const tm = Object.fromEntries(
    app.targets.map((t) => [t.label, timings(app, { stack: t.stack, engine, condition, network: network || null }, steps)]),
  );
  const where = `(${engine}, ${condition}${network ? `, ${network}` : ""})`;
  const keys = ["fcp", "lcp", "ready", ...steps.map((s) => `step:${s}`)];
  return [...keys.map((k) => timeRow(app, where, tm, k)), shiftRow(app, where, tm)];
}

const timeRows = (app) => blocksOf(app).flatMap((block) => blockRows(app, stepNames(app), block));

const standInNote = (standIns) =>
  standIns.size
    ? ` (${[...standIns].join("'s and ")}'s server is the instrument's static host, standing in for any static host or CDN: a static file needs no application server, so Pleris's host doing better is no win)`
    : "";

const serverValues = (app, ok, f) =>
  app.targets
    .map((t) => {
      const s = summary(ok.filter((r) => r.stack === t.stack).map(f));
      return [t.label, s, fine(s.median)];
    })
    .filter(([, s]) => s.n > 0);

/** The servers' rows: medians of repeated runs, judged by their spreads a
 * whole unit apart; never a win over a stand-in. [measurement, what, the
 * resolution's key, the number, whether a stand-in's server is judged]. */
const SERVER_ROWS = [
  ["loads", "server CPU per request, ms, median", "cpuMsPerRequest", (l) => l.cpuMsPerRequest],
  ["loads", "server resident memory at its peak under load, MiB, median", "rssMiB", (l) => l.rssMiB.peak],
  ["loads", "latency under load, p50, ms, median", "latencyMs", (l) => l.latencyMs.median],
  ["loads", "latency under load, p99, ms, median", "latencyMs", (l) => l.p99Ms],
  ["idle", "requests an idle visitor's page makes a minute (20 visitors, a browser context each), median", "requestsPerPagePerMinute", (i) => i.requestsPerPagePerMinute],
  ["idle", "server CPU, ms a second, for 20 idle visitors, median", "cpuMsPerSecond", (i) => i.cpuMsPerSecond],
  // A press is the page's and its server's together, what a person waits
  // for: every stack's own, no stand-in.
  ["presses", "a press with 19 other tabs of the app open in one browser, ms to its change (Chromium), median", "pressMs", (p) => p.pressMs, false],
];

function serverRow(app, standIns, [list, what, key, f, standIn = true]) {
  const ok = asList(app[list]).filter((r) => !r.error);
  const resolution = serverResolution(ok, key);
  const judge = (a, b) => compareSpread(a[1], b[1], resolution);
  return rowOf(app, `${what}${standIn ? standInNote(standIns) : ""}`, serverValues(app, ok, f), judge, standIn ? standIns : new Set());
}

function serverRows(app) {
  const standIns = new Set(app.targets.filter((t) => t.standIn).map((t) => t.label));
  return SERVER_ROWS.map((spec) => serverRow(app, standIns, spec));
}

/** An app's rows, in the order they are reported: bytes, times, servers; none where Pleris is not one of its stacks. */
function appRows(app) {
  if (!app.targets.some((t) => t.stack === "pleris")) return [];
  return [...byteRows(app), ...timeRows(app), ...serverRows(app)];
}

/**
 * Where Pleris loses: every row of the tables in which another stack does
 * better than Pleris, bytes by their value and times by their spreads (one
 * row per metric and cell, every stack's value on it). A row Pleris is best
 * in is a win; a row where the spreads overlap is neither and is counted.
 */
export function lossesFirst(result, sections = {}) {
  const rows = [...sectionRows(sections), ...result.apps.flatMap(appRows)];
  const verdicts = rows.filter((r) => plerisIn(r.values)).map(classify);
  return {
    losses: verdicts.filter((v) => v.kind === "loss").map((v) => v.text),
    wins: verdicts.filter((v) => v.kind === "win").map((v) => v.text),
    unclear: verdicts.filter((v) => v.kind === "neither").length,
  };
}

// --- the tables ---------------------------------------------------------------

const rawCell = (raw) => rpad(fmt(raw.value) + (raw.varies ? "*" : ""), 9);
const receivedCell = (received) => rpad(fmt(received.value) + (received.varies ? `~${fmt(received.min)}` : ""), 9);

const cold = (app, t) => runsOf(app, { stack: t.stack, engine: "chromium", condition: "cold" });
const warm = (app, t) => runsOf(app, { stack: t.stack, engine: "chromium", condition: "warm" });

/** An app's lines, one for each stack that has them. */
const byTarget = (app, line) => app.targets.map((t) => line(app, t)).filter(Boolean);

const BYTES_HEADER = [
  "bytes of the first load, cold, Chromium (every cold run of a stack received the same, or its range is shown):",
  "  decoded: the responses' bodies; Brotli 11, gzip 9: those bodies as a static host compresses them, for every stack alike;",
  "  received: what the browser received as each host sent it (headers included); app br: Brotli 11 without TodoMVC's common files (base.js, learn.json)",
  `  ${pad("stack", 10)} ${rpad("requests", 8)} ${rpad("decoded", 9)} ${rpad("Brotli 11", 9)} ${rpad("gzip 9", 8)} ${rpad("received", 9)}  ${pad("as sent", 8)} ${rpad("app br", 14)}  by kind (decoded/br)`,
];

const kindsText = (byKind) => Object.entries(byKind).map(([k, v]) => `${k} ${v.requests}: ${fmt(v.raw)}/${fmt(v.br)}`).join("; ");

const loadAbortedNote = (n) => (n ? `; ${n} request(s) aborted before their response ended (a held stream, or a read the page cut short)` : "");

function loadBytesLine(app, t) {
  const rs = cold(app, t);
  if (!rs.length) return null;
  const b = rs.map((r) => bytesOf(r, "load", t.common));
  const first = b[0];
  const wo = bytesOf(rs[0], "load", t.common, { withCommon: false });
  const cells = [rpad(first.requests, 8), rawCell(same(b.map((x) => x.raw))), rpad(fmt(first.br), 9), rpad(fmt(first.gzip), 8), receivedCell(same(b.map((x) => x.received)))];
  return `  ${pad(t.label, 10)} ${cells.join(" ")}  ${pad(first.encodings, 8)} ${rpad(fmt(wo.br), 14)}  ${kindsText(first.byKind)}${loadAbortedNote(first.aborted)}`;
}

const looseText = (t, q) => `${new URL(q.url).pathname} ${fmt(q.raw)} -> ${fmt(q.minifiedBytes)}${isCommon(q.url, t.common) ? " (TodoMVC's common file)" : ""}`;

function looseLine(app, t) {
  const r = cold(app, t)[0];
  if (!r) return null;
  const shown = (r.requests ?? []).filter((q) => q.minified === false).map((q) => looseText(t, q));
  return `    ${pad(t.label, 10)} ${shown.length ? shown.join("; ") : "none"}`;
}

const urlsText = (r, phase) =>
  (r.requests ?? [])
    .filter((q) => q.phase === phase && !q.failed)
    .map((q) => `${q.method === "GET" ? "" : `${q.method} `}${new URL(q.url).pathname} ${fmt(q.raw)}`);

const abortedPaths = (r, phase) =>
  [...new Set((r.requests ?? []).filter((q) => q.phase === phase && q.failed).map((q) => new URL(q.url).pathname))].join(", ");

const interactionAbortedNote = (b, r, phase) =>
  b.aborted ? `; ${b.aborted} request(s) aborted before their response ended (${abortedPaths(r, phase)})` : "";

function interactionLine(app, t, step) {
  const r = cold(app, t)[0];
  if (!r) return null;
  const phase = `interaction:${step}`;
  const b = bytesOf(r, phase, t.common);
  return `    ${pad(t.label, 10)} ${rpad(b.requests, 3)} ${rpad(fmt(b.raw), 9)} ${rpad(fmt(b.br), 9)} ${rpad(fmt(b.received), 9)}  ${urlsText(r, phase).join(", ")}${interactionAbortedNote(b, r, phase)}`;
}

function warmLine(app, t) {
  const rs = warm(app, t);
  if (!rs.length) return null;
  const b = bytesOf(rs[0], "load", t.common);
  const received = same(rs.map((r) => bytesOf(r, "load", t.common).received));
  return `    ${pad(t.label, 10)} ${rpad(b.requests, 3)} ${rpad(b.notModified, 3)} ${receivedCell(received)}`;
}

function bytesTable(app) {
  return [
    ...BYTES_HEADER,
    ...byTarget(app, loadBytesLine),
    "  scripts and styles a minifier still shrinks by 10% or more (esbuild 0.28.2's, lib.mjs's rule), as served:",
    ...byTarget(app, looseLine),
    ...stepNames(app).flatMap((step) => [
      `  interaction "${step}" (cold, Chromium): requests / decoded / Brotli 11 / received`,
      ...byTarget(app, (a, t) => interactionLine(a, t, step)),
    ]),
    "  warm (the same context after one whole visit; Chromium): requests / answered 304 / received",
    ...byTarget(app, warmLine),
  ];
}

const TIMES_HEADER =
  "times, ms from navigation start: median [first-third quartile] (min-max, n); interactions from the input event to the frame after the change; layout shift a score";

/** The profile as it was applied (the run's own record), never a copy of its numbers. */
const shapedLabel = (profile, network) => (profile && profile.name === network ? profile.describe : "its profile not recorded");

function targetTimeLines(app, steps, [engine, condition, network], t) {
  const tm = timings(app, { stack: t.stack, engine, condition, network: network || null }, steps);
  return [
    `    ${pad(t.label, 10)} first contentful paint ${spread(tm.fcp)}`,
    `    ${pad("", 10)} largest contentful p. ${spread(tm.lcp)}`,
    `    ${pad("", 10)} layout shift           ${spread(tm.cls, cls)}`,
    `    ${pad("", 10)} interactive            ${spread(tm.ready)}`,
    ...steps.map((s) => `    ${pad("", 10)} ${pad(`"${s}"`, 22)} ${spread(tm[`step:${s}`])}`),
  ];
}

function blockLines(app, steps, profile, block) {
  const where = block.split("|");
  const [engine, condition, network] = where;
  const shaped = shapedLabel(profile, network);
  return [
    `  ${engine}, ${condition}${network ? `, ${network} (${shaped})` : ", unshaped (loopback)"}:`,
    ...app.targets.flatMap((t) => targetTimeLines(app, steps, where, t)),
  ];
}

const timingTable = (app, profile) => [
  TIMES_HEADER,
  ...blocksOf(app).flatMap((block) => blockLines(app, stepNames(app), profile, block)),
];

/** A server measurement's runs, a stack's: their summary by `f`, and how many failed. */
function serverRuns(list, stack, f) {
  const mine = asList(list).filter((r) => r.stack === stack);
  return { s: summary(mine.filter((r) => !r.error).map(f)), failed: mine.filter((r) => r.error) };
}

const runsFor = (list, t) => list.filter((l) => l.stack === t.stack);

const failedNote = (mine) => {
  const failed = mine.filter((l) => l.error);
  return failed.length ? `; ${failed.length} run(s) FAILED: ${failed[0].error}` : "";
};

/** A server measurement's table: nothing where it was not made; else its
 * header (from a run that did not fail, and the most runs a stack has) and
 * each stack's lines. */
function serverTable(app, list, header, linesFor) {
  if (!list.length) return [];
  const any = list.find((l) => !l.error) ?? {};
  const n = Math.max(...app.targets.map((t) => runsFor(list, t).length));
  const stacks = app.targets.filter((t) => runsFor(list, t).length > 0);
  return [header(any, n), ...stacks.flatMap((t) => linesFor(t, runsFor(list, t), list))];
}

const loadHeader = (any, n) =>
  `server under a fixed open load: ${any.rate} requests/s for ${any.seconds} s, each of the page's document (after 50 warm-up requests), ${n} runs a stack, every stack once a round in a shuffled order: median [first-third quartile] (min-max, n)`;

/** The statuses the runs of a fixed load were answered with, counted. */
const sumStatuses = (ok) =>
  ok
    .flatMap((l) => Object.entries(l.statuses ?? {}))
    .reduce((counts, [k, c]) => {
      counts[k] = (counts[k] ?? 0) + c;
      return counts;
    }, {});

function loadLines(t, mine, list) {
  const ok = mine.filter((l) => !l.error);
  const sum = (key) => ok.reduce((c, l) => c + l[key], 0);
  return [
    `  ${pad(t.label, 10)} answered ${sum("answered")}/${sum("sent")} ${JSON.stringify(sumStatuses(ok))}; processes ${ok[0]?.processes ?? "-"}${failedNote(mine)}`,
    `  ${pad("", 10)} CPU per request, ms         ${spread(serverRuns(list, t.stack, (l) => l.cpuMsPerRequest).s, fine)}`,
    `  ${pad("", 10)} latency p50, ms             ${spread(serverRuns(list, t.stack, (l) => l.latencyMs.median).s, fine)}`,
    `  ${pad("", 10)} latency p99, ms             ${spread(serverRuns(list, t.stack, (l) => l.p99Ms).s, fine)}`,
    `  ${pad("", 10)} resident memory at peak, MiB ${spread(serverRuns(list, t.stack, (l) => l.rssMiB.peak).s, fine)}`,
  ];
}

const loadTable = (app) => serverTable(app, asList(app.loads), loadHeader, loadLines);

const idleHeader = (any, n) =>
  `idle visitors: ${any.pages} pages of the app in Chromium, each in a browser context of its own (as separate people's browsers), loaded and left alone for ${any.seconds} s; ${n} runs a stack: median [first-third quartile] (min-max, n)`;

const idleLines = (t, mine, list) => [
  `  ${pad(t.label, 10)} requests a page a minute    ${spread(serverRuns(list, t.stack, (i) => i.requestsPerPagePerMinute).s, fine)}${failedNote(mine)}`,
  `  ${pad("", 10)} server CPU, ms a second     ${spread(serverRuns(list, t.stack, (i) => i.cpuMsPerSecond).s, fine)}`,
  `  ${pad("", 10)} server resident memory, MiB ${spread(serverRuns(list, t.stack, (i) => i.rssMiB).s, fine)}`,
];

const idleTable = (app) => serverTable(app, asList(app.idle), idleHeader, idleLines);

const pressHeader = (any, n) =>
  `a press with other tabs open: ${any.tabs} tabs of the app in ONE browser context (one person's browser, whose six connections to a host over HTTP/1.1 they share), left ${any.settleSeconds} s, then the app's first interaction in the last tab, ms to its change; ${n} runs a stack: median [first-third quartile] (min-max, n)`;

const pressLines = (t, mine, list) => [
  `  ${pad(t.label, 10)} ${spread(serverRuns(list, t.stack, (i) => i.pressMs).s)}${failedNote(mine)}`,
];

const pressTable = (app) => serverTable(app, asList(app.presses), pressHeader, pressLines);

// --- the report -----------------------------------------------------------------

const browsersOf = (result) => result.apps.map((a) => Object.entries(a.browsers).map(([k, v]) => `${k} ${v}`).join(", "))[0] ?? "-";

const settledLine = (meta) =>
  `settled before measuring: one-minute load ${meta.settled.before} -> ${meta.settled.after} in ${meta.settled.waitedSeconds} s (waits at most ${meta.args?.settleSeconds} s for under ${meta.settled.goal})`;

const shapedNote = (meta) =>
  meta.network
    ? `${meta.network.describe} (latency ${meta.network.latency} ms, ${meta.network.download} B/s down, ${meta.network.upload} B/s up, as sent)`
    : meta.args.shaped
      ? "Slow 4G, its profile not recorded"
      : "no";

const serversNote = (meta) => (meta.args.load === false ? "not measured" : `${meta.args.serverRuns ?? 1} runs a stack`);

const runsLine = (meta) =>
  `runs: ${meta.args.runs} per cell (at least ${meta.minRuns}); conditions ${meta.args.conditions.join(", ")}; engines ${meta.args.engines.join(", ")}; Chromium also shaped: ${shapedNote(meta)}; servers: ${serversNote(meta)}; seed ${meta.args.seed}`;

function machineLines(result) {
  const { meta } = result;
  const m = meta.machine;
  return [
    `machine: ${m.cpu}, ${m.logicalCpus} logical CPUs, ${m.memoryGiB} GiB; ${m.os}; kernel ${m.kernel}; node ${m.node}`,
    `load average: ${m.loadAverage.join(" ")} at the start, ${meta.loadAverageAfter?.join(" ")} at the end`,
    ...(meta.settled ? [settledLine(meta)] : []),
    `playwright: ${meta.playwright}; browsers: ${browsersOf(result)}`,
    runsLine(meta),
    `commit: ${meta.commit}${meta.dirty ? ` + ${meta.dirty}` : ""}; ${meta.started} to ${meta.finished}`,
  ];
}

function verdictLines(result, sections) {
  const { losses, wins, unclear } = lossesFirst(result, sections);
  return [
    `== where Pleris loses: ${losses.length} row(s) of the tables below in which another stack does better`,
    ...losses.map((l) => `  LOSS ${l}`),
    ...(losses.length ? [] : ["  none in these tables"]),
    "",
    `== where Pleris is best: ${wins.length} row(s); ${unclear} more where it is neither (spreads overlap, or it is between the others)`,
    ...wins.map((w) => `  BEST ${w}`),
    "",
  ];
}

const sectionLines = (sections) =>
  Object.entries(sections.texts ?? {}).flatMap(([name, text]) => [`== ${name}`, text.trimEnd(), ""]);

const READY_WHEN = new Map([
  ["dom", "its field is in the document (rendered by the client, handlers attached as it renders)"],
  ["pw", "the runtime sets data-pw-ready"],
  ["next", "the hydrated App Router writes its state to history.state"],
]);

const readyWhen = (ready) => READY_WHEN.get(ready) ?? "SvelteKit's router starts after hydrating (history.scrollRestoration set)";

const targetLine = (t) =>
  `  ${pad(t.label, 10)} ${t.version}; written by ${t.who}; served by ${t.server.cmd} ${t.server.args.join(" ")}; interactive when ${readyWhen(t.ready)}; build minifies: ${t.expect.minifies ? "yes" : "NO"}; host compresses: ${t.expect.compresses || "NO"}; host lets a cache keep its files: ${t.expect.caches ? "yes" : "NO"}`;

const appLines = (app, meta) => [
  `== ${app.title}`,
  ...app.targets.map(targetLine),
  "",
  ...bytesTable(app),
  "",
  ...timingTable(app, meta.network),
  "",
  ...loadTable(app),
  "",
  ...idleTable(app),
  "",
  ...pressTable(app),
  "",
];

export function renderReport(result, sections = {}) {
  const lines = [
    ...machineLines(result),
    "",
    ...verdictLines(result, sections),
    ...sectionLines(sections),
    ...result.apps.flatMap((app) => appLines(app, result.meta)),
  ];
  return `${lines.join("\n")}\n`;
}
