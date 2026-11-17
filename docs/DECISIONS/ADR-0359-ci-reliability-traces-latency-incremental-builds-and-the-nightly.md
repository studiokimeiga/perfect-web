# ADR-0359: a failure's trace is kept, the latency ceiling bounds the fastest check, a mutant is built incrementally, and the nightly does not repeat a verified head

Status: proposed by track `ci-diagnostics` (2026-10-19) for the integrator;
local gates and the track's recipe recorded below; not yet in a train.
Date: 2026-10-19 to 2026-10-20. Milestone: E14; the evidence's own harness (ADR-0245,
ADR-0281, ADR-0344, ADR-0349).
Track `ci-diagnostics`, on `8c5618c8` (`track/train-10`'s head), not on
`master` `32757966` (the first correction).
Amends ADR-0349's `scripts/run_suite.sh`, ADR-0344's section 4
(`check_latency`), and `verify.yml` (ADR-0245, ADR-0281): its recipe step and
its nightly.

## Corrections first

- **The brief's base held neither thing it asked to fix.** Master
  `32757966` has no `scripts/run_suite.sh` and no
  `scripts/tests/test_run_suite.py` (ADR-0349), and its `check_latency.rs`
  is still the wall-clock 2 s version: both the helper and the
  CPU-time-per-thousand-lines ceiling (ADR-0344, `2f343bd1`) are train-10's.
  The branch was moved to train-10's head before any commit of its own,
  as the other tracks for train-11 are (`browser-host-map-train11` was "on
  `587657ff`"). Reversible: `git reset --hard 32757966` drops nothing of
  this track's.
- **`test-results/` was never the failing run's, only the last run's.**
  `verify.yml` said so ("each run of a suite clears the directory, so this
  is the last run's") and uploaded it once, at the shard's end. In
  train-10's second verify (39131257549) that was a mutation control's
  intended failures, as the integrator read it: `e10-browser`'s second
  Chromium run (a `scheduled-orders.spec.mjs:74` timeout) had failed, its
  third and every Playwright run after it in the shard had cleared the
  directory, and the trace of the one failure the train had to chase was
  not in `traces-23` (this track downloaded nothing of it).
- **No hardware counter is to be had here.** The brief allowed "a hardware
  instruction count if one is available in the test environment". In this
  WSL2 machine `perf stat -e instructions:u,cycles:u` says "not
  supported" (the virtual machine has no PMU: `arch_perfmon` is not among
  the CPU's flags, `perf_event_paranoid` is 2), and a GitHub-hosted runner
  is a virtual machine too. The statistic is the fastest of several CPU
  times (section 2).
- **At the PC's own loads the old ceiling held for train-10's checker.**
  Measured below: 143 to 330 ms CPU a thousand lines, alone, beside a
  build, or beside 64 busy loops (loads of 25 to 60), and 333 to 644 at
  loads of 49 to 116 (the table), against 800. The failures the coordinator
  saw (three of five local `just ci` runs at load 40 to 80) need the
  complete-validation checker of track `check-is-build` (2.0 to 2.9 times
  the work: 508 to 809 ms a thousand lines at loads of 21 to 55, its own
  notes), which is not on this base. So the proof of section 2 runs the
  checker also made two and three times over, as that one will be.

## 1. A failing run's output is kept

- **Cause.** Playwright removes each project's `outputDir`,
  `test-results/`, as a run starts (`createRemoveOutputDirsTask`, which
  only the watch mode skips; `playwright@1.58.0`, `lib/runner/tasks.js`).
  A recipe that runs a suite several times (`e10-browser`, three), and every
  Playwright run after it in a CI shard, a mutation control's among them,
  left the last run's output there, and the shard uploaded that once.
- **Fix.**
  - `scripts/run_suite.sh` copies a run's `test-results/` as the run ends,
    when it failed, into `<suite dir>/kept-traces/<recipe>/<time>-<pid>/
    run-<i>/` (`scripts/keep_traces.py`), before the next run clears it,
    and says where, before it says the run's exit. The recipe is
    `$PW_RECIPE`, which `ci_recipes.py` now sets for each recipe it runs
    (`local` where unset); the time and the helper's pid keep one call's
    runs apart from another's in the same recipe. Its status is the
    suite's whether or not the keeping worked.
  - **A run inside a mutation script is not kept, except its baseline,
    and that only when it came out red.** A mutant's suite fails by design,
    once per mutant, many in a shard (`e14-refusal` alone plants 12 browser
    mutants, each failing tests); its trace says nothing the kill does not,
    and they are the swamp the brief names. A red baseline is a
    real failure that stops the script, and it would be lost with the
    shard's later runs as `e10-browser`'s second run was. `mutation_baseline.explain()`, which
    all 277 call sites call where a baseline is red, now also keeps the
    output of the script's last command where that was a browser run that
    failed, in `<suite dir>/kept-traces/<recipe>/<time>/<script>-baseline/`.
    "The last command" excludes a mutant's: of the 277 calls, 272 follow the
    baseline's own red print, and the five others (a refused producer, the
    restored source's own check) find a cargo command or a real failure
    last; a test names the five.
  - **Bounded.** `kept-traces/` holds at most 32 MiB (`PW_KEPT_TRACES_KIB`)
    in all: ten times the 3 MB of train-10's whole `traces-23`, which held
    a run's failed tests. An entry that does not fit is not copied, and the
    run's directory names
    it and its size (`NOT-KEPT.txt`), so a run in which hundreds of tests
    failed keeps its first and says what it left. The upload steps of
    `verify.yml` (`traces-<shard>`, `browser-<engine>`) name every
    `kept-traces/`, beside the last run's `test-results/`, as before.
- **Claimed.** `test_run_suite.py`: a run that fails, then one that passes,
  keeps the first's trace though the second cleared `test-results/` (the
  stand-in clears it, as Playwright does), a passing run's is not kept, each
  failed run of three is kept apart, two calls in a recipe keep apart, a
  recipe's name (or `local`) is the directory and cannot leave it, the
  benchmark harness keeps under its own, the budget holds and names what it
  left, and a failure to keep leaves the suite's status. `test_keep_traces.py`:
  the copy, the budget (the root's whole, not the run's), a name that cannot
  leave the root, the command line, `ci_recipes.py`'s `PW_RECIPE`, and both
  upload steps of `verify.yml` naming `kept-traces/`.
  `test_mutation_baseline.py`: a red baseline's failed browser run is kept,
  a passed one, a mutant's (anything but a browser run last) and an earlier
  recipe's are not. 25 mutation controls
  (`scripts/ci_diagnostics_mutations.py`, `e14-ci-diagnostics`).
- **Not claimed.** That a trace explains a failure: it is kept, not read.
  That every Playwright run is kept: only a failed run of the helper, and a
  red baseline. A mutation script that runs Playwright and fails in a way
  that is not a red baseline (a mutant's test failing for the wrong reason)
  keeps nothing, as before. The browser jobs run the suite once, so
  their `test-results/` was never lost; they upload `kept-traces/` for
  symmetry only. The 32 MiB is a choice, not a measurement of the artifact
  store's quota.

## What the planner plans when `run_suite.sh` or `verify.yml` changes

Measured with `scripts/ci_plan.py` on this track's commits, each changed
file alone (`scripts/ci_plan.py` as `recipes_for` reads a file's hunks):

- **`scripts/run_suite.sh` alone plans 91 recipes** of the 362 CI can run:
  every recipe whose lines name the helper's path (`reads_what_changed`'s
  rule for "a shell script it runs", ADR-0281), `e10-browser` and every other
  browser recipe among them. The helper's behaviour differs only on a run
  that failed, but the planner cannot see that, so any change to it
  re-verifies every browser recipe.
- **`.github/workflows/verify.yml` alone plans three**: `e14-ci-diagnostics`
  (this track's upload, incremental and nightly mutants), and
  `e14-dealt-by-time` and `e14-evidence-integrity`, whose mutants of the plan
  step's `--stale` case are anchored within 30 lines of the nightly's edit
  (it plans one only for the upload edit, which is further). A workflow is no
  recipe's input; a recipe is planned by it only where one of its mutants is
  anchored within 30 lines of a changed line. Both ran whole after the edit
  (section Acceptance).
- `scripts/mutation_baseline.py` plans four, by mutants anchored near the
  hunks (`e14-ci-diagnostics`, `e14-derived-outside`,
  `e14-evidence-integrity`, `e14-mutation-bound`); `check_latency.rs` two
  (`e14-ci-diagnostics`, `e14-evidence-integrity`); `ci_recipes.py`,
  `keep_traces.py`, `mutant_incremental.py`, `nightly_skip.py`, the
  `.gitignore`s and the recipe file plan this track's recipe alone.
- **Together, against train-10's head (`8c5618c8`): 94 recipes in 33
  shards**, 93 with `--current` (`ci_plan.py --shards 9 --changed 8c5618c8
  HEAD --current`, which leaves out `e14-ci-diagnostics`, its evidence
  current). **Against `master`, which is where a track's push measures from
  (`git merge-base origin/master`): 177 in 39**, since train-10's own changes
  count there, `--current` leaving out `e14-tables`. So a push of this branch
  starts a verify of 177 recipes (43 jobs: the plan, three browsers, 39
  shards), hours of runners repeating train-10's dispatches. The integrator
  may cancel it (train-6's and train-10's precedent) and dispatch what the
  changes reach: `e14-ci-diagnostics e14-evidence-integrity
  e14-evidence-integrity-2 e14-derived-outside e14-dealt-by-time
  e14-mutation-bound e10-browser`.

## 2. `check_latency` bounds the fastest check

- **Cause.** ADR-0344's finding 4 moved the ceiling from the wall clock to
  the checking thread's CPU time, and noted that CPU time "is not free of
  load either" (about 1.5 times). This track measured how not, one
  6,568-line check at a time (a probe of the same call, thread CPU time and
  the CPU it ran on, 15 to 20 checks in a row): 0.94 s alone; 1.7 to 2.2 s
  beside a looping workspace build; 1.7 to 2.0 s beside 64 busy loops, and
  drifting by up to 2 times within 30 seconds as other workers' builds
  start and stop. The PC is an i9-13900K, SMT and efficiency cores under
  Hyper-V; a thread's CPU time counts the sibling's contention, the slower
  core and the host's, as well as its own work. The test took the median of
  seven consecutive checks, ten to thirty seconds of it, so the drift of
  those seconds was taken whole. For train-10's checker that is far from
  800 ms a thousand lines (143 to 644 measured, by load); for the
  complete-validation checker of track `check-is-build` (2.0 to 2.9 times
  the work, its notes) it was 508 to 809 at loads of 21 to 55, and the
  integrator reported three of five local `just ci` runs failing at 40 to
  80.
- **Fix.** The ceiling is unchanged: 800 ms of the thread's CPU time a
  thousand lines (a test pins the number, so a raise is a visible change to
  a test). It bounds the fastest of at least seven checks, not their median:
  a machine's load only adds to a check's time, so the fastest is the
  least moved, and a checker that is slow itself is as slow in its fastest
  check as in its slowest. Where the fastest so far is over the ceiling,
  more checks are taken, until one is within it, or 40 have been taken, or
  their wall clocks add up to four minutes (`sample`): a quiet machine takes
  exactly seven, as before, and a machine that quiets down is waited for.
  The slow path's ratio to the clean (5) is of the fastest of both; the
  control (a check that happened, more than 200 microseconds of CPU time)
  is of the fastest. The medians are still printed, with the fastest and
  the number of checks, so that CI's logs go on saying what the runners
  measure; a failure says the load average.
- **Hardware counts: not available** (the corrections): the statistic is
  time. **Not the other fixes:** a calibration loop on the same thread (a
  new unit, calibrated on a machine not at hand: the ARM runner), pinning
  to a performance core (privileged and not portable), waiting for the load
  average to fall (it lags a minute, and a busy loop never falls), skipping
  the test under load (a gate that passes where it cannot measure), and
  raising the ceiling (ruled out).
- **Claimed.** Nine tests that run no check (`the_ceiling_is_800_ms_...`
  through `a_clock_that_reads_nothing_...`), on checks that cost what a
  script says: the ceiling is 800 and is per thousand lines; six checks of
  seven stretched past it and one not pass, by the fastest, though their
  median fails; the fastest is of CPU time, not wall; a quiet machine is
  asked seven times and no more; a machine that quiets down after ten
  stretched checks is waited for, to the first that fits; the waiting ends
  with the window or the most checks; **the checker made ten times slower
  fails however many checks are waited for**; a clock that reads nothing,
  or once among real readings, is refused by the control; the slow path is
  judged at the fastest of both. 13 mutation controls on them
  (`ci_diagnostics_mutations.py`), killed; and `evidence_integrity_mutations.py`'s
  two on the measurement itself still killed by the real tests, run whole
  (42 of 42): the checker run ten times over fails 1 of its 11 tests (the
  cold test), the thread's clock reading nothing 2 of 11.
  **The proof, at load.** The first test as committed, and the old
  one (`8c5618c8`'s, the median of seven) beside it, alternately, as test
  binaries built from a scratch copy of the tree (`/tmp/cid-proof`), the
  checker as it is, made two and three times over (the complete-validation
  checker costs 2.0 to 2.9 times as much), and ten times over (the
  control). On the PC (32 threads) beside the other workers' builds and
  tests and a looping `cargo build --workspace`; the one-minute load average
  at the start of each run in the table. ms are of CPU time a thousand
  lines; the ceiling is 800.

  | checker | test | runs passed | median of seven | fastest of seven or more | load |
  |---|---|---|---|---|---|
  | as is | old | 10 of 10 | 333 to 562 | | 49 to 92 |
  | as is | **new** | **10 of 10** | 323 to 644 | 240 to 461 | 57 to 116 |
  | 2 times over | old | 3 of 5 | 678 to 976 | | 45 to 76 |
  | 2 times over | **new** | **5 of 5** | 559 to 781 | 476 to 651 | 47 to 71 |
  | 3 times over | old | 0 of 4 | 931 to 1,329 | | 47 to 52 |
  | 3 times over | **new** | **4 of 4** | 730 to 1,181 | 642 to 702 | 43 to 56 |
  | 10 times over | **new** | **0 of 10 pass** | 2,742 to 3,610 | 2,389 to 3,442 | 66 |

  So: **the new test is green in 10 of 10 runs at load, and the slowed
  control fails in 10 of 10** (three to four times the ceiling at its
  fastest, so no number of checks lets it through). The old statistic
  failed the checker two times over in 2 of 5 runs and three times over in
  4 of 4, which the fastest of seven or more passes; one run of the latter
  took 12 checks, waiting for a quieter moment (the median of its 12 was
  1,181). At loads of 77 to 89, an earlier build of the same test (90 s
  window), the checker three times over failed both (the fastest 985, the
  median 1,209 and 1,329): see the next bullet.
- **Not claimed.** That the ceiling is proof against any load: a machine
  loaded through the whole of a measurement (four minutes) with every check
  stretched past the ceiling still fails it. Beside a load of 77 to 120 the
  PC's CPU time for the same check is up to 3 times its quiet time (the
  hybrid cores, SMT and the host, which a guest cannot see: WSL2 reports 16
  cores of two threads), and the checker three times over fails there
  whatever the statistic. That is the ceiling's margin to carry
  (train-10's checker: 800 against 143 to 644 on the PC, 321 on ARM; the
  complete-validation checker is 2.0 to 2.9 times that: ARM would measure
  640 to 930 against the 800, which is the integrator's to rule on, and
  no measurement here changes). A calibration loop would hold the verdict
  at any load, and needs a constant per machine class that only the ARM
  runner can give. That CPU time is the work. That `e9-latency`'s wording ("median of 7 runs") changed: it
  prints the medians still, and the fastest beside them. The real
  measurement's assertion lines are tested through `fast_enough` and
  `happened`, not mutated in place.

## 3. A mutation script's mutants are built incrementally

- **Cause.** Each mutant undoes one line, so its build redoes, compiled
  incrementally, what the line reaches, and a crate whole when not. The
  integrator measured a one-line `pw-core` mutant's test binaries at 21 to 23
  s with incremental compilation on and 33 to 37 s off (the PC, load 37), 1.6
  times as quick; train-9's verify ran 1,391 mutants. It was off for two
  reasons, neither about mutants: 104 recipe lines say `CARGO_INCREMENTAL=0
  python3 scripts/..._mutations.py` since 2026-09-24, when the Mac's disk
  filled (STATUS: the incremental cache was 3.6 GB), and `verify.yml` sets it
  for every job.
- **What `Swatinem/rust-cache` does with `target/*/incremental`.** The
  workflow uses `@v2`, which is `v2.9.2` (commit `6323deb1`, 2026-08-06); its
  README and source, read through the API:
  - README, "Cache Details": "Before being persisted, the cache is cleaned of:
    ... Incremental build artifacts. ..." and "For this reason, this action
    automatically sets `CARGO_INCREMENTAL=0` to disable incremental
    compilation, so that the Rust compiler doesn't waste time creating the
    additional artifacts required for incremental builds."
  - `src/cleanup.ts`, `cleanProfileTarget`: `let keepProfile = new
    Set(["build", ".fingerprint", "deps"]); await rmExcept(profileDir,
    keepProfile);` A profile directory keeps those three and loses the rest,
    `incremental` among them, before the cache is saved. `src/restore.ts:30`:
    `core.exportVariable("CARGO_INCREMENTAL", 0);`.
  - So the saved cache never holds incremental artifacts, whatever the
    recipes' builds left (and only shard 0 saves: `save-if`); the action's
    zero is for the builds it caches, the `Build` step's, which keep it; and
    the recipe step is free to differ.
- **Fix.** `PW_MUTANT_INCREMENTAL=1`, set on `verify.yml`'s `Recipes` step
  alone, makes `scripts/mutation_baseline.py` (which every mutation script
  imports first) remove `CARGO_INCREMENTAL` from the script's environment and
  so from what it starts, its recipe's `CARGO_INCREMENTAL=0` with it
  (`scripts/mutant_incremental.py`). No recipe line is edited, so none is
  planned for one (104 would have been). The `Build` steps, the browser jobs
  and a recipe's own commands outside its mutation scripts keep the
  workflow's zero.
- **Removed, not set to 1.** `CARGO_INCREMENTAL=1` forces incremental
  compilation for every profile, the `browser` profile's wasm among them
  (`run.sh --profile browser`: opt-level `z`, LTO), whose size a browser spec
  gates; removed, cargo's defaults apply: incremental for the dev and test
  profiles, which are a mutant's rebuilds, never for a release-like one. The
  real cargo shows it, on a probe crate with such a profile
  (`test_mutant_incremental.py`, `CargosDefaults`): removed, a dev build is
  incremental and a `browser` build is not; a zero makes neither; a one makes
  the `browser` build incremental too. (The speed-up below was measured with
  the variable at 1; for the dev and test profiles of its builds that is
  cargo's default.)
- **Disk.** A runner has about 14 GB free and 25 GB more once what its image
  ships is removed (`verify.yml`). Measured on the PC, with debug info (CI's
  workflow sets it off, so its caches are smaller):
  `metadata_mutations.py`, whole, with incremental on: the target 2.91 GiB at
  its peak, 1.33 GiB of it incremental, against 1.60 GiB with it off. The
  widest crate graph any one script builds (that script's, and `pw-conformance`'s
  29 test targets, `pw-host` with its engine, the wasm crates, `pw-cli`,
  `pw-syntax`, `pw-time`, `pw-materialize`, `pw-resume`, `kiokun-server`,
  `corpus-check` and the server) took the target to 8.38 GiB, 2.18 GiB of it
  incremental (the largest: `pw-core`'s three, 414 + 404 + 238 MiB, and the
  server's two, 332 + 114). No script builds `pw-core`'s 230 test targets at
  once: of 465 cargo test commands in the 274 scripts, the broad ones
  (`cargo test -p <crate>`, no `--test`) are of `pw-cli` (4 targets),
  `pw-render` (16), `pw-host` (12), `pw-conformance` (29), `pw-render-wasm`
  and a few smaller. Bounded all the same: after each cargo command a script
  ran, `mutant_incremental.bounded()` removes every incremental cache
  (`target/*/incremental`, a triple's `target/*/*/incremental`) if together
  they hold more than 6 GiB (`PW_INCREMENTAL_MAX_MIB`), and
  `ci_recipes.prune` now removes a triple's as well as the profile's,
  between recipes (it removed only `target/debug/incremental`).
- **The speed-up, on one real script, whole, the same kills both ways.**
  `metadata_mutations.py` (34 mutants, three lanes: a `pw-core` test, the
  renderer's, two of the server's), run from a clone of this track's head, one
  with `CARGO_INCREMENTAL=0`, one with `1`, at once, on the PC beside the
  other workers (load 30 to 66):

  | | off | on |
  |---|---|---|
  | mutants killed | 34 of 34 | 34 of 34 |
  | verdict lines | the same 34 | the same 34 |
  | the whole script | 2,513 s | 1,658 s (**1.52 times** as quick) |
  | the baseline (three cold builds) | 249 s | 256 s |
  | the 34 mutants | 2,264 s, 66.6 s each | 1,402 s, 41.2 s each (**1.61 times**) |
  | target at its peak | 1.60 GiB | 2.91 GiB (1.33 GiB incremental) |
  | the memory bound | no process stopped | no process stopped |

- **Not claimed.** A run on a GitHub runner: its speed-up, its disk and its
  4 vCPUs are not measured here; the numbers are the PC's. That every script
  is as much quicker: a script of few mutants gains little (the baseline
  costs the same), and a mutant in a crate's root rebuilds more of it than
  one in a leaf. That incremental compilation cannot change a
  verdict: the same 34 of 34 verdicts are one script's, and the verify run
  with the step on is the evidence for the 273 others, as it is for every
  recipe. The release-like profiles are not
  built incrementally, so the wasm bytes the browser suites load are as they
  were.

## 4. The nightly does not repeat a verified head

- **Cause.** In the seven days to 2026-10-20 the nightly `verify` (`on:
  schedule`) used 691 runner-hours, about 100 a night and 27% of all the CI
  there was, and re-verified the same head of `master` on 2026-10-18 and
  2026-10-19 (it has been `32757966` since 2026-10-17).
- **How the skip decides.** One query of the GitHub API,
  `GET /repos/{repo}/actions/workflows/verify.yml/runs?head_sha=<GITHUB_SHA>&status=success`
  (`gh api`, with the workflow's `GH_TOKEN`; the plan job has `actions:
  read`): the nightly is skipped where one of the runs it returns is of a
  **push or a schedule**, a landing train's own run or an earlier nightly's.
  It is the rule `verified_base.py` already holds a push to `master` to
  (ADR-0281, amended 2026-10-13: `COVERING = ("push", "schedule")`, imported,
  not copied). A `workflow_dispatch`'s run is not one, since it runs the
  recipes it names; a run that failed, was cancelled or is still going is not
  one either, since `status=success` leaves it out. **Any error runs the
  nightly in full** (the API unreachable, a reply that is no JSON, no `gh`).
- **What a skip does.** The plan step (`verify.yml`) writes `shards=[]` and
  `browsers=false`, so no recipe shard and no browser job runs (the browser
  suites ran at that commit in the run that passed), and says in its log and
  in the run's summary which run it stands on: `skip: 32757966 was verified by
  the push's run <id> (<url>); Sunday's schedule verifies it in full`. The
  summary job reports "0 recipes, 0 failed" and passes.
- **The weekly full run.** `verify.yml` has two schedules, `17 6 * * 1-6` and
  `17 6 * * 0`; the plan step is told which fired (`github.event.schedule`)
  and Sunday's (`WEEKLY` in `scripts/nightly_skip.py`, held to the workflow's
  string by a test) never asks: it runs every recipe whatever has passed, so
  that slow drift (a dependency's new advisory, a test that fails one run in
  twenty) is met within a week.
- **The override.** `workflow_dispatch` never asks: with no recipes named it
  runs every one, as before; a test holds that only the schedule's branch
  of the plan step calls the script.
- **Claimed.** `test_nightly_skip.py` (21): the decision on the API's
  answers (a push's success skips, a schedule's skips, a dispatch's does not,
  a failure, a cancellation or none does not, the weekly never, any other
  schedule asks, any error runs it, and the events are `verified_base.py`'s);
  the command line against a stand-in `gh` (the query asked, the exit status
  skip, run or misuse, an API that fails or answers nonsense); the schedules;
  and the plan step run, as the runner runs it, on stand-ins for the API and
  the planner, for each of a skipped nightly (outputs, summary and log), an
  unverified one, Sunday's, one whose API fails, and a dispatch with and
  without recipes. 15 mutation controls.
- **Not claimed.** The saving: it is read from next week's runs (when
  `master` does not move, six nights of seven lose their recipe shards). That
  a skip is the same as a full run: it is the integrator's rule, a verified
  head not verified again. A head verified by a *track's* push at that exact
  commit counts (the landing train's own run), and so does a skipped
  nightly's own success at it: each stands on the run it names, which must
  have been real once. A red Sunday does not stop the weekdays' skips at that
  head (the weekly's failure is the signal; the next push re-verifies). The
  nightly's browser suites are skipped with its shards (3 jobs of about 45).

## End to end, with a real Playwright

`scripts/run_suite.sh --runs 3` on a toy spec that fails its first and
third runs and passes its second (Playwright 1.58.0, Chromium, `trace:
"retain-on-failure"`): run 1's `trace.zip` (5.8 KiB) was in
`kept-traces/e10-browser/<time>-<pid>/run-1/` after run 2 had cleared
`test-results/` and run 3 had left its own there; run 3's was kept as well;
the helper's lines named both before their exits. (A toy tree under `/tmp`:
the repository's own suite needs its builds, and is the verify run's.)

## Alternatives

- **A `test-results/` of its own for each run** (`outputDir` in
  `playwright.config.mjs`, by a timestamp): keeps every run's failures, a
  mutation control's among them (the swamp), and bounds nothing.
- **Upload after every recipe**: an artifact per recipe, hundreds a run,
  against the artifact store's quota, to keep what a few recipes fail.
- **The helper alone, not the red baseline**: a baseline that came out red
  loses its trace to the shard's later runs as `e10-browser`'s second run
  did; 59 scripts run Playwright, all through `explain()`.
- **Keeping a mutant's failing run too, under a label**: hundreds of traces
  a shard that the kill already explains.
- **A hardware instruction count, a calibration loop, pinning to a core,
  skipping under load, a higher ceiling**: section 2.
- **`CARGO_INCREMENTAL=1` for the recipe step**: forces it for every profile,
  the browser wasm's too (section 3). **`CARGO_INCREMENTAL=${PW_...:-0}` in
  the 104 recipe lines**: 104 recipes planned for one changed line each, and
  a conflict for every track that adds a recipe beside one.
- **Keeping incremental off and building mutants in a larger cache**: the cap
  and the prune bound the disk, which was the reason it was off.
- **Skipping the nightly by the date of `master`'s last commit, or by a
  stored file**: the API says what ran, at that commit, and a file would be
  one more thing to keep true. **A status check on the commit**: `verify` sets
  none; the runs are the record (`verified_base.py` reads them the same way).

## Next

- Merge with train-11 (the integrator's); `check_latency.rs` differs from
  `track/check-is-build-train11`'s by one line, `check_program_sources`:
  take both.
- Rule on the ceiling for the complete-validation checker (800 ms a thousand
  lines; ARM would measure 640 to 930), or on a calibration loop with
  per-architecture constants, once `check-is-build` lands.
- A push of this branch plans 177 recipes in 39 shards (93 against
  train-10's head): cancel it and dispatch `e14-ci-diagnostics
  e14-evidence-integrity e14-evidence-integrity-2 e14-derived-outside
  e14-dealt-by-time e14-mutation-bound e10-browser`, or merge first.
- When this lands on `master`, the first nightly after a verified landing
  shows `skip: <sha> was verified by the push's run <id>` in its plan job and
  its run's summary, and runs nothing; Sunday's shows `run: the weekly full
  run`. Read the first few; the saving (691 runner-hours a week, 27% of our
  CI) is read from the week after.

## Questions for the integrator

1. The base: this track is on train-10's head, not on `master`
   `32757966` (the first correction). Is that what you want, or should its
   commits wait for train-10 to land, or move onto train-11's tip?
2. Is a mutant's browser run never kept right (section 1), or should a
   failing mutant run in a recipe whose baseline passed (a mutant killed for
   the wrong reason) be kept under a label, within the same 32 MiB?
3. Should `e9-latency`'s wording ("Median of 7 runs") say that the ceiling
   bounds the fastest of them? (Its evidence is the Mac's, `LOCAL_ONLY`;
   unchanged here.)
4. A skipped nightly skips its three browser jobs with its shards (they ran
   at that commit in the run that passed). Keep them as a daily signal
   instead (3 jobs of about 45)? One line of the plan step.
5. `PW_MUTANT_INCREMENTAL` is on in CI's recipe step only. The recipes'
   `CARGO_INCREMENTAL=0` stand locally (the Mac's disk, 2026-09-24), so a
   local `just e14-...` builds its mutants as it did. Should the PC's
   too, with the same cap?

## Acceptance

- **Tests.** `scripts/tests/test_keep_traces.py` (11), `test_run_suite.py`
  (16, 8 new), `test_mutation_baseline.py` (26, 8 new),
  `test_mutant_incremental.py` (18, three on the real cargo),
  `test_nightly_skip.py` (21), `check_latency.rs` (11, 9 new, none timed);
  `just evidence-gates` passes whole at the head: 331 tests (2 skipped).
- **Every mutation script the changes reach, run whole**, each on the tree
  named (clones of it, so as not to wait for one another): this track's
  `ci_diagnostics_mutations.py` 69 of 69 (25 + 13 + 16 + 15, the findings in
  order) at `a17cf246`, and as `e14-ci-diagnostics` at that commit;
  `derived_outside_mutations.py` 10 of 10, `mutation_bound_mutations.py` 16
  of 16 and `dealt_by_time_mutations.py` 54 of 54 at `a17cf246` (the first two
  by `mutation_baseline.py`'s hunks, the third by the plan step's, which sit
  within 30 lines of its mutants' anchors); `evidence_integrity_mutations.py`
  42 of 42 (the checker run ten times over fails 1 of its 11 tests, the thread's clock reading nothing 2 of 11) at `a17cf246`; and `evidence_integrity_2_mutations.py`
  35 of 35 at `e438f689` (`run_suite.sh` and `test_run_suite.py` have not
  changed since). The memory bound stopped nothing; `mutation_anchors.py`
  finds 3,348 mutants in 274 scripts, each matching once, after each.
- **`just ci`** on `df112643` (this track's first four findings' code),
  02:50 to 04:13Z on the PC at loads of 40 to 90: `ci: OK`; evidence-gates
  307 tests (2 skipped), case-check 2,878 paths, anchors 3,332 mutants in 274
  scripts, 346 test binaries with 3,074 tests passed, none failed, 25
  ignored, mutant programs 301 judged in 12. Its own `check_latency` ran
  with the fastest of seven at 176.1 ms a thousand lines, the median 198.0.
  The commits since touch no Rust (a fix to `mutant_incremental.py`, the
  nightly skip, the ADR and the evidence): evidence-gates (331 tests),
  case-check, anchors and `cargo fmt --check` ran again on the head.
- **GitHub's `ci` on `dd9b2dc3`** (the first two findings, pushed
  2026-10-20): `licenses and advisories`, `ubuntu-24.04` and
  `ubuntu-24.04-arm` all succeeded.

## Merged into train-11 (2026-10-20)

Merged after the render merge, the committed-components check, the browser
host map, the allocator, kiokun-cache and bounded-race (train-11's
526ccf66). There the scripts call `mutation_baseline.explain()` 287 times,
not 277, and `EveryCallOfExplain` failed `just ci` with a StopIteration: a
call after a line it could not name. The five the train brought were each
read: the browser host map's follows a red baseline of one suite in its own
words; the committed components', the render merge's and the skipped specs'
guard's follow a refused producer, whose last command is a cargo build or a
unittest run, never a mutant's browser run; kiokun-cache's follows a red
baseline and the output it printed. The test names all ten
(`test_each_call_follows_a_red_baseline_or_one_of_those_named`, "five"
before), and a call it cannot name now fails its assertion, named by its
line, not with a StopIteration.

Its later commits (df112643 to 54be8c66: a mutant built incrementally on the
verify run's recipe step, the nightly that does not repeat a verified head)
were merged again on top. `mutation_baseline.py`'s docstring holds both the
allocator's paragraph on slots and this track's on incremental builds; as the
module is imported, the slots engage first (a run asked for slots ends there,
and each slot's copy imports the module again), then the memory bound, then
the incremental builds.

## Numbered on train-11 (2026-10-20)

- **Numbered ADR-0359** (the integrator's ruling of 2026-10-20: a train is
  numbered before it is pushed, after master's ADR-0353, in train order),
  merged from `54be8c66` (`track/ci-diagnostics`). `ADR-XXXX` reads
  `ADR-0359` in `just/ci-diagnostics.just`, as in this file's name and
  title.

## Amended 2026-11-17: the longest recipe runs in parts

The integrator's, under the owner's grant of 2026-11-17 to make CI faster
with no gate given up (no paid runners: the account keeps its plan). A
verify deals recipes to shards, each runs its recipes in turn, and the run
takes its slowest shard's time. No recipe is split across shards, so the
longest recipe sets every full verify's length. In L5's repairs' verify
(43978661391) `e14-row-ownership` took 11,009 s on a shard by itself, its
241 mutants about 45 s each over the 27 commands of
`row_ownership_mutations.py`. The next longest, `e14-d3-postgres` (9,710 s),
had been split into two parts for the same reason (ADR-0393, amended).

`row_ownership_mutations.py` runs in three parts (`PARTS`, three even
ranges of `MUTANTS`, `scripts/mutation_parts.py`): `e14-row-ownership` runs
the tests, the browser suite and part 1, `e14-row-ownership-2` and
`e14-row-ownership-3` parts 2 and 3. Each part runs the unmutated baseline
first, as a whole run does, so each part's kills mean what a whole run's do.
`evidence_counts.py` holds the three recipes to every mutant once, as it
held d3's two (`split`). The plan deals the three to three shards, and
`ci_seconds.json` carries projections (4,400 s, 3,800 s and 3,800 s) until a
run measures them. `mutation_parts.chosen` is d3's `chosen`, shared (d3's
own copy stays, so its recipes' inputs do not change). A word given to the
script that names no mutant refuses to run, as d3's does (L5's second
review): the baseline alone would pass.

**And d3's first part in two.** With row ownership in three, the longest
recipe a data-layer change reaches is `e14-d3-postgres`, D3's own 96 mutants
in 9,710 s. Its `PARTS` are three: the first and second halves of D3's own
(48 each, `e14-d3-postgres` and `e14-d3-postgres-2`) and the uploads' (39,
`e14-d3-postgres-3`, which `e14-d3-postgres-2` was), each beside a database
(`NEEDS_DATABASE`). Projections: 5,000 s, 4,900 s and the uploads' measured
2,868 s.

**What a split does not shorten.** Every recipe together is about 74 hours
(`ci_seconds.json`, 364 recipes), so a full verify's 24 shards hold about
three hours each whatever the parts: a whole run is bounded by its total, a
pull request's by the longest recipe it reaches. The split is for the
second, which is every pull request's.

