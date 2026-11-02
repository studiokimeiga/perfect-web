# Glossary

Pleris is built by many parallel agent sessions under a strict process, and the
project has grown a private dialect on top of an unfamiliar language. This page
is the bridge. For each word it says what the word means *in this project*, the
standard software-engineering term if there is one (marked ≈), and where it is
defined. Checked against `master` at `115895de` (train-11 landed, 2026-10-20), with
train-12 integration entries explicitly marked below. Its final gates are pending.

## Start here

Six terms, in the order a newcomer needs them:

- [charter](#charter): `PROJECT_CHARTER.md`, the constitution. Where documents disagree, it wins.
- [ADR](#adr): a numbered decision record in `docs/DECISIONS/`. Read it for *why*.
- [evidence](#evidence): a file under `docs/evidence/` that a recorded command wrote. A claim without one does not count.
- [track](#track): one workstream (a `track/<name>` branch, a worktree, a worker). Only the [integrator](#integrator) merges it, with others on a [train](#train).
- [control](#control): a mutation control breaks the code on purpose to prove a test notices.
- [effect row](#effect-row): the `!{ ... }` after a function's type, listing what it may do besides compute. With [labels](#label) and [placement](#placement) it is how the compiler refuses whole classes of bugs.

## Why the project has its own words

Where a standard term exists the project often uses its own anyway: a *track*
is a feature branch, a *train* a merge-queue batch, *evidence of record* the CI
test reports kept as proof, a *control* a planted bug a test must catch. The
words are precise inside the project and opaque outside it, and an agent handed
these docs meets them cold. Read with this page open; when you write for
outsiders, give the standard word first. Add an entry when a doc uses a word it
does not define.

Each entry opens with its kind: *Standard* (used as everyone uses it),
*Standard, used narrowly here* (the usual word with a narrower or stricter
meaning), or *Project term* (coined here). "In train-12 integration" marks implemented work awaiting the combined gates
and merge to master; its numbered ADR records the decision and remaining limits.

## Terms

### accepted fixture
*Standard, used narrowly here.* A program in `examples/accepted/` (`A-nnn-*.pw`) the checker must take with no diagnostic. With the [rejected fixtures](#rejected-fixture) it forms the [corpus](#corpus). [charter §16], [CORPUS.md].

### ADR
*Standard (architecture decision record).* One file per consequential decision in `docs/DECISIONS/`, written before the change and indexed in `DECISIONS.md`. Here it also says what is *not claimed* and usually reads "accepted under the owner's delegation of 2026-10-02" ([owner's delegation](#owners-delegation)). [DECISIONS.md], [charter §3.1].

### `ADR-XXXX`
*Project term.* The placeholder number of an ADR a [worker](#worker) wrote that the [integrator](#integrator) has not numbered: `docs/DECISIONS/ADR-XXXX-<slug>.md`. A tracked file may mention it only in a code span (`scripts/unnumbered_adrs.py` fails `master` and every train otherwise). See [number an ADR](#number-an-adr). [PARALLEL.md: branches].

### anchor
*Project term.* ≈ injection site of a mutation operator. In a mutation script, the exact text a [mutant](#mutant) replaces; it must occur in its file exactly once. `just ci` runs `scripts/mutation_anchors.py`, which fails when a refactor makes an anchor match zero or several times, or when a mutant was left applied. [mutation_anchors.py].

### architect
*Project term, undefined in the repo.* ADRs and code cite dated "architect rulings" as binding ("The project architect ruled", ADR-0012; `codes.rs`: "Architect ruling, 2026-08-06"), and mark a decision made without one "ruling needed" and "offered for reversal" ([ADR-0031]). No document says who or what the architect is or how a ruling is made. [codes.rs].

### baseline
*Standard, used narrowly here.* (1) A mutation script's unmutated run (`baseline (<suite>): N passed, F failed`); if red, the script stops, since "no mutant can mean anything". (2) The Next.js and SvelteKit builds Pleris is benchmarked against. [mutation_baseline.py], [ADR-0120].

### bug museum
*Project term.* ≈ regression corpus of real incidents. Each exhibit is a [census](#census) failure with a dated primary source, reproduced in Next.js or SvelteKit (`benchmarks/museum/`) and shown refused by Pleris, or shown *not* refused (a miss, reported as prominently as a win). `just e14-museum` writes its [evidence](#evidence). [ADR-0342], [museum.txt].

### capability
*Standard, used narrowly here.* ≈ object capability / permission. An effect such as `database.read<Store>` the host must grant before code needing it runs. A capability *authorizes* operations, it does not *identify* one (many-to-many). Effects say what code *requires*; WIT worlds say what a target *receives*. [ADR-0026], [SEMANTICS §3].

### capability matrix
*Project term.* ≈ feature-coverage matrix. `docs/vision/capabilities.md`: a row per thing complex web apps need, with status *built*, *partial*, *missing* or *not built here*, the ADRs behind it and its limits. Not a [capability](#capability). [capabilities.md].

### census
*Project term.* ≈ failure taxonomy / threat catalogue. `research/failures/`: 224 invariant-level web failures in 24 families (edition 1), each a "leaf" with a stable ID (`E01`, `G01`) naming an invariant. Rules and [bug museum](#bug-museum) exhibits cite leaves. Leaf `E01` is unrelated to milestone [E1](#e-milestone). [census README], [ADR-0027].

### chain, the
*Project term.* Two senses. (1) The recipes a commit's changes touch, once run by hand after `just ci` (ADR-0239's: 39 recipes, for hours) and now planned and run by [verify](#verify). (2) In a diagnostic, the chain of calls through which an effect reaches a function. [ADR-0245], [MILESTONES.md] (E2D).

### charter
*Project term.* ≈ project constitution. `PROJECT_CHARTER.md`: goals, the operating contract (§3), language semantics (§7), milestones (§14), the store app (§15), the corpus (§16), tests (§17), risks (§20). Cited by section (`§7.2`). Changed only by explicit human decision; it beats `AGENTS.md` where they differ. [PROJECT_CHARTER.md].

### ci
*Standard, used narrowly here.* Two things. `just ci` is the fast local gate set (`evidence-gates`, `fmt-check`, `lint`, `case-check`, `mutation-anchors`, `test-unit`, `test-compile`, `mutant-programs`) required before a push. The GitHub workflow `ci` runs on every push: the same on x64 and arm64, plus a licenses-and-advisories job. Neither runs the [evidence recipes](#recipe); [verify](#verify) does. [justfile], [ci.yml].

### command
*Standard (CQRS), used narrowly here.* A `command` declaration is the one way to change data. It runs at the origin in one transaction, `requires` a [predicate](#predicate), is `idempotent_by` an interaction ID (a second delivery is answered, not rerun), may `emit` events and show an `optimistic` value first. [guide: commands], [ADR-0121].

### component
*Standard (WebAssembly component model), used narrowly here.* A Wasm component the compiler builds from a program's declarations; the [host](#host) runs it in Wasmtime with only the imports its contract grants. Not a React-style UI component: that is a [view](#view), though charter §8.1 still says "component format". [ADR-0032], [ADR-0020], [ADR-0317].

### composite
*Project term.* ≈ coarse-grained service operation. A host operation doing the work of several reads in Rust (`checkout#parts`, `search#find`). ADR-0337 moves the store's composites "above the seam" into Pleris over narrower primitives, one track at a time. [ADR-0337].

### control
*Standard (test control), used narrowly here.* A case with a known outcome run beside a check to show the check can tell the difference. A *mutation control* is a planted [mutant](#mutant) the tests must [kill](#killed); a *repair control* is the accepted neighbour of a rejected program, which must check clean so the refusal is for this bug alone. Not a form control. [ADR-0342], [mutation_anchors.py].

### coordinator
*Project term.* The session that launches and paces [workers](#worker) for the owner (for example when disk allows another [slot](#slots)); not the [integrator](#integrator), who merges. The repo names the role only in dated PARALLEL.md entries. [PARALLEL.md: rulings].

### corpus
*Standard, used narrowly here.* ≈ conformance suite. The [accepted](#accepted-fixture) and [rejected](#rejected-fixture) programs of charter §16, the language's executable specification, versioned (C1 frozen 2026-08-06) so a fixture cannot be edited into compliance. [CORPUS.md], [charter §16].

### device store
*Project term.* ≈ client-side persistence. In train-12 integration ([ADR-0366]). A module `signal` with a `storage device "name"` clause (and `privacy Device`): the browser keeps its value (`localStorage` first), reads it before the page is shown, and never sends it to the server. [charter §9.1] (the fourth kind of state).

### dispatch
*Standard (GitHub `workflow_dispatch`), used narrowly here.* Starting a [verify](#verify) run by hand for named recipes (`gh workflow run verify.yml --ref <branch> -f recipes="..."`); an empty list runs every recipe. [verify.yml], [ADR-0245].

### effect row
*Standard (effect systems), used narrowly here.* The `!{ ... }` after a function's return type, listing what it does besides compute a value: `!{}` is pure, `!{ database.read<Store> }` reads the store. Callers inherit their callees' effects (`!{}` on such a caller is PW0400), so the compiler can refuse I/O in a `view` and derive [placement](#placement). [guide: effects], [SEMANTICS §2], [charter §7.2].

### E-milestone
*Project term.* Engineering milestones `E0` to `E15`, with inserted lettered ones (`E1A`, `E2A` to `E2D`, `E6F`, `E7V`, `E7-R`/`-P`/`-L`, `E8-0`); public demonstrations `P0` to `P9`; risk experiments `RQ-n` ([RISK_QUEUE.md]); never a bare `M`. E14 (developer tooling, semantic diffs, AI benchmark) is current, E11 to E13 are deferred behind it, E15 is not started. [MILESTONES.md], [ADR-0119].

### `e<N>-` naming
*Project term.* The prefix of an [evidence recipe](#recipe): `e14-museum` writes `docs/evidence/E14/museum.txt`; `N` is the milestone whose evidence folder it writes. The [planner](#planner) plans every `e<N>-…` recipe except machine measurements (`LOCAL_ONLY`). [PARALLEL.md: owns], [ADR-0245].

### evidence
*Project term.* ≈ archived CI test reports kept as proof. A text file under `docs/evidence/<milestone>/` written by a recorded command (`just e14-<name>`); its header names the command, commit, toolchain and, for CI-produced files, the run. "Evidence or it did not happen": a gate or claim without such a file does not count. [AGENTS.md], [ADR-0245].

### evidence of record
*Project term.* ≈ the system-of-record test report. The evidence that counts for a merge: only what a green [verify](#verify) run produced and [evidence-fetch](#evidence-fetch) copied in. A local run is "a first look", never the record. [ADR-0281], [ADR-0344].

### evidence-fetch
*Project term.* ≈ downloading build artifacts. `just evidence-fetch <run>` copies a successful verify run's evidence into `docs/evidence/`, adding a `ci:` line (run and runner) after each `commit:` line. It refuses a file naming another commit, a run of a commit other than HEAD, and files whose inputs changed since. [ADR-0245], [ADR-0281], [evidence_fetch.py].

### flake
*Standard.* A test that fails on some runs and passes on others with no code change. A failing browser test is re-run once before investigation, never loosened to pass, and recorded as a finding. "Flake" is a claim to check: failures first called one in train-10 were WebKit's network process crashing. [PARALLEL.md].

### four status documents
*Project term.* `docs/STATUS.md` (current milestone, gate state, reproduce commands), `NEXT.md` (next tasks), `DECISIONS.md` (ADR index) and `KNOWN_LIMITATIONS.md`. Only the [integrator](#integrator) edits them; a [worker](#worker) puts what it would write there in its ADR. Charter §3.4 lists a longer set of maintained files; "the four" is PARALLEL.md's integrator-only subset. [PARALLEL.md: who], [charter §3.4].

### gate
*Standard, used narrowly here.* (1) A *milestone gate*: the objective pass criteria of a milestone; none is complete until its gate passes, each gate item backed by [evidence](#evidence). (2) The checks of [`just ci`](#ci), run before a commit or push. (3) One *gate item*, such as `E9-V1`. [charter §3.1], [justfile].

### guarantees
*Project term.* What a data [source](#source) promises about its database, written as the source's `transactions`, `reads`, `changes` (and `sets`) clauses. The compiler holds queries and commands to them; the server refuses to serve on a database that does not match. [ADR-0207], [guide: sources].

### guest
*Standard (Wasm), used narrowly here.* Two things. (1) The Wasm [component](#component) a [host](#host) runs; it reaches only what its WIT world imports. (2) A *guest user*: a [principal](#principal) of its own who is not signed in; the guest's cart joins the user's at sign-in. [SEMANTICS §3], [ADR-0318].

### head-to-head
*Project term.* The measured comparison of Pleris with the mainstream stacks on the same apps: TodoMVC against TodoMVC's own React and Svelte apps, then the store against Next.js and SvelteKit. Bytes, paint, interactive and each interaction's time, servers under load and idle, source size, the same feature change and what each stack's checker catches in it, behaviour, and which [museum](#bug-museum) bugs each stack lets a developer ship; Pleris's losses reported first. Protocol: [ADR-0342]; first measurement, its instrument and `just e14-head-to-head`, recorded on the development machine: [ADR-0392] (accessibility not measured yet). The [capability matrix](#capability-matrix) says what has been measured.

### host
*Standard, used narrowly here.* Three things. (1) The Rust process that runs [components](#component), holds the database and secrets, and supplies [host bindings](#host-binding) (`runtime/pw-host`, the development server); its counterpart is the [guest](#guest). (2) One running instance of the development server, which a browser test starts per "host role" on its own port ([host map](#host-map)). (3) One server of a deployment; a second one hears nothing of the first's commits. [ARCHITECTURE.md], [ADR-0020], [capabilities.md].

### host binding
*Project term.* ≈ foreign function declaration / WIT import. A function with no body bound to an operation the host provides: `fn load(id: Int) -> Store !{ database.read<Store> } host "shop:data/stores#get"`. The string is `namespace:package/interface#operation` in WIT's identifier rules; the effect row is the operation's contract. [ADR-0262], [guide: host bindings].

### host map
*Project term.* ≈ test-fixture dependency map. A registry of which development-server hosts each browser spec needs, so a Playwright run starts only the hosts its selected tests need instead of all of them; without it `e14-refusal` outgrew a CI job's time limit. [ADR-0356](DECISIONS/ADR-0356-browser-host-map.md).

### ID prefixes
*Project term.* `A-nnn`, `R-nnn`: accepted and rejected corpus fixtures. `E<n>`, `P<n>`, `RQ-<n>`: milestones, public proofs, risk experiments. `R<n>`: charter risks. `T01`..`T12`: benchmark tasks. `W<n>`: workers. `C1`..: corpus versions. `D0`..`D5` are two unrelated schemes: ADR-0338's data-layer steps (D1 tables, D2 memory adapter) and ADR-0339's document-app steps (D1 replicated text, D2 device store). [ADR-0338], [ADR-0339].

### integrator
*Project term.* ≈ maintainer / release manager. The one session working on `master`: it alone merges, numbers ADRs, writes the [four status documents](#four-status-documents), runs the [chain](#chain-the), pushes `master`, and rules on anything that crosses tracks. [PARALLEL.md: who].

### keyed read
*Project term.* A read of one entry of a keyed query or resource, one key per host call. Below the [seam](#seam-the) that is one call; a keyed read made once per element of a loop is refused at check time (PW0405), and the repair is the [set read](#set-read). Older ADRs also use "keyed read" for the browser's per-document read (`doc=`). [ADR-0337], [ADR-0340], [ADR-0161].

### killed
*Standard (mutation testing).* A [mutant](#mutant) caught by an actual assertion in a complete valid test run: verdict `KILLED`. Compilation failures, timeouts, memory-guard stops and broken producers earn no kill. A failed *build* shows nothing about the tests, so `mutant-programs` checks that a mutant of a `.pw` source is still a well-typed program unless the script names the rule that refuses it. [mutant_programs.py], [ADR-0292].

### KNOWN_LIMITATIONS
*Project term.* ≈ known-issues list. `docs/KNOWN_LIMITATIONS.md`: what does not work, stated honestly (charter §3.3, "never hide uncertainty"). One of the [four status documents](#four-status-documents). [KNOWN_LIMITATIONS.md].

### label
*Standard (information-flow label), used narrowly here.* A privacy label on a value: `Public`, `Session<SessionId>`, `User<UserId>`, `Organization<OrganizationId>`, `Device` or `Secret<Capability>`. A value carries the join of what it was made from; a labelled value cannot reach a shared cache (PW5001), a public page, a log or a browser it must not. Not an HTML `<label>`. [SEMANTICS §8], [ADR-0129], [ADR-0282].

### landing
*Project term.* ≈ merging to the main branch once a merge queue passes. A [train](#train) lands when `master` is moved to its final evidence commit, after its [verify](#verify) run is green and its evidence fetched ("a local merge is not a verified landing", ARCHITECTURE.md). [ADR-0281], [ARCHITECTURE.md].

### lanes
*Project term.* ≈ parallel CI runners. In train-12 integration ([ADR-0364]). In `just preverify`, throwaway worktrees, each with its own `CARGO_TARGET_DIR` and `PORT` window, in which a plan's recipes run side by side; the integrator first did this by hand ("lanes A to E") for train-11. See [pre-verify](#pre-verify).

### layout
*Standard (Next.js `layout`), used narrowly here.* A `layout` declaration: the markup pages share, with a `<slot />` where each page goes. A navigation between pages of one layout and one build keeps the layout in place. Not browser layout (reflow). [guide: layouts], [ADR-0303].

### materialization
*Project term.* ≈ materialized view; the project's successor to ISR. A declared value derived from resources by a body, kept by the runtime (transactional outbox, coalescing, last-known-good) and read by pages. Typed events invalidate it, not a route timer. [charter §9.4], [ADR-0273].

### merge
*Standard, used narrowly here.* The [integrator](#integrator) combining a [track](#track) into a [train](#train) as its own merge commit, ADR numbered and documents written, once the track's tip passes `ci` and `verify`. Contradiction: PARALLEL.md's branch rules say "rebase on `master` before pushing"; the worker rules say to merge `origin/master` and never rebase a pushed branch. [ADR-0281], [PARALLEL.md: branches].

### mutant
*Standard (mutation testing).* One deliberate defect planted in the source: a tuple `(what, [suite,] path, anchor, replacement)` in a script's `MUTANTS` list. The script replaces the [anchor](#anchor), rebuilds, runs the suite, restores the source and prints a verdict. [page_parameters_mutations.py] (a typical script), [ADR-0292].

### mutation testing
*Standard.* Checking the tests by breaking the code: each [mutant](#mutant) must make some test fail ([killed](#killed)); one that does not ([survived](#survived)) is a hole in the tests. Every behaviour here needs a test and a mutation control; there are hundreds of `scripts/*_mutations.py`, each ending `k of n mutants killed`. [mutation_baseline.py], [ADR-0245].

### nightly
*Standard, used narrowly here.* The scheduled [verify](#verify) run (06:17 UTC): every recipe, unless that exact master commit already has a successful run. Sunday always runs everything. The integrator reads failures and imports successful evidence through [evidence-fetch](#evidence-fetch). [verify.yml], [PARALLEL.md: working rules].

### non-vacuity
*Project term.* ≈ vacuous-pass check. A guard showing a check did not pass by checking nothing: `suite_status.py` requires as many browser-suite invocations recognised as appear in the recipes, and at least one; `mutant-programs` requires every mutant of a `.pw` source to stay a well-typed program. [suite_status.py], [mutant_programs.py].

### number an ADR
*Project term.* The [integrator](#integrator) replacing `ADR-XXXX` with the next number in the ADR's file name, title and every mention in code, tests, recipes and evidence. That can rewrite inputs recipes read. A train is numbered before publication; final verification and [evidence currentness](#stale-evidence) must cover the numbered inputs. Fetch only eligible reports through [evidence-fetch](#evidence-fetch). [ADR-0281], [PARALLEL.md: branches].

### oracle
*Standard (test oracle), used narrowly here.* A reference whose answers the compiler's are compared with. Koka was a temporary "effects-only oracle" for effect rows; compiled declarations are later checked against independent references. [ADR-0011].

### owner's delegation
*Project term.* The owner's instruction of 2026-10-02 that the integrator decide open design questions itself, each researched and recorded as an ADR "accepted under the owner's delegation of 2026-10-02". Decisions marked as the owner's (downloads, the AI benchmark's models and budget) stay with the owner. The repo cites the delegation and never restates it. [PARALLEL.md: working rules], [ADR-0195].

### page
*Standard, used narrowly here.* A `page` declaration: a route plus a `view`. Rendered on the server as plain HTML, then [resumed](#resumability) in the browser; it reads data only through declared queries and has an empty effect row (charter §7.4). [guide: pages].

### placement
*Standard (multitier programming), used narrowly here.* Where code may run: `Build`, `Browser`, `Edge`, `Origin` or `Database`, derived from the effects it needs, its data's [labels](#label), consistency and cache policy, plus explicit constraints. The compiler refuses code placed where its effects cannot run (database access in the browser: R-002). [SEMANTICS §9], [charter §7.9].

### planner
*Project term.* `scripts/ci_plan.py` (`just verify-plan`). Given a commit range it picks the [evidence recipes](#recipe) to run (mutation script changed, a mutant within 30 lines of a changed line, tests changed, or the recipe's own lines changed; with `--stale`, also [stale evidence](#stale-evidence)) and deals them to [shards](#shard), costliest first. [ADR-0245], [ADR-0344].

### Pleris
*Project term.* The language and platform this repository builds (the repo is `perfect-web`; the README says "Perfect Web / Pleris"): `.pw` source, the compiler and CLI `pw`, a Wasm [host](#host) on Wasmtime, a development server and a browser runtime. Aim: any complex web app, made as correctly as possible. [README.md], [guide: why].

### predicate
*Project term.* ≈ authorization rule. `predicate SignedIn says "Sign in to post."`: a named condition a `command` [requires](#requires). The program names it and says what a refused reader is told; its meaning is the deployment's. The development server evaluates only `SignedIn`, `OwnsPost(post)` and `MayMessage(to)`. [guide: predicates].

### pre-verify
*Project term.* In train-12 integration ([ADR-0364]). `just preverify [BASE]` plans, with CI's own [planner](#planner), the recipes [verify](#verify) would run for HEAD and can run them locally in [lanes](#lanes). It writes no evidence and pushes nothing. The owner's current workflow keeps short checks local and sends heavy batches to GitHub on a `wip/*` branch.

### principal
*Standard (security), used narrowly here.* The authenticated identity behind a session. The host answers `current_user()` from the session's principal, typed `User<UserId>`, via `pw:host/principal#read`; a program cannot forge one. A guest is a principal too. [ADR-0263], [ADR-0270].

### privacy
*Standard, used narrowly here.* Not a policy statement but a checked property: a value's [label](#label) never reaches a place less private than itself (cache, page, log, browser, event). A query says whose data it shows: `public`, `session` or `user`. [SEMANTICS §8], [guide: privacy labels].

### producer ownership
*Project term.* A verification runner keeps responsibility for the test process it starts until completion or cancellation cleanup. On cancellation it stops the owned process group and reaps its leader before the caller restores source. The native lifecycle tests and the cooperative nesting limits are in [ADR-0376].

### PW codes
*Project term.* Diagnostic codes `PWnnnn`, each registered once in `codes.rs` with its invariant and owner. `PW00xx` is syntax, `PW01xx` and up semantic invariants, `PW50xx` placement, capability and unsafe boundaries. A parallel track's codes live only in its own block, held by a test: `PW55` identity, `PW56` uploads, `PW57` notifications, `PW58` messages, `PW59` store on PostgreSQL, `PW60` kiokun, `PW62` store accounts, `PW63` store search, `PW64` payments, `PW65` schedules. The header of `codes.rs` lists only some. [codes.rs], [guide: diagnostics].

### query
*Standard, used narrowly here.* A declared, keyed, cached read stating freshness, consistency, cache (`shared` or `private`), key, the events that invalidate it and a timeout. Pages read data only through queries. `public`, `session` or `user` says whose data it returns. [guide: queries].

### re-anchored
*Project term.* A [mutant](#mutant) whose [anchor](#anchor) was edited to follow code that moved. Running only the re-anchored mutants after a change missed two survivors, so each script a change reaches is run [whole](#whole-run). [PARALLEL.md: working rules].

### recipe
*Standard (`just`), used narrowly here.* A named command in the `justfile` or `just/*.just`. An *evidence recipe* (`e14-…`) runs a feature's tests, browser specs and mutation controls and writes one file under `docs/evidence/`. There are hundreds; [verify](#verify) runs those a change reaches. [justfile], [PARALLEL.md: owns].

### reference app
*Project term.* An app built to exercise the language, called an "instrument": the feed (Twitter-like), the store (DoorDash-like), kiokun (the owner's Chinese, Japanese and Korean dictionary, kiokun.com) and TodoMVC, under `examples/`. Chosen for the [capability matrix](#capability-matrix) rows they cover, not their brand. [capabilities.md], [ADR-0285].

### region
*Project term.* ≈ out-of-order streamed section. A part of a page streamed after the shell: `<stream query={...}>` with `placeholder`, `ready` and `failed` arms, filled when its query's whole arm has arrived. Not the ARIA "status region" that announces a refused press. [ADR-0148], [ADR-0302].

### rejected fixture
*Standard, used narrowly here.* A program in `examples/rejected/` (`R-nnn-*.pw`) that must be refused. Its header `// @rule: PWnnnn` names the code `pw check` reports, and it must be refused for that rule only (a wrong-reason catch fails the corpus). [CORPUS.md], [ADR-0344].

### render merge
*Project term.* ≈ request-scoped read de-duplication (DataLoader per request). One snapshot per page render: its host calls run in one read transaction and each identical call is made once. Host-side only; no program changes. [ADR-0354](DECISIONS/ADR-0354-a-render-reads-each-call-once.md).

### requires
*Project term (keyword).* ≈ precondition / authorization check. A `command` clause naming a [predicate](#predicate) the server checks before the body runs: `requires SignedIn`. A refusal is answered 403 and the page shows the predicate's words where the button was. [guide: commands], [ADR-0302].

### resource
*Standard, used narrowly here.* Two things. (1) A remote resource: a keyed, cached remote value such as a store, cart or menu (`runtime/pw-resource`). (2) An *affine resource*: a handle (transaction, upload, lock) that must end exactly once and cannot escape its scope, held by a `resource` declaration's `acquire` clause. [charter §9.2], [SEMANTICS §7], [ADR-0250].

### resumability
*Standard (Qwik), used narrowly here.* Server rendering emits semantic HTML plus only the metadata needed to continue; the browser attaches handlers without re-running the page. A handler captures only serializable values, resource keys and signal IDs, and a stale manifest is detected by the handler's content-addressed identity. [charter §8.5], [ADR-0134].

### ruling
*Project term.* A dated decision on an open question: "the integrator's ruling" (logged in PARALLEL.md and the ADRs), "the owner's ruling", or the undefined "architect ruling". A finding comes before a feature, and a correction is reported first. [PARALLEL.md: rulings].

### seam, the
*Project term.* Two unrelated senses. (1) The data seam: the `DataLayer` boundary between the program (above: Pleris text the compiler checks) and the host's data layers (below: Rust operations that each state what a call costs). PW0405 acts above it. (2) `TRACK SEAM`: markers in the development server's `main.rs` for the only lines a given track may touch. [ADR-0337], [ADR-0338], [PARALLEL.md: owns].

### set read
*Project term.* ≈ batched read, the fix for N+1. One host call reading many keys (`hours_of_stores(ids)` instead of `hours_of(id)` per store). A read keyed by a loop's element is refused (PW0405) and the set read is the repair; a source's [`sets`](#sets) states what one call costs. [ADR-0337], [ADR-0340], [ADR-0348].

### sets
*Project term.* A [source](#source) clause stating the cost of a [set read](#set-read): `one_call` (one statement or batch, such as PostgreSQL's `= ANY($1)`) or `per_key`. A source that states nothing is `per_key`, and PW0406 warns where a program's read budget then grows. [ADR-0337], [ADR-0338].

### shard
*Standard, used narrowly here.* One GitHub job of a [verify](#verify) run: it sets up the toolchains, builds, and runs its share of recipes one after another. The [planner](#planner) deals recipes to shards, costliest first; a recipe that needs a database is a shard of its own. [ADR-0245].

### signal
*Standard (Solid, Svelte), used narrowly here.* Ephemeral local UI state, written by handlers and read where the browser reads it again, with no server round trip. [ADR-0130], [guide: signals].

### slots
*Project term.* Three senses. (1) *Mutation slots* ([ADR-0357](DECISIONS/ADR-0357-mutants-run-in-parallel-slots.md)): `PW_MUTANT_SLOTS=N` runs a script's mutants in N parallel copies of the tree, each with its own cargo target, ports and temp directory. (2) *Worker slots*: how many [workers](#worker) run at once, bounded by disk and usage. (3) A layout's `<slot />`. [PARALLEL.md], [guide: layouts].

### source
*Project term.* ≈ a declared consistency contract for a data store. A `source` declaration states what the database behind some types guarantees (`holds`, `transactions`, `reads`, `changes`, `sets`). Not a source file. [ADR-0207], [guide: sources].

### stale evidence
*Project term.* An evidence file whose recorded `commit:` is no longer current (what its recipe reads changed, or it is missing) or whose `k of n mutants killed` disagrees with its script. The planner's `--stale` plans such recipes at each train's push, because a push plans only what it touched and the nightly keeps no evidence. [ADR-0344].

### stream
*Project term.* Two uses: the `<stream>` markup that makes a [region](#region), and the live change feed over which an open page hears of changes (`/stream?doc=..&since=..`). [ADR-0148], [ADR-0161].

### survived
*Standard (mutation testing).* A [mutant](#mutant) no test caught (a *survivor*): verdict `SURVIVED`, a hole in the tests. The verify summary fails a run on any survivor. [ADR-0245].

### tape
*Project term.* ≈ stopgap dependency. The ADR "strategy" for a dependency used temporarily behind an adapter (Koka as oracle, Marko as renderer), beside `reuse`, `fork` and `build`; each carries a deletion condition. [charter §4], [DECISIONS.md].

### track
*Project term.* ≈ feature branch / workstream. One unit of parallel work: a branch `track/<name>`, a worktree of its own, one [worker](#worker), and the files and [PW code](#pw-codes) block it owns. Only the [integrator](#integrator) merges it. [PARALLEL.md: who], [ADR-0253].

### train
*Standard (merge train / merge queue), used narrowly here.* Tracks ready at once are merged, one merge commit each, onto a branch `track/train-<n>` cut from `master`; `just ci` runs there once and [verify](#verify) plans every recipe the train reaches, once. A track whose recipe fails leaves the train, which is rebuilt without it. A green train [lands](#landing). [ADR-0281].

### verify
*Project term.* The GitHub workflow `verify.yml`: a plan, the recipe [shards](#shard), the browser suite in each engine (Chromium, Firefox, WebKit), and a summary that fails on a failed recipe, a survivor, a memory-guard failure or a shard that reported nothing. It runs on pushes to `master` and `track/**`, nightly and by [dispatch](#dispatch). "A verify" is also one such run, cited by ID. [verify.yml], [ADR-0245].

### view
*Project term.* ≈ UI component. A reusable piece of markup with typed props and an empty effect row; an element named with a capital letter is a view (`<Money value={...} />`). [ADR-0072], [guide: markup].

### whole run
*Project term.* Running every mutant of a script (or every spec in every engine) rather than a subset. After a change, each mutation script its code or tests reach is run whole. [PARALLEL.md: working rules].

### wip branch
*Project term.* `wip/<track>`: a scratch branch for in-progress commits, even red, pushed so GitHub runs `ci` instead of the busy local machine; deleted when the track is done. `verify` triggers only on `master` and `track/**`, so a `wip/*` push runs `ci` alone. [verify.yml].

### witness
*Standard, used narrowly here.* A concrete program or test that demonstrates one claim. Most often a *generality witness*: a program in `examples/generality/` showing that a rule's invariant holds beyond the single case in its [rejected fixture](#rejected-fixture). The corpus counts "generality-tested n / m" invariants and does not version witnesses. [CORPUS.md].

### worker
*Project term.* ≈ contributor. An AI agent session (W1, W2, ... in the logs) that builds one [track](#track) in its own worktree and branch. It never merges to `master`, never edits the [four status documents](#four-status-documents), and sends cross-track questions to the [integrator](#integrator). [PARALLEL.md: who].

### worktree
*Standard (`git worktree`).* A second checkout of the same repository in another directory. Each [track](#track) gets one, with its own `target/` build cache, so parallel workers never edit the same files; abandoned ones are deleted. [AGENTS.md], [charter §3.5].

<!-- link references -->
[ADR-0011]: DECISIONS/ADR-0011-koka-is-an-effects-only-oracle.md
[ADR-0020]: DECISIONS/ADR-0020-component-contract-is-the-compiler-host-boundary.md
[ADR-0026]: DECISIONS/ADR-0026-a-capability-authorizes-an-operation-it-does-not-identify-one.md
[ADR-0027]: DECISIONS/ADR-0027-failure-census-and-evidence-validity.md
[ADR-0031]: DECISIONS/ADR-0031-value-relations.md
[ADR-0032]: DECISIONS/ADR-0032-compiled-components.md
[ADR-0072]: DECISIONS/ADR-0072-an-element-named-with-a-capital-is-a-view.md
[ADR-0119]: DECISIONS/ADR-0119-e10-closes-and-the-ai-benchmark-comes-next.md
[ADR-0120]: DECISIONS/ADR-0120-three-stores-one-contract.md
[ADR-0121]: DECISIONS/ADR-0121-an-idempotent-command-runs-once-per-interaction.md
[ADR-0129]: DECISIONS/ADR-0129-a-values-label-follows-it.md
[ADR-0130]: DECISIONS/ADR-0130-ui-state-is-a-signal-provided-where-it-lives.md
[ADR-0134]: DECISIONS/ADR-0134-every-handler-is-resumable.md
[ADR-0148]: DECISIONS/ADR-0148-a-stream-region-shows-its-query-s-state.md
[ADR-0161]: DECISIONS/ADR-0161-each-document-is-its-own-subscriber.md
[ADR-0195]: DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md
[ADR-0207]: DECISIONS/ADR-0207-a-data-source-states-what-it-guarantees.md
[ADR-0245]: DECISIONS/ADR-0245-the-heavy-verification-runs-on-github-actions.md
[ADR-0250]: DECISIONS/ADR-0250-let-discards-and-an-acquisition-is-held-or-refused.md
[ADR-0253]: DECISIONS/ADR-0253-two-tracks-are-built-in-parallel-each-in-files-of-its-own.md
[ADR-0262]: DECISIONS/ADR-0262-a-host-binding-is-an-operation-a-host-provides.md
[ADR-0263]: DECISIONS/ADR-0263-a-handle-is-the-platforms-to-make.md
[ADR-0270]: DECISIONS/ADR-0270-the-readers-user-is-the-hosts-and-a-listener-binds-a-users-id.md
[ADR-0273]: DECISIONS/ADR-0273-a-materialization-is-a-value-its-body-derives.md
[ADR-0281]: DECISIONS/ADR-0281-a-merge-is-held-to-ci-s-verification-run.md
[ADR-0282]: DECISIONS/ADR-0282-what-a-value-holds-is-one-label.md
[ADR-0285]: DECISIONS/ADR-0285-kiokun-parity-inventory.md
[ADR-0292]: DECISIONS/ADR-0292-a-mutation-scripts-processes-are-bounded-in-memory.md
[ADR-0302]: DECISIONS/ADR-0302-a-refusal-is-told-where-the-press-was.md
[ADR-0303]: DECISIONS/ADR-0303-a-page-is-shown-in-its-layout.md
[ADR-0317]: DECISIONS/ADR-0317-a-component-is-compiled-once-a-process.md
[ADR-0318]: DECISIONS/ADR-0318-a-cart-and-an-order-are-a-users.md
[ADR-0337]: DECISIONS/ADR-0337-a-read-is-bounded-by-construction.md
[ADR-0338]: DECISIONS/ADR-0338-three-databases.md
[ADR-0339]: DECISIONS/ADR-0339-a-collaborative-document-app.md
[ADR-0340]: DECISIONS/ADR-0340-a-read-is-bounded-by-construction-the-checkers-half.md
[ADR-0342]: DECISIONS/ADR-0342-the-claim-measured.md
[ADR-0392]: DECISIONS/ADR-0392-head-to-head.md
[ADR-0344]: DECISIONS/ADR-0344-evidence-integrity.md
[ADR-0348]: DECISIONS/ADR-0348-a-set-read-keeps-each-keys-result.md
[AGENTS.md]: ../AGENTS.md
[ARCHITECTURE.md]: ARCHITECTURE.md
[capabilities.md]: vision/capabilities.md
[census README]: ../research/failures/README.md
[charter §3.1]: ../PROJECT_CHARTER.md#31-execute-do-not-just-speculate
[charter §3.4]: ../PROJECT_CHARTER.md#34-preserve-progress-across-context-windows
[charter §3.5]: ../PROJECT_CHARTER.md#35-git-and-subagent-discipline
[charter §4]: ../PROJECT_CHARTER.md#4-reuse-fork-tape-or-build-decision-framework
[charter §7.2]: ../PROJECT_CHARTER.md#72-effects
[charter §7.9]: ../PROJECT_CHARTER.md#79-placement
[charter §8.5]: ../PROJECT_CHARTER.md#85-resumption
[charter §9.1]: ../PROJECT_CHARTER.md#91-four-kinds-of-state
[charter §9.2]: ../PROJECT_CHARTER.md#92-resource-declarations
[charter §9.4]: ../PROJECT_CHARTER.md#94-isr-successor
[charter §16]: ../PROJECT_CHARTER.md#16-accepted-and-rejected-language-corpus
[ci.yml]: ../.github/workflows/ci.yml
[codes.rs]: ../compiler/pw-core/src/codes.rs
[CORPUS.md]: CORPUS.md
[DECISIONS.md]: DECISIONS.md
[evidence_fetch.py]: ../scripts/evidence_fetch.py
[guide: commands]: guide/tour-server.md#commands
[guide: diagnostics]: guide/diagnostics.md
[guide: effects]: guide/tour.md#effects
[guide: host bindings]: guide/tour-server.md#host-bindings-and-capabilities
[guide: layouts]: guide/tour-pages.md#layouts-and-navigation
[guide: markup]: guide/tour-pages.md#markup-blocks
[guide: pages]: guide/tour-pages.md#pages-and-routes
[guide: predicates]: guide/tour-server.md#who-may-predicates
[guide: privacy labels]: guide/tour-server.md#whose-data-privacy-labels
[guide: queries]: guide/tour-server.md#queries
[guide: signals]: guide/tour-pages.md#state-in-the-browser-signals-and-handlers
[guide: sources]: guide/tour-server.md#sources
[guide: why]: guide/why.md
[justfile]: ../justfile
[KNOWN_LIMITATIONS.md]: KNOWN_LIMITATIONS.md
[MILESTONES.md]: MILESTONES.md
[museum.txt]: evidence/E14/museum.txt
[mutant_programs.py]: ../scripts/mutant_programs.py
[mutation_anchors.py]: ../scripts/mutation_anchors.py
[mutation_baseline.py]: ../scripts/mutation_baseline.py
[page_parameters_mutations.py]: ../scripts/page_parameters_mutations.py
[PARALLEL.md]: PARALLEL.md
[PARALLEL.md: branches]: PARALLEL.md#branches-commits-and-adrs
[PARALLEL.md: owns]: PARALLEL.md#what-each-track-owns
[PARALLEL.md: rulings]: PARALLEL.md#the-integrators-rulings
[PARALLEL.md: who]: PARALLEL.md#who-does-what
[PARALLEL.md: working rules]: PARALLEL.md#the-integrators-working-rules
[PROJECT_CHARTER.md]: ../PROJECT_CHARTER.md
[README.md]: ../README.md
[RISK_QUEUE.md]: RISK_QUEUE.md
[SEMANTICS §2]: SEMANTICS.md#2-effects-charter-72
[SEMANTICS §3]: SEMANTICS.md#3-capabilities-charter-73
[SEMANTICS §7]: SEMANTICS.md#7-affine-resources-charter-77
[SEMANTICS §8]: SEMANTICS.md#8-privacy-labels-charter-78
[SEMANTICS §9]: SEMANTICS.md#9-placement-charter-79
[suite_status.py]: ../scripts/suite_status.py
[verify.yml]: ../.github/workflows/verify.yml

[ADR-0364]: DECISIONS/ADR-0364-a-train-is-pre-verified-locally.md
[ADR-0366]: DECISIONS/ADR-0366-device-local-storage.md
[ADR-0376]: DECISIONS/ADR-0376-producer-process-ownership.md
