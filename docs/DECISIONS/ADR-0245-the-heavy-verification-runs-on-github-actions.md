# ADR-0245: the heavy verification runs on GitHub Actions

Status: accepted under the owner's delegation of 2026-10-02, on the owner's
direction to move the heavy verification off the laptop (relayed
2026-10-07). Date: 2026-10-07. Milestone: E14.

## Context

- **A commit's verification was serial, and on one machine.** After `just
  ci`, the chain ran each recipe the commit touched, each re-running its
  tests and its mutation controls one mutant at a time: ADR-0239's chain
  ran 39 recipes, for hours. An ADR's own controls and the scripts near its
  change took hours more: ADR-0243's were 27 mutants, then 99. While they
  ran, the worktree was theirs, and ADR-0244's 21 scripts were not run.
- **Master's CI went unread for two days** (ADR-0244).
- **Charter §13.5 asks for Linux CI**, "to catch casing errors". The
  recipes had only run on macOS.
- **The repository is public**, and GitHub Actions is free there on
  standard runners: "GitHub Actions usage is free for self-hosted runners
  and for public repositories that use standard GitHub-hosted runners"
  ([billing](https://docs.github.com/en/billing/managing-billing-for-your-products/managing-billing-for-github-actions/about-billing-for-github-actions)).
  The Free plan runs 20 jobs at once, each for at most 6 hours, a matrix of
  at most 256 ([limits](https://docs.github.com/en/actions/reference/limits)).
- **The whole set is large**: 208 evidence recipes plant about 1,900
  mutants, each a build and a test run.

## Decision

1. **A `verify` workflow** (`.github/workflows/verify.yml`): a plan, the
   recipe shards, the browser suite in each engine, and a summary.
   - **A push to master** runs the recipes its commits touch, the chain's
     choice, in up to 16 shards;
   - **a nightly run** runs every one, in 48;
   - **on demand**, named recipes, or every one.
2. **The plan** (`scripts/ci_plan.py`, `just verify-plan`) takes `just`'s
   own recipes, every `e<N>-…` one but the measurements of a machine
   (`LOCAL_ONLY`: a time, a load, a memory curve). A range of commits
   touches a recipe that runs a mutation script it changed, whose mutant
   sits within 30 lines of a changed line, or whose tests it changed, and a
   recipe whose own lines it changed. Recipes are dealt to the least loaded
   shard, the costliest first, by the mutants each plants.
3. **A shard** bootstraps the pinned toolchains, installs the browsers,
   builds, and runs its recipes one after another (`scripts/ci_recipes.py`).
   Each recipe's exit status, time, output, and the evidence files it wrote
   (those whose bytes changed while it ran) are the shard's artifact.
4. **The summary** (`scripts/ci_summary.py`) fails the run where a recipe
   failed, a mutant survived, or a shard reported nothing.
5. **Evidence is fetched by a recorded command**: `just evidence-fetch
   <run>` (`scripts/evidence_fetch.py`) copies a successful run's evidence
   into `docs/evidence/`. A file with a `commit:` line gains a `ci:` line
   after it naming the run and its runner, so it says which command produced
   it, at which commit, and where. A file naming another commit is refused,
   and so is a run of another commit than `HEAD`. The rule that evidence is
   "a file under `docs/evidence/` that a recorded command produced" holds as
   written: the recipe produced it, on the run, and the recorded fetch
   brings it here.
6. **Locally, before each commit**: the precommit gates, the workspace, the
   ADR's own mutation controls and the browser specs it touches. The chain
   runs on the push.

## Acceptance

- **`scripts/tests/test_ci_scripts.py`, 14 tests**:
  - a plan deals the costliest first, the same every time, and leaves out
    the measurements;
  - a change reaches the script it is, a test file a script runs, and a
    mutant near it, and not one far from every anchor;
  - a recipe wrote what changed while it ran;
  - a run fails for a failed recipe, a survivor or a missing shard;
  - the fetch names the run, replaces its own line, refuses another commit,
    and copies a file with no commit as it is.

  `just evidence-gates` runs them, with the gates' own.
- **The first runs**: RUNS.

## Not claimed

- **The measurements** stay on the development machine, which each names in
  its `host:` line.
- **A run commits nothing.** Its evidence is fetched here and committed
  after reading the run.
- **A push's plan covers its own commits.** A run is not cancelled for a
  later push, and the nightly run covers every recipe.

## Found by the first run, 2026-10-07

- **`rust-toolchain.toml` named one of the two WebAssembly targets the build
  uses.** `run.sh` builds the browser's WebAssembly for
  `wasm32-unknown-unknown`, which the development machine had installed by
  hand; every job of the first run failed building it. The toolchain file
  names it now, so rustup installs it wherever the toolchain is.
- **A runner's disk is about 14 GB free, and a shard ran out.** Each mutant
  a recipe plants builds its tests again under a new hash, as on the
  development machine (where the disk filled the same day). The second run's
  shard 7 was stopped by the runner mid-recipe, after two that passed. Each
  heavy job now deletes what the runner ships and nothing here uses (.NET,
  Android, GHC, CodeQL), and a shard frees, between recipes, the object
  files, the incremental cache and each test binary but the newest of its
  name, and records the disk left after each.

## Amended 2026-11-18: a campaign's bundle is kept while a record names it

Three recipes write their campaign into a bundle, a directory of its own
made by `mktemp -d docs/evidence/E14/<name>.XXXXXX`, so that a run never
writes over the last one's files while it runs:
`e14-session-command-authority` (`just/session-command-authority.just:8`),
`e14-session-receipt-recovery` and `e14-upload-authority`. Each recipe's
evidence file names its bundle (`campaign: …/report.json`), and the receipt
campaign's report names the session campaign it read. A fetch copied each
run's bundle in and removed none. On 2026-11-18 master held 22 bundles,
2,218 files, and records named four.

**Decision.**

- A bundle is kept while a record names it: a file under `docs/` outside
  the bundles, or a file of a bundle so kept, transitively.
  `scripts/evidence_bundles.py` reads each bundle's template from the
  recipes' own `mktemp -d` (the justfile and every file under `just/`), so
  a campaign recipe added later is held too.
- `evidence_fetch.py` prunes once it has copied a run's evidence: the
  bundles that evidence superseded go, and the fetch names each.
- The repository is held to it by its own test
  (`test_the_repository_holds_no_unnamed_bundle`, in `evidence-gates`'
  suite), and `--check` and `--prune` serve a tree a recipe ran in locally.
- The 18 bundles no record named are removed. Git keeps them in history;
  none was a gate's evidence.

**Not chosen.**

- **A fixed directory a run writes over**: a failed run would leave half
  of its files over the last good run's.
- **Pruning in the recipes**: a recipe would remove what a record still
  names, such as the session bundle the current receipt campaign read.

**Acceptance.**

- `scripts/tests/test_evidence_bundles.py`, nine tests, each failing on the
  code before it. The repository's own test failed on master's 18 bundles.
  The fetch's end-to-end test failed with its prune taken out.
- `scripts/evidence_bundles_mutations.py`, nine mutants, each killed
  locally (`just e14-evidence-bundles` records them):
  - an unnamed bundle kept;
  - the check passing one;
  - a bundle named through a kept one removed;
  - a superseded bundle's files keeping what they name;
  - a record outside the evidence naming none;
  - a bundle as any directory its template begins;
  - an imported recipe naming no template;
  - a fetch pruning nothing;
  - a fetch pruning before it copies.
