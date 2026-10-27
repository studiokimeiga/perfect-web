# STATUS

<!-- Charter §3.4 requires exactly these sections. Keep them. -->


Local continuation 2026-10-26: `wip/session-revocation-20261026` is an
unmerged implementation branch; its immutable-reader slice and remaining gates
are recorded in [the scoped continuation record](diagnostics/session-readers-2026-10-26.md).
The master worktree was observed at `a13b3c3806b928d4a7e135819d58ff5f39c2a6a6`,
matching fetched origin/master. The dated integration ledger below is historical;
no new hosted-CI or complete-session acceptance is asserted by this local work.

Local diagnostic continuation 2026-10-27: the complete check audit also found a
push-CI PID-publication fixture race and a failing aggregate CodeQL security gate.
Repairs and raw regressions are in [the command continuation record](diagnostics/session-commands-2026-10-27.md).
Focused reproduction: `python3 -m unittest -v scripts/tests/test_producer_ownership.py`;
`cargo test --locked -p pw-dev-server --bin pw-dev-server -- identity::invocation_tests:: --test-threads=1`;
`cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::session_commands:: --test-threads=1`.
Final gates remain `just preflight`, `just e14-session-command-authority`,
`just e14-scheduled-replay` and the planned hosted CI/browser/recipe checks at one
new immutable head. No earlier head's result authorizes its merge.

Reviewed 2026-10-21 against master `e7664d2eb01aa8c6e91df594bd7270a84a8bf8de`,
train-12's landing, and its completed hosted runs. The
[snapshot at that landing](STATUS-HISTORY-2026-10-21.md) and
[the prior ledger](STATUS-HISTORY-2026-10.md) remain verbatim, including
failed runs, corrections and their original scope.

## current milestone

**Integration checkpoint2026-10-26:** master `0035f923` contains PR54's
image-privacy repair and PR59's unused Wasmtime disk-cache dependency removal,
both landed by ordinary fast-forward to the exact tested heads after independent
reviews and all published gates. The prior current Zstandard advisory blocker
is repaired without an audit waiver. Post-merge reruns are separate from those
passing pre-merge gates.

PR55's derived-purity producer repair is still awaiting its final integration
head gates, and PR56 remains a design document rather than a dating app.
Outbound HTTPS and staged D2 table writes are queued foundations; generic
server routing/principal policies, unified business/platform transactions,
migrations and private-media/session authority remain unfinished.

PR59's canonical exact-source proof at `109f459e` is
[verify40341658053](https://github.com/studiokimeiga/perfect-web/actions/runs/40341658053):
`just e8-host` and `just e14-compiled-once`, fetched with
`just evidence-fetch 40341658053`. Receipts are
[artifact-audit](evidence/E8/artifact-audit.txt) and
[compiled-once](evidence/E14/compiled-once.txt). Host audit105passed/0ignored;
server suite771passed/9ignored and1.71GiB peak; ignored cases get no credit.
The2compile-once controls remain legacy producer evidence. Older ledger
statements below retain the scope/date of their review.

**E14, the AI benchmark**, ahead of E11-E13 under the owner's 2026-10-02
ruling ([ADR-0119](DECISIONS/ADR-0119-e10-closes-and-the-ai-benchmark-comes-next.md),
[E14 plan](milestones/E14.md)). The offline harness and reference tasks exist.
Agent runs still require the owner's choices of models, budget and how
Pleris is taught to an agent. Compiler and browser tests establish no agent
benchmark. Numbering remains engineering E0-E15, public proofs P0-P9 and
risk experiments RQ-* ([charter](../PROJECT_CHARTER.md)).

**Train-12 landed on master on 2026-10-21** at `e7664d2e`: generic-result
and effect repairs, device-local data and TodoMVC, cross-host database
checks, verification and producer repairs, security pins, component
namespaces, keyed-read retry and effect convergence (ADR-0351, ADR-0365 to
ADR-0380). The [acceptance record](diagnostics/train-12/acceptance-record-2026-10-21.md)
states what its evidence proves and what it does not.

**Train-13, verified, not landed** (`track/train-13`): `command-deadline`,
`press-modules` and `e14-readiness`, merged at `c57e5570`, and the benchmark's
Next.js baseline moved from 16.3.8 to 16.4.1 for three advisories (ADR-0120,
amended). At `5a4378fe`, CI [39892760379](https://github.com/studiokimeiga/perfect-web/actions/runs/39892760379) and verify [39892771765](https://github.com/studiokimeiga/perfect-web/actions/runs/39892771765)
passed. Its [acceptance record](diagnostics/train-13/acceptance-record-2026-10-22.md)
holds each track's runs, the failed ones among them. The owner approved
landing it once exact-commit CI and the track verification pass at its
head.

**Verified, not landed:** track `command-deadline` (`wip/command-deadline`,
[ADR-0381](DECISIONS/ADR-0381-a-commands-send-is-answered-within-its-deadline.md)).
Each send of a command now has ten seconds for its whole answer and is then
sent again as `retry` allows, so a press settles in bounded time; a send held
for the command before it waits its turn. At `bfda9ae5`, CI
[39658029334](https://github.com/studiokimeiga/perfect-web/actions/runs/39658029334)
and verify [39658074296](https://github.com/studiokimeiga/perfect-web/actions/runs/39658074296)
passed (141 reached recipes, `e14-pleris-controls` and the three browser
engines); the evidence is fetched at `973fbbcc`. Landing waits for the
owner's approval and for exact-commit CI at the landed commit.

**In review, not landed: upload image metadata repair** (PR #54, track
`upload-metadata`,
[ADR-0383](DECISIONS/ADR-0383-an-uploaded-image-is-kept-without-its-metadata.md)).
The preceding status incorrectly called the PR landed. Master remains
`4345726a2db9a12a8bfb5c755cd4e80ee90585d5`; the older `58591617` and
`818f4278` results are historical track evidence. Integration review and
subsequent research found additional ICC, container, staging-accounting and
PNG/APNG compressed-stream privacy gaps. The revised source
`1afd58fcfc66434070422b3112debeac94bad07f` passes exact CI
[40264422871](https://github.com/studiokimeiga/perfect-web/actions/runs/40264422871)
and whole upload verification
[40264616606](https://github.com/studiokimeiga/perfect-web/actions/runs/40264616606):
117 native and 45 browser baseline cases, all 146 strict mutation controls,
and the three browser suites. All nine reached recipes have canonical evidence: the remaining eight
passed [40265548018](https://github.com/studiokimeiga/perfect-web/actions/runs/40265548018),
including three full-browser repeats of 1,357 passed and 20 skipped each
(the skipped cases receive no passing credit). Exact final publication/PR
gates must still pass before merge. Unsupported complex ICC profiles are dropped; browser-
ambiguous APNG interleaving and excessive expected filtered extent refuse
with an unsupported diagnostic. Numeric color values, pixels, existing
stored media and private-media authorization/revocation remain outside this
metadata-minimization claim.

## last passing commit

**Latest verified master:** `e7664d2eb01aa8c6e91df594bd7270a84a8bf8de`
(train-12), fast-forwarded from `track/train-12` without rewriting history.

- Exact-commit CI passed where the commit was published:
  [39567756677](https://github.com/studiokimeiga/perfect-web/actions/runs/39567756677)
  on `wip/train-12-state`,
  [39575516309](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516309)
  on `track/train-12` and
  [39584219053](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219053)
  on `master`, each with x64, ARM and license/advisory checks.
- Automatic track verification
  [39575516650](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516650)
  planned the four recipes the canonical plan still required, `e10-browser`
  and the legacy `e8-wit`, `e10-component` and `e8-contracts`, and passed
  them and the full Chromium, Firefox and WebKit jobs.
- On master, verify
  [39584218974](https://github.com/studiokimeiga/perfect-web/actions/runs/39584218974)
  (push) and the scheduled
  [39596027504](https://github.com/studiokimeiga/perfect-web/actions/runs/39596027504)
  planned no recipe, the source's evidence being current (ADR-0281);
  failure census
  [39584219332](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219332)
  passed.

Before its record commit, the landed source `ec15e5b1` passed
[CI 39558466827](https://github.com/studiokimeiga/perfect-web/actions/runs/39558466827)
and [verify 39558474966](https://github.com/studiokimeiga/perfect-web/actions/runs/39558474966);
the [acceptance record](diagnostics/train-12/acceptance-record-2026-10-21.md)
reconciles the earlier eligible recipe records.

**Previous landing:** `115895de63d336352cd9051a828257abb92afedb` (train-11,
2026-10-20), with [CI 39489264202](https://github.com/studiokimeiga/perfect-web/actions/runs/39489264202)
and [verify 39489264142](https://github.com/studiokimeiga/perfect-web/actions/runs/39489264142)
at that exact commit. Older snapshots and failed or cancelled runs remain in
the [landing snapshot](STATUS-HISTORY-2026-10-21.md#last-passing-commit) and
the [last-passing ledger](STATUS-HISTORY-2026-10.md#last-passing-commit).

## completed gate items

On master since PR #53 (`4345726a`, 2026-10-25), ADR-0338 D2's read path
([ADR-0385](DECISIONS/ADR-0385-a-component-reads-its-tables-through-the-table-gate.md)):
a compiled component reads declared tables only through the table gate,
each import typed as `tables.json` declares before instantiation, each
answer held to its read; [evidence](evidence/E14/table-host.txt) by
`just e14-table-host`, 49 of 49 mutants killed. Writes and the other reads
are implemented on this unmerged branch under ADR-0386, with whole updated
campaigns and final review pending. Generic server routing, atomic platform
records, durable storage and an app with no Rust data layer remain.

At train-12 source `ec15e5b1`, the latest complete campaign detects all
43 TodoMVC, 62 device-storage and 50 second-host faults, plus TodoMVC's
8 compatibility, 55 Cypress producer and 17 strict producer controls.
TodoMVC's official Cypress suite passes 29 of 29 on the pinned compatibility
copy, not as unchanged upstream execution. The repeated full-browser recipe
passes three whole runs, each with 1,258 passed and 20 explicitly skipped
tests; the skips are not passes. All four records were imported with the
canonical fetcher after workflow completion, and the evidence-count gate
reports zero disagreements and failures. The landing's track verification
then passed the legacy `e8-wit`, `e10-component` and `e8-contracts`
obligations the canonical plan kept. The earlier failed runs remain failed.

On landed train-11:

- Renders share identical calls within a snapshot
  ([ADR-0354](DECISIONS/ADR-0354-a-render-reads-each-call-once.md)); committed
  components are checked against the build
  ([ADR-0355](DECISIONS/ADR-0355-the-committed-components-are-the-builds.md)).
- Browser tests start selected hosts
  ([ADR-0356](DECISIONS/ADR-0356-browser-host-map.md)); mutation slots isolate
  inputs ([ADR-0357](DECISIONS/ADR-0357-mutants-run-in-parallel-slots.md));
  Kiokun cache publication preserves the reader's version
  ([ADR-0358](DECISIONS/ADR-0358-kiokun-cache.md)).
- CI retains failing traces and red baselines, bounds checker latency and
  avoids repeating a verified nightly head
  ([ADR-0359](DECISIONS/ADR-0359-ci-reliability-traces-latency-incremental-builds-and-the-nightly.md)).
  Coverage-guided fuzzing measures or fails
  ([ADR-0360](DECISIONS/ADR-0360-the-coverage-guided-fuzzer-measures-or-fails.md)).
- Check and build share validation
  ([ADR-0361](DECISIONS/ADR-0361-check-is-build.md)); guide mutation kills
  require valid producer outcomes
  ([ADR-0362](DECISIONS/ADR-0362-guide-producer-validity.md)); Wasmtime 48.0.7
  addresses its recorded advisories
  ([ADR-0363](DECISIONS/ADR-0363-wasmtime-48-0-7.md)).

Earlier task controls and commands remain in the [E14 plan](milestones/E14.md)
and [completed-gates ledger](STATUS-HISTORY-2026-10.md#completed-gate-items).
Changed inputs can stale a historical pass;
[ADR-0281](DECISIONS/ADR-0281-a-merge-is-held-to-ci-s-verification-run.md) governs current
evidence. No failed result is promoted by this summary.

## failing gate items

- **On master, a command request that never settles holds its press, and
  the page's order behind it** (ADR-0173's "not claimed"). The repair is
  verified on `wip/command-deadline` and in train-13 (above), not landed. Its own limits
  stay open after it lands: an unknown outcome is told as "could not be
  sent", a command given up may still run after those behind it, and the
  host's waits have no deadline (ADR-0381,
  [KNOWN_LIMITATIONS](KNOWN_LIMITATIONS.md)).
- **On master, a page's on-demand modules** (found 2026-10-21, reproduced
  in Chromium): one failed load of a page's speculation module, renderer or
  computed values is kept, so every later press that speculates, or change
  that computes, fails the same way until reload; and a module that never
  loads holds its press and, by ADR-0152, the presses after it. Repaired on
  `wip/press-modules`
  ([ADR-0382](DECISIONS/ADR-0382-a-page-loads-its-modules-within-the-deadline.md),
  stacked on `wip/command-deadline`): its eight tests pass in three
  engines and its ten controls are killed. Its verify
  [39701638548](https://github.com/studiokimeiga/perfect-web/actions/runs/39701638548) at `11d7917c` **failed**: two of
  ADR-0381's tests counted given-up sends by an event Firefox and WebKit do
  not report, and a feed test timed out once in Chromium, not reproduced
  here ([NEXT](NEXT.md)). The two tests now time each send in the page; at
  `d57f18a1`, CI [39808460517](https://github.com/studiokimeiga/perfect-web/actions/runs/39808460517) and verify
  [39808499525](https://github.com/studiokimeiga/perfect-web/actions/runs/39808499525) passed, the feed test among them.
- **On master, CI's Node audit fails** since 2026-10-22: the benchmark
  baseline's next 16.3.8 is named in GHSA-673q-mjm6-r5xr and
  GHSA-2464-fhr4-w9xw (critical) and GHSA-p3m5-59mp-jv5r (high). Train-13
  carries the bump to 16.4.1 ([39857310130](https://github.com/studiokimeiga/perfect-web/actions/runs/39857310130) failed at its merge, before
  it).
- **A follow once went unanswered in Chromium** (`feed.spec.mjs:691`,
  verify 39701638548 at `11d7917c`): its request released, the test timed
  out waiting for its answer. Not reproduced in fifty local runs, two
  concurrency profiles among them, nor in its diagnostic rerun on GitHub at
  `d57f18a1`. Cause unknown; open in [NEXT](NEXT.md).
- Hosted evidence does not prove the local-only performance, memory, load
  or Kiokun obligations. E14 agent benchmarking still requires the owner's
  model, budget and teaching choices. Neither performance universality nor
  production readiness follows from these checks.
- Device storage becomes durable when the queued handler applies the
  assignment. Navigation before it runs can discard pending input; the
  privacy-test repair establishes storage before reload and does not change
  that runtime boundary. See [known limitations](KNOWN_LIMITATIONS.md).
- **E14 gates 3 and 4 remain open:** agent runs and analysis await model and
  budget decisions. Framework-wide performance superiority and broad
  application correctness are not established.
- The landed tree's [backend refusal inventory](../examples/accepted/build-refusals.json)
  records nine exact refusals: resource mounting (A-007), body-level secret
  binding (A-010), build-time page values (A-013), style directives (A-018),
  and placeholder dependencies (A-014/A-025/A-026/A-027/A-034). Repairing
  A-008's WIT collision exposed six existing gaps; the historical compiler
  reproduces the same nine when only A-008 is omitted. The
  [causal probe](evidence/tooling/corpus-after-wit.txt) preserves both runs.
  All 42 historical accepted programs remain unchanged, and A-008 builds.
- The scheduled-order Chromium timeout, WebKitGTK navigation/cancellation
  investigation, E7 long-frame instrument and E10 carry-over work retain
  their scope in [NEXT](NEXT.md), [KNOWN_LIMITATIONS](KNOWN_LIMITATIONS.md)
  and the [historical failing gates](STATUS-HISTORY-2026-10.md#failing-gate-items).
  Temporal authorization, live-version compatibility, unknown outcomes,
  composed budgets and browser-owned editing remain proposals where
  generated-runtime enforcement is not demonstrated.
- Train-12's dated failures and repair obligations are retained verbatim in
  the [landing snapshot](STATUS-HISTORY-2026-10-21.md#failing-gate-items);
  their replacements are reconciled in the acceptance record. None of the
  original failed runs becomes a pass.

## exact commands to reproduce

Quick inspection and focused regressions on master:

```sh
just doctor
cargo fmt --all -- --check
python3 scripts/mutation_anchors.py
bash scripts/case-check.sh
cargo test --locked -p pw-core --test query_values --test let_discard --test effect_convergence
cargo test --locked -p pw-dev-server --bin pw-dev-server -- --exact tests::a_flight_another_reader_waits_for_is_not_stopped tests::a_longer_read_is_not_applied_over_a_commit_it_did_not_see
python3 scripts/ci_plan.py --changed origin/master HEAD
```

The full gate is `just ci`; the changed recipe plan and browser suites run
on GitHub under [PARALLEL](PARALLEL.md)'s branch and port rules. Run reached
mutation scripts whole, then check restoration with the anchor command.
`just evidence-gates` validates machinery using injected producers; it does
not establish real compiler, runtime or browser behavior. Fetching and
freshness follow [ADR-0281](DECISIONS/ADR-0281-a-merge-is-held-to-ci-s-verification-run.md).
`just doctor` is read-only; `just bootstrap` installs pinned dependencies
under existing authorization, not a grant from this document.

## known environmental issues

Development uses Windows/WSL2 and GitHub hosted Linux x64/ARM. Quick local
checks stay under roughly ten minutes; heavy campaigns, full browser suites
and busy-host CI run on GitHub under the owner's 2026-10-20 grant. Workers
use assigned ports and their own targets. Pass
`--repo studiokimeiga/perfect-web` to `gh` for the connected repository.

The PC's WSL `gh` credential failed during this integration and again on
2026-10-21; authenticated Windows `gh.exe` works. Shared load affects timing;
build caches can exhaust disk space. On 2026-10-21 the PC still ran
development servers and `python3 -m http.server` processes that earlier
sessions left a day before, and on 2026-10-22 four such servers and a
dozen `http.server` processes, two days old; a run checks that its ports
are free rather than assuming its range is. On 2026-10-22,
`extractions/setup-crate` found no `just` 1.58.0 release in two jobs of
verify 39808499525 that the run's other jobs found; the two ran again. Follow [PARALLEL](PARALLEL.md)'s bounded diagnostic
rerun and owned cleanup rules; never loosen a bound to obtain a pass. No new
platform-wide outage is asserted here.

October 2 Firefox stalls and September macOS disk/cache incidents are
[dated observations](STATUS-HISTORY-2026-10.md#known-environmental-issues),
not current WSL or hosted-runner claims. Partial output, a missing producer
exit or an existing evidence file is never a successful result.

## last benchmark summary

The E10 close benchmark retained here is 2026-10-02 at
`bff437c6ae641529461ca56410b1f0337e06557a`, Apple M2 Pro arm64:
[raw evidence](evidence/E10/close-bench.txt), `just e10-close-bench 8`.
Store components were 3,037-4,221 bytes. The host's `add_to_cart` median
was 18.767 microseconds versus 20.620 for the Rust guest and 0.302 natively.
Three of eight Chromium runs reported an interaction long frame. These
are old measured inputs, not train-12 performance or framework comparisons.

[Benchmark requirements](BENCHMARKS.md) retain revision, machine, warm/cold
state, samples, distributions and raw commands. Narrow checker timings do
not replace an application benchmark. ADR-0027's correction still applies
to older frame attribution.

## next three concrete tasks

1. Land train-13 as the owner approved: exact-commit CI and the automatic
   track verification at its head, then `master` fast-forwarded to it.
2. E14-E: the owner's model, budget and teaching choices, from the decision
   brief; run nothing paid on an assumed choice.
3. Device-local durability (ADR-0366's boundary), and the remaining
   [NEXT](NEXT.md) follow-ups, the unexplained feed timeout among them.


### Session command authority continuation (2026-10-27, acceptance pending)

ADR-0389 now specifies immutable browser command actors, transaction fences,
reader-bound receipt metadata/reservations, request admission and final private
response checks. Same-cookie reopening also partitions applied interaction and
speculated-version history. Independent review found a real lost-ACK warm Store
refresh defect; a queued original-document frame regression first failed, then
passed after explicit authoritative materializer regeneration.

Reproduce the new bounded native/real-PG acceptance with
`just e14-session-command-authority` (both PW_FEED_DATABASE_URL and
PW_STORE_DATABASE_URL required). It records the exact source commit, frozen
binary inventories, raw native logs, strict controls and independent owned-schema
cleanup under docs/evidence/E14/session-commands.*/. A test declared passed while
skipped, an assertion outside the test boundary, a source drift or an uncertain
cleanup cannot pass. Browser/affected legacy recipes, independent final review,
preflight, full native tests and hosted exact-head CI remain separate merge
gates. The initial actual failures are preserved under
`docs/evidence/E14/session-commands-2026-10-27/`. No pending gate is waived here.
