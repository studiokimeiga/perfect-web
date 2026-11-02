> Session authority follow-up snapshot, 2026-10-27: freeze the reviewed
> upload/receipt repairs and strict instruments; execute their whole exact-source
> controls and all reached CI/browser recipes; merge only after independent
> final-head review and successful published gates. The older milestone queue
> below retains its historical scope. See `diagnostics/session-followup-2026-10-27.md`.

# Next


## Local session continuation, 2026-10-26

The active unmerged slice binds synchronous initial renders and stream workers
to one immutable actor (ADR-0389). See the
[continuation record](diagnostics/session-readers-2026-10-26.md); historical
integration queues below are retained and must be reconciled against current
master before any landing.

1. Guard query-derived redirects/errors at their final response boundary,
   with a revocation regression and a valid neighboring response. Complete
   immutable-reader binding for existing-document rederivations and keyed reads,
   including stale install/last-document/timer teardown cases.
2. Make request-local admission results explicit; bind browser commands after
   waits/body reads, and fence current actor/session authority plus receipt recall
   inside the same business/platform transaction. Exercise the actual new
   PostgreSQL session migrations and two-host races specifically.
3. Finish scoped and affected legacy campaigns, actual browser/HTTP acceptance,
   canonical evidence, independent final review and exact-head hosted quality
   gates before publication or merge. The broader session branch remains open.

The next executable tasks, in order, with acceptance criteria. Charter §3.4.

## Current, 2026-10-22: train-13

**Train-13** (`track/train-13`) merges three tracks onto master `e7664d2e`,
each verified on GitHub at its own head. Its merged source `c57e5570` passed
verify [39857464106](https://github.com/studiokimeiga/perfect-web/actions/runs/39857464106); its CI [39857310130](https://github.com/studiokimeiga/perfect-web/actions/runs/39857310130) failed only at the
Node audit, which that day began to name the benchmark baseline's next
16.3.8 in three advisories, two of them critical. With the owner's
approval the baseline moved to 16.4.1 (ADR-0120, amended), and at
`5a4378fe` CI [39892760379](https://github.com/studiokimeiga/perfect-web/actions/runs/39892760379) and verify [39892771765](https://github.com/studiokimeiga/perfect-web/actions/runs/39892771765) passed. The
[acceptance record](diagnostics/train-13/acceptance-record-2026-10-22.md)
states what its evidence proves and what it does not.

- `command-deadline` ([ADR-0381](DECISIONS/ADR-0381-a-commands-send-is-answered-within-its-deadline.md)):
  each send of a command is answered within ten seconds, or given up and
  sent again as its `retry` allows; a send held for the one before it waits
  its turn.
- `press-modules` ([ADR-0382](DECISIONS/ADR-0382-a-page-loads-its-modules-within-the-deadline.md)):
  a page's on-demand modules load within the deadline, a failed load is not
  kept, and a press whose speculation cannot load is sent unspeculated.
- `e14-readiness`: the benchmark counts the lines of every file a run makes
  (Next.js's T01, T06, T08 and T11 were undercounted), and the E14-E
  [decision brief](milestones/E14-E-decision-brief.md).

In order:

1. **Land train-13**, as the owner approved on 2026-10-22: exact-commit CI
   and the automatic track verification at its head, then `master`
   fast-forwarded to it without rewriting history. Until it lands, CI's
   "licenses and advisories" fails on `master` and on every branch from it.
2. **The follow that once went unanswered** (`feed.spec.mjs:691` in
   Chromium, verify 39701638548, below): cause unknown; it passed fifty
   local runs and its diagnostic rerun on GitHub. Should it recur, read its
   kept trace first. A raised bound, or a rerun until green, is no repair
   ([PARALLEL](PARALLEL.md)).
3. **E14-E**: the owner's choices of models, budget and teaching method,
   from the decision brief; nothing paid runs before them. The Next.js
   stack's task controls (`e14-harness`) run only on the development
   machine, where its sandbox could not install offline when last tried
   (2026-10-21, corepack lacking pnpm 12.11.2); they have not run on 16.4.1.
4. **Device-local durability** (ADR-0366): specify a durable-acknowledgement
   boundary, or state a scope-limited guarantee, before changing the
   runtime; test it against real navigation and reload races with the
   privacy invariants held.

The local-only performance/load/memory and Kiokun checks remain separate
unproved work; keep their scope and the known backend/runtime limitations
explicit.

## Queued 2026-10-25: `upload-metadata`'s follow-ups

Landed by pull request
([ADR-0383](DECISIONS/ADR-0383-an-uploaded-image-is-kept-without-its-metadata.md));
the first two follow the integrator's answers to its questions. Each
waits behind the order above, and none blocks it.

1. **Every cargo call in a recipe or a script is `--locked`** (ADR-0383's
   Found and its question 4). `run.sh`, `feed.sh` and `just ci`'s `cargo
   test --workspace` call cargo without `--locked`, so a `Cargo.lock` out
   of cargo's order (as master's was from `7b7edea6` until PR #53,
   2026-10-25) is rewritten and leaves a fresh checkout dirty; a recipe
   recorded after that names "+ uncommitted changes", which `ci_plan.py`
   reads as no commit, so its evidence is never current. Pass `--locked`
   there, and add a gate, with its test and mutation control, that every
   cargo call in a recipe or a script is locked, and that finds at least
   as many calls as there are today, so that it cannot pass by finding
   none.
2. **Clean a deployment's already-stored images** (the production track;
   ADR-0383 §3 and its question 1): with uploads closed, rewrite each blob
   a post names, point its posts at the rewritten bytes and their size in
   one transaction, collect the old blob and have the CDN purge its
   address; a blob that does not parse is reported and left, and a second
   pass changes nothing. Due before a deployment keeps images; none does.
3. **Fallow's audit runs only on a pull request** (found landing
   ADR-0383): its new-only gate refuses a new JavaScript function of
   cyclomatic complexity 5 or more (a CRAP score of 30 at its threshold of
   30, no coverage being measured), and `just preflight` does not run it,
   so a track learns of it from its pull request's check. Running
   `fallow` 3.23.0 in preflight needs its download, the owner's to
   approve.

## After train-12's landing, 2026-10-21

Kept as it stood when train-13 was assembled; its first three items are
train-13's.

Train-12 landed on master at `e7664d2e` on 2026-10-21. Its record commit
passed exact-commit CI ([39567756677](https://github.com/studiokimeiga/perfect-web/actions/runs/39567756677),
[39575516309](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516309)
and, on master, [39584219053](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219053))
and automatic track verification
([39575516650](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516650):
`e10-browser`, the legacy `e8-wit`, `e10-component` and `e8-contracts`, and
the full Chromium, Firefox and WebKit jobs). The
[acceptance record](diagnostics/train-12/acceptance-record-2026-10-21.md)
states its proof limits. The promotion steps of the next section are done.

In order:

1. **Land track `command-deadline`** (ADR-0381), with the owner's approval.
   Verified at `bfda9ae5`: CI 39658029334 and verify 39658074296 (141
   reached recipes, `e14-pleris-controls`, three browser engines), evidence
   fetched at `973fbbcc`. What remains is exact-commit CI at the commit that
   lands, and the merge.
2. **Verify track `press-modules`** (`wip/press-modules`, ADR-0382, stacked
   on the above). Reproduced 2026-10-21 in Chromium: after one failed load
   of the store's speculation module, every later Add failed with the same
   error and the network was not asked again; the renderer's and the
   computed values' loaders kept their failures the same way; and a module
   that never loaded held its press and the presses after it. Repaired: a
   page's on-demand modules load within the deadline, a failed load is
   forgotten and asked for again under a new specifier, as a handler's is,
   and a press whose speculation cannot load is sent unspeculated. Its
   eight tests pass in three engines and its ten controls are killed, none
   refused (`e14-press-modules`, verify 39701638548).

   **Verify [39701638548](https://github.com/studiokimeiga/perfect-web/actions/runs/39701638548) at `11d7917c` failed**, in
   `browser firefox`, `browser webkit` and `summary` (CI
   [39701625541](https://github.com/studiokimeiga/perfect-web/actions/runs/39701625541) passed). Three of its 143 recipes
   failed. Open; named `--known` where the run's other 140 recipes'
   evidence was fetched, and the three failed recipes' files were not
   copied:
   - **Two of ADR-0381's tests, in Firefox and WebKit**
     (`e14-command-deadline`, the two of `e10-browser`'s three runs its log
     shows, and both browser jobs): they counted a command's given-up sends
     by Playwright's `requestfailed`, which Firefox and WebKit do not
     report for a request the page aborts while a route holds it
     ("Expected: 1, Received: 0"; "Expected: 3, Received: 0"). They pass
     in Chromium. **Repaired** in the tests, the runtime unchanged: each
     send is timed in the page, from its `fetch` to its rejection. Both
     pass in the three engines here; `command_deadline_mutations.py`, run
     whole, kills 18 of 18, none refused. Verified at `d57f18a1` (below).
   - **`feed.spec.mjs:691` in Chromium** (`e14-browser-host-map`: 1 failed,
     110 passed): "a follow shows before the server answers, and is the
     server's after" showed the follow's speculation, released its held
     request, and timed out waiting for the answer. Not reproduced here:
     40 serial runs of the test, and five runs of the recipe's three specs
     in three engines (111 tests each), passed, and five more pinned to
     four cores with two workers, as on a runner. Its one diagnostic rerun
     on GitHub, `e14-browser-host-map` at `d57f18a1` (below), passed.
     Cause unknown. Should it recur, its trace is kept (`kept-traces/`, the
     verify run's `traces-*` artifact): read that first.

   **Verified at `d57f18a1`** (2026-10-22): CI
   [39808460517](https://github.com/studiokimeiga/perfect-web/actions/runs/39808460517) passed x64, ARM and licenses; verify
   [39808499525](https://github.com/studiokimeiga/perfect-web/actions/runs/39808499525) passed the four recipes the repair
   reaches, each exiting 0 (`e10-browser`, its three runs each 1,312 passed with 20 skipped;
   `e14-browser-host-map`; `e14-command-deadline`; `e14-second-host`), and
   the three full browser jobs. Its first attempt failed before any test in
   `recipes 0` and `browser webkit`: installing `just` 1.58.0,
   `extractions/setup-crate` found "no release for just matching version
   specifier 1.58.0" in half a second, where the run's other five jobs found
   it; those two jobs ran again. Its evidence is fetched.
3. **E14-E, decision-ready** (`wip/e14-readiness`). The decision brief
   (`docs/milestones/E14-E-decision-brief.md`) and the changed-lines repair
   (Next.js's footprint was undercounted in T01, T06, T08 and T11) await
   hosted CI and the owner's choices of models, budget and teaching method;
   nothing paid runs before them.
4. **Device-local durability** (ADR-0366): specify a durable-acknowledgement
   boundary, or state a scope-limited guarantee, before changing the
   runtime; test it against real navigation and reload races with the
   privacy invariants held.

The local-only performance/load/memory and Kiokun checks remain separate
unproved work; keep their scope and the known backend/runtime limitations
explicit.

## Train-12 acceptance and promotion, 2026-10-21

Done (see above). The plan below is kept as it stood before the landing.

Source `ec15e5b1858b608d5a4c8aa39e00a0683c678505` passed
[CI 39558466827](https://github.com/studiokimeiga/perfect-web/actions/runs/39558466827)
on x64 and ARM, and all of
[verify 39558474966](https://github.com/studiokimeiga/perfect-web/actions/runs/39558474966).
The four reached recipes are recorded: TodoMVC 43/43, device storage 62/62,
second host 50/50, and three full-browser runs of 1,258 passed / 20 skipped.
The separate full Chromium, Firefox and WebKit jobs also passed. All native
inventories and strict producer checks remain intact. Earlier failed runs,
setup failures and invalid witnesses remain historical failures.

The [acceptance record](diagnostics/train-12/acceptance-record-2026-10-21.md)
reconciles the completed evidence cohorts and documents the proof limits.
Promotion is held to these checks in order:

1. The final documentation/evidence commit passes exact-commit CI.
2. Its automatic track verification passes, including the canonical legacy
   `e8-wit`, `e10-component`, `e8-contracts` obligations and full browsers.
3. The integrator rechecks master and promotes the verified tip without
   rewriting published history. The handoff records the final SHA and runs.

After promotion, choose the E14 benchmark models, budget and agent teaching
method with the owner before spending on benchmark runs. The local-only
performance/load/memory and Kiokun checks remain separate unproved work;
keep their scope and the known backend/runtime limitations explicit.

## Historical train-12 findings

The dated details below preserve the failures and obligations as originally
recorded. Their eligible replacements are reconciled above and in STATUS;
they are not a second list of outstanding reruns.

The earlier assembled repair candidate `34f3c2d1` began the first follow-up on `wip/train-12`.
[Verify39538607183](https://github.com/studiokimeiga/perfect-web/actions/runs/39538607183)
completed 155 of the 156 required recipes; the known-invalid graph campaign remains
an explicit separate obligation below. Exact-source
[CI39538597877](https://github.com/studiokimeiga/perfect-web/actions/runs/39538597877)
failed on both architectures at Clippy's `collapsible_if` lint in the generic
representation test; the preceding evidence gates passed. The semantics-preserving
let-chain repair `4623d304` passes targeted Clippy and its native regression.
Exact combined CI is still required. Chromium job119020991409 failed during
`setup-just` because GitHub's
release API returned HTTP502, before tests ran. Its first exact-job rerun was
refused while the workflow remained active. Retry when permitted, or record a
current successful full-browser follow-up; never count that setup failure as
browser evidence. The same run's `e14-guide-ownership`
recipe failed its 15-test baseline before any controls: the timeout fixture's
child/grandchild readiness file was absent when its 0.3-second deadline elapsed.
The repaired fixture gates execution deadlines on bounded native readiness
and cleans up failed startup using verified process identities. All 15 native
tests and 13 controls pass locally with independent review; its hosted whole
campaign remains due.
The same run's `e14-bind` completed its baseline but refused one of 14 controls:
five render-again tests panicked at an expected-build extraction. An explicit
valid-build assertion now gives a complete nine-test assertion failure under the
unchanged lowering fault; the restored nine pass. The whole campaign and its
reached consumers remain due.
The same run's `e14-computed-conditions` detected 11 of 12 controls and refused
the browser fault that omits a block's computed values: disabled-button clicks
timed out rather than producing valid assertion receipts. Preserve the fault
and complete 66-test inventory, repair its semantic witness, and rerun the whole
campaign. Ten valid-draft enabled-state assertions now preserve all 66 cases:
baseline and restored runs pass 66/66, while the unchanged fault completes
with 42 assertion failures and 24 passes, without action/request timeouts
([diagnostic](diagnostics/train-12/computed-browser-post-precondition.txt)).
All 20 reached feed consumers still require current hosted verification.
`e14-evidence-integrity` correctly rejected the earlier short result in the
same shard; it needs an explicit fresh run after that repair.

The same 155-recipe run exposed two further native failures.
`e14-check-is-build` now covers all 626 source files in 453 assemblies and
passes its 99-test baseline, but detects only 50/51 controls: the parsed WIT
failure loses its generated source identity without an assertion failing.
`e14-todomvc` passes its server/browser baselines and the separate 54/54
Cypress producer controls, but its application campaign detects 39/43 faults.
The four survivors concern empty toggle-all checked state, removing or setting
checked state during patching, and inserted edit-field autofocus. Repair the
witnesses and migrate its legacy timeout/build-failure kill logic to strict
completed native assertion receipts, preserving all 43 intended faults.
Intentional assertion transcripts in Cypress producer diagnostics also contain
reserved survivor markers; separate those raw diagnostics from accepted
evidence without weakening the canonical summary guard. Both full recipes
need current replacements; neither failed result is accepted.

The graph repair now retains all 19 operators and 25 native identities with strict
completed-assertion validation. Targeted renderer faults and the mutual-type
refusal produce complete assertions instead of a memory stop or native abort.
Its full campaign and `e14-cart-lines` are required on the repaired source.
The producer-ownership campaign's native nine-control record is valid, but its
bare witness names confused evidence-count ownership. Qualified metadata and
an exact-output regression repair that reporting contract without changing
any operator or parser; its hosted campaign also must be rerecorded.

1. Audit all 239 native recipe results from
   [verify 39526960864](https://github.com/studiokimeiga/perfect-web/actions/runs/39526960864)
   at `f15117de`, including producer status and memory stops. The completed
   census contains exactly 239 recipes:228 valid and11 invalid; summary is the
   only failed job. All three full browser jobs passed. A successful wrapper job does not override a
   failed native recipe, and an incomplete run is not accepted evidence.
2. Repair the failed `e14-todomvc` harness. Host tests passed 4/4 and the
   separate Playwright suite passed 150/150; the unchanged official Cypress
   files passed 24/29 with native status 3. Cypress 15.21.1's spy has
   `resetHistory`, not the invoked `reset`, and the custom command's final
   `then` leaves the persistence alias pointing to a detached element after
   reload. Preserve the seven pinned upstream blobs, stage an explicitly
   versioned two-patch compatibility copy, and keep every behavioral and
   storage assertion. Acceptance still requires all 29 cases, zero skips
   or failures, and native status zero. A native compatibility diagnostic
   passes 29/29; the combined recipe and its negative controls remain due.
3. Repair the failed `e14-device-store` fault-model witnesses. Its native
   baseline passed 15 core, 15 browser and 50 TodoMVC tests, but only 53 of
   62 mutation controls produced valid assertion failures; nine were
   refused and none survived. Compile errors, setup panics and browser
   timeouts do not prove a bug was caught. Preserve each intended fault,
   correct the invalid witness, and rerun the whole affected campaign.
4. Rerun `e14-check-is-build` after adding TodoMVC's normal program roots
   to its census. The original run covered 625/626 tracked source files
   and correctly refused the missing `examples/todomvc/app.pw`. Focused
   inventory validation now covers 626/626 across 453 assemblies, and the
   actual 23-file TodoMVC program checks clean. All 453 complete checks
   and the rest of the recipe remain to be rerun together.
5. Repair the surviving `e14-bind` control, "a binding's handler captures
   what it sets". The baseline passed eight tests, but only 13/14 controls
   were detected. Preserve the intended captured-value fault and prove the
   complete campaign; a green baseline is not sufficient.
6. Repair `e14-speculated-arms`: its core/parser/browser baselines passed,
   but only 7/8 controls were detected. The test of a page without speculation
   queried its old UI identity and accepted a missing lookup. Assert the
   compiled canonical page identity before checking that speculation is
   absent. Preserve all eight fault models, require complete native assertion
   receipts, and rerun every reached feed test consumer after instrumentation.
7. Repair the remaining failed controls: `e14-representations` detected
   3/4, and `e14-computed-conditions` detected 9/12. Their old witnesses or
   operators no longer reach the canonical type/computed-value paths. Preserve
   each intended fault, establish actual assertion failures and valid neighbors,
   and rerun the complete campaigns with strict producer validation.
8. Replace `e14-mutation-bound` and `e14-guide` evidence. The former correctly
   refused one of31 controls because another test read an absent output file
   instead of asserting its presence. The latter stopped before any control
   because one renamed operator was absent from its required-witness map.
   [Focused native probes](diagnostics/train-12/train12-catalog-and-bound-witnesses.txt)
   demonstrate the repairs; whole31/74 campaigns remain required. Tighten the
   guide producer to require actual assertions and complete inventories, while
   preserving its three explicitly named semantic checker refusals.
9. Repair the surviving `e14-browser-host-map` reserved-host control. Its
   25 fast, 17 CLI and 111 browser baseline cases passed, but the fast suite
   detected only 53/54 controls overall. The intended rejection test still
   passed when reserved host names were allowed. Preserve the host contract
   and all fault models; demonstrate the actual assertion failure and rerun
   the complete campaign.
10. Repair the invalid `e14-graphs-on-the-wire` campaign: native status zero
   concealed a memory guard stop for "the renderer takes a node twice". The
   canonical summary/fetch guard rejects the written evidence. Preserve the
   fault model, require a real completed assertion failure, and rerun all 19
   controls; a memory stop is never mutation credit.
11. Rerecord `e14-guide-ownership` on the current source: the post-dispatch
   Linux/procfs capability guard changes its proof scope. Unsupported hosts
   skip explicitly and cannot satisfy the strict mutation baseline.
12. Reconcile eligible current evidence under ADR-0281, run exact-commit CI
   and all reached follow-ups, then land only the verified combined tree.
   All ten failed recipe results and the memory-stopped graph campaign
   remain invalid and none is fetched as a pass. The effect queue passed its
   isolated exact-source CI and eight reached campaigns; its integrated
   source still needs the combined verification.

The older follow-ups below retain their dated context. A-008's WIT
collision, generic-result effects and producer-ownership repairs are now
on the unlanded candidate; their historical open entries are not current
claims that those repairs are absent.

## Integration follow-ups, 2026-10-16

- D1's local compiler integration gate is green (ADR-0350), on train-10.
  Integrate the completed generic-result effect repair and cache
  publication repair on train-11, preserving canonical expression types
  and generated bounded-read pairs; run the combined regression tests,
  applicable whole scripts and full CI.
- Continue D2's common memory adapter and TodoMVC integration. Preserve
  exact Result/Option answers, table ordering, atomic overflow/rollback and
  measured source-cost admission. A declared one_call promise is not an
  optimistic provider override; legacy cost conformance remains explicit.
- Complete guide producer exit validation and browser host/port isolation.
  Preserve the recorded negative witnesses, reject invalid producers and
  keep the remaining restored-browser cleanup gaps visible. Quiet-host E9
  (the check's latency recorded on a quiet machine) remains to be recorded.

- After the browser host map (train-11): revisit `e14-refusal`'s split in
  two (ADR-0302's amendment), and deal any script's mutants across shards
  (the parallel slots' allocator, `PW_MUTANT_PICK`). `e14-harness` runs on
  CI only once a runner's store holds its stacks' dependencies; it is
  local until then.

- **WebKitGTK's network process crashes on a navigation that cancels a
  page's just-started requests** (ADR-0280's amendment of 2026-10-18;
  KNOWN_LIMITATIONS). It is a use-after-free in libsoup 3.x. It reproduces
  with this program's server, master's included (3 of 750 navigations in a
  60-line script), and not with a plain Python server serving the same
  paths (0 of 1,200).
  - A track of its own: find what in our server's traffic trips it, and
    whether a newer Playwright WebKit build changes it.
  - Until then, in WebKit, the two specs that navigated right after a soft
    navigation let the page settle first.
  - The owner has an upstream report to file (the handoff's
    `tracks/train-10-webkit/upstream-report.md`); nothing is posted from
    here.
  - Its scripts and traces are beside the report.

- **A-008's generated WIT names an item twice** (a small track; train-11's
  verify 39442025240, ADR-0361): the complete check refuses
  `examples/accepted/A-008-streamed-public-recommendations.pw` with "the
  generated WIT does not parse: duplicate item named
  `store-recommendations-recommendations-api`". Find where the WIT
  generator names a streamed public query's interface, make the names
  unique, and take A-008 off `examples/accepted/build-refusals.json`, with a
  test and its control.
- **A Chromium timeout in `scheduled-orders.spec.mjs`, open** (train-10's
  third verify, 39131257549: `e10-browser` run 2 of 3, the test "an order
  for a time is paid now...", `page.waitForURL` past the test's 30 s).
  Once in about 12 Chromium runs of the test on CI, in no other engine,
  and in none of 40 on the PC; the dispatch after it passed. The spec now
  names its own failure in the run's log: its step, where the page was,
  the runtime's and the host's last lines (`pw-record`). At its next
  failure, read those lines and find the cause; it is not re-run until it
  passes.
- **Done on train-11** (STATUS): every committed component held to the build, and the planner
  reading a binary's change (ADR-0355); a failing browser run's trace kept (ADR-0359).
- ADR-0347: split scheduling's when_said from Checkout so an estimator
  outage degrades that line alone; offer delivery-streamed times. The
  current chosen-time checkout and times page both wait on the estimator.
- ADR-0348: the scheduled estimate N+1 is repaired by a set operation;
  continue the common per-adapter conformance suite and composite migration.

Train-9 landing gate, 2026-10-16: met. GitHub's `verify` 38909882781 and
`ci` 38909883157 passed at the exact head `e2a4acd9` (85 recipes, 1,391 of
1,391 mutants killed); its evidence was fetched and train-9 landed
2026-10-17, master `32757966`. The failed 38863242953 run stays failed.


Integration follow-up, 2026-10-16: reproduce Kiokun search-cache publication
and version-lifetime races with private fixtures, then repair them. Sample
test isolation does not prove production cache concurrency. Do not inspect
or modify the external dataset owned by another process.

## Current: E14, the AI benchmark (E10 closed 2026-10-02)

**2026-10-02, E10 closed and E14 started** ([ADR-0119](DECISIONS/ADR-0119-e10-closes-and-the-ai-benchmark-comes-next.md)).
E14 comes before E11-E13. Its plan, controls and task list are
[docs/milestones/E14.md](milestones/E14.md). In order:

1. ~~**E14-A, the store in Next.js and in SvelteKit.**~~ Done 2026-10-02
   (ADR-0120, `just e14-contract`). Feature parity with
   `examples/store`: the store heading, the menu, Add with an idempotent,
   optimistic, rolled-back cart update, Clear, a private cart beside a shared
   menu. One Playwright suite, run against all three stores, passes on each.
   Acceptance: the suite is green on three stacks, and a deliberately broken
   copy of each fails it (negative control). Framework versions verified
   against primary sources and pinned in `tools/versions.lock`.
2. ~~**E14-B, the harness, offline.**~~ Done 2026-10-02 (ADR-0123,
   ADR-0124, `just e14-harness`), with T08 as its pilot task. Copy a baseline into an isolated
   directory without `hidden/` or `reference/`, run an agent command, grade
   with the hidden tests and the stack's checkers, write one result JSON.
   Acceptance: the no-op agent scores 0 and the reference replay scores 1 on
   every task and stack; the harness refuses to run if a hidden file is
   readable from the sandbox.
3. **E14-C, tasks T01-T12**, each with a prompt, hidden tests, a reference
   patch and an unsafe patch per stack, and all four controls green. A task
   Pleris cannot express is recorded as such. All twelve are written. T12's
   Pleris wrong fix is refused since ADR-0128.
   - ~~**E14-L, a value's label.**~~ Done 2026-10-02 (ADR-0129).
   - ~~**Rulings: UI state, view composition, typed events.**~~ Done
     2026-10-02 (ADR-0130, ADR-0131).
   - ~~**Local signals on a page.**~~ Done 2026-10-02 (ADR-0133), with the
     resume decision knowing the build (ADR-0132).
   - ~~**Every handler is resumable.**~~ Done 2026-10-02 (ADR-0134).
   - ~~**View composition.**~~ Done 2026-10-02 (ADR-0136), ADR-0130's step 2.
   - ~~**What the browser renders again, it can.**~~ Done 2026-10-02
     (ADR-0137), a correction to ADR-0133.
   - ~~**A handler is given its event.**~~ Done 2026-10-02 (ADR-0138):
     event records, typed parameters, `|prevent` and `|stop`.
   - ~~**A page of queries holds signals; a modal dialog.**~~ Done 2026-10-02
     (ADR-0140, ADR-0141).
   - ~~**T11, an accessible dialog.**~~ Written 2026-10-02: all four
     controls hold on all three stacks (`just e14-harness T11`). Pleris's
     unsafe patch is refused by `pw check` (PW5303).
   - ~~**`bind:value`.**~~ Done 2026-10-02 (ADR-0142), for a `String`
     signal.
   - ~~**T06, an accessible form.**~~ Written 2026-10-02: all four controls
     hold on all three stacks (`just e14-harness T06`). Pleris's unsafe patch
     is refused by `pw check` (PW5014). The form needed no typed record: a
     `String` signal bound to its field, and a submit handler, express it.
   - ~~**Correction: what names a form control.**~~ Done 2026-10-02
     (ADR-0143). Writing T06 found that PW5014 took any `id` as a name, and
     refused a control wrapped in its label.
   - ~~**Provided signals.**~~ Done 2026-10-02 (ADR-0144), ADR-0130's step
     3:
     - a view's own signals, an instance per use;
     - a signal a page provides to the views it composes;
     - PW5305, PW5306 and PW5307.

     Demonstrated by `examples/demo/provide.pw` in three engines.
   - ~~**T03, a private resource.**~~ Written 2026-10-02, with E14-Q's third
     slice (ADR-0145): the server renders every list a session's queries
     fill, and keeps a page current by difference, in one frame per change.
     All four controls hold on all three stacks; Pleris's unsafe patch is
     refused by `pw check` (PW5001).
   - ~~**A block a query decides.**~~ Done 2026-10-02 (ADR-0146): planned,
     rendered from each binding's whole value, and sent again where its
     rendering changed.
   - ~~**T04, an order-state variant.**~~ Written 2026-10-02, with ADR-0147:
     all four controls hold on all three stacks. Pleris's unsafe patch is
     refused by `pw check` (PW0305).
   - ~~**A stream region.**~~ Done 2026-10-03 (ADR-0148): `<stream>` is
     compiled, and a query declared `delivery streamed` fills its region in
     the same response as the document, with the WHATWG's `<template for>`
     patch. PW5400-PW5402. The failed arm is given `Option` of the declared
     error. Demonstrated by `examples/demo/streamed.pw` in three engines, and
     natively in Chrome 154.
   - ~~**T05, streaming recommendations, and T10, a loading and an error
     state.**~~ Written 2026-10-03: all four controls hold on all three
     stacks for each. Pleris's unsafe patches are refused by `pw check`:
     T05's by PW5400, T10's by PW5401.
   - ~~**T09, a cache's freshness.**~~ Written 2026-10-03: all four
     controls hold on all three stacks. That makes ten tasks, which gate
     item 2 asks for.
     - The store's notice is kept five minutes, from an expensive board. A
       new notice must show within ten seconds, while the board is still
       asked at most once in ten seconds.
     - Every stack's unsafe patch stops keeping the notice, and the hidden
       tests catch it, Pleris's included.
     - Writing it found two things. Any `revalidatePath` of a Next.js page
       expires the `unstable_cache` entries the page read. And the
       development server kept a materialized fragment past its query's
       value (ADR-0150).
   - ~~**T02, a duplicate request storm.**~~ Written 2026-10-03: all four
     controls hold on all three stacks, eleven tasks. In Pleris the fix is
     `concurrency one_per_key`; in the frameworks, a map of asks under way.
     Each stack's unsafe patch is caught by the hidden tests. Writing it
     found that the development server read each page while holding its
     subscriber table, so no two pages ever shared a flight (ADR-0151).
   - ~~**Gate item 5.**~~ Done 2026-10-03 at `f68f723`
     (`just e14-unsafe-table`, `docs/milestones/E14.md`). Pleris's checker
     refuses nine of the eleven wrong fixes, and the frameworks' checkers
     none. The statement is made rule by rule, with what each rule leaves
     open.
   - ~~**T07, a stale navigation request, with the charter's §15.6 tests
     6-8.**~~ Done 2026-10-03 (ADR-0152). A page's query given a signal is
     read again for the new key. `on_key_change` decides the old key's work:
     `cancel` stops it, `supersede` lets it finish unshown, and `keep` reads
     the new key once it has finished. All four controls hold on all three
     stacks. All twelve tasks are written. Found on the way: presses ran in
     the order their code arrived.
   - ~~**The two rules gate item 5 points at.**~~ Done 2026-10-03:
     - ADR-0153: a template tests a case with `{#match}` (PW0337);
     - ADR-0154: a command a page's handler calls declares `idempotent_by`
       (PW0338). T08's setup no longer checks.
   - ~~**Keyed reads in three engines.**~~ Done 2026-10-03: the charter's
     store tests 6-8 in Chromium, Firefox and WebKit (`e2e/keyed.spec.mjs`).
   - ~~**The store against charter §15.**~~ Audited 2026-10-03
     (`docs/research/charter-15-store-audit.md`). Its three runtime defects
     are fixed (ADR-0155).
   - ~~**Item availability, end to end.**~~ Done 2026-10-03 (ADR-0157,
     `just e14-command-answers`), the audit's first gap. A handler is
     answered what its command did, `Ok` or the declared error, never the
     value. The store refuses an item sold out since the page was rendered,
     and says so (§15.6 test 10). PW0339 and PW0620.
   - ~~**A handler that drops its command's declared error.**~~ Done
     2026-10-03 (ADR-0159, `just e14-handler-failures`). PW0618 refuses a
     `Result` a handler gives the runtime, whether as its last value, by
     `return` or by `?`. A handler matches the answer, or discards it by name.
     The benchmark's store discards by name, with T11 re-based.
   - **Next: the audit's remaining gaps, in its order.**
     1. ~~The route `/stores/{id}`, and more than one store.~~ Done
        2026-10-03:
        - ~~The compiler's half~~ (ADR-0160): the plan carries the route,
          and PW0340, PW0621 and PW0341 hold it to its page.
        - ~~Each document its own subscriber~~ (ADR-0161): a second tab
          no longer clears what the first was waiting for.
        - ~~The store served at its route, beside a second store~~
          (ADR-0162), with §15.6 test 11 end to end.
        - ~~A page says when it is absent~~ (ADR-0163,
          `just e14-not-found`): `not_found_on StoreError.NotFound`, held
          by PW0342, and `/stores/999` answered 404.
        - ~~Correction: a menu change dropped every store's kept menu~~
          (ADR-0164, `just e14-menu-changed`). It is the event
          `MenuChanged(47)` now, which the store's `Menu` declares.
     2. ~~The recommendation and estimate slots in the store itself~~
        (ADR-0165, `just e14-slots`): tests 3 and 17 hold in Chromium and
        Firefox. T05 and T10 needed no re-basing (ADR-0156).
     3. ~~§15.1's descriptions~~ (ADR-0166, `just e14-descriptions`):
        the store and its items say what they are, a host's answer is read
        through the program's own types, and WebKit paints the store before
        its slots. The browser suite refuses a build whose sources changed
        (a correction to ADR-0165).
        - ~~Refuse a `//` line inside markup~~ (ADR-0167,
          `just e14-markup-comments`). It found more: markup was read as
          code, so URLs in text and unquoted attribute values mis-built.
          Markup text, `<!-- -->` and unquoted values are read as HTML reads
          them; PW5028 refuses a line that reads as a comment.
        - ~~Each Add named by its item, the count in a live region~~
          (ADR-0168, `just e14-instance-changes`): a change now reaches every
          part that reads it, attributes included. Found on the way: E7-P
          scrambled its list on a move to where an item already was.
        - ~~Money shown as text~~ (ADR-0169, `just e14-row-reads`): each
          menu row shows its item's price, which a host computes for the row
          through `display`. Every other member read no host computes is a
          build refusal. Until 2026-10-03 the plan neither planned nor
          refused one, and a page that made one built, then failed to render.
     4. ~~Decrement, remove, and a per-line cart.~~ Done 2026-10-04
        (ADR-0172, `just e14-cart-lines`): each line's name, − and +, total
        and Remove, the cart's subtotal, optimistic and idempotent, in three
        engines.
        - ~~A loop over a list inside a query's value~~ (ADR-0170,
          `just e14-nested-lists`): `cart.lines` by its path, its rows'
          member reads computed, and a part a speculation would not reach
          refused.
        - ~~An attribute at the top that reads a query's value~~
          (ADR-0171, `just e14-query-attributes`): set again when the value
          changes. Found designing the cart's empty message.
        - ~~A speculation reaches every part that reads it, and the cart's
          lines~~ (ADR-0172): the browser renders a loop, a block or an
          attribute again from the speculated value, rows set where they
          are. Corrections: a press shown twice when its value came before
          its answer, and two changes of one session sent at once.
     5. ~~A command retried on a transport failure~~ (ADR-0173,
        `just e14-command-retry`): sent again with the same interaction
        where no answer came, as its `retry` clause bounds it, in three
        engines. Correction: PW0312 let `transport_only` retry a command
        that is not idempotent.
     6. **§15.5's missing controls.**
        - ~~A store delay, a cart delay and a one-shot database error~~
          (ADR-0174, `just e14-test-controls`). Correction: the one test
          of a rolled-back command could not fail.
        - ~~A one-shot network error the server makes, and a forced
          reconnect~~ (ADR-0175, `just e14-connection-faults`): a press
          survives either drop as one mutation, and a page cut off hears
          what changed meanwhile, in three engines.
        - ~~A materializer failure, end to end~~ (ADR-0176,
          `just e14-materializer-failure`). Correction: a failed
          regeneration left a page a line short, silently.
     7. ~~Last-known-good for declared public data~~ (ADR-0177,
        `just e14-last-known-good`): PW0343, the query runtime's fallback for
        public data alone, and the store's page shown with its last store
        kept while its origin fails, in three engines.
     8. **Next: §15.1's remaining fields.**
        - ~~`MenuItem.available`, shown before the press, and told~~
          (ADR-0178, `just e14-availability`): a sold-out row has no Add,
          and a stock change reaches every page open, in three engines.
          Correction: a new version of the shared menu could leave the
          pages open a version behind.
        - ~~`PositiveInt` checked where a request gives one~~ (ADR-0179,
          `just e14-invariants`): an opaque type states its invariant,
          every construction is shown to hold it at build (PW0622), and the
          host checks a command's arguments and a data layer's answers. A
          forged quantity of 0 is refused, in three engines.
        - ~~`DeliveryEstimate`'s range~~ (ADR-0180, `just
          e14-estimate-range`): `min_minutes` and `max_minutes`, each a
          `PositiveInt`, and `generated_at`, an `Instant`; the slot says
          the range in words, in three engines.
        - ~~`MenuItem.category` and `store_id`, and `Store.menu_version`~~
          (ADR-0181, `just e14-menu-categories`): the menu is grouped by
          category, a list inside a row changed where it is, and every menu
          change derived from the menu's values, in three engines. A menu's
          version and a cart's identity stay the runtime's.
     9. ~~Accessibility (test 14)~~ (ADR-0182, `just e14-accessibility`):
        the store is read by axe's and WCAG 2.2 AA's rules in three engines,
        as served and after each kind of change. The spec also tests the
        keyboard, the tree a screen reader reads, live regions, reflow, a
        phone's width and reduced motion. It found that one Add said its
        count up to four times, and that no page had a viewport.
        - ~~A page states its title~~ (ADR-0183, `just e14-titles`):
          `<title>` at the top of a page's view, written into the head and
          set again as `document.title`; PW5029 and PW5030; corpus C9. The
          store's title is its name, in three engines.
    10. ~~Tests 2 and 13 against the running store's shared output~~
        (ADR-0184, `just e14-shared-output`). Found: the store's page went
        out with no `Cache-Control`, and a fresh session's cookie went on a
        build's files. A session's response says `private, no-store`, a
        build's file names no session, and the shared caches are the same
        whether or not anyone pressed.

     **Every gap of the charter §15 audit is closed.**
    11. ~~Ids and the ARIA that names them, checked at build~~ (ADR-0185,
        `just e14-ids`): charter §8.2's duplicate ids and invalid ARIA
        relations, PW5031 to PW5033; corpus C10.
    12. ~~A page states its description~~ (ADR-0186, `just e14-metadata`):
        found by Lighthouse, SEO 75 with no meta description. `<meta>` at
        the top of a page's view, written into the head; PW5034, as HTML
        limits a name; a `<meta itemprop>` written where it is; corpus C11.
        With a correction to ADR-0183: a static page that stated its title
        shipped the runtime.
    13. ~~The store's page shifts as it loads~~ (ADR-0187,
        `just e14-stable-layout`): Lighthouse, CLS 0.136. An item is
        contained only 28 items or 16 categories down its page, the
        placeholder is an item's height, and the style is in the head. No
        shift at a phone's or a desktop's size; E7's gate item 10 kept.
    14. ~~E7's gate item 7b had failed, unseen, since ADR-0172 at the latest~~ (ADR-0188,
        `just e14-runtime-size`, `just e7-performance`): bounded as sent,
        compressed with Brotli, in every suite; the renderer's WebAssembly
        bounded too; E7's record recorded again.
    15. ~~A `<link>` the head holds, written in markup~~ (ADR-0189,
        `just e14-links`): PW5035; corpus C12.
    16. ~~Every page that binds a query, served at its route~~ (ADR-0190,
        `just e14-pages`): E14-Q's next slice; the store's cart as a page of
        its own, kept current beside the store's.
    17. ~~Every page speculates~~ (ADR-0191, `just e14-page-speculation`):
        from its own module; the cart's page shows a press before the
        server answers.
    18. ~~A list of stores as the home page~~ (ADR-0192, `just e14-home`):
        `StoreList()` and `HomePage` at `/`.
    19. ~~Placing an order~~ (ADR-0193, `just e14-orders`): the cart placed
        as an order, and its page kept current as the store moves it along.

     **The owner's priority (2026-10-04): the app layer before the AI
     benchmark.** Pleris is to be the perfect web application maker whatever
     the database. Next, each as its own ruling:
    20. **Types that contain themselves**, lifting ADR-0059:
        - ~~through a list, inside a component and across its boundary as
          nodes and indices~~ (ADR-0194, `just e14-recursive-types`), with
          PW0624 for a type no finite value has;
        - ~~held in place, boxed: `Option<Node>`, `Add(Expr, Expr)`~~
          (ADR-0202, `just e14-boxed-types`); through another declaration
          across a boundary still to build;
        - ~~views that contain themselves, each use an instance of the
          view's template, bounded below the 512 elements a browser's parser
          nests~~ (ADR-0203, `just e14-view-instances`), which found
          PW5012 reading neither a view's markup nor a block's rows
          (ADR-0204, `just e14-rendered-children`);
        - ~~the browser's wire, as nodes: a signal's tree, read and set by
          a handler, and a command's argument~~ (ADR-0205, `just
          e14-graphs-on-the-wire`);
        - then a query's value rendered from its nodes, past the host's
          `NESTED_DEPTH`; a capture, a command's error and a speculation's
          value as nodes, which need what writes them to know the type; and
          a signal in a view that contains itself.
    21. **A data source states what it guarantees**:
        - ~~its transactions, its reads and its change feed, stated by a
          `source`; a query or a command that asks more of its source than
          it gives is refused~~ (ADR-0207, `just e14-data-sources`), and the
          store states its own;
        - ~~ruling 11's other half, a command's events computed by the
          command and committed by the host with its writes~~ (ADR-0208,
          `just e14-command-events`);
        - ~~`invalidates` keys computed the same way~~ (ADR-0209, `just
          e14-command-invalidations`): the server evaluates no key of a
          command;
        - then a host comparing a source's clauses with the database it
          opens.
    22. **Reference apps unlike the store**, to find the gaps: a feed
        (pagination, live public data, threads, optimistic posting), a
        multi-tenant SaaS (organizations, roles, forms, uploads), and one app
        on three kinds of database.
    23. **The owner's rulings of 2026-10-05**, relayed by the session "Web
        Pleris capabilities and limitations": fifteen open "(ruling
        needed)" items. Record them as ADR-0195, each confirmed or overruled
        against primary sources, then implement:
        - first, the quick, correctness-critical ones: 14, a speculation
          never invents a value (the new line takes the price the page
          shows, done by ADR-0172); 12 and 13, KNOWN_LIMITATIONS' stale
          entries (done); ~~2, a case name is uppercase and a lowercase
          pattern name always a binding~~ (ADR-0197, `just e14-case-names`);
          ~~3, only statement-starting words
          reserved~~ (ADR-0196, `just e14-reserved-words`); ~~5, a bare case
          with a payload resolved like a nullary one~~ (ADR-0198, `just
          e14-bare-cases`), and ~~its other half, resolution from the
          expected type for both forms~~ (ADR-0201, `just
          e14-expected-cases`); ~~12's remainder, a
          named function as a handler, `e => save(e)`~~ (ADR-0199, `just
          e14-named-handlers`);
        - ~~found beside 12: ADR-0200, `()` as a value, which checks as an
          expression that did not parse and is typed as anything~~
          (ADR-0200, `just e14-unit-value`), with `check_sources` held to
          `pw check` and an unclosed `{#if}` reported at the block;
        - with the app layer: 10, a materialization may read another (no
          cycles, transitive invalidation) for the feed; 11, `emits` keys
          evaluated by the command and written with its writes (an outbox),
          with data-source guarantees; 8 and 9, cache keys by information
          flow and labels on clause keys;
        - then 1 (Rust's record-literal rule), 4 (bidirectional typing, else
          "annotation needed"), 6 (Float `%` Euclidean; text as ECMAScript's
          Number::toString), 7 (`return` stays a statement) and 15 (E7 gate
          8 counts attributed long frames only, with a thrashing control);
        - ~~and a sweep of the 109 "(ruling needed)" marks in 56 ADRs~~,
          ruled and relayed 2026-10-05 (ADR-0210, item 24).
    24. **The owner's rulings on the pre-delegation marks** (ADR-0210),
        relayed 2026-10-05, each in its own ADR:
        - first, the soundness defects its probes found: ~~a transaction
          left open by an exit inside a loop~~ (ADR-0211, `just
          e14-affine-loops`); ~~a handler calling a `todo`
          command~~ (not reproduced: the build refuses it); ~~stale captures
          at the top of a page~~ (ADR-0217); ~~a retry's
          strategy dropped~~ (ADR-0215); ~~misplaced policy heads ignored~~
          (ADR-0216); ~~`query
          helper(id)` over a `fn`~~ (ADR-0212); ~~`List.maximum`'s −0~~
          (ADR-0213); ~~an opaque type's member named `value`~~ (ADR-0214);
        - **then the feed reference app** (the owner, 2026-10-05; item 22's
          first): first a host that serves any program, its data the
          deployment's (ADR-0218: the store's state, reads, staging and
          grants moved, and the feed served); ~~a post reaching every open
          reader~~ (ADR-0219, `just e14-cross-session`); ~~the feed in
          browsers~~ (ADR-0220, `just e14-feed`); ~~a `<textarea>`'s
          bound value rendered as its text~~ (ADR-0221, `just
          e14-form-controls`); ~~optimistic posting~~ (ADR-0222, `just
          e14-optimistic-posts`); ~~a streamed region filled when its whole
          arm has arrived~~ (ADR-0223, `just e14-whole-fills`); ~~a longer
          read applied after a commit it did not see~~ (ADR-0224, `just
          e14-keyed-race`); ~~a `String`'s length as an invariant~~
          (ADR-0225, `just e14-string-invariants`); ~~pagination~~ ("Load
          more", a keyed read, in three engines with a post after it,
          ADR-0222's browser test); ~~a value the template computes, the
          host's part~~ (0073-a, ADR-0226, `just e14-computed-holes`: the
          thread's counts); ~~the browser's part~~ (ADR-0227, `just
          e14-computed-signals`: a draft's characters left, a post button
          disabled while it is empty); ~~rows, and a speculated value~~
          (ADR-0228, `just e14-computed-rows`: the likes in words, and
          optimistic likes); ~~conditions and blocks~~ (ADR-0229, `just
          e14-computed-conditions`); ~~0071-a~~ (ADR-0230, `just
          e14-condition-truth`: a condition is a `Bool`, or a `List` or
          `String` tested non-empty); ~~replies, and a page's parameter
          rendered on every page~~ (ADR-0231, `just e14-page-parameters`);
          ~~an opaque value's representation in a template~~ (ADR-0232,
          `just e14-representations`); ~~speculation on a value of a type
          that contains itself~~ (ADR-0233, `just e14-speculated-graphs`);
          ~~a speculated view's instance rendered again~~ (ADR-0234, `just
          e14-speculated-instances`); ~~a block's subject computed from a
          speculated value~~ (ADR-0235, `just e14-speculated-values`);
        - **first, soundness** (found 2026-10-07): ~~a handle made by a
          program, answered by a data layer or supplied by the browser~~
          (ADR-0263, `just e14-handles`); ~~a session's handle reaching
          the browser~~ (ADR-0264, `just e14-session-to-browser`: no
          answer, and nothing markup prints, holds one);
        - **then the Twitter gaps, in the owner's order** (relayed
          2026-10-07), each its own ADR as the feed hits it:
          1. ~~route-keyed speculation (0122-d): an optimistic reply~~
             (ADR-0236, `just e14-speculated-routes`); ~~a like on the
             thread page~~ (ADR-0238, `just e14-speculated-arms`). Writing it
             found that lowering dropped its parses' errors: ~~what lowering
             parses, it reports~~ (ADR-0237, `just e14-read-whole`). Then the
             findings beside it, first: ~~the registry's gaps~~ (ADR-0239,
             `just e14-registered-codes`); ~~a clause read once~~ (ADR-0240,
             `just e14-clauses-read-once`); ~~a transition producing another
             type as a literal~~ (ADR-0241, `just e14-transition-values`); ~~an
             `{#each}`'s head read once, by the grammar~~ (ADR-0242, `just
             e14-each-heads`); ~~two statements on one line~~ (ADR-0243,
             `just e14-statements-separated`). **First of all (owner,
             relayed 2026-10-07): CI green**: ~~the Wasmtime advisories~~
             (ADR-0244); ~~the heavy verification on GitHub Actions~~
             (ADR-0245); ~~the `pg-data-layer` branch merged~~ (ADR-0246,
             the owner approved its crates, 2026-10-07); ~~a clause written
             in a block judged by its domain~~ (ADR-0247, `just
             e14-block-clauses`); ~~a shard sets up only what its recipes
             need~~ (ADR-0249). What Linux has found in the recipes: the
             toolchain's missing target and a runner's disk (both fixed,
             ADR-0245), and ~~`stream.spec.mjs`'s "Chrome 150 and later fills
             a region itself"~~, which timed out in run 37651362024: the
             runtime records a region that settles after it starts, Chrome
             150 can fill one first, and the test waited for a record; it
             now reads which happened, and holds the record to it;
             ~~`feed.spec.mjs`'s "Load more shows the next page" in
             WebKit~~ (ADR-0271, `just e14-telling`): the records a failing
             feed test keeps showed a page sent some seventy renders of
             others' posts while its own read waited past five seconds; each
             commit told every reader, and the session's hold was taken in no
             order. A reader is told once per burst now, and its hold taken
             in turn: 13 of 160 runs failed before, 2 of 160 after; and
             ~~`stream.spec.mjs`'s "Chrome 150 and later fills a region
             itself" again~~ (ADR-0272, `just e14-stream-boot`, found by the
             notifications track): Chrome filled the region while the
             runtime booted, which had indexed it pending and never read it
             again, its buttons bound to nothing; and
             ~~`e10-recursion`'s "a record or variant is passed flat"~~,
             equivalent since ADR-0059 and retired (STATUS, 2026-10-07);
             and, open, **`feed.spec.mjs`'s "Load more shows the next page"
             in WebKit, on CI again** (run 37826467131 at `b393cee`, job
             113480498704: its rows stayed at twenty, and again in run
             37867532722's job 113617759049, and in run 37877464406's job
             113649231199, read 2026-10-09: the runtime's own record shows
             the Load-more read answered `{"applied":1}` at 8804 ms on an
             open stream, with no reconnect, and no frame applied in the
             four seconds after, while the 25 seeding posts had taken up to
             598 ms each to commit. The server applied the read, and its
             frame never came, or came late: what the server did with doc
             22's frames is the next thing to record, in the failure's
             attachment). ADR-0271 left 2 of 160 failing under loads past
             CI's; this one was on CI's own; and, open,
             ~~**a resume manifest's document and scope are constants**~~
             (ADR-0300, `just e14-build-id`, merged 2026-10-09: the build
             named by what it built, `pw-build` on every document and on
             the handler table, each page's document schema and scope on
             it, and the decision holding a document to what the serving
             build says of its page; ruled against comparing the build's
             name, which would read every open page again at each
             deployment that changed nothing it shows) (found writing the
             capability matrix, 2026-10-08): every
             document's manifest says `"document": "cart-doc"` and `"scope":
             "public"` (`resume_manifest` in the server's `main.rs`), and the
             browser's decision holds them to the same constants
             (`pw-resume-wasm`), so ADR-0132's checks of a document's schema
             and its privacy scope pass for any page, a private cart's
             among them. **The build id is a constant too** (`BUILD`,
             `"B1"`, read 2026-10-09), so the mixed-build check compares a
             constant with itself; only the handler's identity is the
             build's. ADR-0132 deferred both "until a page other than the
             store's needs it", and the feed's and kiokun's pages do. The
             integrator's, after the refusal and stream-records merges:
             the build's id derived from what it built; `/pw-handlers`
             answering it and each page's document schema, from the build
             the runtime was served with (ADR-0132's rule: never from the
             document); each document's manifest naming its own; the
             decision refusing a document of another build or another
             schema (codes 10 and 6), each with its recovery; the scope's
             meaning ruled with it (a session document's scope, and what a
             runtime knows of its session without reading its cookie); and ~~**four soundness findings of 2026-10-03**~~
             (ADR-0282, `just e14-held-labels`: one label for what a value
             holds; the session "Project
             readiness for kiokun.com rewrite", at `29ebcf9`; its message
             expired unread), each reproduced at `d346c43` on 2026-10-08:
             1. a secret kept in a cached fragment: a `public query`
                answering `Secret<Payments>`, kept by a `partition public`
                materialization placed at the edge or in the browser, checks
                clean, where the same value rendered by a shared page is
                refused (PW5001, PW5003). ADR-0118 left a secret out of a
                fragment's value label, and no other rule took it up;
             2. a fragment built before any request from a session's data:
                `placement build` with `depends_on Cart(current_session())`
                checks clean, where a build-placed page reading it is
                refused (PW5002). Nothing compares a fragment's placement
                with what its dependencies hold;
             3. placement and caching read different labels: a build-placed
                page reading the session through a query of its own
                (`query MyCart() { Carts.current(current_session()) }`)
                checks clean, and reading `Cart(current_session())` itself is
                refused. Since ADR-0128 the cache rules see the session
                either way; placement does not, in the checker or the
                contract;
             4. a placement diagnostic cut short: PW5002 says "cannot run in
                any world: it requires" and nothing after it, where a label
                alone, not an effect, rules out every world;

             and ~~**W6's two compiler findings**~~ (ADR-0283, ADR-0284,
             `just e14-what-a-component-does`, 2026-10-09): a host call
             reached through a function, imported; a fold's empty seed,
             typed, after the checker was found passing ill-typed folds;
             and, found probing them, a query that read the database
             through a function value or inside a lambda required no
             capability and could be placed in the browser; and, open,
             small, each over-stating or refusing rather than granting:
             **a call through a local is charged its namesake's effects**
             (ADR-0283's first limit: the effect inference's call walk
             resolves a callee's name without its scope), and **only a
             fold's seed takes the checker's type in the backend**
             (ADR-0284's: `List.concat([], xs)` in a `let` is refused);
             and ~~found probing ADR-0283, **a page's handler is held
             to the page's placement**~~ (ADR-0287): the row check keeps an `on:`
             handler's effects in the page's row and grants them against
             the page's world, so a page placed at `build` whose button
             sends a command is refused (PW5005, `database.write<Thing>`
             not available at Build), where its contract, which defers the
             handler, allows `build`. A handler runs in the browser, and a
             command it sends at the command's own placement: the
             DoorDash menu's static pages with an Add button need this;
             and, found 2026-10-09 in CI itself: ~~**the nightly could not
             finish**~~ (ADR-0290: two shards cancelled at 345 minutes, the
             plan dealing by mutants; now dealt by each recipe's seconds,
             the first nightly under it to be named here); ~~**the WebKit "Load more" failure's
             cause**~~ (ADR-0296, `just e14-derived-outside`: every commit's
             telling derived each open document inside the host's one
             `pending` lock, 50 to 200 ms a hold, back to back, and the
             page's stream waited up to 1.6 s a pass; a change is derived
             outside it now, and pushed only where the document still shows
             what it was derived against, 10 of 10 mutants; WebKit's "Load
             more" passed in each of the branch's five runs, where every
             other branch's failed it); and ~~**`feed.spec.mjs`'s "a follow
             reaches another reader" failing in CI's three-engine runs**~~
             (ADR-0297, `just e14-tell-at-once`: found by the failing test's
             printed records, run 37925722922, the second reader told five
             seconds and 47 tellings after the follower, or not at all: a
             commit told the other sessions one after another, closed pages'
             documents among them; only documents whose pages asked in the
             last three seconds are derived now, one passed by when its page
             asks, and eight sessions at once, 6 of 6 mutants; green in all
             36 recipes of run 37943438178);
             ~~**`e14-graphs-on-the-wire` took CI's runner down**~~
             (ADR-0292) every time it ran there (the nightly of 2026-10-08,
             navigate's verify twice): the heartbeat (run 37902799186) named
             a mutant, "the renderer takes a node twice", under which the
             100,000-node chain test holds 14.5 GiB in thirty seconds; it did
             here too, where the binary ended without a result (corrected:
             this file said the recipe passed here, 2.2 GB at most). Every
             process a mutation script starts is now bounded at 4 GiB, a
             kill by the bound its own kind; the recipe passes on CI in ten
             minutes (run 37908865560); `e14-contract`, which exited 1 in
             that nightly, passed alone (run 37902799186);
             and, open, next, the integrator's, **the host's paths
             reserved under `/_pw/`** (W6's question, PARALLEL.md
             2026-10-08): the runtime's files and its root endpoints moved
             there, a page's route refused that begins with it, then
             `/command/…` and the test controls, and among declared routes a
             literal segment over a parameter; kiokun's `/{word}` waits on
             it; then, **a refusal is never silent** (the owner's,
             using the feed by hand, relayed 2026-10-08 23:30). Signed out,
             a reader types into the home page's composer and presses Post:
             the post shows, then goes, with no word. The server refuses it
             (`requires SignedIn`, 403) and the speculation is taken back,
             so the safety net holds, but the press fails silently, the
             dead button this project exists to remove. In order, each its
             own ADR where it rules:
             1. ~~the feed's composer is a signed-in reader's~~ (ADR-0302,
                2026-10-10), hidden from one signed out by `hidden`, since a
                bound draft in a block a query decides is not rendered again
                (ADR-0137);
             2. ~~a refusal shown by the runtime~~ (ADR-0302): told where
                the press was, beside the control and through the page's
                announcer, in the predicate's words, which the program may
                declare (`predicate P says "…"`); a press no answer came
                for, a stale page's and a failed handler's in the
                platform's;
             3. ~~the compiler holds what the page can tell~~ (ADR-0307,
                2026-10-10): a page asks a predicate of its reader by naming
                it, the deployment answering it as it answers `requires`,
                and a control whose command requires one with no parameters
                stands where the page asks it and it holds, or is
                `|refusable`. Next, **a predicate with parameters asked of
                each row** (`{#if OwnsPost(post.id)}`), ruled with how a
                deployment answers many at once: the feed's Delete is still
                guarded by the program's own `mine`.

             And for the tests: a kept-alive `/command` request is not
             listed among the page's resource-timing entries, so no
             browser test relies on them for one.
             Further grammar and clause defects go under 0047-a below,
             unless the feed, the next Twitter item or soundness needs one;
          2. ~~map keys (0057-a)~~ (ADR-0248, `just e14-map-keys`) and ~~`let
             _` (0099-a)~~ (ADR-0250, `just e14-let-discard`, which found
             PW2005 following only `let x = acquire()`: an acquisition is
             held or refused now). Its tests found two more, next, since
             soundness needs them: ~~a `resource` declaration's clauses~~
             (ADR-0251, `just e14-resource-clauses`: what `acquire` makes,
             the resource holds, and what `release` is given, it ends); and
             ~~a member names whichever declaration registered last~~
             (ADR-0254, `just e14-member-resolution`: the one the module
             sees, or PW0628). Before it,
             found fixing `e10-lexical`: ~~a secret returned early is
             public~~ (ADR-0252, `just e14-returned-labels`). Then ~~0057-c,
             a map or set sorted on arrival~~ (ADR-0259, `just
             e14-arrival`);
          3. ADR-0195's ruling 10: ~~materialization chains~~ (ADR-0255,
             `just e14-materialization-chains`: one may read another, a
             cycle is refused, PW5109, and a write reaches each of a
             chain); ~~`invalidates Cart(_)`~~ (ADR-0256, `just
             e14-every-entry`: every entry at the rest, and an entry
             dropped in every session's partition); ~~a timeline built from
             who you follow~~ (ADR-0257, `just e14-follows`: follow and
             unfollow, `/following`, and a user's page with follower
             counts, on queries); then materializations made real: ~~a body
             and a type~~ (ADR-0273, `just e14-materialization-bodies`: held
             to its type, its reads its edges), ~~a generator, a page reading
             one, and a chain rebuilt in order~~ (ADR-0277, `just
             e14-materializations-kept`: the store's menu counted, live).
             What it leaves, in order: a private materialization, kept per
             principal, and a timeline kept per reader on it; the store's
             `MenuFragment` made a value the page renders; `regenerate
             on_read`; incremental maintenance;
          4. ~~**track `identity`, W1**~~ (ADR-0258, `just e14-identity`:
             accounts and sign-in, Pleris the relying party and the
             provider the deployment's). What it leaves, in order: ~~a
             typed principal, `context.current_user()` answered by the
             host from the session's principal~~ (ADR-0270, built by W3);
             ~~the routes a deployment serves in
             the route table, and every form's `action` and `method`
             checked~~ (its second, and ADR-0260's fourth: ADR-0265, `just
             e14-form-routes`); a refusal shown by the runtime, a stale
             tab's command refused 403 with its predicate;
          5. ~~**notifications, track `notifications`, W3**~~ (ADR-0270,
             ADR-0274, `just e14-notifications`: rows each act writes,
             private to their user by cache and by session, an unread count
             and mark-read). What it leaves, in order: ~~a speculated
             row's Like acting on an id no server has~~ (ADR-0275, `just
             e14-waiting-rows`); telling by principal, an event naming a user
             reading again only that user's open pages; `identified_by`
             gone, the feed's reads by session moved to `current_user()`;
             and a stream's `current_user()`;
          6. ~~**direct messages, track `messages`, W4**~~ (ADR-0279, `just
             e14-messages`, merged 2026-10-08; docs/PARALLEL.md, under the
             integrator's rulings there). What it leaves, in order:
             blocking, the integrator's; and telling by principal, which
             would read again only the two users' pages. As ruled: a
             conversation private
             to its two users, read from each side by a private query keyed
             by the reader's handle and the other's id; a list with unread
             counts; sending shown before the server answers; live to both;
             to someone who follows you or has messaged you, by X's rule;
             and a third user held to seeing none of it by page, by
             `/pw-read`, by cache and by stream; then, the integrator's,
             ~~**a handler navigates after its command commits**~~
             (ADR-0280, `just e14-navigate`; the owner's brief, 2026-10-08,
             from a Next.js bug: a save, then a soft navigation to a page
             the Router Cache served from before the save).
             `navigate Page(args)`, last in the `Ok` arm of a command's
             answer, the page by its declaration and its parameters typed
             (PW5042, PW5043); after the commit, never on a refusal or an
             `Ok` that did not commit; each press made before it answered
             first, both ways, and none taken while the page leaves; the
             page read after the commit by order, with no cache, no
             cache-busting parameter and no reload. Two of this brief's
             premises were the integrator's and wrong: a link's arguments
             are not checked (PW5009 matches its text to a route), and
             ADR-0224 carries no basis across a navigation, which here is a
             document load. Next, **the parts the pages share kept in
             place**: a soft navigation, the page fetched after the commit
             and the current one changed into it, identical parts kept with
             their focus and scroll, history pushed, the runtime moved to
             the new document's subscription, and links as well as
             `navigate`; on the History API, since the Firefox and WebKit
             that Playwright 1.58 installs have no Navigation API;
          7. ~~**track `uploads`, W2**~~ (ADR-0260, `just e14-uploads`: an
             image on a post, a typed upload served safely). What it leaves,
             in order: ~~a deleted post's image no longer served, and its
             blob collected where no post names it~~ (ADR-0261, `just
             e14-uploads`); a runtime that sends a file, so a
             draft typed before attaching survives (its second, after
             notifications); and from its "Not claimed", a budget kept across
             restarts and hosts, EXIF orientation outside JPEG, and more than
             one image a post;
          8. ~~a real database for the feed~~ (ADR-0246, written on
             `pg-data-layer` in parallel and integrated here; `just
             e14-feed-postgres`). What it leaves (its "Not claimed"): a
             second host on one database, a measured serialization failure
             between concurrent writers, idempotency committed with the
             writes, and the browser suite and the store on PostgreSQL;
        - **then the DoorDash customer app** (the owner, relayed
          2026-10-08): the store built out into DoorDash's customer side,
          after W4's merge and ADR-0280, each gap its own ADR, findings
          before features. The merchant, courier and support apps are out of
          scope. The integrator's, in order:
          1. **the parts two pages share kept in place**, ADR-0280's next:
             a soft navigation, which item 3's test needs. In three steps:
             ~~the build named~~ (ADR-0300, merged 2026-10-09); ~~a page
             shown in its layout~~ (ADR-0303, merged 2026-10-10); ~~a
             navigation that keeps the layout~~ (ADR-0306, merged
             2026-10-11, with ADR-0305, a page's commands in the order it
             sent them);
          2. ~~**accounts in the store**~~ and 3. ~~**delivery
             addresses**~~: **W8's**, merged 2026-10-12 (ADR-0318, ADR-0319;
             the owner: DoorDash faster, with a third worker; PARALLEL.md,
             "the DoorDash track"). Accepted by the owner's own Next.js bug:
             save an address, go to a store, and its page shows the new
             address's estimate, with no cache-busting parameter, no reload
             and, once item 1 lands, no header mounted again, in three
             engines. The integrator's own items here (1, 4, 5, 6) come
             before kiokun's and the infrastructure follow-ups (the owner,
             2026-10-09);
          3a. ~~**`private` means two things**~~ (ADR-0324, merged
             2026-10-12): `private` is "not importable" alone; the user's
             scope is `user`'s, and `private` on a declaration that holds
             data is refused by name (PW0029);
          3c. ~~**a member read inside a block at the top of the page**~~
             (ADR-0323, merged 2026-10-12): computed by a host once a
             change, by its path, into what the block is rendered with;
          3b. ~~**benchmark task T11's patches apply again**~~ (merged
             2026-10-12, track `t11-patches`): each patch made again against
             the store holds' merge changed, and a script that names a
             program or benchmark that changed plans its recipe;
          3d. **a flight another reader waits for was stopped once, under
             load** (the integrator's finding, 2026-10-13): train-3's first
             `just ci` saw `a_flight_another_reader_waits_for_is_not_stopped`
             end with the hot read stopped (1, not 0), at a load average of
             20. It has passed 20 of 20 alone and 40 of 40 eight at once
             since. Both of the host's read paths subscribe before they
             fetch, and T07's timeout is 5 s against the read's 1 s, so
             neither a late holder nor an expiry explains it. "It fails
             occasionally under parallelism" is a description of a race
             (RISK_QUEUE.md, "The recurring bug"): the test says the query
             runtime's trace when it fails, and the next failure is read
             from it;
          4. **store hours**: open and closed by the platform's clock
             effect, ordering refused while closed, and orders scheduled.
             In three steps: ~~the clock~~ (ADR-0304, merged 2026-10-10);
             ~~the store's hours~~ (ADR-0328, merged 2026-10-13, W9's);
             orders scheduled: ~~a command the clock runs~~ (ADR-0334, merged
             2026-10-14, its first use ADR-0332's sweep); an order placed
             for a time its reader chooses, sent to its store then: W9's,
             after 4b, on the integrator's design of 2026-10-14, built on
             ADR-0304, ADR-0328, ADR-0329, ADR-0332 and ADR-0334. The
             practice it follows, read that day: DoorDash's merchant help
             ("Manage Orders") keeps a scheduled order in a Scheduled tab
             until its preparation is due, and makes it active about five
             minutes before; Uber Eats ("Scheduled Orders") takes one up to
             about a week ahead and no later than an hour before, and while
             the restaurant is closed, for a time it is open. The rulings:
             - **the reader chooses when, at checkout**, as they choose a
               tip: a checkout choice, `choose_time(when: Option<Instant>)`,
               none meaning as soon as possible, kept with the cart and
               shown by `Checkout`; one time for the whole cart (one order);
             - **the times offered are the cart's stores' open times**:
               fifteen-minute windows from an hour to seven days ahead (Uber
               Eats' bounds), each whose start every store of the cart is
               open at, by ADR-0328's hours in each store's zone, computed
               by the program (the clock's comparison, as `closed_in` is),
               never by the page;
             - **`place_order` holds the time to them again**, in its
               transaction, as it prices the cart again (ADR-0329): a time
               no longer offered (a store's hours changed, the hour passed)
               is refused by name, `OrderError.NotOffered(when)`, and the
               reader is shown the times now. As soon as possible keeps
               today's rule, refused while a store is closed (`Closed`); a
               time is allowed while the stores are closed now;
             - **paid now, sent then**: the payment is begun and settled as
               today (ADR-0332). Settled, a scheduled order is
               `Scheduled(at)`, and the store is told nothing yet:
               `schedule send_to_store(order) at prep_starts`, the window's
               start less the upper bound of the store's estimate (ADR-0180)
               and five minutes more (DoorDash's lead). The run moves it to
               `Placed` as an order for as soon as possible is placed at
               once: the store's queue and the reader's page told, once;
             - **the sweep still applies** to a scheduled order that awaits
               its payment (ADR-0334's `expire_order`): a time an hour away
               does not hold a cart unpaid past thirty minutes;
             - **the order page says when**, "Scheduled for Thu 6:30 PM" in
               the store's zone, until the order is placed; then as today;
             - not claimed in v1: cancelling a scheduled order once paid (a
               refund at the provider, which no adapter has; before payment
               the reader's cancel and the sweep apply as today); a store
               declining scheduled orders; a time per store in a cart of
               several; a window the store sets;
             - accepted by the checker and the store's tests on each layer
               (the times offered at the seed's hours: open, closed, across
               midnight and a zone; a time no longer offered refused; an
               order scheduled while its store is closed, for a time it is
               open; paid now and placed at its time and not before, the
               clock moved; the order page's words), by the store in three
               engines (choose a time, pay, the page says when, the clock
               moved, the page says placed), and by mutation controls;
          4a. ~~**typed page query parameters**~~ (ADR-0309, merged
             2026-10-12): a route's query part, `route "/search{?q}"`,
             RFC 6570's form-style expansion; each name an `Option` of
             text; a plain `<form method="get">` and a link make the
             address. W6's kiokun search box and W9's store search move to
             it;
          4b. ~~**a read is a bounded number of statements on PostgreSQL**~~
             (ADR-0336, merged 2026-10-15 on train-8, W9's, `just
             e14-bounded-statements`): rows read by set (`= ANY($1)`), a
             scan in memory; every operation, page and command counted at
             the seed and with fifty stores more, pinned and equal at both;
             mutants restore each loop;
          4c. **a read is bounded by construction** (the owner's proposal,
             the integrator's ruling of 2026-10-13, PARALLEL.md; W9's design
             note ADR-0337, ruled 2026-10-14, merged on train-8): a page's
             and a command's source calls bounded by its text, a read keyed
             by a loop's element refused with its set read as the repair;
             each source operation's cost per call stated with its
             guarantees (ADR-0207) and measured by a conformance suite every
             layer runs at two data sizes; the budget, calls times cost,
             recorded as evidence and checked against the host's counter.
             Built in the ruling's order, each its own track: (1) the render
             merge, one snapshot per render and identical calls made once
             (track `render-merge`, in flight); (2) ~~PW0405, `set` pairs,
             `sets` and PW0406~~ (ADR-0340, merged 2026-10-15 on train-9,
             with PW0407 for the pair's shape, `just e14-bounded-reads`). What
             it leaves: the store's own pair, `set hours_of_stores` with
             `sets one_call` on `StoreData`, written together as
             `bounded_reads.rs` applies them; kiokun-site's four reads per
             element, warnings until kiokun's layer has set forms and the
             pair takes a list of key records (built with the kiokun track
             or D1's key records); the pair's key field matched by name
             (track `pair-key-by-name`); and the bound emitted beside each
             page's plan; (3) the conformance suite, one with the
             three-database note's decision 5.5, next; (4) the store's
             composites moved above the seam, one at a time. Before the
             capability matrix's apps;
          5. ~~**idempotency committed with the writes**~~ (ADR-0325,
             merged 2026-10-12): an interaction's answer commits in its
             command's transaction, on both layers, and answers a day;
          6. **checkout**, in steps: ~~6a, money rounded only where named~~
             (ADR-0327, merged 2026-10-13); ~~6b the order's total~~ (ADR-0329,
             merged 2026-10-13, W9's); ~~6c payment~~ (ADR-0332, merged 2026-10-13);
             6d a Stripe adapter
             (the owner's approval of each crate, asked 2026-10-14). Fees,
             taxes, a tip and
             promo codes, in money that
             cannot round silently (`Money` is USD's alone, so no other
             currency, conversion or locale's format, said where it
             limits); payment through a deployment's payment capability
             with a development provider, card data never reaching Pleris
             (the provider's hosted fields), a payment idempotent and
             charged at most once, under `secret<Payments>`; a Stripe
             adapter in test mode only with the owner's approval of each
             crate, and with test keys only.
          6e. **`orders#place` and `user-orders#place` reached by no
             program** (train-5's finding): the store places through
             `user-orders#await-payment` since ADR-0332. Removed with their
             pins, the libraries' `Orders.place` and `UserOrders.place`
             and the rows they alone use, or kept for a program that places
             without paying, said why; until then no mutant guards them;

          Then the language's, found on the way:
          - **the corpus specifies a quarter of the codes** (the
            integrator's finding, 2026-10-14, corpus C24): the rejected
            corpus exercises 56 of the 199 codes (C25 added PW0405 and
            PW0407; PW0406, a warning, is held by `bounded_reads.rs`
            alone); the rest are held by the
            compiler's own tests where they have them, and the conformance
            figure and generality score say nothing of them. From C24 on, a track that
            adds a code adds its rejected fixture and its generality pair,
            as the scheduled-commands track and C24 did; the 142 before are
            added a family at a time (Syntax's 22, Resolution's 10, UiState's
            10 and Capability's 5 first, none exercised);
          - **the host's engine refuses function references** (ADR-0335's
            answer, 2026-10-15): `wasm_function_references(false)` in
            `engine::engine_config()` refuses RUSTSEC-2026-0359's types by
            configuration, where Wasmtime 48.0.6 alone closes it today; the
            backend's only reference is a `funcref` table behind
            `call_indirect`. A hardening track of its own: a test that the
            engine refuses them, and a full run, the development server's
            suites among them;
          - **a program's declaration of another origin** (ADR-0195's
            ruling 13, its last clause; ADR-0333 built the rest): an origin a
            program declares, which its pages' policy allows and PW5052 lets
            its markup load from; built when a program needs one;
          - ~~**a template's bindings shadow module members**~~ (ADR-0310,
            merged 2026-10-12): a name means its binding in every analysis,
            the template's bindings, a call through a local and a loop's
            name beside a page's binding among them;
          - **a full-text source a program declares** (the integrator's
            ruling for W6, 2026-10-10): its index's columns, tokenizer and
            ranking, designed from two real uses, kiokun's search (whose
            two statements run in the kiokun layer over FTS5 meanwhile) and
            the next, DoorDash's store search likely;
          - **a route that answers data, not a document** (the same day):
            kiokun's `/api/search` and the other endpoints ADR-0285's
            inventory names; Pleris has pages and uploads, and no form for
            a route whose answer is JSON.

          - ~~**a page's canonical address**~~ (ADR-0311, merged
            2026-10-12; Q2 of 2026-10-09). Next by the same ruling, **a
            page's structured data** (Q3): JSON-LD typed as a std `Json`,
            written by the host at the top of a page's view. It needs a
            float among the renderer's values (JSON's numbers) and a
            recursive `Json` from a query across the component boundary
            (ADR-0205), so it waits for a page that asks for it; kiokun's
            Difference 13 meanwhile;
          - **a value computed from two queries** (found by W9's reader's
            search, 2026-10-12): a page computes from one query's value
            (ADR-0226), so the store's search, filtered by the reader's
            reach, is a `user` query whose full-text match is no longer
            shared. With two, it splits into a shared `Search(q)` and the
            reader's reach, and the match is kept once for every reader;
          - **one telling path for every page a change reaches** (W9's
            finding, 2026-10-11): a menu change tells the store's pages by
            `broadcast_menu`'s own patches, and every other page by what
            the change dropped (ADR-0219, the path W9 gave it), with the
            store's pages excluded from the second only to save a reading.
            One path for every page, `broadcast_menu`'s folded into it;
            after W9's merge.
          - ~~**one entry for a query a page and its layout both bind**~~
            (found 2026-10-11, ADR-0191's amendment): one binding for the
            entry and the layout's name its alias, in the speculation
            manifest, the host and the runtime (ADR-0312, merged
            2026-10-12).

          The infrastructure follow-ups, after these (the owner's order of
          2026-10-09):
          - ~~**the server's components compiled once a process**~~ (W6's
            finding, 2026-10-09; ADR-0317, merged 2026-10-12): each
            component's bytes compiled once a process, its imports read from
            the same compile. The development server's suite, whole, four
            tests at once on CI: 302 s and a 1.56 GiB peak, from about 950 s
            and over ADR-0292's 3.9 GiB bound. Still to measure: an
            opt-level for `cranelift-codegen` and `regalloc2` in the dev
            profile, against what it adds to a cold build and to `target/`;
          - ~~**a train fixed by a dispatch is verified**~~ (the
            integrator's finding at train-4's landing, 2026-10-13; ADR-0281
            amended (6), track `current-evidence`): master's push after
            train-4 planned the whole train again (verify 38494831583,
            cancelled by hand). Not by counting a dispatch: a push runs no
            recipe whose committed evidence is current, and a train whose
            run failed takes the run's evidence with its fix;
          - **the checker's latency** (the integrator's finding,
            2026-10-13): a full check of the store program takes about
            800 ms for 52 files, where E9's gate item 4 passed on 37 ms for
            36 files (2026-08-08), and an edit costs a full check, so
            editor feedback is near a second. Every analysis added since
            resolves names and types bodies on its own, and none shares
            what another computed. A profile (macOS `sample`, the latency
            test): the value relations about 10%, a session-handle analysis
            that built a typer for each printed value about 16% (one typer
            a body since, track `check-latency`), the effect inference and
            the summaries about 8% each, and name resolution through every
            import throughout (each unit's module indexed, one key a lookup,
            the same track). Nothing read the number again in two months:
            its recipe is the development machine's (`just e9-latency`).
            The work: each declaration's typer and resolution computed once
            a check and shared by every analysis that reads them, then an
            edit that redoes only what it reaches (E9's query system), the
            number recorded again at each step;
          - **a recipe not yet measured is planned at its kind's upper
            quartile** (done, ADR-0290's amendment of 2026-10-09).
          - **a shard dealt within a ceiling under its limit** (done,
            ADR-0290's amendment of 2026-10-10: a track's verify was dealt
            21,900 s into nine shards, and command order's shard was stopped
            at 345 minutes; halved to 10,350 s 2026-10-11, a branch having
            run its recipes up to twice their recorded seconds);
          - **each mutant given the tests that can see its rule** (W6's
            ruling of 2026-10-09, measured 2026-10-10): 45 mutation scripts
            run a crate's tests whole for each mutant, most of them the
            server's 377; the top 20 cost about 150,000 of a full verify's
            265,000 seconds, and `e14-command-answers` alone took 15,426 s,
            past the ceiling by itself. Each script narrowed is run whole
            first, every mutant still killed (done for `refusal_mutations`'
            core suite, 8 of 8 in 103 s). First the nine scripts whose core
            mutants run `pw-core`'s tests whole, relinking its 211 test
            targets each: `e14-cart-lines` 16,552 s, `e14-availability`
            12,125, `e14-row-reads` 5,879, `e14-invariants` 5,183,
            `e14-last-known-good` 4,802, `e14-command-retry` 3,663,
            `e14-nested-lists` 3,532, `e14-menu-categories` 3,260,
            `e14-query-attributes` 3,199 (about 58,000 s together). A
            mutant's killers are measured, not guessed: a tool applies each
            mutant, runs its suite whole with `--no-fail-fast`, and records
            the test targets that fail. The script is then given their union.
            A target renamed later matches nothing, and a run of no tests is
            a failure, not a survival. A task for a worker with a target of
            its own.
            Progress, 2026-10-12:
            - `pw-core`'s whole suite: seven scripts narrowed (`aa174750`
              and before); `availability`'s core suite and `command_retry`
              remain, measured next;
            - the development server's whole suite, by test name
              (`--exact`), each mutant failed by two of the named tests
              where two fail: `command_answers` 10,599 → 2,105 s,
              `availability` 8,012 → 3,206 s and `connection_faults` 9,614
              → 1,498 s (verify 38206663536, every mutant still killed),
              about 21,400 s a full verify; eight more by narrowing-2
              (merged 2026-10-12), 55,286 → 10,486 s; nine more
              measured, in track `narrowing-3`; and `query_blocks` to
              measure again since ADR-0320 changed its mutants' killers;

          The worker tracks, one at a time beside the integrator (the
          owner's budget rule), each launched under docs/PARALLEL.md when
          the slot frees:
          - ~~**W5, the store's data on PostgreSQL**~~ (ADR-0298, `just
            e14-store-postgres`, merged 2026-10-09): behind the DataLayer
            seam as the feed's is, on both layers, with five findings fixed
            on the way;
          - **W8, the DoorDash track** (PARALLEL.md, 2026-10-09: accounts in
            the store, delivery addresses, the owner's Next.js bug as its
            acceptance), in the slot W5 frees, before W7;
          - **W6 is kiokun.com's**, beside this app (below);
          - **W7, TodoMVC** (the owner, relayed 2026-10-08: "something
            people are familiar with"), the next slot free, since it pays
            three times: the comparison's first app (TodoMVC's own
            implementations, React's, Vue's, Svelte's and the rest, to
            measure against), an objective suite (its spec,
            `app-spec.md`, and its official behavioural tests, one Cypress
            spec, `tests/cypress/e2e/spec.cy.js`, read 2026-10-08), and the
            tutorial's first lesson. What it asks that no app has: editing
            on a double-click, Enter and blur saving, Escape discarding, the
            edit field focused; toggle-all and clear completed; "1 item
            left" against "2 items left"; and the hash routes `#/`,
            `#/active` and `#/completed`, which reach no server, so a page
            reads its fragment: the integrator's to rule when W7 asks.
            Persistence: the spec allows "the framework's own persistence"
            in place of localStorage, so the first version persists through
            Pleris's own data layer, a session's, and says so; device-local
            storage, charter §9.1's kind 4, starts with the Docs app, and
            TodoMVC takes it then as a second version. Its styling,
            `todomvc-app-css` and `todomvc-common` (npm, MIT), and Cypress
            to run the official tests unchanged: the owner said yes
            (2026-10-08), each one's source and size stated when fetched.
            Then, W7's second milestone, **the showcase** (the owner,
            relayed 2026-10-08), the presentation of TodoMVC and of the
            comparison:
            - for each demo app (TodoMVC, the store, the feed), three
              panes: the running app; its Pleris source; and what the
              browser received, the served HTML and its `pw-parts`
              manifest, made readable;
            - "compare with" swaps the second and third panes for the same
              app's React and Svelte versions, taken from TodoMVC's own
              repository and never written here; so only where such a
              version exists, TodoMVC's, until the comparison's stacks
              exist with who wrote them stated;
            - under the panes, numbers each from a recorded command, never
              asserted: lines of source by a stated rule; the app's
              JavaScript before the first press; the bytes the first press
              downloads (its lazily loaded handler); the parts patched a
              press; then Core Web Vitals and an accessibility audit;
            - "break it": a museum exhibit's edit applied, and the
              compiler's diagnostic shown in the source pane, as a command
              recorded it at build time; a compile in the browser would
              need the checker in Wasm, its own ruling;
            - "the timeline of one press": the speculation shown, the
              request sent, the commit, then only the patched parts flash
              in the app's pane, read from the runtime's own log and
              patches;
            - styled without Tailwind, whose class lists would bury the
              output the third pane exists to show: TodoMVC by its own
              stylesheet, the store and the feed by one small hand-written
              stylesheet over semantic HTML;
            - built in Pleris where it can be, as more evidence; code panes
              highlighted at build time, never by script in the browser;
            - honest: the other stacks' code is TodoMVC's own, every number
              reproducible, a loss shown as plainly as a win, and no
              placeholder shipped as a number; served here first, any
              public deploy on the owner's explicit go;
          - ~~**search and filters**~~: stores and dishes, by name and
            cuisine, searched in the store's data layer to one meaning
            (ADR-0308, W9's, merged 2026-10-12), at `route "/search{?q}"`
            with a plain GET form (amended the same day). Its next steps,
            W9's, after W8's merge: the home page's form, the cuisine as the
            store's own field (ruled 2026-10-12), `user`, and results
            filtered to the stores that reach the reader's address, through
            W8's `Reach`. ~~A part inside an arm whose subject did not change
            told when the query it reads changes~~ (ADR-0320, merged
            2026-10-12; the sentence is back inside the arm);
          - ~~**W9, ratings and reviews** of orders and stores, after
            accounts~~ (ADR-0341, merged 2026-10-15 on train-9, `just
            e14-reviews`): each store of a reader's latest delivered order
            rated once, 1 to 5 stars and a comment; a store's mean said from
            ten ratings; its reviews public, unnamed and unmoderated. What
            it leaves: a list of past orders to rate from, item and courier
            ratings, a badge, a window or recency weighting with the clock,
            and moderation, which needs the merchant side (out of scope).
            The DoorDash customer side's one piece left is scheduled orders
            (item 4 above).

          Not claimed: a live courier map (ADR-0075 refuses a mounted
          resource); the merchant, courier and support apps; subscriptions
          (DashPass); currencies other than USD; and courier dispatch,
          routing and fraud, the backend's services, outside the app layer
          (the owner's scope);
        - **beside it, kiokun.com, Pleris's production target** (the owner,
          relayed 2026-10-08): the owner's site, SvelteKit today
          (`/Users/haki/code/kiokun-data`: `sveltekit-app/`, the Rust
          builder, and the 1.49 million files of `output_dictionary`),
          rewritten in Pleris and served, from the kiokun slice (ADR-0037,
          ADR-0041). Its inventory and its word page are merged (ADR-0285,
          ADR-0286, ADR-0288, ADR-0289, ADR-0293, ADR-0299, and the page's
          moves, ADR-0301, 2026-10-10), and so are Contains and Appears in,
          search, the reading index with Homophones, and deinflection
          (ADR-0313 to ADR-0316, 2026-10-12); and so are the search box at
          `/search{?q}` and the word page's canonical link (merged
          2026-10-12); next (W6), `/{word}` once `/_pw/` lands, and the
          inventory's next routes (102 at kiokun.com's `2391117c`: 0 built, 5
          partial, 97 missing). Real
          users, data, devices and deploys are the strongest
          evidence there is. Its conditions: parity with the live site,
          measured; served first on a preview origin beside the live site;
          and no production cutover, DNS change or deploy without the
          owner's explicit go, asked when it is reached. It is W6, the
          worker track beside W5, as the owner chose (two workers at once,
          relayed 2026-10-08), so the tracks after them are W7, TodoMVC,
          W8, search and filters, and W9, ratings and reviews. It decides
          which of these come first, each its own ADR:
          - **developer experience**: ~~docs and a tutorial for a developer
            from outside~~ (ADR-0343, merged 2026-10-15 on train-9, `just
            e14-guide`: `docs/guide/`, every example compiled by the real
            `pw`, the tutorial the guestbook a browser test drives, and the
            checks' gaps held as examples; `docs/SEMANTICS.md` still reads
            as the M0-era status, the guide's finding); editor support, a language server (diagnostics as
            one types, completion, go-to-definition) on the check's ~37 ms;
            `pw new`, live reload, and debugging that traces which query or
            command caused what; and error messages read by a fresh reader;
          - **the production path**: a deployment, a container image or a
            host target; observability (charter §10.5: causal traces carrying
            the semantic ids); a real sign-in provider, **Google** (the
            owner's choice, relayed 2026-10-08), through a generic OIDC
            relying party behind ADR-0258's `identity::Provider`: discovery,
            the JWKS fetched and cached, and the ID token's signature,
            issuer, audience, expiry and nonce checked, with PKCE as built;
            tested in CI against a local fake provider, no real account in
            any test; Google's specifics (its discovery URL, its issuer's
            forms, the `hd` and `email_verified` claims) verified against
            Google's own Identity documentation. Then migrations across live
            deploys, secrets management, rate limiting and backups; then
            E11, E12 and E15;
          - **trust**: mechanised proofs of the core privacy and affine
            rules (Lean or Coq, over a core calculus, scoped first, being
            large); fuzzing the compiler, the host and the browser protocol;
            an external security audit when it is ready; and real
            accessibility testing (axe-core in the suite, VoiceOver and NVDA
            runs, real mobile browsers), since the capability matrix finds
            accessibility narrow;
          - **interop**: existing JavaScript libraries and components used
            behind a capability and privacy boundary (ADR-0075's mounted
            resources), which the Docs app's editor and maps need first; and
            adoption one route at a time, from an existing Next.js or
            SvelteKit app (charter P9), kiokun's SvelteKit app the natural
            first.

          The owner's own steps, asked when each is reached: any crate or
          npm download (a JWT or JOSE library among them); creating Google
          Cloud's OAuth client, in the owner's own account; where the client
          secret is kept, which is never committed; and anything deployed or
          outward-facing;
        - **then the evidence that answers "is Pleris perfect?"** (the
          owner, relayed 2026-10-08): the reference apps prove breadth, and
          these the claim. In this order, ahead of the next app, so that
          what the museum finds Pleris does not catch is fixed before more
          breadth is built:
          1. **the bug museum, extended and finished**: charter proof P0,
             "the executable thesis" (MILESTONES), which exists in three
             parts already: `examples/rejected/`'s 59 programs (R-001 on),
             its exhibits; `research/failures/`, the failure census of 224
             invariant-level failures in 24 families, with 76 external
             sources (SOURCES.md, which "extends rather than replaces" the
             museum) and statuses down to "missing"; and
             `web-recompiled/proof-roadmap.md` §3's Milestone 0, the shape
             planned (the familiar implementation, why it compiles, the
             Pleris form, the refusal, a source and a runnable test).
             **Begun** (ADR-0342, track `proof`, merged 2026-10-15 on
             train-9, `just e14-museum`): five exhibits, each a census leaf
             with a dated primary source, its bug shown in Next.js or
             SvelteKit and its repair the spec's control, and Pleris's
             answer with its own: F01, G01 and J08 refused by a code, J01 at
             runtime, and H07, a read per row, a miss at its first record and
             refused since by ADR-0340's PW0405, the museum's first closed
             miss. Next, the proof track's: J01's variant on a streamed page
             (vercel/next.js#99028 reports `router.refresh()`, its repair,
             fetching nothing there), then the head-to-head (item 2), and
             the sixth exhibit, H01's lost update. What it lacks, in order:
             - each `R-*` exhibit linked to its census leaf and to a
               primary source where one exists (a postmortem, a CVE, a
               public incident report, a framework's issue);
             - the roadmap's missing half, "compiles in TypeScript/React but
               fails here": a runnable Next.js or SvelteKit reproduction
               showing the bug happen there;
             - the census's "missing" and "partially covered" leaves made
               exhibits that say "Pleris does not catch this yet", each a
               gap here, or fixed;
             - the new real-world entries, each with its primary source: a
               page stale after a mutation because it is keyed by its URL
               alone (the owner's own Next.js case, ADR-0280's); one user's
               private data served from a shared cache; a double charge or
               submission from a retried request that is not idempotent; a
               dead button; a transaction or resource leaked on an early
               return; a session credential exposed to the page; a
               cross-site request forgery on an endpoint that changes state;
               a request storm from a reactive identity change (charter
               §9.5's own example); a race between a pending navigation and
               a save; layout shift from images without dimensions; and an
               inaccessible form or dialog;
             - each with its recipe and its evidence; and the playground,
               where a reader runs an exhibit, when it is judged worth
               building;
          2. **the head-to-head comparison**, extending E14's three-stack
             store contract (ADR-0120) rather than starting again: TodoMVC
             first (W7), against TodoMVC's own implementations, then the
             DoorDash customer side, in Pleris, Next.js and SvelteKit. Its
             protocol is ADR-0342's (the metrics, their instruments and the
             fairness rules); its downloads (TodoMVC's React and Svelte
             examples, axe-core) wait for the owner's approval.
             - Fair: idiomatic in each framework, never a strawman, each
               stack's standard libraries allowed.
             - Measured by lines of code under a stated counting rule; the
               defects one hidden suite finds; the museum's entries each
               stack permits; bytes shipped; Core Web Vitals; and an
               accessibility audit.
             - Every number from a recorded command; each framework's
               version pinned and verified against primary sources
               (tools/versions.lock); any package it adds waits for the
               owner's approval.
             - Where Pleris loses, said as plainly as where it wins.

             The feed's comparison follows the Google Docs-shaped app;
        - **then the reference apps the capability matrix chooses** (the
          owner's north star, relayed 2026-10-08: Pleris makes any complex
          web app as perfectly as possible, and Twitter and DoorDash are
          instruments for that, not the goal). `docs/vision/capabilities.md`
          rows what complex web apps need, each built, partial, missing or
          never Pleris's, with its ADR or its reason, and the next apps are
          chosen to cover its missing rows, in the owner's order (relayed
          2026-10-08):
          1. **a Google Docs-shaped collaborative app**: rich-text
             documents; several editors at once, with live cursors and
             presence; comments on ranges; sharing and permissions (owner,
             editor, commenter, viewer, and link sharing), which the
             privacy checker holds; version history and restore; and
             offline edits merged on reconnect. Its three rulings, each
             researched when it is reached:
             - how concurrent edits merge: an existing CRDT or OT reused
               (Yjs or yrs, Automerge, or OT) behind charter §9.1's kind 4,
               `replicated` with an explicit conflict policy, which is
               parsed only today; a universal CRDT system is one charter §2
               says not to build;
             - the editor surface: whether this is where mounted resources
               and foreign components are built, with their capability and
               privacy boundary (ADR-0075 refuses them today), or whether
               Pleris renders the document itself;
             - how presence and cursors ride the live transport, at a
               frequency far past anything it carries now.

             Any npm or crates.io download it needs (Yjs, yrs, Automerge,
             ProseMirror, ...) waits for the owner's approval, asked when
             it is reached.

             **Ruled** (ADR-0339, the integrator, 2026-10-15): concurrent
             edits merge in Automerge, behind `replicated`'s `rich_text`
             policy; Pleris renders every reader's document, and an
             editor's page mounts ProseMirror at the page's authority; presence
             and sync ride the stream down, one sync request up. No
             app-specific Rust: the app's own data is declared as tables on
             the generic adapters (ADR-0338), so its phase 3 waits for D3.
             **Its downloads wait for the owner** (the note's "Downloads for
             the owner's approval", owner decision 2): no track that needs
             one starts before the owner approves it;
          2. **end-to-end-encrypted direct messages**, ADR-0279's upgraded
             (the owner, relayed 2026-10-08), after the Docs app because both
             need the same foundation, data and computation that live in the
             browser:
             - the guarantee that makes it Pleris's: a placement label
               (device-only, charter §9.3's "browser/device-only → browser
               execution", under §7.8's privacy) by which the compiler proves
               a message's plaintext and the private keys never reach the
               server: not in a command's argument, a query, a log, a cache
               or an event; only ciphertext crosses;
             - its prerequisites, each likely its own ADR: computation and
               rendering placed in the browser, which decrypts and renders
               what the server cannot (E10-T2's browser Wasm, the renderer's
               browser blocks); device-local storage under an explicit
               policy, the key store and history in IndexedDB with
               WebCrypto keys made non-extractable where they can be
               (charter §9.1's kind 4, shared with the Docs app); keys per
               device, for several devices; and the features that change
               shape, each said so: a push or notification preview carries
               no content, search runs on the device, and the server cannot
               moderate;
             - the protocol reused, never invented: MLS (RFC 9420) through
               OpenMLS, MIT and built for wasm by its own CI, though not
               tested there (read 2026-10-08); libsignal is AGPL-3.0, which
               `deny.toml` admits only by an ADR, and which this MIT or
               Apache-2.0 repository does not take. Any crate or npm
               download waits for the owner's approval;
             - not claimed, and said plainly: the web's code delivery. The
               server that serves the encrypting code could serve another;
               reproducible, content-addressed builds anyone can check,
               Subresource Integrity and a published manifest in the manner
               of Code Verify are mitigations to weigh, not a fix;
             - tested and mutated: a command sent a plaintext is refused at
               build; the server's stored rows and its frames never hold a
               plaintext; and a third user, and the server's operator, see
               ciphertext alone;
          3. **a multi-tenant SaaS admin** (item 22's): organizations and
             roles, data tables, charts, i18n, background jobs and email,
             webhooks, and an audit log;
          4. **the app on three kinds of database** (item 22's third), on
             the databases the owner approved (relayed 2026-10-08), chosen to
             differ most from PostgreSQL, the strong reference (ADR-0246,
             W5). One app on each, behind the DataLayer seam, its `source`
             stating what the database gives (ADR-0207), and each layer's
             `provides()` measuring what the opened database gives, never
             assuming it. Each database's guarantees verified against its
             own documentation when it is reached:
             - **SQLite, libSQL or Cloudflare D1**: a single writer, in one
               file or at the edge;
             - **MongoDB or Firestore**: documents, multi-document
               transactions with their limits, and change streams or live
               listeners where an outbox beside the data would be. Proven:
               `emits` delivered from the change feed, as ruling 11 says for
               a source without transactions;
             - **Cassandra or ScyllaDB**: eventual consistency, and no
               transaction across partitions. The decisive test: the
               compiler refuses the cart's `transaction serializable` and
               `read_your_writes` against this source, while the parts of
               the app whose declarations it can satisfy build and run;
             - optionally **CockroachDB**: PostgreSQL's wire protocol,
               serializable by default, the cheapest way to show several
               regions, perhaps sharing most of the PostgreSQL layer.

             Each crate, npm package or server download, and running any of
             these servers locally, waits for the owner's approval, asked
             when it is reached; containers in CI are preferred, and nothing
             is installed on the owner's machine without asking.

             **Ruled** (ADR-0338, the integrator, 2026-10-15): an app's data
             declared in Pleris, one generic adapter per database
             implementing every app's, no app-specific Rust; built as the
             note's tracks, in order. **D1 `tables`**, the declaration, is in
             flight on `track/tables`; **D2 the memory adapter** (the table
             primitives and the generated conformance suite, with TodoMVC
             the first app with no Rust data layer) is next; then D3 to D5,
             T1 and T2, which download nothing. **The owner's decisions 1 to
             7** (the note's last section: which database of each pair,
             their downloads, what runs on the PC and on CI, CockroachDB,
             the Cassandra slice, the store's program changes) are asked
             before T3, T4 or T5 starts;
        - then what ruling 0073-a still refuses (several values, a page's
          parameter, the browser's values inside a block, instances), and
          everything below;
        - then 0101-a to 0101-d, 0105-a, 0100-a, and the rest below;
        - with the app layer: computed holes (0073-a: the host's part done,
          ADR-0226, and the browser's, ADR-0227, rows, ADR-0228, conditions,
          ADR-0229; the rest later), then ~~0071-a~~ (ADR-0230);
          route-keyed speculation (0122-d); ~~map keys (0057-a, 0057-c)~~
          (ADR-0248, ADR-0259); ~~`let _`~~ (ADR-0250), `while`, 0052-a, 0055-b, 0061-a, 0049-b; cheaper
          instantiation (0046-a);
        - after it: 0060-a, 0078-a, 0056-a, 0047-a's parser split, and
          what waits for it, found on the way:
          - ~~**an export's parameters past the flat limit**~~ (ADR-0266,
            `just e14-wide-parameters`: held in memory where the host
            stores them, each aligned; a call's arguments to a host's
            operation past it stay refused);
          - ~~**a component's trap carries its cause**~~ (ADR-0267, `just
            e14-trap-causes`: each trap of the component's own calls a
            function named by its cause, and the host reads the cause from
            the frame it stopped in, Wasm's own by their code);
          - ~~**a host binding's shape, checked where it is written**~~
            (ADR-0262, `just e14-host-bindings`: `"namespace:package/
            interface#name"`, each part a WIT identifier, PW0335 at the
            clause);
          - **clauses nothing reads** (ADR-0243): `respects`,
            `intrinsic_height` and a `handler_policy`'s clauses are read by
            no analysis, generator or runtime (the rolling deployment a
            handler policy is for is RISK_REGISTER's R5);
          - **an index has no message of its own** (ADR-0243): `items[0]`
            is refused as two statements in a block, and as what an
            argument list or a hole reads elsewhere; read as an index and
            refused by name, `List.get(items, 0)`, it would say what it is;
          - ~~**a resource matched where it is made**~~ (ADR-0269, `just
            e14-matched-resources`: held by the arm or the `?` that takes
            apart what carries it, and refused where an arm drops it);
          - ~~**a speculated row's control acts on an id no server has**~~
            (ADR-0275, `just e14-waiting-rows`: the feed's row waits, its
            links placeholders and its Like and Delete disabled, until the
            server's row stands in its place; a reply's link waits on ruling
            0073-a, and ids the client makes on their own item);
          - ~~**`pw fmt` changes what a clause says**~~ (ADR-0276, `just
            e14-fmt-meaning`: a digit separator the lexer reads, a policy's
            value printed with its gaps as written, `pw fmt` refusing a
            change of what a program says, and 342 programs held to `pw fmt
            --check`). What it leaves: a policy's value parsed as what it
            is, so it can be spaced as an expression is;
          - ~~**`lexical_mutations.py`'s two label survivors**~~
            (ADR-0242), "a `for` loop's name carries no label" and "a
            lambda's parameters carry no label": each test held a value
            secret by its type, and two that log a `String` secret by its
            label alone hold them (2026-10-07, STATUS);
        - housekeeping: 0032-a's field made required, 0049-a's wording,
          kiokun's NOTICE, an inlining budget, KNOWN_LIMITATIONS cleaned.

     Next: E14-E's design, ready for the owner's choice of models and
     budget, and the owner's decisions logged in ADR-0182 (axe-core) and
     ADR-0188 (a minifier for the runtime).

     The benchmark's store is its own copy since ADR-0156, so the canonical
     store grows without re-basing the tasks. Then E14-E's design, ready for
     the owner's choice of models and budget.
4. ~~**E14-D, `pw diff`** for the store (gate item 1).~~ Done 2026-10-03
   (ADR-0149, `just e14-diffs`). It reports each task's Pleris reference
   and unsafe patch in charter §19.2's sections. Its first use found that a
   page streaming a query was charged the query's capability.
5. **E14-E, agent runs**, after the owner chooses models, budget, and how
   Pleris is taught to an agent.

Carried from E10, owed at E15 (EVIDENCE_LEDGER): E10-T2, E10-S10, E10-M. Open
before any P1/P2 publication: E7-G8.

The E10 material below is history.

## E10 (closed): E10-I closed, handlers compiled; the rest of E10, then the kiokun proof slice

**2026-10-01, authorization held (ADR-0115).** A command's `requires`
predicates now cross the compiler-host contract and are evaluated before the
component can run. The raw host call refuses an export with unevaluated
preconditions; unknown predicates fail closed. The own-renderer development
deployment explicitly models its local demo session as `SignedIn`; real
identity verification remains a deployment integration, not a compiler guess.
Next remains the first unresolved item in the lists below.

**2026-09-25:** resumable handler bodies compile to ES modules
([ADR-0033](DECISIONS/ADR-0033-compiled-handlers.md),
[evidence](evidence/E10/handlers-2026-09-25.md)). The store page's behaviour is
compiled from `.pw` end to end, from the handler to the command component.
Item 3 below is done.

E10-I closed on 2026-09-24 ([ADR-0032](DECISIONS/ADR-0032-compiled-components.md),
[evidence](evidence/E10/e10-i-2026-09-24.md)). The locked order's steps 4-9 are
done:

```text
4  Canonical ABI adapters from checked callable signatures        DONE
5  component wrapping via UPSTREAM wit-component                  DONE
6  independent validation (decoder audit, core-identical control) DONE
7  instantiate through the E8 linker                              DONE
8  run the `.pw` add_to_cart                                      DONE
9  delete the Rust closure path; close E10-I                      DONE
10 then investigate replacing external `Carts.add`                NEXT
```

### Next, in order

1. ~~**The kiokun proof slice.**~~ DONE 2026-09-25 (ADR-0037): entry lookup and
   search over shard `han-1char-3`, with the backend breadth it needed (matches,
   fields, variants: ADR-0036). What the full site needs next, in order:
   pure computation (a real standard library for lists and strings, and
   arithmetic and `if` in the backend), so the shard rule and the ranking can
   move from the host into Pleris; then the template gaps (an interpolated
   attribute, `else`, `Option`).
2. ~~**Backend breadth, driven by that slice.**~~ DONE 2026-09-25 (ADR-0039):
   records built, field projection, branches, string constants, arithmetic and
   comparisons, and calls between compiled declarations, inlined. Each has a
   test run through the host and a mutation control (`just e10-pure`). Still
   refused: declared variants built or matched, recursion, generic callees.
   The standard library came next (ADR-0040): `List` and `String`, compiled.
   kiokun's shard rule and ranking followed (ADR-0041); the host keeps only
   the index and the files. The template gaps followed (ADR-0042): `{:else}`,
   which was a silent miscompile, `{#match}` over `Option` and `Result`, and
   interpolated attributes. kiokun's pages use all three. Pure computation
   compiles to JavaScript modules too (ADR-0044), and PW2005's "exactly
   once" is checked on every path (ADR-0045, task 7), and memory strategies
   are evaluated on kiokun's calls (ADR-0046, task 4). Next: a handler that
   computes its arguments with that encoder; then, if ruled, instance reuse,
   which the measurements favour.
3. ~~**Compiled resumable handler bodies**~~ DONE 2026-09-25 (ADR-0033). Then
   step 10. Two things the handler work left open, in order of need:
   - captures as a patched part, so that a captured field a patch changes is
     re-carried (KNOWN_LIMITATIONS);
   - the event as a handler parameter, for inputs and forms.

**2026-09-25, "ok fix all the known gaps":** the six gaps named that day, in
order. Every name resolves (ADR-0047, done). Every member exists (ADR-0048,
done). String escapes are defined (ADR-0049, done). Recursion and generic
callees compile (ADR-0050, done). An early `return`, `?` and `for` loops
compile (ADR-0051, done). A function is a value (ADR-0052, done). A lambda's
parameters take the types its use declares (ADR-0053, done: a correction
found on the way). An opaque value is built and read inside a component
(ADR-0054, done). Slicing, and `sum` and `maximum` computed (ADR-0055, done).
Unicode case mapping (ADR-0056, done). Maps and sets (ADR-0057, done).
Handlers that compute (ADR-0058, done). The six gaps are closed. Next, each
needing a ruling first: the event as a handler parameter (a syntax, and a
runtime that listens for the part's own event), and the limitations
KNOWN_LIMITATIONS still lists for the backend, among them declared sum
types built and matched, generic records, and `Float` formatting.

**2026-09-26, "keep working until its perfect":** the limitations
KNOWN_LIMITATIONS lists, in order. Declared sum types are typed, built and
matched (ADR-0059, done), which found four ways a wrong program passed
`pw check`. Next, in order:
1. ~~nested and literal patterns~~, done (ADR-0060), which found a silent
   miscompile: `Some(Empty)` bound every payload;
2. ~~a declared sum type in a template's `{#match}`~~, done (ADR-0061);
3. ~~generic records and generic sum types in the backend~~, done inside a
   component and a module (ADR-0062);
4. ~~the value relations' flat environment: a name bound at two sites~~,
   done (ADR-0063). It was three flat environments. Type errors through any
   name bound twice passed. A handler's capture was typed by another binding
   of its name. Secrets were logged or rendered through a `for` loop's, a
   lambda's or an `{#each}` block's name. And the accepted A-017 was
   ill-typed.

Next, in order, taken up 2026-09-26 after ADR-0063, findings before features:
1. ~~a privacy label carried through a declared function~~, done
   (ADR-0064), with an undeclared call's receiver;
2. ~~a value with a hole that means any type~~, done (ADR-0065): 41 of the
   store's undecided relations to 1, 20 of kiokun's to 1;
3. ~~the call check reads scopes~~, done (ADR-0066), with what it found: a
   nested function read its enclosing body's secret unlabelled, and a nested
   declaration's annotations never resolved. Then ~~a record's fields, and an
   `if` without `else` as a value~~, done (ADR-0067), and ~~calls through
   function values, `for`'s list, `?`'s operand and used branches~~, done
   (ADR-0068), which found `elif` chains miscompiled, ~~a list's items and
   an `Int` literal's range~~, done (ADR-0069), ~~an assignment to a
   field~~, done (ADR-0070), and ~~what a template's blocks and events
   take~~, done (ADR-0071); all found probing for wrong programs that check.
   ~~A view used in another view~~, refused (ADR-0072): it built as an
   unknown HTML element. Composing views needs a ruling on the design, and
   inlining at compile time is the proposal. ~~What a template reads by
   path~~, done (ADR-0073): `pw build` wrote templates that fail every
   render, and keyed loops on the wrong field. ~~What a template writes~~,
   done (ADR-0074), which found ADR-0071's `{:else if}` gap, and ~~a
   `<stream>` and a mounted resource~~, refused by name (ADR-0075). ~~A
   call through a field holding a function~~, checked (ADR-0077), which
   found ~~an effect travelling through a function value unseen~~, done
   (ADR-0078), and ~~a secret through one~~, done (ADR-0079), and ~~the
   affine rule by names~~, done (ADR-0080), which found a transaction never
   ended passing. Next, the name check's own scope walk onto
   `crate::lexical`, the last analysis that keeps its own;
4. a function passed to a generic declaration in the backend (found writing
   ADR-0062's tests), then a generic type at the boundary, a WIT type per
   instance;
5. structural `==`; ~~reporting an arm no case reaches~~, done (ADR-0076);
6. ~~named-argument calls~~, done (ADR-0081), which found them compiled
   in written order; ~~`derived` purity~~, done (ADR-0082); ~~`()` on the
   next line parsed as a call~~, done (ADR-0083); calling a
   function held in a record field (checked since ADR-0077; the backend
   refuses it by name), and a hole holding a string;
7. `Float` `%` and `Float` formatting, then `split` and `range`;
8. each needing a ruling first:
   - the event as a handler parameter;
   - instance reuse;
   - captures and patches for templates;
   - a record literal whose first field is shorthand (ADR-0063).

**Found 2026-09-26 probing policy clauses, taken up before the list above.**
~~A call to a view, a page, an event or an effect~~, refused (ADR-0087).
Next, in order:
1. ~~**a clause's keys**~~, done (ADR-0088), which found the store's own
   `emits` ill-typed and five fixtures passing the wrong key. Then ~~**a
   policy's value**~~, done (ADR-0089), which found `cache Shared` escaping
   the privacy rule and three rules bypassed by spelling, and this:
   ~~**`pw build` does not run the declaration rules**~~, done
   (ADR-0090): it compiled R-015's `retry forever` query into a component;
2. ~~**a listener's key**~~, done (ADR-0091): an inventory change never
   reached its store's menu. Then ~~**where a graph clause is written**~~,
   done (ADR-0092). Next, what is left of policy position, each needing a
   ruling first:
   - `requires`, whose predicates nothing declares;
   - `privacy`, which nothing reads: `replicated OrderDraft` is labelled
     public whatever it declares;
   - which declarations every other head belongs to;
   - a key's labels (ADR-0088);
3. ~~**a policy operator's arguments**~~, done with the values
   (ADR-0089).

**Found 2026-09-26 probing markup and declarations.** ~~An `on:` naming no
declared event~~ (ADR-0093); ~~code a template writes~~ (ADR-0094: a script
element, an inline handler, a script URL, `srcdoc`, a stylesheet's value);
~~an attribute's context read by its written case~~ (ADR-0095); ~~a `<base>`
and an animated link~~ (ADR-0096); ~~a page's embedded JSON~~ (ADR-0097);
~~a name written twice~~ (ADR-0098); ~~a failure nothing handles~~
(ADR-0099); ~~a write inside a query~~ (ADR-0100); ~~a command's write that
invalidates nothing~~ (ADR-0101), which found A-005 leaving A-004's cart
stale; ~~an event that stops at its listeners~~ (ADR-0102); ~~a fragment
no write reaches~~ (ADR-0103); ~~an event no command declared~~ (ADR-0104);
~~a speculation nothing reconciles~~ (ADR-0105); ~~a file whose name
another has~~ (ADR-0106), which `pw check` passed with an error; ~~a key
that omits a parameter~~ (ADR-0107); ~~a query naming nothing~~ (ADR-0108);
~~a timeout of zero~~ (ADR-0109); ~~a handler reading what it does not
capture~~ (ADR-0110); ~~a capture read through a shorthand field~~
(ADR-0111); ~~a secret passed by an import's spelling~~ (ADR-0112); ~~a
handler performing what only the origin may~~ (ADR-0113); ~~a build-placed
page reading its parameter~~ (ADR-0114). Next, in order:
1. ~~**a materialization is not reached through what it reads**~~, done
   (ADR-0102): A-009's fragment kept a changed store's old name. Then
   ~~**a command's write held to the fragments built on it**~~, done
   (ADR-0103): a fragment built from a query with a window kept a renamed
   store for good;
2. ~~**the dev server emits `CartChanged` after any cart write**~~, done
   (ADR-0104): it commits the events a command declares. Next, the event a
   command's own arguments key, which needs the compiled command to return
   its events (ruling needed);
3. ~~**an optimistic target no command invalidates**~~, done (ADR-0105):
   the speculation stayed on the page as if committed;
4. **a Marko page's refresh**: a binding refreshes only for a query that a
   command in the page's own module names in `invalidates`, where the
   checker and the dev server accept an event, and a command anywhere
   (KNOWN_LIMITATIONS).
5. ~~**a key that omits what its entry depends on**~~, done (ADR-0107):
   two calls differing in an unkeyed parameter shared one entry;
6. ~~**a timeout of zero**~~, done (ADR-0109);
7. ~~**a resumable handler reading what it does not capture**~~, done
   (ADR-0110). It found:
   - ~~**a capture read through a shorthand field**~~, done (ADR-0111):
     a correct handler was refused by PW5017;
   - ~~**a handler performing what only the origin may**~~, done
     (ADR-0113): one calling `Carts.add` itself checked, and `pw
     emit-handlers` refused it;
8. ~~**a build-placed page reading its parameter**~~, done (ADR-0114):
   `placement build` with `<h1>{id}</h1>` checked, and the file it is built
   into exists before any request supplies `id`.
9. ~~**a public query reading the session through what it calls**~~, done
   2026-10-02 (ADR-0118): a shared fragment, a shared cache and a shared
   page each took it for public, and its page's contract allowed `build`.

Then, each needing a ruling first:
- ~~**authorization**~~, done 2026-10-01 (ADR-0115): `requires` is an
  export-level invocation precondition and cannot reach the component body
  without a deployment authorization decision. Unknown predicates fail closed;
- head elements in a body, `<meta>` and `<link>` (ADR-0096);
- what a handler that is not resumable compiles to: the runtime leaves
  its element inert (KNOWN_LIMITATIONS);
- what `consistency strong` promises beside a staleness window: the
  library's `Order` declares `freshness 2.seconds` with it, and PW5106
  exempts such a reader (A-026);
- the host's `wasi:` exemption from contract membership, which rests on
  the WASI context the host links (`pw_host::is_runtime`);
- whether a build-placed page may take a parameter at all, and whether a
  build may enumerate a parameter's values (ADR-0114).

**The dated material below is the decision history and original migration
sequence, not a second current status.** Its old NEXT/BLOCKED labels describe
the state at those dates and are superseded by this section where applicable.

## 2026-09-15 follow-up: recursive written-type prerequisite repaired

The signature migration exposed a prerequisite: `DeclaredType` stored generic
arguments as strings and `resolved::resolve` refused nested arguments. The parser
already knows their structure, so [ADR-0028](DECISIONS/ADR-0028-recursive-written-types-before-signature-cutover.md)
keeps it through lowering rather than adding another type parser. Parameters,
record fields and return annotations now share that recursive representation.
`Decl.ret_args` is removed; manifests preserve complete direct return types.

The resolver now enforces the arity of built-in type constructors, refuses a
qualified non-type declaration, and recognizes the grammar's unit spelling.
[Evidence](evidence/E9/recursive-written-types-2026-09-15.md) records the five
initial failing regressions and the broader tests. This closes only that
prerequisite, not the signature cutover, E9-V1..V6, or E10-I.

Next remains **the atomic resolved-signature migration** below, followed by
argument inference/unification and declared-return checking. The existing
`3b3-signature-resolved-types` branch is not merged: it still has dual fields.
Callable generics and higher-kinded parameters are not newly implemented here.

---

## Now: E10-A — the first backend slice

**The pre-codegen semantic gate closed on 2026-08-11.** Architect ruling: stop
semantic cleanup and encode, and *do not expand the semantic scope again unless
the encoder exposes another concrete defect.*

```text
1  TermRoot + ExecutionContext, generalizing Transition        DONE
2  block-policy headers introduce real lexical binders         DONE
3  `measure {}` is syntax; `measure(..)` is a call             DONE
4  optimistic target / binder / result type agreement          DONE
5  re-run C6, the ownership gate, the consumer matrix,
   E10-P and the ADR-0025 controls                             DONE
6  Wasm encoding                                               STARTED
```

**The first encoding slice, 2026-08-19.** `backend/wasm.rs` emits a core module
— types, imports, functions, exports, code — and `wasmparser` validates it.
Three of the store's five functions encode.

`add_to_cart` does **not**, and the reason is the first concrete defect the
encoder exposed:

```text
add_to_cart   HostCall database.write<Carts> [session, item, quantity]
clear_cart    HostCall database.write<Carts> [session]
```

**A capability is not a function** — and the model was repaired on the
architect's ruling of 2026-08-20. `Instr::ImportCall` names an `ImportId`;
`CallableImport` carries the ABI and a set of capabilities; a host binding is
explicit declaration metadata. All five store functions encode now.

### The locked order, 2026-08-20

The architect's sequence for the rest of E10-A and E10-I. It is an order, not a
menu — each step's evidence is what makes the next one readable.

```text
1  ownership: platform-defined vs host-supplied              DONE
2  the operation→capability audit, as three separate layers   DONE
3  freeze the signature/authority mutation controls           DONE
4  Canonical ABI adapters from checked callable signatures  BLOCKED
5  component wrapping, via UPSTREAM wasm-tools                 —
6  validate with an independent component parser               —
7  instantiate through the E8 linker                           —
8  run the `.pw` `add_to_cart`                                 —
9  delete the Rust closure path; close E10-I                   —
10 then investigate replacing external `Carts.add`             —
```

### The resolved-type ruling, 2026-08-21 — the locked sequence

Three rulings, and the sequence they fix.

**1. `3 → 4 → 2'` is the order, and 3 + 4 are ONE gate.** Not two independently
valid end states — there must never be a successful

```text
ResolvedType { head: DefId, args: ["MenuItem"] }   // argument still textual
```

so resolution is recursive and Blocks if any argument cannot resolve. That
prevents step 3 from creating another partly-semantic representation that step
4 then has to clean up.

**2. Replace the written types outright.** Not this:

```rust
struct Signature {
    written_params: Vec<String>,
    resolved_params: Vec<ResolvedType>,   // two authorities — the shape we keep deleting
}
```

but a `ResolvedType` with private internals, carrying **provenance** rather
than a parallel semantic type:

```text
ResolvedType
├── semantic identity   DefId / type-parameter identity / resolved arguments
└── origin              source span, DeclaredType reference
```

with `def_id()`, `args()`, `type_parameter()`, `canonical_name()`,
`display_name()`, `written_source()`, `span()`. A diagnostic still says
*expected `SessionId`* as the programmer wrote it, and an artifact can say
`pw-platform-web.capability.SessionId`, without any consumer holding a string
it might treat as meaning:

> **Written syntax can explain a resolved type. It cannot compete with it.**

**A `DefId` is compiler-process identity and must not become the host
artifact format.** `ComponentContract.signature` serializes a **StableTypeId** —
package plus declaration identity plus resolved arguments — derived from the
resolved signature:

```text
resolved semantic type → stable contract semantic type
    → component/WIT representation → core ABI representation
```

Each step may discard information; **none may regain information an earlier
representation failed to know.**

**3. Call checking is unification, not equality.** Instantiate the callee's type
parameters freshly, infer each argument, unify, and let the substitution
determine the result. Representation transparency belongs to ABI lowering, not
value typing — `opaque type Tag = String` does **not** let unification unwrap
`Tag` to `String`.

The frozen discriminators:

```text
takes(String); takes(42)                       reject
takes(alpha.Tag); pass alpha.Tag               accept
takes(beta.Tag); pass alpha.Tag                reject   (ABI-equal, semantically distinct)
takes(List<MenuItem>); pass List<MenuItem>     accept
takes(List<MenuItem>); pass List<Store>        reject
identity<T>(T); identity(Store)                infer T = Store
fn wrong() -> String { 42 }                    reject
```

The last matters because the finding is broader than arguments: **declared
returns are not enforcing value types either.**

**4. Privacy consumes the same object.** `Session<"SessionId">` must become
impossible as successful semantic state — `Session<DefId(SessionId)>` or
Blocked — and `Session<A> → Session<B>` compares resolved identities, never
final-segment strings. Resume privacy, remote obligations, partitioning and ABI
privacy erasure all already depend on these labels, so one repair improves
several security-relevant consumers.

### The sequence

```text
3a  private ResolvedType + source provenance                    DONE
4   recursive resolution: head + every argument, or Blocked     DONE
    StableTypeId — the artifact identity, no DefId              DONE
3b  migrate Signature and the contract's semantic signature;
    delete every semantic use of a written type head            NEXT
2'  call-site typing: instantiate + infer + unify                 ...
2'' declared-return compatibility                                 ...
    → re-run E9 evidence, close E9 again                          ...
    → regenerate contracts and WIT                                ...
    → component-level same-core-ABI mutation control              ...
    → Canonical ABI adapters                                      ...
```

**3b is measured, not estimated:** 35 call sites across `annotations.rs`,
`check.rs`, `infer.rs`, `backend/lower.rs`, `binding.rs` and `wit.rs` read
`Signature`'s written types. It cannot be done incrementally, because the ruling
forbids carrying both authorities at once — so it is one commit that moves every
consumer, and it wants a session's full attention rather than the tail of one.

### The fork is settled: B, 2026-08-21

> **There is one semantic type identity: `ResolvedType`/`DefId`. `TypeEnv`
> never discovers identity; it only supplies declaration facts for an identity
> it is handed.**

That rule is what makes B safe, and it is the difference from
`HostCall { capability }`. There, a `CapabilityId` and a `DefId` each answered
*which operation is this* — two answers to one question. Here:

```text
ResolvedType::Nominal(DefId(Tree), [Int])   this expression has Tree<Int>
TypeEnv[DefId(Tree)]                        that declaration has Leaf and Node
```

The environment may never say *I think this is `Tree` because the name is
`Tree`*. The identity arrived already established. It is
`Call(DefId(foo))` + `FunctionBodies[DefId(foo)]`, which are not two
representations of which function was called.

**Constructors stay out of `ResolvedType`.** A use is compact —
`Nominal { def: Tree, args: [Int] }` — and does not recursively carry its
declaration. Exhaustiveness goes scrutinee → `TypeEnv[Tree]` → constructors →
substitute `T := Int`, which is also the substitution machinery E9-V2 needs.

**The vocabulary changes with the migration**, because `Type` beside
`ResolvedType` invites a reader to assume either might be an expression's type:

```text
ResolvedType    the occurrence's semantic type
TypeDef         facts about a declaration
TypeEnv         DefId → TypeDef
StableTypeId    serialized semantic identity
AbiType         boundary representation
```

`TypeDefId`/`AdtId` become **storage handles**; `DefId` is semantic identity.
`by_adt_name` and `by_opaque_name` are deleted — that is the bug.

### The migration order

```text
1  re-key type definitions DefId → TypeDef; delete bare-name semantic lookup  DONE
2  move exhaustiveness and check.rs onto ResolvedType.def_id() → TypeEnv      DONE
3  Signature: params: Vec<ResolvedType>, returns: Option<ResolvedType>        NEXT
```

### 3b.3 is under way on a branch: `3b3-signature-resolved-types`

**Landed there, green:** `Signature` carries `resolved_params` and
`resolved_returns` beside the written strings — the dual state the ruling
permits on a branch and forbids on `master`. `Signature` lost its derived
`PartialEq` on the way (it would compare spans; nothing used it).

**The remaining dominoes, in dependency order.** Traced by following the
chain, not guessed:

```text
1  binding.rs `Interface`      built from DeclaredType directly, not from
                               Signature — so it can gain resolved types
                               independently. Feeds BOTH remote_support and
                               wit.rs, which is why it is first.

2  boundary.rs `TypeFacts`     `profile(ty: Option<&str>)` is name-keyed, and
                               `name-keyed-allow.txt` already promises "E9
                               replaces it with a resolved type id". The gate
                               will demand it the moment binding.rs moves.

3  binding.rs `remote_support` a DECIDE site: it matches `Some("Result") |
                               Some("Option") | Some("List")` on the written
                               head to find carriers. Becomes `as_builtin()`,
                               and the arguments arrive already resolved
                               instead of via `returns_args`.

4  wit.rs `wit_type`           takes a written string and re-resolves it.
                               Becomes `def_id()` → `Types::by_def`, which
                               already exists — this DELETES a resolution
                               rather than moving one.

5  infer.rs / labels.rs        `label_of_type(head, args)` matches privacy
                               qualifiers by SPELLING. This is the architect's
                               step 4: `Session<A> → Session<B>` must compare
                               resolved identities. Needs the platform's
                               DefIds.

6  check.rs, annotations.rs, backend/lower.rs   the remainder.
7  delete the written fields; the dual state ends and the branch can merge.
```

**Do not merge before step 7.** A state where some semantic consumers read
strings and others read resolved identities is exactly what the ruling forbids.

### The criterion for every site

### 3b.3 is intentionally boring

> The 19-site migration should contain **no new type semantics.**

It replaces `Signature`'s written head and argument strings with
`params: Vec<ResolvedType>` and `returns: Option<ResolvedType>`, and moves each
consumer to what it actually needs:

```text
check / infer     semantic ResolvedType
binding           semantic ResolvedType, or a derived StableTypeId
WIT               the ABI projection of a ResolvedType
backend           checked ResolvedType and stable ABI facts
diagnostics       display_name() / written_source()
```

**The criterion for every site**, and it settles the arguments before they
start:

> A consumer asking for the old string because it needs to **print** it is
> fine. A consumer asking for the old string because it needs to **decide**
> something is a defect.

`Builtin` stays a third kind. `List`/`Option`/`Result` have no
user-addressable declaration whose `DefId` is their identity, and synthesising
one would make the representation cosmetically uniform while lying about where
its meaning comes from. If Pleris later makes collections package-defined,
*that* is the evidence for moving them — not tidiness. And `Option<Store>`
resolving while a bare `Option` does not is correct: recognising a constructor
is not having an instantiated type.

```text
4  delete semantic access to written type heads and args                       ...
5  derive StableTypeId from ResolvedType                                      DONE
6  ComponentContract carries the StableTypeId representation                   ...
7  structural ratchet: zero semantic name → TypeDef lookups after resolution   ...
```

**1 and 2, landed 2026-08-21.** `Env::adts` and `Env::ctor_names` are keyed by
`DefId`; there is no by-name entry point; a scrutinee's declared type is
resolved through `ResolvedType` before the environment is consulted. `locals`
carries the `DeclaredType` rather than a rendering of it, because re-parsing a
rendered type is where arguments get dropped.

It found one immediately. `the_environment_spans_every_file_checked_together`
declared `type T` in `module a` and matched a `T` in `module b` with **no
import** — checkable only because the lookup was ambient. That is `PW0021`'s
defect one layer over, and it was holding up the test that makes A-009
testable. The fixture imports now, and its control improved on the way:
checking `b` alone reports `PW0020` for the missing module instead of silence,
so *the type is unknown* is stated rather than inferred from an empty list.

Intermediate work on a branch is fine; **a state where some semantic consumers
read strings and others read resolved identities must not merge.**

### The exhaustiveness control to freeze while migrating

```pleris
module alpha    type Status = Ready | Failed
module beta     type Status = Ready | Waiting
```

A match whose scrutinee resolves to `alpha.Status` must take its constructors
from `DefId(alpha.Status)` — even though `beta.Status` shares the bare name, was
declared later, and overlaps on `Ready`. **Reordering the modules must not
change the answer.** That is what proves `by_adt_name` is dead rather than
wrapped behind a new API.

### Done ahead of the migration, because it had to be

**`StableTypeId::Parameter` is binder + index, not the spelling.**

```pleris
fn id<T>(x: T) -> T        m.id#0
fn id<U>(x: U) -> U        m.id#0
```

> Renaming a generic parameter should not change the public semantic contract.

It carried the name for one commit, which would have made `T` → `U` a change a
host could observe in an authority artifact. `ResolvedType` carries the binder
too, and `same_as` compares binder and position — so α-equivalence holds at the
semantic layer as well, not only at the boundary. A parameter with no binding
declaration is **Blocked**: `T` alone is not an identity.

**And `ResolvedType` lost its derived `PartialEq`.** It included `origin`, so
two uses of one type at two spans were `same_as` and not `==` — a second
comparison disagreeing with the first, in the module written to delete exactly
that. `same_as` is now the only one, enforced rather than claimed.

**E9 is REOPENED** (`docs/MILESTONES.md`), with closing gates added rather than
history rewritten:

> If existing machinery already satisfies some, measure and mark them
> satisfied. Don't rewrite anything merely because it was historically
> scheduled under E9.

Measured 2026-08-21, and the column that matters is the second one. **A
representation that CAN express a rule is not a checker that enforces it** —
conflating those is the claim-versus-witness failure that reopened this
milestone, so each gate is scored on what a program actually gets refused for.

```text
gate   representation                  consumers        gate
────   ─────────────                   ─────────        ────
V1     yes, for arity                  call sites       PARTIAL — arity only
V2     ResolvedType::Parameter         none             OPEN
V3     DefId nominal identity          exhaustiveness   OPEN
V4     none                            none             OPEN
V5     recursive resolved-or-blocked   exhaustiveness   OPEN
V6     yes                             —                MET, as a control
```

**V2 has no surface syntax to be tested against.** `fn id<T>(x: T) -> T` does
not parse — `PW0007`. The grammar gives type parameters to `opaque type` and
`effect` and to nothing else, so no callable is generic and the architect's
discriminator `identity<T>(T); identity(Store) → infer T = Store` cannot be
written. Closing V2 needs either callable generics added to the language, or
the gate restated in terms of the generics that do exist — `Secret<C>`,
`Session<S>`, `List<T>`. A language-surface decision, not a repair. Pinned by
`a_callable_cannot_declare_a_type_parameter`.

**V3 and V5 gained a consumer on 2026-08-21 and did not move.** Architect
ruling:

> Don't let "one consumer now uses it" move either gate to complete.

The closing question for V3 is not *can some compiler pass distinguish
`alpha.Tag` from `beta.Tag`* — that is proven. It is:

> Can an ordinary program pass an `alpha.Tag` where `beta.Tag` is required?

Until the compiler rejects that, V3 is open. V5 closes when the **value-type
relations themselves** cannot operate on unresolved or partly resolved types,
not because one other analysis consumes `ResolvedType`.

Recording it this way rather than as six checkboxes is deliberate. Six ticks
against *representation exists* would read as a nearly-closed milestone and
would be exactly as true as E9's original headline.

### Recorded, and deliberately not acted on yet

R-011's history — an unresolved call found in 2026-08-10, a wrong arity found
in 2026-08-20, and its declared invariant unrelated to both — shows that *a
rejected fixture isolates one defect* is not enforceable by checking only the
declared expected diagnostic. The stronger framing:

> **Evidence belongs to a claim, and a witness is valid only if all
> prerequisites for observing that claim are themselves valid.**

Architect ruling: record it, do **not** interrupt the type repair to redesign
the corpus framework again.

### The type-identity ruling, 2026-08-20 — and where it stopped

The architect's sequence after the WIT emission, and its state:

```text
1  canonicalize SessionId as a platform principal        DONE
2  nominal compatibility by resolved DefId               PARTIAL — see below
3  resolved semantic types in contract.signature          ...
4  every type argument used by a signature must resolve   ...
5  re-run corpus and evidence; open a corpus version
   rather than weakening the rule                         ...
6  regenerate WIT                                         ...
7  same-core-flattening component mutation control        ...
8  resume Canonical ABI adapters                          ...
```

**1 — done.** `examples/domain.pw` no longer declares `SessionId`; 25 files
import the platform's:

> "current session" is supplied by the execution environment. The application
> doesn't get to independently define what identity type the platform's current
> session has.

```wit
before   read: func() -> capability-session-id;
         add:  func(arg0: domain-session-id, ..) -> ..;

after    read: func() -> capability-session-id;
         add:  func(arg0: capability-session-id, ..) -> ..;
```

The `capability` → `principal`/`context` rename the architect also flagged is
**not** done: it is naming, and it should not move in the same change as the
identity fix.

**2 — the premise did not hold.** The step says to make opaque/nominal
compatibility use resolved `DefId` rather than representation. There was no
compatibility check to change: **no call site was type-checked at all.**

```text
fn takes_str(s: String) -> Int      takes_str(42)              accepted
fn wrong_return() -> String { 42 }                             accepted
fn wrong_arity(a: Int, b: Int)      wrong_arity(1)             accepted
fn takes_store(s: Store)            takes_store(makes_cart())  accepted
```

The narrow finding — two `opaque type SessionId = String` interoperating — was
a symptom, and an opaque-identity rule would have produced the right verdict on
it from a mechanism unrelated to the gap.

**`PW0604`, call arity, is the first piece of the repair.** Arity first because
it needs no inference and could therefore be correct against the whole corpus
immediately. Argument **types** are the next piece and are deliberately absent:
a rule that fires on some mismatches reads as a rule that catches them.

It found `Carts.clear(tx, current_session())` against a one-parameter `clear`
in **three files** — R-011 and two copies in the generality and rules
witnesses.

**The ordering question is answered by the code, not by judgement.** I asked
the architect whether the checker comes first or steps 3–4 do. Attempting the
next piece — argument types — settles it:

```rust
pub struct Signature {
    pub returns: Option<String>,        // the return type's HEAD
    pub returns_args: Vec<String>,
    pub params: Vec<Option<String>>,    // each parameter's declared type HEAD
}
```

A parameter's type is a **written head string**. Comparing an argument against
it would fail twice over, and both failures are ones this project has already
recorded:

```text
domain.SessionId vs capability.SessionId    one written head, two declarations
List<MenuItem>                              the head is `List`; the argument is gone
```

The first is exactly the defect that started this sequence. The second is the
bug `Interface::of`'s own comment records — *"reading only `p.ty` gave `List`
for `List<OpenTransaction>`"* — and it is the whole reason `DeclaredType`
exists.

So an argument check written against today's `Signature` would reproduce the
defect it is meant to catch. **Step 3 is a hard prerequisite for the rest of
step 2**, not an alternative ordering:

```text
3  resolved semantic types in Signature / contract.signature
4  every type argument a signature names must resolve
2' argument type checking, consuming 3
```

A narrow interim check — only where both sides are primitives, catching
`takes_str(42)` — is deliberately **not** taken. It is the shape refused in
`PW0604`'s own commit: a rule that fires on some mismatches reads as a rule
that catches them.

**And a milestone claim with no witness.** `docs/MILESTONES.md` records E9 —
*permanent value type checker* — as COMPLETE, and charter §14 M9A lists
`unification-based inference` and `opaque nominal types`. E9's evidence
witnesses what it covers and none of it witnesses call-site typing. Recorded,
not reopened unilaterally.

### The ruling, 2026-08-20 — Pleris is the ABI authority

The block is resolved, and not the way I proposed. I suggested splitting
emit-vs-check by `owner`. The architect's ruling:

> **Do not choose "emit vs check" based on `owner`. Choose it based on which
> artifact is the ABI source of truth.** For everything currently in the
> project, the Pleris declaration should be the source of truth, including
> `pw:host/session#read`.

```text
Pleris-declared callable
        ↓
canonical semantic signature
        ↓
ABI lowering
        ↓
canonical WIT signature
       ↙ ↘
 emitted WIT   core/component adapters
```

for application-owned and platform-owned interfaces alike. The deployment
**implements** the emitted interface; it does not independently specify what
that interface means.

`owner` stays, and is a different question from ABI source:

```text
owner            who controls this API's namespace and lifecycle
ABI source       which representation is authoritative for its signature
implementation   who actually executes it at deployment time
```

so `store:data/carts#add` (owner: application, ABI source: Pleris,
implementation: the deployment host) and `pw:host/session#read` (owner:
platform, ABI source: Pleris, implementation: the Perfect Web host) are both
coherent. ADR-0018 is intact: the compiler emits a data artifact and the host
consumes it, without linking the compiler.

An `abi_source` enum is **not** added yet — every callable is
`PlerisDeclaration` today. A genuinely foreign API whose owner hands us a WIT
package would reverse the direction, and that path is not built during E10-A.

The invariant:

> **One interface has one ABI authority.** Never a Pleris signature plus an
> independently authored WIT signature with a comparison keeping them
> synchronized.

### The component-level audit is mandatory, as a fourth layer

Core Wasm ABI equality is not component ABI equality — the two derivations
flatten identically, so the validator can never see the difference. The audit
becomes:

```text
1  identity            is this exact ImportId permitted?
2  component ABI       is this exact component-level parameter/result shape?
3  authority           operation.required_capabilities
                          ⊆ component.required_capabilities
4  admission           component.required_capabilities ⊆ node grants
```

The current mismatch becomes the permanent negative control, because it is an
unusually good one: two wrong signatures whose core flattenings coincide.

### Two signatures on a callable, one derived from the other

```text
CallableImport
├── id
├── owner
├── semantic_signature
├── component_signature      DERIVED, never authored
├── required_capabilities
└── binding
```

with each layer consuming the right one — type/effect/privacy from the
semantic, WIT emission and the component audit from the component, adapters
from its flattening. Information is lost in one direction only, and it is never
inferred upward:

```text
semantic type → may erase privacy metadata → component type → may flatten → core type
```

### The sequence

```text
1  Pleris declarations canonical for all host-supplied operations  DONE
2  generate deployment-facing WIT from them                        DONE
3  handwritten WIT stops being authoritative                       DONE
4  explicit ABI representation for privacy-qualified types         DONE
5  delete `effect .. host ..` completely                           DONE
6  component-level ABI mutation control, same core flattening      NEXT
7  one lowering pipeline: semantic → component → core               ...
8  generate adapters from that single lowered result                ...
9  component-wrap with upstream `wit-component`                     ...
10 validate and compare COMPONENT import types, not core            ...
11 invocation-region allocation, then E10-I                         ...
```

**1–3, as emitted.** `pw emit-wit` now produces one file holding four packages:
`pw:app` (the worlds and export interfaces), `pw:types`, and the host packages
`pw:host` and `store:data` generated from the Pleris declarations.

```wit
package store:data {
    interface carts {
        use pw:types/types.{domain-cart, domain-cart-error, domain-session-id, ..};
        add: func(arg0: domain-session-id, arg1: domain-menu-item-id,
                  arg2: domain-positive-int) -> result<domain-cart, domain-cart-error>;
    }
}
```

The types moved into their own package because a `pw:app` world imports
`store:data/carts`, so types living in `pw:app` would make the two packages
depend on each other. Dependency packages are nested and **unversioned** — a
`use pw:types/types` does not find a nested `pw:types@0.1.0`.

`tests/support` no longer carries a hand-authored `PLATFORM_WIT` or
`APPLICATION_WIT`. The resolver states the ruling directly: a stand-in beside
the generated package is *"package `pw:host` is defined in two different
locations"*. A deployment still writes an implementation by hand; what it no
longer writes is the signature.

**And the emitted artifact now shows the two-`SessionId` incoherence**, which is
the point:

```wit
pw:host/session      read: func() -> capability-session-id;
store:data/carts     add:  func(arg0: domain-session-id, ..) -> ..;
```

`add_to_cart` passes the first into the second. That was invisible before and is
now in a published interface — an improvement, not a regression, and still with
the architect.

4 and 5 came first because 1–3 need them: `pw:host/session#read` could not
render at all until a privacy qualifier had an ABI rule, and the effect clause
was actively producing wrong answers.

**Step 4, as ruled.** `AbiRepresentation(Session<T>) = Transparent(AbiRepresentation(T))`,
read from `labels::label_of_type` — the one place that says what a privacy
qualifier is — so the set is authored once. The crucial restriction holds:
opacity is **not** ABI transparency, and a generic opaque type that is not a
qualifier still has no WIT form. The semantic contract keeps
`Session<SessionId>`; WIT sees `capability-session-id`.

**Step 5, as ruled.** The clause is gone from the effect vocabulary, the
ontology's unused field is deleted, the fixtures no longer carry it, and
`PW0332` refuses it rather than ignoring it — because semantically dead syntax
survives for a long time if it still parses.

### Three findings the same work turned up

**Two `SessionId` types, never compared.** `examples/domain.pw` and
`packages/pw-platform-web/capability.pw` each declare `opaque type SessionId =
String`, and `add_to_cart` pipes the platform's into the application's. Two
distinct opaque types passed one for the other produce **no diagnostic** —
general, not specific to this pair. Emitting the WIT would publish
`read: func() -> capability-session-id` beside
`add: func(arg0: domain-session-id, ..)`, which is a faithful projection of an
incoherence the Pleris program already contains. Pinned by
`two_opaque_types_of_one_name_are_not_compared_at_a_call`.

**The semantic signature is less precise than the component one.** The contract
stores the *written* name `SessionId`, unqualified, so it cannot tell the two
apart — while the WIT projection can. That inverts the intended one-directional
loss and should be decided rather than inherited.

**A label's type argument was never required to resolve.** `context.pw` wrote
`Session<SessionId>` without importing `SessionId`, because a privacy label
compares written spellings and never looked the argument up. Putting the
operation on the ABI forced the question; the import is now there.

**Step 5 is a constraint, not a convenience.** Architect ruling:

> Do **not** hand-implement the Component Model binary format. Pleris owns the
> lowering facts; upstream owns the component-format encoding.

Step 6 is what makes step 5 checkable: a component this repo wrote and this repo
parsed proves the two agree, which is not the claim.

### E10 follow-up: `store:data/carts#*` should probably not stay external

`Carts.add`, `Carts.clear`, `Carts.current`, `Stores.get` and `Menus.for-store`
are marked host-supplied. That was the minimum that let the store encode, and
the ownership field now records that they are the **application's** operations
rather than the platform's — but it does not make them the right design.

Architect ruling, 2026-08-20:

> Do **not** let `pw:host/carts` become the permanent standard-library design
> merely because it was the first thing that made the demo executable.

The likely answer is that `Carts.add` is **compiled Pleris** calling a narrower
platform data primitive, so the application's cart semantics live in the
application and the platform publishes something smaller and more general. That
is step 10 above: it happens after `add_to_cart` runs end to end, because until
then there is no way to tell a design improvement from a change that broke the
only executable path.

Deferred deliberately, and not blocking: the current arrangement is honest about
what it is, which is what step 1 bought.

The reason those four and not arbitrary cleanup, in the architect's words:
`draw(ctx)` contained a name with no binder, `measure(el)` could have its
syntactic category changed by spelling, and an optimistic transition could
return the wrong resource type — all cases where the Backend IR would otherwise
faithfully encode a program whose meaning the front end never established.


Architect ruling, 2026-08-09: **narrow application surface, general backend
spine.** Compile one real command end-to-end, and build no stage around it.

```text
checked Pleris HIR
      ↓
Backend IR                 DONE — backend/ir.rs
      ↓
Wasm core module           NEXT — wasm-encoder, lowering owned here
      ↓
ComponentContract + WIT    done (E8)
      ↓
Wasm Component
      ↓
E8 host                    done
      ↓
add_to_cart actually runs  E10-I
```

**The IR and the HIR→IR lowering are done.** Nothing in either is named: a call
is a `DefId`, a type is a `DefId`, a host call is the CONTRACT's `CapabilityId`.
`the_backend_never_decides_from_a_name` is the structural guard.
`Lowering = Lowered | Unsupported | Blocked` is the ruling's poison discipline —
`nothing_lowers_silently` is what says an unknown construct cannot become an
empty instruction list.

### The finding that stopped `add_to_cart` — repaired 2026-08-10

`examples/store/app.pw` called `current_session()` and **never imported it**,
and `pw check` reported nothing: `unresolved_uses` only examined dotted paths
whose head looks like a module. The backend was the first consumer for which the
absence was fatal rather than quiet.

It was one of four, and the repair took a whole sequence of the architect's
because resolving a name that never existed can move much more than name
checking. Everything below is done and every movement is classified in the
frozen matrices:

```text
tests/policy_consumers.rs       which analyses see a POLICY value    4 of 6 do
tests/evidence_reachability.rs  can each fixture witness its claim   9 of 24 could not
src/policy.rs                   what a policy head's value MEANS     34 heads
tests/semantic_ownership.rs     who owns each call-shaped thing      Unowned = 0
src/check.rs                    PW0021 examines bare calls           23 more found
```

`the_real_add_to_cart_lowers_to_two_host_calls` is the result:
`session.read` and `database.write<Carts>`, named by the contract's
capabilities rather than by matching a spelling.

### Next, in order

```text
1  DONE 2026-08-11. PolicyExpr / TermExpr — the half that remained. Declaration-level
   policy values are typed by domain and the executable ones are real
   terms (`hir::Policy::term`, ADR-0024). What is still in the
   executable body tree is the BODY-EMBEDDED policy statement: a page's
   `placement origin` / `cache private`, a `handler_policy { .. }`, a
   `replicated { .. }`. Those lower as flat `Name` siblings, which is
   what `tests/policy_consumers.rs` measured — four of six consumers
   cannot tell them from terms. It needs the grammar to emit a
   `PolicyList` inside a UI declaration's body; `src/policy.rs` is the
   table it must be driven by. `Domain::Body` — `acquire`, `release`,
   `draw` — must STAY executable, and its binder must start binding
   (`a_for_loop_is_syntax_that_binds_and_a_policy_block_is_not_yet`).
2  DONE — E8 evidence regenerated against C5 and C6
3  DONE 2026-08-10 — `backend::lower::Checked`. A resolved-program
   invariant carried by a type with a private field, not a convention:
   `program` takes `Checked`, and `Checked::of` runs the checker on the
   same units. It found three of this file's own controls measuring the
   backend against programs `pw check` rejects.
4  Wasm encoding of the IR — wasm-encoder, ours to lower
5  E10-A invocation-region memory (NOT the final memory model)
6  E10-I, with the ruling's controls, including the structural test
   that the Rust closure path is GONE
7  DONE 2026-08-11 — E10-P closed. `contract.rs` calls
   `check::declaration_label`. A `session query` was placeable at
   build time and is not.
8  broaden from evidence: Store query, then a pure helper, then a
   command with branching
```

### Parser defects the audits found

```text
for (i, v) in xs { .. }    FIXED 2026-08-10 — K::ForExpr, and the
                           pattern binds
measure(el)                lowers to Expr::Keyword because `measure`
                           is in STMT_KEYWORDS, and a Keyword
                           contributes no effects — so the same call
                           charges a page for a database read or for
                           nothing, depending on the callee's spelling
draw(ctx) { .. }           the policy block's binder binds nothing;
                           belongs to the PolicyExpr split
```

`measure` is in `docs/RISK_QUEUE.md` and is unrepaired. It is the twin of
the `for` defect from the other direction — there syntax was promoted to
a call, here a call is demoted to syntax — and both are decided by a word
list rather than by what the callee is.

### Two obligations recorded rather than implied

```text
ADR-0024   nothing checks that an `optimistic` lambda's parameter is
           the resource the command returns. An optimistic transition
           over the wrong resource is a bug the compiler should catch.
ADR-0024   `optimistic` / `rollback` are EXECUTION CONTEXTS — placement
           browser, before the round trip and after a failure. The model
           does not exist, so an embedded term is resolved and excluded
           from the declaration's effect row. When it lands, the term's
           effects attach to the context rather than to nothing.
```

---

## Now: E9 — the permanent value type checker and effect compiler

E8 closed 2026-08-08 with its gate amended by ruling (ADR-0023). E9's gate,
**measured before building anything**:

| # | gate item | state |
|---|---|---|
| 1 | The own compiler accepts the accepted corpus and rejects the rejected corpus | **met** — 24/24 accepted clean, 46/46 rejected each for its declared invariant |
| 2 | Differential tests match Koka for the common semantic subset | **thin** — 5 tests in `differential_vs_koka.rs`. They cover the four places ADR-0011 says Koka is WRONG, which is the opposite of a parity claim. See below |
| 3 | Full store demo builds without Koka | **met** — no crate depends on Koka; `just ci` never invokes it |
| 4 | Incremental checks fast enough for editor feedback | **met, and not by the route the charter assumed.** See below |
| 5 | No compiler panic in the current fuzz corpus | **met** — 6 targets, 3,600 executions, 0 findings (`just fuzz`) |
| 6 | Koka optional as a conformance tool, not a build dependency | **met** — same measurement as 3 |

### Item 4 — measured, and the answer changes the plan

`just e9-latency`, median of 7 in-process runs on the 36-file store program:

```text
cold      37 ms      the whole program from source text
edit      37 ms      the same, one file's text changed
reject    37 ms      with a diagnostic to produce
```

**There is no query system, so an edit costs a full check — and a full check is
37 ms.** The cross-cutting task "use incremental compiler queries so edits
recompute only affected results" is therefore an *optimization*, not a gate
requirement: the gate asks for editor-feedback latency and the non-incremental
path already delivers it at this corpus size.

Recorded before any query system exists, which is the point — an instrument that
arrives with the optimization it measures cannot say what the optimization did.
The row worth watching is `reject`: editor feedback is worth most when the
program does not compile, and a checker that is fast only on the happy path
would be fast exactly when nobody needs it.

**What would change this:** a corpus an order of magnitude larger. The number to
beat is in `docs/evidence/E9/check-latency.txt`.

### Item 2 — the real gap, and it is not what it looks like

`differential_vs_koka.rs` has five tests and every one of them asserts a
DIVERGENCE:

```text
nominal wrappers stay distinct where Koka erases them
Option and List stay distinct where Koka conflates them
pw rejects the match Koka accepts under exn
the result does not depend on any effect row
```

Those are ADR-0011's findings — the reasons Koka is an effects-only oracle — and
they are valuable. They are not "differential tests match Koka for the common
semantic subset", which is a claim about AGREEMENT and has no test at all.

The other direction is now built — `just e9-parity`, `scripts/e9-parity.sh` —
and **its finding is that the common subset is EMPTY for the corpus.**

```text
agreed       0
disagreed    0
skipped     24    not in the common subset
```

Both controls fire, so the harness works: a non-exhaustive match under `total`
and an ill-typed term are each rejected by the real Koka 3.2.3. What it measures
is a bridge that reaches exactly one hand-curated file. 23 of 24 accepted
fixtures emit no function body at all, and the one that does — `A-001` — names
types declared in `domain.pw`, which `emit-koka` neither emits nor imports.

**Counted as skipped, never as a disagreement.** A type the emitted module does
not declare is a limitation of the BRIDGE, not a disagreement between the
languages, and reporting it as one would be a false finding about Koka.

### Gate item 2 — RETIRED and replaced, 2026-08-08 by ruling

> Do not expand `emit-koka` just to satisfy a historical parity gate. Koka
> helped answer the research questions we needed it to answer. Pleris no longer
> needs to resemble Koka broadly enough for ordinary application parity, and
> pretending otherwise would make the test suite less honest rather than more
> rigorous.

The empty common subset is not a harness problem. It is evidence the gate went
obsolete when ADR-0011 made Koka an effects-only oracle. The replacement, now in
the charter:

> **E9-K — Koka effect-oracle conformance.** For the effect-language subset
> intentionally shared with Koka, Pleris agrees on higher-order effect
> propagation, row polymorphism, pure/total computations, and selective effect
> discharge. Every recorded intentional divergence remains explicitly tested as
> a divergence.

```text
ordinary Pleris corpus        no Koka parity claim
effect-semantics oracle cases differential against real Koka 3.2.3
known intentional differences must remain different, for the recorded reason
```

`scripts/e9-parity.sh` keeps its controls and its finding; what it no longer
does is claim the corpus should have compiled. The multi-module backend is NOT
built — it would have spent implementation effort making an obsolete test
meaningful.

### Three defects the harness found on its first run

All three were invisible while the oracle pointed only at
`examples/koka/pricing.pw`, which was written to work with it.

- **`type_name` dropped a type's arguments**, so `List<CartLine>` emitted
  `lines : list` — which Koka rejects for the arity it needs. The **third**
  instance of read-the-head-and-drop-the-arguments found on 2026-08-08, after
  `Interface::of` and `wit.rs`'s record fields. Fixed, through the same
  `wit::written` all three now use.
- **`type_name` separated on every uppercase character**, so `USD` became
  `u_s_d` — three type names where the program has one. Visible only once
  generic arguments were carried through and `Money<USD>` reached it at all.
  Fixed with the rule `wit::ident` already used.
- **`emit-koka` is single-module and emits no imports.** Recorded rather than
  fixed: multi-module lowering is a real piece of work and ADR-0015 scopes the
  backend deliberately.

`spikes/pw-to-koka` still passes all three of its checks, so the fixes did not
move the one program the bridge did reach.

---

## Now: the architect's sequence of 2026-08-07 (second ruling)

Ten steps. 1-4 and 10 are done; 5-9 are E8's remaining work.

The language is **Pleris** (ADR-0021); Perfect Web stays the project, `.pw` the
source format, `pw-*` the internal crates. Whether the developer-facing CLI
becomes `pleris` rather than `pw` is undecided and purely mechanical —
`justfile`, docs, evidence scripts — so it blocks nothing.

| # | task | state |
|---|---|---|
| 1 | Delete program-wide-unique resolution | **done** — audit landed; prelude deferred by ruling |
| 1a | Repair the two `forbidden` last-segment sites | **done** — ratchet at 0 |
| 2 | Build-time diagnostic for an unresolved capability argument | **done** — `PW5200`, `Owner::Capability`, 8 controls. Unknown FAMILY and OPERATION still need the declared capability table (E9) |
| 3 | Placement consumes `effective_effects` | **done** — both discriminating controls, and it found a witness proving an annotation |
| 4 | The deployment planner abstraction | **done** — `runtime/pw-host/src/plan.rs` |
| 5 | Freeze `ComponentBinding` / remote-capable semantics | **done** — `pw-core/{boundary,binding}.rs`, `BindingSupport` on each `Export` |
| 6 | Generate WIT worlds from semantic contracts | **done** — `pw emit-wit`, resolved by `wit-parser` |
| 7 | Typed linking only from `Granted` | **done** — `engine::instantiate`, the engine refuses |
| 8 | Run real `add_to_cart` through Wasmtime | **partly** — authority decided by `admit`; the BODY needs a Pleris→Wasm backend that does not exist |
| 9 | Fuel and memory limits | **done** — `Limits`, per instance, from policy |
| 10 | Replace `worlds_for` with declared node topology | **done** — deleted 2026-08-07; placement is the declaration's own clause |

**Revised 2026-08-07 by ruling: pull the effect/capability ontology forward.**

> Do not wait for full E9 to declare effect families and operations. Pull
> forward only the effect/capability ontology before E8 hardens WIT.

`PW5200` can catch an unknown ARGUMENT and cannot catch an unknown FAMILY or
OPERATION, because nothing declares what a family is — `log` and a misspelled
`databse` are indistinguishable. That distinction must exist before WIT
generation, and it does NOT need E9's permanent inference algorithm.

A platform-level declaration model, sketched by the architect:

```text
effect log                { capability none }
effect database.read<T>   { capability database.read<T>
                            host_interface pw:host/database#read }
effect style.mutate<T>    { capability dom.style.mutate<T> }
```

giving

```text
source effect spelling → EffectDefId + resolved arguments → EffectInstance
  → effective_effects(DefId) → optional capability lowering → CapabilityId
  → ComponentContract → WIT
```

and separating three facts that are currently one string:

```text
Effect           something computation does
Capability       authority required to do it
Host interface   one ABI representation of that authority
```

**Not every effect needs a host capability.** After this lands, unknown family,
unknown operation and unknown argument are all compile errors, legitimate
non-authority effects stay valid *because they are declared*, and no checker
contains a hard-coded list of valid family spellings.

### The grammar decision — MADE, and the form LANDS

Architect ruling, 2026-08-07: **flat dotted**, with an effect-specific
qualified name.

```pleris
effect log {
    capability none
}

effect database.read<T> {
    capability database.read<T>
    host       "pw:host/database#read"
}
```

> The declaration should look like the thing that appears in an effect row. […]
> But implement `database.read` as an effect-specific qualified name, not as a
> new ability for arbitrary declarations to have dotted names.

**Slice 1 is done** (`tests/effect_declarations.rs`, 9 controls): the form
parses, lowers to `DeclKind::Effect`, and takes its own `Namespace::Effect` so
`effect log` and `fn log` coexist. `type foo.bar`, `fn foo.bar()` and
`query foo.bar` all still fail to parse — asserted, because a local grammar
change that quietly went global is the thing the ruling warned about.

`capability` and `host` are POLICY clauses, not expressions: `database.read<T>`
is a name and the expression grammar reads `<` as a comparison, which is the
same reason a `materialize` block's policies live inside its braces (E6).

### What provenance covers so far

`pw-core/src/provenance.rs`: `Fact { kind, origin, depends_on }`, an `Evidence`
DAG, and `Route` — `Qualified`, `ReceiverType`, `ScopedName`, and
`ProgramWideName`, which is deleted from the compiler and **kept nameable so a
test can forbid it**. A route with no name cannot be forbidden, and that is the
route every instance of coincidental correctness so far has travelled by.

One consumer: effect inference's member resolution records the receiver-type
edge and the callback it travelled through.

`tests/causal_evidence.rs` asserts R-037's claim as a chain — required edges,
the forbidden source, and a semantics-breaking control (a receiver whose type
has no such member produces no chain at all).

**Verified against the defect it exists for:** unbinding the callback parameter,
as before `3a0f319`, turns the two chain assertions red. Two others stay green,
which is correct — they are about the forbidden route and the breaking control.

Still to record: privacy labels, placement demands, capabilities, resource
invalidation. Tier 1 in ADR-0022, and none of them recorded yet.

### Slice 2 — and it is the first consumer of ADR-0022

Architect ruling, 2026-08-07: **do not delay slice 2 for the provenance work —
build it into slice 2.**

> So slice 2 becomes both: completion of the effect ontology; and the first
> production implementation of semantic provenance.

Each transition below must produce evidence, and a test must assert that no
later analysis splits `"database.read"` at the dot.

### The immediate sequence — the effect ontology is COMPLETE

```text
 1  package-declared Effect prelude                 DONE
 2  pw-platform-web exports through it              DONE
 3  effect arguments obey normal scope              DONE — 31 rows, C4 opened
 4  delete TypeArgument::Unscoped                   DONE — A-017 retired
 5  finish EffectPath → EffectDefId → EffectInstance DONE
 6  unknown family / operation / arity diagnostics  DONE — PW5201/5202/5203
 7  remove frame-phase intrinsic_effect             DONE — R-042 repaired,
                                                     phases_at reads the chain
 8  add database.connect                            DONE
 9  database.write becomes database.write<T>        DONE
10  declaration-driven capability/host lowering     DONE
11  re-run the ComponentContract matrix             DONE
12  semantic effect facets; phase rules consume     DONE — the family check
    them; delete the family check                    is deleted
13  deployment planning                             see the sequence below
```

### The architect's sequence of 2026-08-07, fifth ruling

```text
1  unresolved facet markers are a build error   DONE — PW5204
2  canonical policy-keyword coverage guard      DONE — directional, with the
                                                 mutation that motivated it
3  session.read: capability + topology, not     DONE — no intrinsic placement
   guessed placement
4  freeze the placement migration matrix        DONE — placement_migration.rs
5  deployment planning on ontology + topology   DONE — pw-host/src/plan.rs
6  delete World::worlds_for                     DONE — see below
7  boundary-transfer local/remote binding feasibility   DONE — see below
8  WIT generation                                       DONE — see below
```

### Step 6, as it landed

`World::worlds_for` is gone. Where an effect is meaningful is the `placement`
clause on its declaration and nothing else. The evidence is
`docs/evidence/E8/placement-migration.txt`, produced by `just e8-placement`.

**The lookup is three-valued**, on the ruling:

> Do not turn `ontology lookup failed` into `no placement restriction`. That
> would recreate the `secret<Payments>` hole in a new form.

```rust
pub enum PlacementLookup {
    Known(Vec<World>),          // the declared constraint
    Unrestricted,               // declared, and declared to constrain nothing
    Blocked { code: &'static str },  // nothing declares it; placement stops
}
```

`Option<Vec<World>>` could not say that — `None` read as both *anywhere* and
*I don't know*, and the deleted table collapsed them. `World::grants` returns
`Grant::{Yes, No, Blocked}` for the same reason, so every consumer had to say
what it does about the third case rather than inheriting a `bool`.

Seven consumers, all migrated, and `contract.rs`'s fallback deleted rather than
threaded. Two things fell out of the deletion:

- `Capability::resolve(effect, types)` became vacuous — it passed an empty
  ontology, and without a table underneath, every call could only answer
  `Undeclared`. It survived exactly as long as there was a table to answer for
  it, which is the argument against having had one. Deleted;
  `resolve(effect, types, ontology)` is the only form.
- `check_effect_rows`'s empty-ontology exemption is gone, on the ruling that
  single-file strictness is correct. What made the silence look harmless was
  the same table: an undeclared effect still had a placement and a capability,
  so nothing downstream noticed the row had never resolved.

**Results.** Every row in `placement_migration.rs` unchanged, and
`docs/evidence/E8/component-contracts.{json,txt}` reproduced byte for byte. One
row moved, in `effect_vocabulary.rs`: `post_paint` joined the
placement-constrained-but-authority-free list, because the old test asked the
table and the table had no `post_paint` entry to speak with. A statement of what
the declarations say, now that they are the only thing that says it.

**The negative control the ruling asked for by name** is
`an_undeclared_effect_gets_no_placement_answer`: `dom.thing` — a family the
deleted table DID restrict, with an operation nobody declares — must not acquire
a placement. It proves the deletion is semantic and not merely that the table
stopped being called in today's fixtures.

**And the structural gate**, `no_source_file_maps_an_effect_family_to_a_world`:
no file under `compiler/pw-core/src` may put a quoted family name on a line with
a `World` variant. The scan stops at each file's `#[cfg(test)]`, because two
test doubles live past it and exist precisely because the code under test no
longer carries a table.

**Step 5's shape**, settled:

```text
candidate nodes = contract.allowed_placements
                  ∩ nodes satisfying required_capabilities
```

`admit(component, node)` stays local; the planner composes those local facts and
never makes `admit` recursive.

**Nothing in the effect ontology is outstanding.** The chain is:

```text
EffectPath + type arguments
        ↓  ontology.rs, the only place a dotted effect name is split
EffectDefId + resolved TypeArguments
        ↓
EffectInstance + semantic facets
        ↓
placement (declared)   capability (declared)   phase legality (facets)
        ↓
CapabilityId → HostInterface → WIT
```

Three structural gates hold it: `last_segment.rs` (no second place splits an
effect name), `no_source_file_maps_a_phase_keyword_to_an_effect_spelling` (a
phase creates an execution context, never an effect), and
`no_effect_is_written_at_two_different_arities`.

### Step 7, as it landed

One boundary-transfer analysis, two policies over it, per the ruling:

```text
                  boundary-transfer analysis      pw-core/src/boundary.rs
                     /                 \
          resume-capture policy      remote-call policy
             resume.rs                  binding.rs
```

`TypeFacts` — is this type a resource, is it produced only by a scoped
declaration — MOVED out of `resume.rs` rather than being copied.
`can_cross(profile, context)` is `Proven | Violation | Blocked`, the same three
answers as `PlacementLookup` and for the same reason.

**Privacy is where the two policies differ, and the difference is principled.**
A session-scoped value may not enter a resume manifest, because the manifest
ships with the document and there is no destination to check. It may cross a
remote call, because there IS one and `World::may_hold` plus the topology
already decides — a second answer here would be the duplication the module
exists to prevent.

`BindingSupport { local, remote }` on each `Export`, not a seventh contract
field: remote capability is a property of an interface edge, so a component
with a remotable query and a handle-passing helper says so about each. Every
position is checked in both directions, results and errors separately —
`recover() -> Result<StoreId, OpenTransaction>` succeeds with a key and fails
with a handle, and a check reading only the success side calls it remotable.

`plan.rs` composes the two independent facts. Four combinations, one failure:

```text
co-locatable + transferable      bind it either way
co-locatable + NOT transferable  a same-process call passing a handle
necessarily remote + transferable   an RPC
necessarily remote + NOT           no binding exists — the only failure
```

**A defect the instrument did not catch and a probe did.** `binding_support`
looked its signature up through `Signatures`, which covers `fn`, `query`,
`command`, `subscription`, `resource` and `task` — and not `page`, `view` or
`component`. Every page export missed the lookup and took the permissive
default, coming out `Transferable`: the right answer for this corpus, from a
mechanism with nothing to do with its types. Every test passed. It reads the
declaration now, which every component kind has. `a_page_returns_nothing_and_
that_is_not_undetermined` is the guard, and it also fixes the second half —
`returns: None` means nothing crosses outbound, not that the build could not
tell.

Evidence: `docs/evidence/E8/binding-feasibility.txt`, via `just e8-binding`.

### Step 8, as it landed

`pw emit-wit` writes a WIT package for a checked program: one `world` per
`ComponentContract`, one `interface <id>-api` per component's exports, and one
`interface types` holding what those signatures name. Committed at
`docs/evidence/E8/store.wit` via `just e8-wit` — 10 worlds, 15 types on the ABI.

Both halves of the gate are tested.
`imports_are_exactly_the_contracts_imports_and_the_projection_inverts` checks
the first in both directions; `wit_parser_resolves_the_generated_package`
checks the second with **`wit-parser`**, the crate `wasm-tools` and
`wit-bindgen` are both built on. A dev-dependency, and already in the lock file
via wasmtime — a WIT reader written here would be a second implementation of
somebody else's format, and the format is the entire product.

**The host's WIT is a test fixture, deliberately.** A world imports
`pw:host/database`, whose signatures the host owns and the compiler must not
invent. The fixture's existence is the statement: a deployment has to publish a
WIT package for the capabilities it grants, or these worlds do not resolve.
`the_resolver_would_reject_a_world_naming_an_interface_nobody_publishes` is the
control that makes that mean something.

**Two defects the real parser found, both invisible to anything else:**

- Record fields read only `f.ty`, the type HEAD, so `List<MenuItem>` emitted
  `list` with nothing in it. The same gap was in `Interface::of`, where it was
  worse and silent: a position typed `List<OpenTransaction>` looked up `List`,
  found no resource, and came out transferable. `TypeFacts::profile` now walks
  every nominal component rather than the head.
- Types were keyed by bare name, so `web.capability.SessionId` and
  `domain.SessionId` both became `session-id`. `wit-parser` refused the package
  for a duplicate definition. It refused — a generator that had deduplicated
  instead would have put one type on the wire where the program has two. Keyed
  by qualified path now, and every reference resolves through the workspace.

Refusal rather than invention throughout: an unmappable type is `Unmappable`
and no output, and two names mangling to one identifier is `Collision`. Only
types an export can transitively reach are emitted, so the platform's
`Decoder`, `Style` and `ElementRef` stay off the ABI.

### Step 13 — deployment planning

`docs/NEXT.md`'s older E8 table item 4, and now unblocked: the compiler emits
`ComponentContract`s carrying declared capabilities and declared placements,
and `runtime/pw-host` admits against a `Topology`. What is missing is the thing
that PLANS — reads a program's contracts and a deployment's topology and says
which component goes where, refusing when nothing fits.

It consumes `allowed_placements` and `required_capabilities` as they now are;
both are declaration-driven, which they were not when the item was written, and
since step 6 there is no hard-coded input left on either side.

### The two rulings this sequence turns on

> **Effect declarations are ambient only because the selected platform package
> explicitly exports them into the Effect prelude. Their arguments are not
> ambient.**

> **`mutate` is a scheduling context, not shorthand for
> `style.mutate<LayoutAffect>`.**

The first is done. The second is step 7.

### Step 7, specified

`effects.rs::intrinsic_effect` maps four frame-phase keywords to effect
spellings, and the ruling is that all four should go:

```text
"measure"    → layout.measure
"mutate"     → style.mutate          (and this one now has the wrong arity)
"post_paint" → paint.post
"animate"    → animation.composite
```

> A frame-phase block says WHEN this body's work executes. An effect says WHAT
> that work actually does. Those are independent.

So `mutate { pure_computation() }` must not claim it mutated style, and
`measure { pure_computation() }` must not claim it read layout. The effect comes
from the operations inside the block.

**Placement still comes from the phase.** `post_paint { pure() }` is
browser-only because the execution CONTEXT has browser semantics, not because a
synthesized effect said so. That constraint wants an `ExecutionContext` concept:

```text
ExecutionContext   when/where scheduled work exists
EffectInstance     what the work does
CapabilityId       authority some effects require
```

The matrix the ruling requires:

```text
empty mutate                   no style effect
mutate + PaintOnly operation   style.mutate<PaintOnly>
mutate + layout write          style.mutate<LayoutAffect>
measure + pure                 no layout.measure
measure + geometry read        layout.measure
```

plus the existing phase-order checks, which must not move.

**Blast radius to expect.** `forbidden_in_phase`, `phase_at` and the corpus
fixtures that turn on phase effects — R-042 (`post_paint` may not measure),
A-022, R-034, R-035 — all read the synthesized spelling today. This is the
change most likely to open another corpus version, and the matrix should exist
before it lands, exactly as it did for step 7 of the previous sequence.

### Slice 2 — what remains### Slice 2 — what remains

1. **Resolve an effect row against the declarations.**

   ```text
   database.read<Stores>
     → EffectPath(["database","read"]) + TypeArg("Stores")
     → EffectDefId + TypeDefId
     → EffectInstance { effect, args }
   ```

   Everything downstream consumes `EffectInstance`. **No checker splits
   `"database.read"` at the dot.**

2. **Declare the vocabulary.** Enumerated below. No wildcards — the ruling is
   explicit: *"A wildcard is convenient documentation but terrible semantic
   identity."* `observe.*` and `durable.*` must become their real operations.

3. **The diagnostics.** Unknown family, unknown operation, wrong generic arity
   — all compile errors, in `PW52xx` beside `PW5200`.

4. **`interface_for` reads the declaration** instead of
   `format!("pw:host/{family}")`, which is the last hard-coded thing in the
   capability path.

### The tests the ruling requires before this is "landed"

```text
effect log                      parses/resolves          slice 1 ✓
effect database.read<T>         parses/resolves          slice 1 ✓
database.read<Stores>           resolves decl + argument
database.read<Stroes>           PW5200                   done
databse.read<Stores>            unknown effect
database.reed<Stores>           unknown effect
database.read                   wrong generic arity
database.read<A, B>             wrong generic arity
foo.read<T> vs database.read<T> never confused           slice 1 ✓
database.read vs database.write distinct EffectDefIds    slice 1 ✓
```

plus the structural one:

> No downstream semantic analysis discovers an effect by splitting or comparing
> its dotted spelling.

which is `last-segment-audit.txt` extended to cover effect spellings.

### The remaining grammar questions — ANSWERED

Three questions, each of which changes the parser, and none of which should be
answered in a hurry:

**1. Dotted effect names — flat, via `dotted_name` in the effect branch only.**
Done. `name()` is untouched.

**2. Type parameters — the SAME binder `opaque type Secret<C>` uses.** Done.
The ruling: *"Don't write an effect-specific generic binder. […] You've already
learned what happens when one representation retains generic arguments and
another quietly drops them."*

**3. Where they live.** `pw-std` for `log`/`trace`, `pw-platform-web` for the
rest. That splits the vocabulary across two packages, which is right — a
non-web host has no `dom.mutate` — but the corpus checks the platform packages
together, so the split is not exercised until something checks one alone.

### The vocabulary that must be declared

From the corpus as it stands, so nothing silently loses its meaning:

```text
database.read<T>   database.write<T>   database.transaction
secret.read<T>     secret<T>
dom.mutate         dom.read            style.mutate<T>
layout.measure     animation.composite paint.custom
observe.*          device.location
network.fetch      cache.read          cache.write
resource.acquire<T>  resource.release<T>
durable.*          log                 trace
```

Roughly twenty-five, and each one wrong is a corpus file that stops meaning what
it says — `LayoutAffect` (`docs/RISK_QUEUE.md` 37) is what one missing
declaration already cost.

### The order to work in

```text
1a  repair the two forbidden last-segment sites          DONE
3   placement consumes effective_effects                 DONE
    the effect/capability ontology above                 NEXT — grammar decision first
4   the remaining unknown-family / unknown-operation diagnostics
5   deployment planner
6   boundary-transfer-derived component binding
7   WIT generation
8   typed linking from Granted
9   add_to_cart through Wasmtime
10  fuel/memory limits (the declared topology replacing worlds_for is DONE)
```

Full E9 still comes after E8. The point of pulling the ontology forward is to
stop E8 freezing today's stringly effect vocabulary into the ABI.

### Step 5's design, ruled 2026-08-07

I asked whether `RemoteCapable` is the predicate the resume manifest already
computes. The answer:

> Remote-capable is not equivalent to resume-serializable. But they are two
> policies over the same underlying semantic fact: whether a typed value can
> safely cross a boundary. Don't build a second `is_remote_capable_type()`
> beside `is_serializable_capture()`.

```text
                  boundary-transfer analysis
                     /                 \
          resume-capture policy      remote-call policy
```

So `pw-core/boundary.rs` owns `transfer_profile(type)` and
`can_cross(profile, BoundaryContext)`, returning `Proven | Violation | Blocked`
— **not a bool**. `resume.rs` adds its version policy; a new `binding.rs` adds
the RPC/ABI policy.

The profile knows: wire schema/codec, privacy label, affine/resource status,
local-only handles, capabilities, schema identity. "Serializable" is too weak a
word for it — `OpenTransaction` could be encoded as a handle number and still
must not be reconstructed elsewhere. The concept is **transferability**.

`BindingMode` as an enum is rejected:

```rust
struct BindingSupport { local: LocalSupport, remote: RemoteSupport }
```

because `Either` is just both, and an enum forecloses "remote ✓ only through a
host-mediated handle proxy".

Remote capability is a property of an **interface edge**, not a component:
`get(StoreId) -> Result<Store, StoreError>` is probably remotable and
`with_transaction(fn(OpenTransaction) -> T)` is not, in the same component.
Arguments, results AND errors are all checked, in both directions.

### A permanent corpus rule, adopted 2026-08-07

After R-037 (`docs/RISK_QUEUE.md` 35):

> Any fixture whose claimed invariant requires propagation across one or more
> semantic edges must contain a discriminating control that breaks one edge and
> causes the target diagnostic to disappear.

R-037 produced *exactly the intended diagnostic for completely unrelated
reasons*, which is worse than an ordinary false positive. Controls to add:
rename the callback parameter (spelling independence); a collection whose
element type has no such member (receiver typing matters); a callback that does
not call the member (the path matters). The same shape applies to privacy
propagation, resource invalidation and placement.

Not yet written. This is corpus work, and it belongs with step 1's audit.

### Step 1, what is done and what is not

**Done.** A member resolves through its receiver's type. `Types::of_body` gives
a callback's parameter the element type of the collection it is applied to;
`effect_rows` consumes that environment instead of building a narrower one;
`param_pattern` binds `fn(el)` parameters, which it never did
(`docs/RISK_QUEUE.md` 35). The program-wide-unique fallback is deleted and the
corpus is 46/46 without it.

**The two items the ruling named, and where each stands.**

1. **The package-declared prelude — DEFERRED, not outstanding.** Architect
   ruling, 2026-08-07:

   > The prelude facility does not need to be invented just to satisfy an old
   > checklist item. Since receiver-directed resolution removed the reason the
   > platform needed ambient visibility, change the status to: prelude design
   > deferred until Pleris actually has a demonstrated need for implicit
   > imports. "The language must have a prelude" isn't itself a goal.

   If a need appears, package metadata defines it and it stays tiny — `Bool Int
   String Option Result List` or similar fundamentals. Implicitly importing a
   whole web-platform module for convenience is the wrong default.

2. **The structural test against global last-segment lookup — DONE.**

   `compiler/pw-core/last-segment-audit.txt` classifies all nine sites and
   `tests/last_segment.rs` enforces it: every site needs a category, a stale
   entry fails, a reason under 40 characters fails, and the scan has its own
   control.

   Categories used: three `display`, two `encoding`, two `scoped`, one
   `syntax`, and **zero `forbidden`**. The ratchet is at 0, which is now a
   ceiling: there is no stock of forbidden sites to hide a new one among.

   The two that were forbidden are repaired (item 1a): `check.rs`'s
   privacy-label lookup resolves through the workspace and `labels.rs`'s
   `declaration_named` is scoped to the module and its imports.

---

## E8's remaining half, in more detail

| # | task | acceptance |
|---|---|---|
| 1 | Generate a WIT world per `ComponentContract` | the world's imports are exactly `contract.imports`, and `wit-bindgen` accepts it |
| 2 | Typed linking from a `Granted` | **done.** `engine::instantiate` populates the `Linker` from `linkable()` and nothing else; the ungranted case fails with wasmtime's own words — *component imports instance `perfect-web:store/stores@0.1.0`, but a matching implementation was not found in the linker* |
| 3 | Run the store's `add_to_cart` as a component | **partly done, and the remainder is named.** The dev server's command path now calls `admit` against a declared topology before doing anything: `add_to_cart` requires `database.write<Carts>` by its contract, and a node without it refuses — the state does not move. The BODY is still a Rust closure. Running it as compiled Wasm needs a Pleris→component backend, which does not exist: `pw emit-koka` covers the pure subset and there is no code generator behind it |
| 4 | Fuel and memory limits per instance | **done.** `Limits { fuel, memory_bytes, table_elements }` beside `Topology` — a deployment's declaration, not a constant. Instantiating the minimal guest costs 16,386 fuel, measured; one byte of memory is refused with *memory minimum size of 18 pages exceeds memory limits* |
| 5 | Retire `worlds_for` in favour of the declared topology | **done 2026-08-07.** The compiler keeps solving placement; where an effect is meaningful comes from its own `placement` clause, and which node grants which capability comes from the deployment's `Topology`. No table in between |

**What is already proved** (`just e8-host`, `docs/evidence/E8/`):

- placement and capability are separate checks that disagree in both directions;
- a capability's type argument is part of what is granted;
- an admitted instance receives its contract, not the node's capability set;
- a handle carries nothing about the value behind it, and one instance's handle
  does nothing in another's hands;
- the ordinary `std` guest is refused for fourteen `wasi:*` interfaces its WIT
  world never declared.

---

## Then: E9, the permanent type and effect compiler

The architect's ruling of 2026-08-07 places it after E8 and says why: E8
*consumes* capabilities and must not define effect semantics. E9 defines them —
real inference, real ADTs, permanent effect rows, and capability/effect lowering
into the `ComponentContract` that E8-0 froze. Koka is retained as a differential
oracle for a while.

Two things E8 found that E9 owns:

- `Inference::known` is keyed by the BARE declaration name, so two declarations
  called `Cart` in different modules share an effect set
  (`docs/RISK_QUEUE.md` 34). Contracts avoid it by inferring from the body;
  nothing else does.
- The placement solver still reads *declared* effect rows in some callers, while
  contracts read inferred ones. One of those is wrong.

---

## Then: E10 onward

`docs/MILESTONES.md` has the register: own backends, deployment, the networking
lab, HTTP/3, Servo, tooling, hardening. None is started.

---

**E1 is closed** on RQ-2's measured Outcome 1. **E1A is algorithmically
implemented, source integration pending.** **E2 is in progress** and now owns
body parsing, HIR, and source-to-checker integration — see `docs/MILESTONES.md`.

---

## The revised plan

Fixed by the project architect after reviewing E2's results. Items 1–3 are done.

| # | task | status |
|---|---|---|
| 1 | Correct the E7 evidence wording — Marko is a resumption/DOM oracle, not a lazy-loading one | **done** |
| 2 | Adopt Rowan | **done** — green tree, `SyntaxKind`, invariants carried over, 68/68 corpus files round-trip |
| 3 | Write the formatter ADR, do not implement | **done** (ADR-0013) |
| 4 | Implement the durable core body grammar | **done** — 68/68 corpus files parse and round-trip; see `docs/milestones/E2.md` |
| 5 | Lower bodies into HIR | **done** — ADR-0014; id-indexed arenas, span on every node, 11 tests |
| 6 | Move declaration rules from `pw-cli` into `pw-core` | **done** |
| 7 | Connect the tested `pw-core` algorithms to `.pw` source | **partial** — exhaustiveness, the E2A-S scope graph, privacy, placement, markup rules, effect inference and the layout relations all run on source; ABI and capability still unconnected, and the placement solver still reads *declared* effect rows rather than inferred ones |
| 8 | Ratchet semantic corpus enforcement upward from 4/44 | **done — 44/44**, ratcheted in `checking_source.rs`; per-fixture table at `docs/evidence/E2D/corpus-enforcement.txt` via `just evidence-corpus` |
| 9 | Implement `pw fmt` after the syntax/HIR boundary stabilises | **done** — gate in the ADR-0013 amendment; 23 of 68 corpus files reformatted |
| 10 | Begin E2A-R only once body-level task operations can be represented | **unblocked** — `Expr::Keyword` represents them |

### The single biggest architectural decision

> **Do not build an effect-only body parser. Build the real lossless expression
> syntax now, but stage how much semantic meaning is implemented over it.**

Both alternatives were rejected with reasons worth keeping:

- **An effect-only grammar** creates a second mini-language. The real parser
  eventually sees one program and the effect parser currently sees another; it
  can misassociate lambda bodies, call arguments, operator precedence, nested
  blocks or match arms while still producing plausible-looking effect results.
  Generic callbacks are specifically a load-bearing test, so syntactic
  approximation is the wrong layer to economise on.
- **Declarations alone** cannot analyse
  `List.map(items, item => database.read(item.id))`. It would prove only direct
  declared-call propagation — not higher-order source programs, which is the
  exact failure mode the corpus exists to prevent.

### Where item 4 starts

The tree layer is in place and proven, so item 4 is now a contained task:

1. **Port the declaration parser to emit into `TreeBuilder`.** It currently
   builds the hand-rolled AST directly. `checkpoint()` / `start_at()` handle the
   cases where a node's kind is only known after its first token.
2. **Make `ast.rs` typed wrappers over `SyntaxNode`** rather than an independent
   structure — the same accessors, backed by the tree.
3. **Add the expression grammar**, filling the kinds already reserved at 200..
   and the patterns at 300...
4. Keep both losslessness suites green throughout. They are the regression net:
   the token stream and the tree are compared against each other, so a mapping
   bug shows up immediately.

### What "core body grammar" means

Not every final language feature. The durable core:

```text
names and qualified names      literals
member/field access            function calls
lambdas                        blocks
let bindings                   if/else
match and patterns             operators with real precedence
records, tuples, collections   type applications where syntactically relevant
parenthesised expressions      error/recovery nodes
```

Queries, commands, subscriptions, resources and tasks should be declarations or
typed constructs **lowering into the same core expression representation**. E4
extends the tree; it must not replace its foundation.

---

## N-0 — Linux CI (deferred, still open)

The only unmet part of the E0 gate. Risk R11 remains the highest-likelihood open
risk. Deferred by operator decision to before **E3**; it is cheap now and
expensive once generated file paths and asset casing start to matter.

| # | part | state |
|---|---|---|
| 1 | `.github/workflows/ci.yml` on ubuntu-24.04 x64 **and** arm64 | written; YAML parses; **never executed** |
| 2 | `scripts/bootstrap.sh` Linux branches, SHA-256 verified | written; checksums computed from the downloaded artifacts; macOS path re-verified; Linux path not executed |
| 3 | case-collision check | **done** — `just case-check` in `just ci`, with a synthetic collision as its control |
| 4 | `cargo deny` + Node advisory gate | **done** — `just audit`, both clean; one esbuild advisory accepted with a written reason |

**Acceptance, restated honestly:** items 3 and 4 are met. Items 1 and 2 are met
only in the sense that the code exists; neither has run on Linux. Do not record
R11 as retired until a real run is green.

---

## The next executable task

Every milestone through E5, plus the inserted E2B/E2C/E2D and E7V, is closed.
All five quality gates are green.

```text
corpus conformance        46 / 46   at C3
single-defect isolation   46 / 46
generality-tested         30 / 31   1 known narrow
headline matrices          9 / 9
resume compatibility      E7V closed
resource graph            E6 closed — 6 gate items
one parser                E6F closed — 5 tests
robustness                11 suites, 0 panics
coverage-guided fuzzing    6 targets, 3600 execs
historical compatibility   9 / 10   classified
KNOWN_GAPs                 0
```

### 1. E7 — the own renderer (E7-R / E7-P / E7-L)

**Unblocked.** E6F converged the front end, which the architect required first:

> Doing it afterward would mean debugging renderer behavior while being unsure
> which interpretation of the source produced it. Marko
is the accepted oracle for resumption and streamed patches (ADR-0002,
ADR-0017), and E7V's compatibility decision already governs the real handler
path in Chromium, Firefox and WebKit. What is missing is a renderer, not a
decision.

**Tasks 1 and 2 are done.** `just spike-own-renderer` runs the chain end to
end — `.pw` → `pw check` → `pw emit-template` → `pw-render` → HTML → a browser —
with Marko nowhere in it. 36 browser assertions in Chromium, Firefox and WebKit,
28 unit tests over escaping and rendering, and a mutation for every gate that
makes the exact measurement go red.

**Task 1.** The renderer-independent golden suite is frozen —
`spikes/pw-to-marko/e2e/golden.mjs` holds the cases as data and
`golden.spec.mjs` is the only file that knows which renderer is under test, so
running it against the own renderer is a different server rather than an edited
test. 12 oracle cases pass in Chromium, Firefox and WebKit; 1 case is the
project's own target with no oracle, recorded as *not holding* for Marko because
RQ-1 falsified it there.

**E7-R is closed.** The store page renders through the own renderer, `decide()`
authorises before attachment, and a click updates only the cart's part because
its declared resource changed.

### 2. E7-P — the own streamed patch mechanism

The foundation E7-R leaves it:

```text
IdentityDomain            E6 decides who shares one
        ↓
InstancePath              a frame per repeatable scope
        ↓
PartAddress               what a patch targets
        ↑
Patch { basis, target, op }
        ↑
server stream
        ↑
E6 resource graph + materializer
```

Three things to build against it:

- **Keyed list operations** — insert, remove, reorder, update one instance,
  replace a whole range. The `InstancePath` model is the address space they
  need; E7-R renders keyed lists and does not yet mutate them.
- **One multiplexed server→browser stream** carrying `ResourceChanged`,
  `Patch` and `Recovery`. Resource subscription and patch transport stay
  logically separate: a subscription changes, the server *may derive* a patch,
  and the transport sends it. Long-poll becomes the fallback adapter rather
  than the mechanism.
- **A causal basis per patch**, as a list of `(ResourceEntryId, Version)` even
  while every patch has one entry. A part can eventually be derived from a
  cart, a promotion and a store's pricing at once, and a field that starts as a
  collection does not need a breaking redesign to hold three.

**The five steps before E7-P proper are done.** The dependency graph is the
contract:

```text
server/runtime                     browser runtime
      \                                /
       \                              /
                 pw-protocol
                /           \
       pw-resource        pw-document
```

`pw-document` holds the address vocabulary — `TemplateSchemaId`,
`InstancePath`, `LocalPartId`, `PartAddress`, `IdentityDomain`, `InstanceToken`
— without `TemplateIR`, the renderer or any server serialization. `pw-protocol`
holds `Patch`, `PatchOp`, `CausalBasis`, `StreamFrame`, `Recovery`, the
protocol version and the decoder, and depends on neither endpoint. Four
structural tests assert that, with a negative control.

What remains for E7-P: **wire the actual Rust materializer to the actual
browser runtime through the protocol.** Today the browser talks to a ~200-line
Node server. A compact Rust development server owning `pw-materialize`,
`pw-resource`, `pw-render` and `pw-protocol` — with the browser seeing only the
protocol — removes one temporary semantic adapter from the integration path.

The three identities are separated and built:

```text
ResourceEntryId    which data changed      opaque, keyed, 128 bits
ResourceVersion    which state of it       monotonic, per ENTRY
PartAddress        where the consequence   IdentityDomain + InstancePath + PartId
                   appears
```

`EntryIdentity` in `pw-resource` is the one semantic answer to "which entry";
`pw-materialize::EntryKey` and `ResourceEntryId` both derive from it. A test
asserts that two storage representations of one identity yield the SAME wire
id — which is what proves the protocol does not depend on the materializer.

**Then E7-L**, where Marko is the negative oracle: RQ-1 measured that its
interaction module loads during initial page load, so the implementation has to
demonstrably do something the scaffolding does not.

Two smaller pieces are E6's and are deliberately not claimed there:

- **Cache-key auditing in `pw explain`** (charter §14 M6 task 9).
  `Graph::key_gaps` computes the missing dimensions and `PW5102` enforces the
  one that is always wrong; reporting the rest as advice needs `explain` to
  read the whole-program graph, which today it does not.
- **A materialized fragment that renders.** `just materialize` proves what
  recomputes and what does not; the bodies are strings a test wrote.

### 2. The large remaining ones

None is started, and each is a milestone rather than a task:

```text
E7-R/E7-P/E7-L  pw's own renderer. Marko is the accepted oracle for
                resumption and streamed patches, and has NO oracle for
                interaction-lazy loading — RQ-1 falsified that property.
E8              Rust capability host, WIT worlds, Wasmtime execution
E9              permanent value type checker and algebraic effect compiler.
                `annotations.rs` answers three corpus questions from written
                types; this is the real thing.
E10–E15         own backends, network lab, HTTP/3, Servo, tooling, hardening
```

### 3. Standing work that is never "done"

The three items the architect required before E6 are closed. They are kept
here, with what closed them, because the standing obligation does not end when
the first version lands.

- **A real coverage-guided fuzzer.** `just fuzz` — stable `-C
  instrument-coverage` plus `llvm-profdata`, an evolving corpus, six targets.
  Reported separately from `just robustness` and never merged into one
  "fuzzed" figure, because structured generation and coverage feedback fail in
  different directions.
- **A browser that calls `decide`.** `runtime/pw-resume-wasm` compiles the
  decision to wasm and the store page's Add button goes through it, in
  Chromium, Firefox and WebKit. Fails closed while the decision is loading.
- **Loop-binding types.** `{#each xs as x}` now gives `x` the element type of
  `xs`, seeing through `Result` and `Option`; `just each-typing`, evidence at
  `docs/evidence/E9/each-typing.txt`. The store demo's Add button is resumable
  because of it. An unresolved type is `PW5016`, never a quiet downgrade to an
  ordinary handler — which would make which handlers resume depend on where
  inference happens to be blind.

- **One parser.** E6F, closed. `just one-parser`. The standing part is the
  structural test: a second semantic tree can be added tomorrow, and only that
  test would notice.

Each is a first version, not a finished one. The fuzzer runs 600 iterations per
target in CI; the browser path exercises four manifests; the type rule knows
three carriers. What matters is that none of the three is now a *claim* with
nothing behind it.

### How to add work here

A new invariant needs, before it counts: a corpus fixture (C1 is frozen, so
this opens C2 — see `docs/CORPUS.md`), a challenge witness, a valid
neighbour, and a registry entry with a symbol. `just generality` and
`just ci` will refuse it otherwise, which is the intent.

---

## Standing obligations (every milestone)

Charter §3.1, plus the admissibility rule in `docs/RISK_QUEUE.md`: a
measurement or checker result is not admissible evidence until its instrument
has a negative control proving it can detect the corresponding failure.
