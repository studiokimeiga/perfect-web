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
evidence file names its bundle (`campaign: …/report.json`). A fetch copied
each run's bundle in and removed none: on 2026-11-18 master held 22
bundles, 2,218 files, and records named three.

**Decision.**

- **A bundle** is a directory named by a recipe's template and as many
  letters and digits as its `X`s. `scripts/evidence_bundles.py` reads the
  templates from the recipes' own `mktemp -d`, quoted or not, in the
  justfile and every file under `just/`, so a campaign recipe added later
  is held too. A directory under a template's name is the campaign's.
- **A bundle is kept while a record names it**: an evidence file's
  `campaign:` line, or a document's citation (a `.md` under `docs/`: an ADR,
  the status), outside every bundle.
- **Nothing else names one.** A campaign's report lists the untracked
  files it started beside (`working_tree`), other bundles among them, and
  reads none of them; another line of a text file could list a failed
  check's bundles.
- **Only a bundle git tracks can go.** One not yet committed is a run's
  here: its recipe makes it first and names it in its evidence file last,
  so a run still writing is never removed.
- **`evidence_fetch.py` prunes once it has copied a run's evidence**, and
  names each bundle it removes. A fetch that copied nothing, one refused,
  and one of times alone prune nothing.
- **The repository is held to it** by its own test
  (`test_the_repository_holds_no_tracked_bundle_no_record_names`, in
  `evidence-gates`' suite). `--check` and `--prune` serve a tree a recipe
  ran in.
- **The 19 bundles no record named are removed**, 1,925 files; git keeps
  them. Among them is `session-commands.0jQZtj`, committed as an earlier
  gate's evidence of record (`a02744aa`), which no record names now.

**Not chosen.**

- **A fixed directory a run writes over**: a failed run would leave half
  of its files over the last good run's.
- **Pruning in the recipes**: a recipe would remove what a record still
  names.
- **A bundle named by another bundle's files**: their mentions are the
  status of a checkout, not a reading.

**Acceptance.**

- `scripts/tests/test_evidence_bundles.py`, fourteen tests, each failing
  on the code before it where it holds new code. The repository's own test
  failed on master's 18 unnamed bundles. The fetch's end-to-end test
  failed with its prune taken out. Six tests failed on this amendment's
  first commit (`21cbc94a`), which its review repaired:
  - a run's untracked bundle;
  - a bundle's own files;
  - a text record's other lines;
  - the template's exact width and quoting;
  - a fetch that copied nothing.
- `scripts/evidence_bundles_mutations.py`, sixteen mutants, each killed
  locally; `just e14-evidence-bundles` records them in the pull request's
  verify:
  - an unnamed bundle kept, or the check passing one;
  - a bundle's own files keeping what they list;
  - any line of a text record naming one;
  - a document outside the evidence naming none;
  - an untracked bundle going;
  - a bundle as any directory its template begins, of more letters than
    its `X`s, or a file;
  - an imported recipe, or a quoted template, naming none;
  - a fetch pruning nothing, before it copies, having copied nothing, what
    it refused, or for times alone.

**Reviewed** at `21cbc94a` (AGENTS.md rule 4): no high finding, two medium
and five low, each repaired in the commit after it.

- **M1, a prune removed a campaign still running here**: its bundle is
  named only at its end. Repair: only tracked bundles go, and a fetch that
  copied nothing prunes nothing.
- **M2, bundles named by another bundle's files**: the only such mention
  was a report's `git status` listing, and a failed check's report would
  keep what it listed. Repair: only `campaign:` lines and documents name a
  bundle, and `session-commands.BNKtmJ`, kept that way, is removed.
- **Lows**:
  - a refused fetch, a fetch of times alone and a file named as a bundle
    had no test (each has one, and a mutant);
  - the repository's test ran in the mutation controls' suite (it is
    `evidence-gates`' alone);
  - a template's width and quoting;
  - the record's words ("none was a gate's evidence"; mutants "killed"
    before the verify recorded them);
  - an INVALID verdict `evidence_counts` could not read (it says `INVALID
    OR SURVIVED`).
