# ADR-0290: a verification run is dealt by the seconds its recipes last took

Status: accepted under the owner's delegation of 2026-10-02, on a finding of
the integrator's of 2026-10-09 (the nightly cancelled). Date: 2026-10-09.
Milestone: E14; the verification run (ADR-0245, ADR-0249, ADR-0278,
ADR-0281). Amends ADR-0249's shards of a kind and ADR-0278's shard each.

## Context

- **The nightly of 2026-10-08 did not finish** (run 37785652028). Two of its
  17 recipe shards were cancelled at the job's 345 minutes, one with 6 of
  its 12 recipes run, the other with 9 of 12, while one ended after 41
  minutes. Its recipes took 38.7 runner-hours in all.
- **The plan dealt by mutants.** A recipe's cost was itself and the mutants
  its scripts plant. Over the 213 recipes that night timed, that count and
  the recipe's seconds correlate at 0.43: a mutant that runs one test file
  and one that runs a crate's 1,316 tests counted alike. The plan replayed
  on the seconds measured puts 437 minutes in its longest shard.
- **Shards of a kind kept apart** (ADR-0249): a recipe that drives a browser
  shared shards only with its kind, and so did one that reads the build,
  and one that needs neither. That night, setting up took a plain shard 0.6
  to 1.0 minutes before its first recipe, and one that builds, or installs
  the browsers and builds, 2.5 to 6.5: small beside an hour's imbalance.
- **A shard each for the recipes run against a database** (ADR-0278): the
  five there are now would hold five shards of seventeen. The three that
  ran that night took 12 to 29 minutes each.
- **Each shard already writes each recipe's seconds** to its `results.json`
  as the recipe ends, and the run uploads it with the evidence.

## Decision

1. **A recipe's cost is the seconds it last took**, in a run that ran it to
   its end (exit 0), kept in `scripts/ci_seconds.json`.
   - Every evidence fetch keeps the seconds of each recipe its run ran
     (`evidence_fetch.py`), over the ones kept before; every other recipe's
     stay as they were.
   - `evidence_fetch.py <run> --times-only` keeps them from any completed
     run, one that failed or was cancelled among them, and copies no
     evidence: a recipe's time is what it took, whatever its shard's others
     did. The file starts from the nightly above: 203 recipes.
   - A recipe not yet measured costs itself and its mutants, each at the
     median of what a measured recipe took one (24 seconds that night,
     `RATE` where none is measured).
2. **Each recipe, the longest first, goes to the shard where it ends
   soonest**, what it adds to that shard's setup counted (`SETUP`, that
   night's measure). A shard still installs browsers, or builds, only for a
   recipe dealt it that needs them (ADR-0249), and a recipe that needs less
   may run where more is set up.
3. **The recipes run against a database keep shards of their own**, each
   with a PostgreSQL of its own (ADR-0278), and no other recipe runs there:
   such a shard names its database to every recipe it runs, so a recipe's
   evidence would depend on where it was dealt. They take as many shards as
   end the run soonest, the fewest of those.

## Acceptance

- **`scripts/tests/test_ci_scripts.py`, 34 tests.** New or rewritten on
  this ruling:
  - a recipe costs the seconds it last took, and one not yet measured, its
    mutants at the measured median;
  - a run is planned by the seconds kept: of three recipes in two shards,
    whichever took longest is alone;
  - a recipe needing less may run where more is set up, and what a recipe
    adds to a shard's setup counts;
  - the recipes run against a database take the shards that end the run
    soonest: long ones a shard each, short ones one between them, the
    fewest where more would end the run no sooner;
  - a recipe that ran to its end has its seconds kept, over its last, every
    other recipe's as it was, from each shard and from a lone shard.
- **`scripts/dealt_by_time_mutations.py`, 8 mutants**, 8 killed here.
- **The nightly replayed** on the seconds kept: 17 shards, sixteen ending
  at about 152 minutes and the database's at 87, where its own plan put
  437 in one.
- **The first nightly dealt by it**, named in NEXT when it has run.

## Not claimed

- **A recipe's seconds are its last run's**, not a mean. A shared runner's
  speed varies from run to run, and a change that adds a mutant is dealt
  by the time before it until the recipe runs again.
- **`SETUP` is one night's measure**, as `RATE` is.
- **The run's length is not bounded.** Nothing here stops the recipes from
  outgrowing 17 shards of 345 minutes; it puts that off until the
  recipes take about 97 runner-hours.

## Alternatives

- **Weigh mutants by kind** (a browser recipe's more): a fit to one night,
  and the time of a mutant still depends on the tests it runs.
- **Read the seconds at plan time** from the last nightly's artifacts: the
  plan would need the API and artifacts within their retention, and two
  runs of one commit could be planned apart. The kept file is read the same
  by every run of a commit.
- **More shards**: 17 recipe shards and the three browser jobs are the Free
  plan's 20 at once.
- **Any recipe in a database's shard**: see decision 3.

## Amended, 2026-10-09: a recipe not yet measured, at its kind's upper quartile

Found by `e14-refusal`'s first run (verify 37942903849): planned at the
median, 24 s a mutant, 31 units, about 12 minutes, it took about 420 s a
mutant. It was dealt after three other recipes, one of which, `e14-not-found`,
took 7,012 s where 4,260 were kept, and the shard reached its 345-minute
bound with 27 of the 30 mutants killed and none surviving. The run was lost
whole for want of a better first guess.

What a unit costs, over the 233 recipes kept that day, spans two orders of
magnitude: a median of 25 s, an upper quartile of 109, 407 at the ninetieth
percentile and 1,040 at most. By what a recipe's work runs, its kind: 144
`core` recipes, median 14 s a unit and upper quartile 27; 25 `host`
recipes, 287 and 483; 64 `browser` recipes, 142 and 408.

- **A recipe not yet measured costs its units at the upper quartile of the
  measured recipes of its kind** (`ci_plan.kind`): `browser` where it, or a
  mutation script it runs, drives Playwright; `host` where they run the
  development server's tests; `core` otherwise. Where none of its kind is
  measured, every recipe's upper quartile; where none is, `RATE`.
- **The upper quartile, not the median**, since the two errors do not cost
  the same: a recipe planned short runs its shard past the bound and the
  run is made again whole, hours; one planned long ends its shard early.
  `e14-refusal` at its kind's quartile is 31 × 408 s, 3.5 hours, what it
  took.
- **Not the alternative this ADR refused**, mutants weighed by kind for
  every recipe: a measured recipe still costs the seconds it last took, and
  the kinds' rates are read from the kept seconds at each plan, not fitted
  to one night.

`scripts/tests/test_ci_scripts.py`:
`test_a_recipe_not_yet_measured_costs_what_its_kind_does`.

## Amended, 2026-10-10: a run is dealt so that no shard ends past a ceiling under its limit

Found by `track/command-order`'s verify 38015617597: its shard 7 was dealt
13,959 s, its two longest recipes then took twice what they last had
(`e14-connection-faults` 10,364 s, `e14-invariants` 9,474 s: mutants that
hang under the command order's wait, killed at their bound), and the shard was
stopped at verify.yml's `timeout-minutes`, 345 minutes. A stopped shard is a
recipe shard that failed: nothing of the run is fetched (ADR-0281), and it is
made again whole. And `track/soft-navigation`'s verify was dealt 21,900 s into
five of its nine shards, past the limit before any overran: a track is dealt
into 9 shards (ADR-0253), and a track that touches the runtime now reaches
recipes of 184,111 s.

- **No shard of more than one recipe is dealt past `CEILING`**, 13,800 s, two
  thirds of the limit: measured shards ended up to half again past their
  recorded seconds. Where a run's asked number of shards would deal one past
  it, the run is dealt into the fewest more that do not, up to `MOST`, 40.
- **A recipe longer than the ceiling by itself** is dealt a shard of its own
  and let be: no number of shards ends it sooner. `e14-command-answers` took
  15,426 s and `e14-cart-lines` 14,021 in that run; each is the mutation
  narrowing's to shorten (NEXT, infrastructure).
- **The jobs past the Free plan's 20 wait for one to end**, which costs a run
  less than a stopped shard does. `track/soft-navigation`'s recipes, dealt
  again: 14 shards, two of one long recipe each, the rest within 13,637 s.
- The ceiling is held under the limit by a test that reads verify.yml.

`scripts/tests/test_ci_scripts.py`: four tests;
`scripts/dealt_by_time_mutations.py`: four more mutants, 17 of 17.

### And again, 2026-10-11: half the limit

`track/store-accounts`' verify 38046546700, dealt within 13,800 s, ran its
shard 6's recipes 1.6 to 1.7 times their recorded seconds (`e14-availability`
12,125 s for 7,405): its server's tests had grown, and each mutant of a
script that runs them whole runs them all. Its shard was stopped at 345
minutes. A branch's recipes run longer than recorded until its own run
measures them, up to twice (command order's two recipes), so the ceiling is
half the limit, **10,350 s**. And `e14-kiokun-word` ran past 20,500 s by
itself on `track/kiokun`: no ceiling holds a recipe longer than the limit,
which is the mutation narrowing's (NEXT, infrastructure), W6's for that
script.

## Amended, 2026-10-16: a long singleton cannot hide another shard's ceiling

A private replay of `e2a4acd960c88144e3b40e7a6e9d9b8bd774ee1a` found
that `plan` minimized only the last shard's estimated finish. An irreducible
long singleton held that finish fixed. Taking more database shards could
therefore split an over-ceiling database bundle without changing the last
finish, but the fewest-database-shards tie retained the bundle. `planned`
kept asking for more shards up to forty without fixing it.

The three-recipe counterexample is a 100,000-second ordinary recipe and two
6,000-second database recipes: the singleton is allowed, but the shared
12,042-second database shard is not. Three shards can keep both database
recipes within 10,350 seconds. A pure replay of the frozen 285-recipe roster
and its unchanged recorded costs likewise found three over-ceiling bundles
in forty shards. The corrected plan takes thirty shards with none; both
plans' estimated last finish is 19,320 seconds. These are plan estimates,
not measured recipe outcomes or a claim that a future run will finish.

- Keep the earliest estimated last finish as the first objective.
- Among those deals, minimize the largest excess over `CEILING` in any
  shard of more than one recipe, counting its setup. Allowed long
  singletons contribute no excess to this tie.
- Among equal scores, retain the fewest database shards. In particular,
  every already-within-ceiling original tie keeps its original choice.
- `planned` still asks for the fewest additional shards whose selected
  deal is within the ceiling, up to `MOST`. This remains a deterministic
  greedy planner, not a claim of globally optimal bin packing.

The recipe selection, costs, `CEILING`, `MOST`, setup estimates, workflow
deadline and PostgreSQL isolation are unchanged. The cancelled master
nightly `38894164532` remains cancelled; progress in its log is not mutation
evidence and is not used here to rewrite `ci_seconds.json`.

Acceptance requires the new regressions failing against the original
planner and passing after the change, the complete CI-script tests, all
dealt-by-time controls with native results and original assertion witnesses,
and fresh default `just ci`. No acceptance outcome is asserted by this ADR
before those commands finish. Raw adoption records are retained separately.

### Two retained controls assert their API contracts explicitly

The retained control that removes `plan`'s early empty-list return raises
`ValueError` from `max([])`. The old empty-selection test reported an ERROR
before reaching `assertEqual`; that is not an original assertion kill.
A private native contrast retained that error and showed the distinction.

The same named test now calls the documented `plan([]) -> []` API under
both database domains, catches only `ValueError` from that call, and uses
an explicit `self.fail` assertion for violating that contract. Other
ordinary exceptions remain errors: the private RuntimeError negative
still reports ERROR. Native traceback context is retained. The control's
source transformation and the selected test-method inventory are unchanged.
The verified-base fallback test had the same issue: its intentionally
broken provider raises RuntimeError, and the retained control that narrows
the production exception handler let it escape before the fallback-value
assertion. The test now catches only RuntimeError around that provider call
and explicitly fails the documented fallback contract. The corrected
implementation remains green, and a private unrelated TypeError remains
an ERROR. The unknown-commit fallback check still runs on the green path.

The new whole run must prove these explicit assertions; the old ERROR
outcomes are never relabelled or credited retroactively. No fixture or
arbitrary exception is blanket-converted, and neither control is removed.

### Refusal tests state the nullable return contract before reading its text

The first strict whole49 attempt stopped at retained control23: removing
the moved-recipe rejection made `evidence_fetch.verdict` return None, and
the old substring assertion raised TypeError. The actual native ERROR is
retained as an error and earns no assertion-kill credit. A private native
contrast found the same issue in retained control27, the unowned-file
rejection. Both transformations and the complete49 roster remain unchanged.

The original named test now asserts that each required rejection returns
a string before inspecting that string. None remains the accepted return
for an eligible unchanged recipe; skipped values and every existing
content assertion stay unchanged. There is no exception handler or broad
error conversion. The private corrected neighbour passes; each old None
result becomes an explicit original `assertIsInstance` failure; an
unrelated TypeError still reports ERROR. These assertions express the
existing str-or-None public API contract and preserve the71-method suite.

The stopped whole is never relabelled GREEN. A new complete49 run on the
refined test source and a new defaultCI are required before integration.

### Merged on train-10 (the integrator's delegate, 2026-10-17)

Merged from `58e3797b` (`track/ci-planner-ceiling`, on train-9's
`e2a4acd9`), with no conflict: train-10's own changes to `ci_plan.py`
(stale evidence planned at a train's push, ADR-0344; publication by
rename, ADR-0349) are in other functions. The track's own runs: its
default `just ci` green, and `dealt_by_time_mutations.py` whole, 49 of 49
killed. On the merge, `test_ci_scripts.py` passes 79 of 79. Train-9's
verify (38909882781), dealt by the old rule, ran its longest shard for
5 h 27 min of the 345 minutes. Its local `dealt-by-time.txt` is not
taken: the file stays train-8's run's record until the train's verify
records it again.

### Amended at train-10's first verify (the integrator's ruling 2, 2026-10-17)

Train-10's verify 39054436657 was dealt by seconds far below what its
recipes took: shards 20 and 22 ran to the 345-minute limit and were
stopped (e14-stream-ack had run 10,419 s against 3,120 recorded,
e14-keepalive took 10,174 s against 3,072, e10-browser 4,045 s against
282, recorded from runs whose `|| true` hid failing suites), and
e14-refusal, alone in its shard, was stopped at 20,507 s.

- **The plan refuses a recipe recorded past a job's limit** (`LIMIT`,
  20,700 s, verify.yml's `timeout-minutes`), its shard's setup counted,
  before any shard runs, naming it (`past_the_limit`): a six-hour run must
  never be the first to find it. Such a recipe is split first (ADR-0302's
  amendment).
- **The run's seconds are kept, and what is only a floor says so.**
  `ci_seconds.json` holds the 196 recipes it ran to their end
  (`--times-only`), less `e14-scheduled-orders`' 0 (it skipped without a
  database, ADR-0347's note); and `_lower_bounds`, a note naming each
  recipe whose seconds are only what a stopped or failed run measured (or,
  for the refusal's two new parts, what that run's mutants project), and
  why. The plan deals them by those seconds; a fetch that records one run
  to its end takes it out of the note.
- **Held by** two tests in `test_ci_scripts.py` (82) and four controls in
  `dealt_by_time_mutations.py` (54): a recipe past the limit planned, its
  shard's setup not counted, a note read as a recipe's, a recorded run
  keeping its lower bound.

## Amendment, 2026-11-02: a pull request's run asks for 24 shards

- **Found.** The repository moved to the `studiokimeiga` organization on
  GitHub Team (2026-10-20), which runs 60 jobs at once, not the Free
  plan's 20 the shard numbers were chosen for. A pull request's run is now
  the merge's gate (PRs #63 onward), and it asked for 9 shards, the planner
  adding shards only where 9 would end past `CEILING` (2.9 h of recorded
  seconds, a shard's recipes then running up to twice theirs). PR #67's
  full run (149 recipes, verify 41476687327) was dealt into 16 shards, the
  longest of which ran 231 minutes: the merge waited nearly four hours,
  with most of the organization's 60 jobs unused.
- **Measured with the planner's own model**, on PR #72's plan (159 recipes,
  35.7 recorded recipe-hours, the slowest `e14-kiokun-word` at 1.54 h):
  9 asked, 2.63 h; 17, 2.25 h; 24, 1.66 h; 32, 40 and 48, 1.65 h each.
  Past 24 the run ends no sooner than its slowest recipe and its shard's
  setup.
- **Decision.** A pull request's run asks for 24 shards. With its three
  browser jobs, the plan and the summary that is 28 jobs, so two pull
  requests' whole runs fit the organization's 60 together. A track's push
  (9), the nightly (17), a dispatch (17 by default) and `CEILING` and
  `MOST` are unchanged: no merge waits on them.
- **Next.** The run is then bounded by its slowest recipes
  (`e14-kiokun-word`, `e14-page-policy`, `e14-computed-holes`,
  `e10-browser`, each one to two hours on a 4-vCPU runner): splitting their
  mutants across recipes, as ADR-0302 did for `e14-refusal`, is what makes
  it end sooner still.
