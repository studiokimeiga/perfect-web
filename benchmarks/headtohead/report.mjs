// The head-to-head's report (ADR-XXXX): every table read from the runs, and
// where Pleris loses, first, in the same tables' terms.
//
// Bytes are the same in every cold run of a stack (a build is a build), and
// the report says so or shows the spread; they are read from Chromium's runs
// (ADR-0342 §4's instrument), and the bytes each engine received are shown
// beside. Times are medians with their quartiles and extremes, per engine,
// condition and network. A difference in time is reported as a loss or a win
// only where the two spreads are apart by at least the engines' timer
// resolution, a millisecond (one's first quartile that much above the other's
// third); otherwise it is "within the spread".

import { summary } from "./lib.mjs";

const fmt = (n) => (n == null ? "-" : Math.round(n).toLocaleString("en-US"));
const ms = (n) => (n == null ? "-" : n < 10 ? n.toFixed(1) : Math.round(n).toString());
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

/**
 * One phase's requests in one run: the responses' count and bytes (decoded,
 * and as a static host compresses them), and the bytes received: on
 * Chromium, the DevTools protocol's count of every byte that arrived in the
 * phase (headers, bodies, streams and aborted requests alike); elsewhere,
 * Playwright's `request.sizes()` of each response. A request that ended
 * without a response (an aborted stream) is counted apart, not as a response.
 */
export function bytesOf(run, phase, common = [], { withCommon = true } = {}) {
  const inPhase = (p) => (phase === "interactions" ? p.startsWith("interaction") : p === phase);
  const qs = (run.requests ?? []).filter((q) => inPhase(q.phase));
  const out = { requests: 0, aborted: 0, raw: 0, br: 0, gzip: 0, received: 0, byKind: {}, encodings: new Set(), notModified: 0, source: run.received ? "chromium" : "playwright" };
  for (const q of qs) {
    if (!withCommon && isCommon(q.url, common)) continue;
    if (q.failed) {
      out.aborted++;
      continue;
    }
    out.requests++;
    if (q.status === 304) out.notModified++;
    out.raw += q.raw ?? 0;
    out.br += q.br ?? 0;
    out.gzip += q.gzip ?? 0;
    if (!run.received) out.received += (q.receivedBody ?? 0) + (q.receivedHeaders ?? 0);
    if (q.encoding) out.encodings.add(q.encoding);
    const k = kindOf(q);
    out.byKind[k] = out.byKind[k] ?? { requests: 0, raw: 0, br: 0 };
    out.byKind[k].requests++;
    out.byKind[k].raw += q.raw ?? 0;
    out.byKind[k].br += q.br ?? 0;
  }
  if (run.received) {
    for (const c of run.received) {
      if (!withCommon && isCommon(c.url, common)) continue;
      for (const [p, n] of Object.entries(c.byPhase ?? {})) if (inPhase(p)) out.received += n;
    }
  }
  out.encodings = [...out.encodings].sort().join("+") || "none";
  return out;
}

function runsOf(app, { stack, engine, condition, network = null }) {
  return app.runs.filter(
    (r) => r.stack === stack && r.engine === engine && r.condition === condition && (r.network ?? null) === network,
  );
}

/** The same numbers in every run, or their range. */
function same(values) {
  const s = [...new Set(values)];
  return s.length === 1 ? { value: s[0], varies: false } : { value: Math.max(...s), min: Math.min(...s), varies: true };
}

function spread(s) {
  if (!s || s.n === 0) return "-";
  return `${ms(s.median)} [${ms(s.q1)}-${ms(s.q3)}] (${ms(s.min)}-${ms(s.max)}, n=${s.n}${s.missing ? `, ${s.missing} not reported` : ""})`;
}

/** Timing summaries for a cell: first contentful paint, the app's first
 * frame, interactive, and each interaction to its frame. */
export function timings(app, cell, steps) {
  const rs = runsOf(app, cell);
  const t = (k) => summary(rs.map((r) => r.timings?.[k] ?? null));
  const out = { fcp: t("fcp"), appFrame: t("appFrame"), ready: t("ready"), ttfb: t("ttfb"), load: t("load") };
  for (const s of steps) {
    out[`step:${s}`] = summary(rs.map((r) => r.interactions?.find((x) => x.name === s)?.toFrame ?? null));
  }
  return out;
}

/** Whether `a` (Pleris) is worse than `b`: a median above, with spreads apart. */
/** A difference in time smaller than the engines' timer resolution is no
 * difference: Firefox and WebKit give `performance.now()` to a millisecond. */
export const RESOLUTION_MS = 1;

export function compareSpread(a, b) {
  if (!a || !b || a.n === 0 || b.n === 0) return "unknown";
  if (a.q1 - b.q3 >= RESOLUTION_MS) return "loses";
  if (b.q1 - a.q3 >= RESOLUTION_MS) return "wins";
  return "within the spread";
}

/**
 * Where Pleris loses: every row of the tables in which another stack does
 * better than Pleris, bytes by their value and times by their spreads (one
 * row per metric and cell, every stack's value on it). A row Pleris is best
 * in is a win; a row where the spreads overlap is neither and is counted.
 */
export function lossesFirst(result, sections = {}) {
  const losses = [];
  const wins = [];
  let unclear = 0;
  const row = (app, what, values, better) => {
    // values: [[label, value, summary?]]; better(a, b) -> "loses" | "wins" | "within the spread"
    const mine = values.find(([l]) => l === "Pleris");
    if (!mine) return;
    const verdicts = values.filter(([l]) => l !== "Pleris").map((v) => [v[0], better(mine, v)]);
    const text = `${app.app ?? app}: ${what}: ${values.map(([l, , shown]) => `${l} ${shown}`).join(" | ")}`;
    const beaten = verdicts.filter(([, v]) => v === "loses").map(([l]) => l);
    if (beaten.length) losses.push(`${text}  (better: ${beaten.join(", ")})`);
    else if (verdicts.length && verdicts.every(([, v]) => v === "wins")) wins.push(text);
    else unclear++;
  };
  const byValue = (a, b) => (a[1] > b[1] ? "loses" : a[1] < b[1] ? "wins" : "within the spread");
  const byMore = (a, b) => (a[1] < b[1] ? "loses" : a[1] > b[1] ? "wins" : "within the spread");
  const bySpread = (a, b) => compareSpread(a[1], b[1]);
  sectionRows(sections, row, byValue, byMore);
  for (const app of result.apps) {
    if (!app.targets.some((t) => t.stack === "pleris")) continue;
    const steps = app.runs[0]?.interactions?.map((x) => x.name) ?? [];
    const coldChromium = (stack) => runsOf(app, { stack, engine: "chromium", condition: "cold" })[0];
    const byteRows = [
      ["first load, bytes received as each host sent them (Chromium, cold)", (r, c) => bytesOf(r, "load", c).received],
      ["first load, bytes decoded (Chromium, cold)", (r, c) => bytesOf(r, "load", c).raw],
      ["first load, bytes as a static host compresses them, Brotli 11, TodoMVC's common files aside (Chromium, cold)", (r, c) => bytesOf(r, "load", c, { withCommon: false }).br],
      ["first load, requests (Chromium, cold)", (r, c) => bytesOf(r, "load", c).requests],
      ...steps.map((s) => [`interaction "${s}", bytes received (Chromium, cold)`, (r, c) => bytesOf(r, `interaction:${s}`, c).received]),
    ];
    for (const [what, f] of byteRows) {
      const values = app.targets
        .map((t) => [t.label, coldChromium(t.stack), t.common])
        .filter(([, r]) => r)
        .map(([l, r, c]) => {
          const v = f(r, c);
          return [l, v, fmt(v)];
        });
      row(app, what, values, byValue);
    }
    const blocks = [...new Set(app.runs.map((r) => `${r.engine}|${r.condition}|${r.network ?? ""}`))];
    for (const b of blocks) {
      const [engine, condition, network] = b.split("|");
      const tm = Object.fromEntries(
        app.targets.map((t) => [t.label, timings(app, { stack: t.stack, engine, condition, network: network || null }, steps)]),
      );
      for (const k of ["fcp", "ready", ...steps.map((s) => `step:${s}`)]) {
        const what = k === "fcp" ? "first contentful paint" : k === "ready" ? "interactive" : `interaction "${k.slice(5)}" to its frame`;
        const values = Object.entries(tm).map(([l, t]) => [l, t[k], `${ms(t[k].median)} ms`]);
        row(app, `${what}, ms, median (${engine}, ${condition}${network ? `, ${network}` : ""})`, values, bySpread);
      }
    }
    if (app.loads && Object.keys(app.loads).length) {
      for (const [what, f] of [
        ["server CPU per request, ms", (l) => l.cpuMsPerRequest],
        ["server resident memory at its peak, MiB", (l) => l.rssMiB.peak],
        ["latency under load, median, ms", (l) => l.latencyMs.median],
        ["latency under load, p99, ms", (l) => l.p99Ms],
      ]) {
        const values = app.targets.filter((t) => app.loads[t.stack] && !app.loads[t.stack].error).map((t) => {
          const v = f(app.loads[t.stack]);
          return [t.label, v, ms(v)];
        });
        row(app, `${what}${app.app === "todomvc" ? " (React's and Svelte's server is the instrument's static host: any static host serves them)" : ""}`, values, byValue);
      }
    }
    if (app.idle && Object.keys(app.idle).length) {
      for (const [what, f] of [
        ["requests an idle open page makes a minute", (i) => i.requestsPerPagePerMinute],
        ["server CPU, ms a second, for 20 idle open pages", (i) => i.cpuMsPerSecond],
        ["a press with 19 other pages of the app open, ms to its change (Chromium)", (i) => i.pressMs],
      ]) {
        const values = app.targets.filter((t) => app.idle[t.stack] && !app.idle[t.stack].error).map((t) => {
          const v = f(app.idle[t.stack]);
          return [t.label, v, ms(v)];
        });
        row(app, what, values, byValue);
      }
    }
  }
  return { losses, wins, unclear };
}

const STACK_LABEL = { pleris: "Pleris", react: "React", svelte: "Svelte", "next-react": "Next.js", sveltekit: "SvelteKit" };

/** The rows the other sections give: behaviour, source, the feature
 * changes, the museum. Each in the same terms as the measured rows. */
function sectionRows({ behaviour, loc, feature, museum }, row, byValue, byMore) {
  if (behaviour?.spec) {
    const values = Object.entries(behaviour.spec.stacks).map(([s, v]) => [STACK_LABEL[s], v.passed, `${v.passed}/${v.total}`]);
    row("todomvc", "TodoMVC's specification, tests passed in Chromium, Firefox and WebKit together (more is better)", values, byMore);
  }
  if (behaviour?.cypress) {
    const values = Object.entries(behaviour.cypress).map(([s, v]) => [STACK_LABEL[s], v.passed, `${v.passed}/${v.total}`]);
    row("todomvc", "TodoMVC's official Cypress suite, tests passed (more is better)", values, byMore);
  }
  for (const [app, stacks] of Object.entries(loc?.counts ?? {})) {
    const v = (f) => Object.entries(stacks).map(([s, c]) => [STACK_LABEL[s], f(c), String(f(c))]);
    row(app, "source the developer writes: app lines", v((c) => c.app.lines), byValue);
    row(app, "source the developer writes: app lines plus the code it needs in its server in another language", v((c) => c.app.lines + c.host.lines), byValue);
    row(app, "source the developer writes: app files", v((c) => c.app.files.length), byValue);
    row(app, "source the developer writes: languages (app, and server code it needs)", v((c) => new Set([...Object.keys(c.app.languages), ...Object.keys(c.host.languages)]).size), byValue);
    row(app, "source the developer writes: build and configuration lines", v((c) => c.build.lines), byValue);
  }
  if (feature?.rows) {
    const totals = {};
    for (const r of feature.rows) {
      const values = Object.entries(r.stacks).map(([s, c]) => [STACK_LABEL[s], c.pr.added + c.pr.removed, `+${c.pr.added}/-${c.pr.removed} in ${c.pr.files} file(s), ${c.pr.languages.join("+")}`]);
      row("store", `the same change, ${r.id} (${r.title}): lines changed`, values, byValue);
      for (const [s, c] of Object.entries(r.stacks)) {
        const t = (totals[s] ??= { lines: 0, files: 0, languages: new Set(), refused: 0, n: 0 });
        t.lines += c.pr.added + c.pr.removed;
        t.files += c.pr.files;
        c.pr.languages.forEach((l) => t.languages.add(l));
        if (c.unsafe && !c.unsafe.clean) t.refused++;
        t.n++;
      }
    }
    const v = (f, show) => Object.entries(totals).map(([s, t]) => [STACK_LABEL[s], f(t), show(t)]);
    row("store", "the twelve changes: lines changed in all", v((t) => t.lines, (t) => String(t.lines)), byValue);
    row("store", "the twelve changes: files touched in all", v((t) => t.files, (t) => String(t.files)), byValue);
    row("store", "the twelve changes: languages touched", v((t) => t.languages.size, (t) => `${t.languages.size} (${[...t.languages].sort().join("+")})`), byValue);
    if (feature.rows.some((r) => Object.values(r.stacks).some((c) => c.unsafe))) {
      row("store", "the twelve changes: unsafe versions the stack's own checker refuses (more is better)", v((t) => t.refused, (t) => `${t.refused}/${t.n}`), byMore);
    }
  }
  if (museum?.exhibits) {
    const n = museum.exhibits.length;
    const values = [
      ["Pleris", museum.exhibits.filter((e) => !e.refused).length, `${museum.exhibits.filter((e) => !e.refused).length}/${n}`],
      ...["next-react", "sveltekit"].map((s) => {
        const shown = museum.exhibits.filter((e) => e.stack === s && e.shown).length;
        return [STACK_LABEL[s], shown, `${shown}/${n} (shown in it)`];
      }),
    ];
    row("store", "the museum's bug classes the stack lets ship (fewer is better)", values, byValue);
  }
}

function bytesTable(app, lines) {
  const steps = app.runs[0]?.interactions?.map((x) => x.name) ?? [];
  lines.push("bytes of the first load, cold, Chromium (every cold run of a stack received the same, or its range is shown):");
  lines.push("  decoded: the responses' bodies; Brotli 11, gzip 9: those bodies as a static host compresses them, for every stack alike;");
  lines.push("  received: what the browser received as each host sent it (headers included); app br: Brotli 11 without TodoMVC's common files (base.js, learn.json)");
  lines.push(
    `  ${pad("stack", 10)} ${rpad("requests", 8)} ${rpad("decoded", 9)} ${rpad("Brotli 11", 9)} ${rpad("gzip 9", 8)} ${rpad("received", 9)}  ${pad("as sent", 8)} ${rpad("app br", 14)}  by kind (decoded/br)`,
  );
  for (const t of app.targets) {
    const rs = runsOf(app, { stack: t.stack, engine: "chromium", condition: "cold" });
    if (!rs.length) continue;
    const b = rs.map((r) => bytesOf(r, "load", t.common));
    const received = same(b.map((x) => x.received));
    const raw = same(b.map((x) => x.raw));
    const first = b[0];
    const wo = bytesOf(rs[0], "load", t.common, { withCommon: false });
    const kinds = Object.entries(first.byKind).map(([k, v]) => `${k} ${v.requests}: ${fmt(v.raw)}/${fmt(v.br)}`).join("; ");
    lines.push(
      `  ${pad(t.label, 10)} ${rpad(first.requests, 8)} ${rpad(fmt(raw.value) + (raw.varies ? "*" : ""), 9)} ${rpad(fmt(first.br), 9)} ${rpad(fmt(first.gzip), 8)} ${rpad(fmt(received.value) + (received.varies ? `~${fmt(received.min)}` : ""), 9)}  ${pad(first.encodings, 8)} ${rpad(fmt(wo.br), 14)}  ${kinds}${first.aborted ? `; ${first.aborted} request(s) aborted before their response ended (a held stream, or a read the page cut short)` : ""}`,
    );
  }
  lines.push("  scripts and styles a minifier still shrinks by 10% or more (esbuild 0.28.2's, lib.mjs's rule), as served:");
  for (const t of app.targets) {
    const r = runsOf(app, { stack: t.stack, engine: "chromium", condition: "cold" })[0];
    if (!r) continue;
    const loose = (r.requests ?? []).filter((q) => q.minified === false);
    const shown = loose.map((q) => `${new URL(q.url).pathname} ${fmt(q.raw)} -> ${fmt(q.minifiedBytes)}${isCommon(q.url, t.common) ? " (TodoMVC's common file)" : ""}`);
    lines.push(`    ${pad(t.label, 10)} ${shown.length ? shown.join("; ") : "none"}`);
  }
  for (const s of steps) {
    lines.push(`  interaction "${s}" (cold, Chromium): requests / decoded / Brotli 11 / received`);
    for (const t of app.targets) {
      const r = runsOf(app, { stack: t.stack, engine: "chromium", condition: "cold" })[0];
      if (!r) continue;
      const b = bytesOf(r, `interaction:${s}`, t.common);
      const urls = (r.requests ?? []).filter((q) => q.phase === `interaction:${s}` && !q.failed).map((q) => `${q.method === "GET" ? "" : `${q.method} `}${new URL(q.url).pathname} ${fmt(q.raw)}`);
      const aborted = b.aborted ? `; ${b.aborted} request(s) aborted before their response ended (${[...new Set((r.requests ?? []).filter((q) => q.phase === `interaction:${s}` && q.failed).map((q) => new URL(q.url).pathname))].join(", ")})` : "";
      lines.push(`    ${pad(t.label, 10)} ${rpad(b.requests, 3)} ${rpad(fmt(b.raw), 9)} ${rpad(fmt(b.br), 9)} ${rpad(fmt(b.received), 9)}  ${urls.join(", ")}${aborted}`);
    }
  }
  lines.push("  warm (the same context after one whole visit; Chromium): requests / answered 304 / received");
  for (const t of app.targets) {
    const rs = runsOf(app, { stack: t.stack, engine: "chromium", condition: "warm" });
    if (!rs.length) continue;
    const b = bytesOf(rs[0], "load", t.common);
    const received = same(rs.map((r) => bytesOf(r, "load", t.common).received));
    lines.push(`    ${pad(t.label, 10)} ${rpad(b.requests, 3)} ${rpad(b.notModified, 3)} ${rpad(fmt(received.value) + (received.varies ? `~${fmt(received.min)}` : ""), 9)}`);
  }
}

function timingTable(app, lines) {
  const steps = app.runs[0]?.interactions?.map((x) => x.name) ?? [];
  const blocks = [...new Set(app.runs.map((r) => `${r.engine}|${r.condition}|${r.network ?? ""}`))];
  lines.push("times, ms from navigation start: median [first-third quartile] (min-max, n); interactions from the input event to the frame after the change");
  for (const b of blocks) {
    const [engine, condition, network] = b.split("|");
    lines.push(`  ${engine}, ${condition}${network ? `, ${network} (latency ${150} ms, 1.6 Mbit/s down, CPU x4)` : ", unshaped (loopback)"}:`);
    for (const t of app.targets) {
      const tm = timings(app, { stack: t.stack, engine, condition, network: network || null }, steps);
      lines.push(`    ${pad(t.label, 10)} first contentful paint ${spread(tm.fcp)}`);
      lines.push(`    ${pad("", 10)} interactive            ${spread(tm.ready)}`);
      for (const s of steps) lines.push(`    ${pad("", 10)} ${pad(`"${s}"`, 22)} ${spread(tm[`step:${s}`])}`);
    }
  }
}

function idleTable(app, lines) {
  if (!app.idle || !Object.keys(app.idle).length) return;
  const any = Object.values(app.idle).find((i) => !i.error) ?? {};
  lines.push(`open pages left alone: ${any.pages} pages of the app in Chromium, each loaded and untouched, for ${any.seconds} s`);
  lines.push(`  ${pad("stack", 10)} ${rpad("requests", 8)} ${rpad("per page a minute", 17)} ${rpad("server CPU ms/s", 15)} ${rpad("server RSS MiB", 14)}  then a press in the last page, to its change (ms)`);
  for (const t of app.targets) {
    const i = app.idle[t.stack];
    if (!i) continue;
    if (i.error) {
      lines.push(`  ${pad(t.label, 10)} FAILED: ${i.error}`);
      continue;
    }
    lines.push(`  ${pad(t.label, 10)} ${rpad(i.requests, 8)} ${rpad(i.requestsPerPagePerMinute.toFixed(1), 17)} ${rpad(ms(i.cpuMsPerSecond), 15)} ${rpad(i.rssMiB.toFixed(1), 14)}  ${ms(i.pressMs)}`);
  }
}

function loadTable(app, lines) {
  if (!app.loads || !Object.keys(app.loads).length) return;
  const any = Object.values(app.loads).find((l) => !l.error) ?? {};
  lines.push(`server under a fixed open load: ${any.rate} requests/s for ${any.seconds} s, each request of the page's document (after ${50} warm-up requests)`);
  lines.push(`  ${pad("stack", 10)} ${rpad("answered", 9)} ${rpad("CPU ms/req", 10)} ${rpad("p50 ms", 7)} ${rpad("p99 ms", 7)} ${rpad("RSS MiB before/peak/after", 26)} processes  statuses`);
  for (const t of app.targets) {
    const l = app.loads[t.stack];
    if (!l) continue;
    if (l.error) {
      lines.push(`  ${pad(t.label, 10)} FAILED: ${l.error}`);
      continue;
    }
    lines.push(
      `  ${pad(t.label, 10)} ${rpad(`${l.answered}/${l.sent}`, 9)} ${rpad(ms(l.cpuMsPerRequest), 10)} ${rpad(ms(l.latencyMs.median), 7)} ${rpad(ms(l.p99Ms), 7)} ${rpad(`${l.rssMiB.before.toFixed(1)}/${l.rssMiB.peak.toFixed(1)}/${l.rssMiB.after.toFixed(1)}`, 26)} ${rpad(l.processes, 9)}  ${JSON.stringify(l.statuses)}`,
    );
  }
}

export function renderReport(result, sections = {}) {
  const { meta } = result;
  const lines = [];
  const m = meta.machine;
  lines.push(`machine: ${m.cpu}, ${m.logicalCpus} logical CPUs, ${m.memoryGiB} GiB; ${m.os}; kernel ${m.kernel}; node ${m.node}`);
  lines.push(`load average: ${m.loadAverage.join(" ")} at the start, ${meta.loadAverageAfter?.join(" ")} at the end`);
  if (meta.settled) {
    lines.push(`settled before measuring: one-minute load ${meta.settled.before} -> ${meta.settled.after} in ${meta.settled.waitedSeconds} s (waits at most ${meta.args?.settleSeconds} s for under ${meta.settled.goal})`);
  }
  lines.push(`playwright: ${meta.playwright}; browsers: ${result.apps.map((a) => Object.entries(a.browsers).map(([k, v]) => `${k} ${v}`).join(", "))[0] ?? "-"}`);
  lines.push(`runs: ${meta.args.runs} per cell (at least ${meta.minRuns}); conditions ${meta.args.conditions.join(", ")}; engines ${meta.args.engines.join(", ")}; Chromium also shaped: ${meta.args.shaped ? "Slow 4G" : "no"}; seed ${meta.args.seed}`);
  lines.push(`commit: ${meta.commit}${meta.dirty ? ` + ${meta.dirty}` : ""}; ${meta.started} to ${meta.finished}`);
  lines.push("");
  const { losses, wins, unclear } = lossesFirst(result, sections);
  lines.push(`== where Pleris loses: ${losses.length} row(s) of the tables below in which another stack does better`);
  for (const l of losses) lines.push(`  LOSS ${l}`);
  if (!losses.length) lines.push("  none in these tables");
  lines.push("");
  lines.push(`== where Pleris is best: ${wins.length} row(s); ${unclear} more where it is neither (spreads overlap, or it is between the others)`);
  for (const w of wins) lines.push(`  BEST ${w}`);
  lines.push("");
  for (const [name, text] of Object.entries(sections.texts ?? {})) {
    lines.push(`== ${name}`);
    lines.push(text.trimEnd());
    lines.push("");
  }
  for (const app of result.apps) {
    lines.push(`== ${app.title}`);
    for (const t of app.targets) {
      lines.push(`  ${pad(t.label, 10)} ${t.version}; written by ${t.who}; served by ${t.server.cmd} ${t.server.args.join(" ")}; interactive when ${t.ready === "dom" ? "its field is in the document (rendered by the client, handlers attached as it renders)" : t.ready === "pw" ? "the runtime sets data-pw-ready" : t.ready === "next" ? "the hydrated App Router writes its state to history.state" : "SvelteKit's router starts after hydrating (history.scrollRestoration set)"}; build minifies: ${t.expect.minifies ? "yes" : "NO"}; host compresses: ${t.expect.compresses || "NO"}; host lets a cache keep its files: ${t.expect.caches ? "yes" : "NO"}`);
    }
    lines.push("");
    bytesTable(app, lines);
    lines.push("");
    timingTable(app, lines);
    lines.push("");
    loadTable(app, lines);
    lines.push("");
    idleTable(app, lines);
    lines.push("");
  }
  return `${lines.join("\n")}\n`;
}
