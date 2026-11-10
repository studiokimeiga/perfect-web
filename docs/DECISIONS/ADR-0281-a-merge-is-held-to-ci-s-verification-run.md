# ADR-0281: a merge is held to CI's verification run

Status: accepted under the owner's delegation of 2026-10-02, on the owner's
decision of 2026-10-08, 22:45 (relayed). Date: 2026-10-08. Milestone: E14.
Amends the integrator's practice under ADR-0245. Amended 2026-10-12 (the
integrator's): a fetch takes no file of code the tree it joins does not
hold.

## Context

- **Merging waited on local chains of mutation scripts.** Before merging,
  the integrator ran whole each script whose code or tests the change
  touched, one after another on the development machine:
  - ADR-0277's 17 scripts took 4.5 hours;
  - ADR-0280's 24 were stopped after five of them, on 2026-10-08, to free
    the machine. Its load was 7 to 10 on 12 cores, with a worker's builds
    beside the chain, and its memory nearly full.
- **CI already runs the same recipes** (ADR-0245, ADR-0249, ADR-0278).
  Verification runs them in 17 parallel shards on GitHub in under an hour,
  each recipe's output an artifact stamped with its run.
- **The owner's decision** (relayed 2026-10-08): CI's verification run is
  the evidence of record for a merge. Locally, the fast gates and the
  touched area's quick checks only.
- **CI's plan missed browser specs.** `ci_plan.py` reached a recipe from a
  change to its mutation script, to a line near a mutant's anchor, or to a
  Rust test file a script runs. A change to a browser spec reached nothing:
  ADR-0280 changed `pages.spec.mjs` and `accessibility.spec.mjs`, and
  `e14-accessibility` and `e14-titles` would not have run.

## Decision

1. **A branch is merged once its tip's runs pass.**
   - CI's `ci` and `verify` runs at its tip are green, apart from failures
     already open in NEXT with their runs. The merge's message names each
     such failure.
   - The integrator's own work is pushed first to a branch `track/<name>`,
     so that verification plans what it changed since it left `master`, as
     a track's push does (ADR-0253).
   - The merge's message names its verification run, and what it found.
2. **Locally, before a push:**
   - `just ci`, the fast gates;
   - the new or changed tests themselves;
   - the change's own mutation script, once. That run is a first look,
     which finds a survivor sooner than a run on CI would. It is not the
     record.

   Every other script the change touches runs on CI only.
3. **CI plans what a change touches** (`ci_plan.py`). A browser spec
   counts as a test, as a Rust test file does. A change to one plans:
   - each mutation script that runs it;
   - each recipe that names it in its own lines.
4. **The record.** `just evidence-fetch <run>` copies a run's evidence into
   `docs/evidence/`, each file stamped with its run and its commit (ADR-0245).
   - **A run that failed only where a failure is open in NEXT** is fetched
     with `--known <job>` for each such job, which the merge names too
     (amended 2026-10-09, when WebKit's "Load more" failed nearly every run).
     A recipe shard is never known: its evidence is what the fetch copies.
   - **A run of one shard is read where its evidence was put** (fixed
     2026-10-09): `download-artifact` extracts a lone match into the path
     itself, so the summary found no shard and failed every run that planned
     one (W6's 1b, run 37889326640, passed its recipe and reported none).
   - **A file is taken only where it is evidence of the tree it joins**
     (amended 2026-10-12). A run records each recipe over its commit.
     Where what a recipe reads changed between that commit and the tree
     the fetch writes into, staged or not, as `ci_plan.py --changed`
     would plan that range, the fetch refuses its files: the branch
     merges master and is verified again. `--skip RECIPE` copies none of
     a recipe's files, where another run records it (a head that changes
     only tests, PARALLEL.md, 2026-10-12). Found at narrowing-2's merge:
     its run at `853674f` recorded scripts that run the server's tests by
     name, and W8, fixed-arms and W6's packet had changed that server on
     master since. A file no recipe of the run wrote is refused too.
   - **The fetch comes before an ADR is numbered** (the integrator's
     correction, 2026-10-12). Numbering `ADR-XXXX` rewrites lines of the
     branch's tests, scripts and recipes, which those recipes read, so a
     fetch after it refused eight more files at related-files' merge. A
     merge is merged, resolved, fetched, then numbered.
   - **Tracks ready at once go on a train** (the integrator's, 2026-10-12).
     Each merge stales, by the rule above, the recipes of every other track
     that read what it changed: narrowing-2 changed nineteen scripts that
     three tracks' runs had read. Merged one at a time, each track is
     dispatched again, and each later merge can stale it again. So the
     integrator merges them, each as its own merge commit with
     its ADR numbered and its documents, on a branch from master
     (`track/train-<n>`); runs `just ci` there once; and verifies the train
     once, which plans every recipe the train's changes reach. Its evidence
     is the train's last commit, master moves to the train when its run is
     green, and each merge names the train's run. A recipe that fails names
     its track: that track leaves the train, which is built again without
     it, as GitHub's merge queue removes a pull request whose changes failed
     its merge group (docs.github.com, "Managing a merge queue", read
     2026-10-12). A track goes on a train with its own run green, or with
     what that run found fixed and checked where it failed. The train's run
     verifies the tree master will hold, the tracks' interactions with it,
     and plans each track's whole recipe set, not only what was stale: its
     cost is one run, not less work in every case. **Correction (the
     integrator's, 2026-10-13)**: this amendment first said a compiler's
     merge stales every track's evidence; the planner reads a change's lines,
     not the crates it builds, and the merge of ADR-0321 staled a few
     recipes.
   - **What a train's run verified is not verified again** (the
     integrator's, 2026-10-13). Master moved to a train whose code differs
     from the run's commit only in `docs/`, the evidence and
     `scripts/ci_seconds.json` keeps the train's run as its record: the
     push's `verify` is cancelled, its `ci` runs, and the nightly runs
     every recipe. **Automated the same day**: a push to `master` plans
     from the newest commit on its first-parent line that a push's or the
     nightly's verify run passed at, and from the commit before the push
     where none did (`scripts/verified_base.py`, read through `gh` with
     the run's token, `actions: read`). A train's evidence commit then
     plans nothing: neither landing's evidence reaches a recipe (`ci_plan.py
     --changed 64cc7da1 f3bc4d21` and `25b01345 bb5dbcb3` each plan none). A
     merge made on master is not its track's head and is planned as before;
     a dispatch's run, which runs only the recipes it names, does not
     count; any error plans from the commit before the push, more and
     never less. Its first use, master's push after train-3, planned from
     the train's head and planned nothing (verify 38440251297, "planned
     from 051e8eb"); its three browser jobs still ran the train's suites on
     the same code. **So the browser suites too** (the same day): where the
     base is a run's commit and what changed since is records alone (the
     documents and `scripts/ci_seconds.json`), they are not run again; any
     error runs them (`verified_base.py --browsers`; a test, two mutants). A track branched from a train not yet landed is verified
     with the next train, not pushed alone: its push plans from master's
     merge base, which re-runs the unlanded train. And a track whose own
     run would plan most of what the train's will (a program most recipes
     read changed) goes on with its local `just ci` and its mutation
     controls green.
   - **The integrator's own small tracks ride a train** (the integrator's,
     2026-10-13, train-4): `arm-members`, `variant-args`, `flight-wait` and
     `plan-shell` went on with no run of their own, each with its local
     `just ci` and its mutation controls green; the train's run verifies
     them with the rest. **Correction, the same day**: `variant-args` went
     on with its own tests and mutants green and no `just ci` of its own,
     and the train's `just ci` failed on another crate's test its change
     superseded (`handlers.rs`'s refusal of a case a browser could not
     send). The rule is the whole `just ci`, never the track's own tests
     alone.
   - **A shell script a recipe runs plans it** (the integrator's finding,
     2026-10-13). The planner read only the `scripts/*.py` a recipe's lines
     name: train-3's run, and the audit track's own, planned no `e8-host`
     for a change to `spikes/wasmtime-component/audit.sh`, the script the
     recipe is, which a dispatch then ran. A path to a `.sh` in a recipe's
     lines that the range changed plans the recipe, as a changed browser
     spec does (`ci_plan.py`'s `reads_what_changed`; a test in
     `test_ci_scripts.py`, a mutant in `dealt_by_time_mutations.py`).
   - **A push runs no recipe whose committed evidence is current** (the
     integrator's, 2026-10-13 (6)). Master's push after train-4 planned the
     whole train again (verify 38494831583, cancelled by hand): the train's
     run had failed in one recipe, fixed and run again by dispatch, so no
     push's run passed on its line, and a dispatch does not count. The
     record was in the tree already: each evidence file names the commit a
     run passed its recipe at. A push now plans as before, less each recipe
     whose every evidence file, as the head holds it, was recorded at one
     whole commit the head descends from, where nothing the recipe reads
     changed since, by the planner's own rule (`ci_plan.py --current`).
     Replayed on that push: 52 recipes without, none with. So a train whose
     run failed takes the run's evidence with its fix, and its push runs
     what the evidence does not cover, the failed recipe and what the fix
     reaches: no dispatch and no cancel. Evidence written from a tree with
     changes records no commit and never counts; the nightly runs every
     recipe, whatever its evidence says, for what the toolchain or the
     runners moved.
     **Found at train-5's landing (2026-10-14)**: a recipe whose own lines
     name a program (`e10-build`'s `examples/store/*.pw`, 50 recipes in
     all) was planned by no change to it, only by its own lines', its
     script's or its tests', so its evidence went stale unsaid:
     `e10-build`'s, recorded at `3fdcf929`, still said `place_order`
     imports `user-orders#place` after ADR-0332. And `--current` would
     have called it current. A program a recipe's own lines name, a glob
     read segment by segment as a shell expands one, or a path and what is
     under it, plans the recipe now (`body_inputs`).
   - **A train is numbered before its run** (the integrator's, 2026-11-10,
     at trains 18 and 19). Numbered after its run's fetch, as the rule above
     has it, train-17 had its numbers written into the comments of what most
     recipes read, the runtime's and the browser specs' among them: six
     recipes' evidence stayed current, and its pull request's run verified
     the train again, 24 shards. Numbers are the integrator's to give, so it
     numbers a train's ADRs before the train's run, and the run records the
     tree the pull request merges; what follows it is documents and
     evidence (the ADRs' results, STATUS, NEXT, the fetched files), which
     plan no recipe. Trains 18 and 19 are the first numbered so. A track
     that lands before a numbered train takes the number after the train's.
   - That is a file a recorded command produced, as CLAUDE.md asks.
   - The fetch is a download, so it waits for the owner's yes in the
     integrator's session. Approval relayed through another session does
     not grant it.
   - Until the owner says yes, a merge cites its run, and a gate's claim
     waits for its fetched file.
5. **What CI cannot run stays local, and says so.** These are recorded on
   the development machine, each recipe naming its machine:
   - measurements of the machine: `LOCAL_ONLY`'s timings, loads and memory
     curves, and the long-frame instrument;
   - behaviour that is macOS's own;
   - anything that needs the owner's local data. Kiokun's whole dictionary
     is read through `KIOKUN_DATA`, and on CI its recipes run on a
     committed sample, saying they skipped the rest.

## Acceptance

- **`scripts/tests/test_ci_scripts.py`, 23 tests.** Among them, one new on
  this ruling: a changed browser spec reaches the scripts that run it
  (`accessibility_mutations.py`), and the recipes that do
  (`e14-accessibility` and `e14-titles`). A spec that nothing runs reaches
  nothing.
- **The first merges under it**: ADR-0280's and the soundness ruling's,
  each naming its verification run.
- **Amended 2026-10-12**: three tests in `test_ci_scripts.py` (a file
  whose recipe reads what changed since its run is refused, and one
  skipped is not copied; what changed is the planner's, from the run's
  commit to the tree here; the tree here is what is staged and what is
  not), and five mutants in `scripts/dealt_by_time_mutations.py`, each
  killed (`just e14-dealt-by-time`).
- **Amended 2026-10-13 (master plans from what a run passed at)**: seven
  tests in `test_ci_scripts.py`'s `VerifiedBase`: the newest commit a run
  passed at is the base; none, the commit before the push; a pushed commit
  a run passed at plans nothing again; a dispatch's run or one that did not
  pass does not count, the nightly's does; a merge's second parent does not
  count; an error, or a commit git does not know, plans from the commit
  before; and the runs are read through `gh` by their commit. Eight
  mutants in `scripts/dealt_by_time_mutations.py`, 30 of 30 killed (`just
  e14-dealt-by-time`). The workflow's lines are exercised by master's next
  push, whose plan step names its base.

## Not claimed

- **CI's runners are not the development machine.** They run Linux, where
  this machine runs macOS, and their WebKit is Playwright's Linux build.
  What only macOS shows is local, under decision 5.
- **A flaky test can pass a run.** NEXT names each flake that is still
  open, and a merge names any it met.
- **Until the owner says yes to `evidence-fetch`**, `docs/evidence/` holds
  only what was recorded here.

## Alternatives

- **Keep the local chains.** That costs hours a merge, and a machine too
  loaded to build on.
- **Run the local chains in parallel worktrees.** Each worktree needs a
  target directory of its own, and the disk and the memory were already
  short.
- **Merge on `just ci` alone.** No mutation control would run before a
  merge, so a test that kills nothing could land.
