# Assumptions

Every assumption this project is running on that has **not** been proven yet.
PROJECT_CHARTER.md §3.2 permits making reasonable assumptions instead of asking
broad questions — but §3.3 forbids hiding them.

Each entry states how it gets validated and what happens if it turns out false.
An assumption is removed from this file only when it is either proven (and moved
into an ADR or `docs/SEMANTICS.md`) or disproven (and moved into
`docs/KNOWN_LIMITATIONS.md` with the consequence).

Status values: `open` · `validated` · `invalidated` · `superseded`

---

## A-001 — Host hardware differs from the charter's stated target

**Status:** `validated` (measured 2026-08-05) — the deviation is real and permanent.

The charter (§1, §13, §14 M11) specifies *"an Apple Silicon MacBook Pro with an
M3 Max and 64 GB RAM"*. The actual host is:

```text
model   Mac14,10 (MacBook Pro 14", 2023)
cpu     Apple M2 Pro, 12 cores
memory  16 GiB   (17179869184 bytes)
os      macOS 26.5.2 (25F84), arm64
```

**Consequence — Milestone 11 VM sizing is not executable as written.** The
charter's table allocates edge 3 GB + origin 6 GB + database 4 GB + router 1 GB
= **14 GB of guests on a 16 GiB host**, which leaves nothing for macOS, the
browser, and the compiler. §13.5 already warns "Do not allocate all 64 GB to
VMs"; on this host the constraint is much tighter.

**Revised plan for Milestone 11** (to be confirmed by an ADR when M11 starts):

| VM | charter vCPU/RAM | revised vCPU/RAM |
|---|---|---|
| edge | 2 / 3 GB | 2 / 1.5 GB |
| origin | 4 / 6 GB | 4 / 2.5 GB |
| database | 2 / 4 GB | 2 / 1.5 GB |
| router | 1 / 1 GB | 1 / 0.5 GB |

Total revised guest allocation ≈ 6 GB, leaving ≈ 10 GiB for host, browser and
toolchain. Stage 11A (native-process topology, no VMs) becomes correspondingly
more important as the primary functional validation path, with 11B reserved for
network-impairment tests that genuinely need separate network namespaces.

**Also affected:** benchmark absolute numbers are not comparable to any figure
produced on an M3 Max. Every benchmark record must carry the host string
(charter §18.5 already requires this).

**Validation:** `just doctor` reports memory and warns below 32 GiB;
`docs/environment/macbook.md` records the full detection output.

---

## A-002 — Node 22 (maintenance LTS) is acceptable instead of Node 24 (active LTS)

**Status:** `open`

The charter §13.2 asks for "a pinned current Node LTS". The current *active* LTS
is **v24.19.0 "Krypton"** (verified on `nodejs.org/dist/index.json`, 2026-08-05).
The host has **v22.21.1**, which is a maintenance-track LTS.

Assumption: v22.21.1 is sufficient for Milestone 0 and Milestone 3, because the
two packages that constrain us declare:

```text
vite@8.2.0    engines.node = ^20.19.0 || >=22.12.0    -> satisfied
marko@6.3.32  engines.node = >=22                     -> satisfied
```

**Why not upgrade now:** installing a second Node would add an unpinned variable
to Milestone 0 without changing any gate outcome. The charter's own rule is to
pin *what was actually tested*.

**Invalidation condition:** any Milestone 3 dependency requiring Node ≥ 24, or a
Node 22 end-of-maintenance date arriving before Milestone 3 completes.
**Action if invalidated:** pin Node 24 via `mise` or Homebrew, re-run
`just env-record`, re-run the Marko spike, and record the delta.

---

## A-003 — Koka is usable as a *semantic oracle* through its JS backend

**Status:** see `docs/milestones/E0.md` (measured by `spikes/koka-js-interop`)

Assumed: Koka 3.2.3's JavaScript backend emits modules that Node can load, and
`maybe`/`result` values can cross the boundary with explicit decoding. The
charter's whole Milestone 1 plan depends on this.

**Fallback if false** (charter §20): keep Koka only as an isolated semantic
oracle, do not route the Marko/browser work through it, and pull the own
type/effect checker (Milestone 9) earlier.

---

## A-004 — Marko 6 can emit a genuinely zero-JS static route

**Status:** see `docs/milestones/E0.md` (measured by `spikes/marko-stream-resume`)

The charter's §18.4 budget asserts "static page: 0 application JS and 0 runtime
JS" as a *target to test, not a fact to fake*. Milestone 0 measures what Marko 6
actually emits rather than assuming the marketing claim.

---

## A-005 — WASI 0.3 may not be reliable; 0.2 is the safe path

**Status:** see `docs/milestones/E0.md` (measured by `spikes/wasmtime-component`)

Charter §10.3 instructs preferring the newest stable WASI the toolchain supports
*reliably*, and to fall back to 0.2 behind an adapter if 0.3 tooling is
incomplete. The assumption entering Milestone 0 is that Rust's `wasm32-wasip2`
target plus Wasmtime 47 is the dependable path today.

---

## A-006 — `docs/`-scoped locations satisfy the operator's requested file names

**Status:** `validated` by construction

The session instruction asks for `STATUS.md`, `DECISIONS.md`, and
`ASSUMPTIONS.md`. The charter (§3.4) mandates `docs/STATUS.md` and
`docs/DECISIONS/ADR-*.md`. Because the charter is the authoritative constitution
and duplicated files drift, all three live under `docs/`:

- `docs/STATUS.md` — charter-mandated, single source of truth.
- `docs/DECISIONS.md` — index and short log over `docs/DECISIONS/ADR-*.md`.
- `docs/ASSUMPTIONS.md` — this file. Not named in the charter; added because
  §3.2 and §3.3 require assumptions to be explicit and it had no home.

**Invalidation condition:** the operator wants them at the repository root.
That is a one-line move plus a pointer update.

---

## A-008 — charter v2 is the governing charter

**Status:** `RESOLVED 2026-08-05` — the operator chose **adopt v2 and reopen
Milestone 0**. `PROJECT_CHARTER.md` is now v2 (3,206 lines, sha256
`5711d0d6…f686e6`); v1 is archived at
`docs/research/charter-v1-superseded.md`. Both new spikes were built and run, and
the Milestone 0 gate was re-evaluated against the six-spike requirement.

`perfect-web-master-agent-prompt-v2.md` was added to the repository at 17:39 on
2026-08-05, after `PROJECT_CHARTER.md` had been created from the v1 prompt and
while Milestone 0 was being executed. All Milestone 0 work was done against v1.

**v2 is a strict superset** (3,206 lines vs 3,080). Verified differences:

| area | v1 | v2 |
|---|---|---|
| mission items | 16 | 17 — new item 12, *frame-phase and layout safety* |
| §7.5A | absent | **new**: `dom.mutate`, `style.mutate<LayoutAffect>`, `layout.measure`, `observe.resize`, `observe.intersection`, `animation.composite`, `paint.custom`, `post_paint`; a default frame transaction; a ban on ordinary code calling `offsetWidth`/`getBoundingClientRect`/etc. directly, replaced by a scheduled `LayoutSnapshot<T>` |
| design donors | — | **new**: Jane Street Incremental, Bonsai/Bonsai_web |
| anti-patterns | — | **new**: *"Fine-grained updates are mistaken for layout safety"* |
| **Milestone 0 spikes** | **4** | **6** — adds `bonsai-incremental-model`, `layout-phase-scheduler` |
| **Milestone 0 gate** | "all four spikes run from documented commands" | "all **six** spikes run…, **or** a primary-source-backed blocker is recorded for any upstream toolchain that cannot currently run" |
| E0 research list | — | adds browser rendering phases, forced synchronous layout, CSS containment, `content-visibility`, ResizeObserver, Long Animation Frame attribution |

**Milestone 0's own task list and gate are otherwise byte-identical between the
two versions**, so every artifact produced so far remains required under v2.
Nothing done is invalidated; two spikes are simply missing.

**Why the charter was not overwritten:** replacing the project constitution is a
consequential, human decision. Charter §3.2 permits resolving design questions
experimentally, but this is not a design question — it is which document is
authoritative.

**What the resolution required, all done:**

- `PROJECT_CHARTER.md` replaced by v2; v1 archived, not deleted.
- `docs/SEMANTICS.md` gained §5A (layout effects and frame phases).
- `docs/research/technology-matrix.md` gained Jane Street Incremental and
  Bonsai/Bonsai_web rows.
- `spikes/layout-phase-scheduler` and `spikes/bonsai-incremental-model` built,
  run, and evidenced.
- `docs/milestones/E0.md` re-evaluated against the six-spike gate.

The OCaml toolchain **did** install on Apple Silicon, so the charter's
"record the blocker" escape clause was not needed.

---

## A-007 — `.pw` is a safe provisional source extension

**Status:** `open` — decided provisionally in ADR-0010, revisited in Milestone 2.

`.pw` is used by no widely deployed language toolchain the author is aware of.
This has **not** been exhaustively verified against editor/language registries.
Milestone 2 task 1 owns the real decision; changing it before Milestone 2 costs
a rename of corpus files only.

---

## A-009 — one `pw check` invocation is one program

**Status:** `open` — narrowed by name resolution, which does not exist yet.

Body-level checks need a type environment: `R-007` matches on `OrderState`,
which is declared in `A-002`. `compiler/pw-core/src/check.rs` therefore builds
one `Program` from **every file passed to the invocation** and checks each file
against it.

**What this assumes.** That the files given together belong together. It is how
a real compiler treats a package, and the corpus is written as excerpts of one
application, so it holds for every use the project has today.

**Where it is wrong.** Import-based visibility is not enforced. A file can
currently match on a type it never imported, and `R-007` in fact does. Two
files in different programs declaring the same type name would collide.

**Why it is safe to hold now.** The failure mode is *permissiveness*, not false
reports: the environment can only make a type visible that should not be, and
the only consequence is that a match gets checked which would otherwise be
skipped. It cannot invent a variant, so it cannot invent a diagnostic. The
narrower direction — refusing to check anything until name resolution exists —
would leave the tested exhaustiveness algorithm unreachable from source.

**Retire when** name resolution lands and the environment is built from a
module's imports rather than from the invocation's file list. The test
`the_environment_spans_every_file_checked_together` pins the current behaviour
in both directions, so the change will be visible.

**Narrowed by architect ruling, 2026-08-06.** The correct reading is *"`pw
check` constructs one workspace module graph"*, **not** *"every declaration in
every supplied file is ambiently visible everywhere"*. There is to be no ambient
union of user declarations, and no opaque-symbol fallback: external
implementation is allowed through an explicit interface module, a missing
declaration is not. E2B owns the change; `examples/domain.pw` was written so the
corpus can survive it.

---

## A-014 — an unclaimed event stays in the outbox forever

**Assumed since** 2026-08-07 (E7-P), `runtime/pw-materialize/src/lib.rs`.

`Materializer::drain` consumes an event when nothing listens for it, or when it
matches one of the instances the caller supplied. An event that IS listened for
but matches none of the supplied instances stays pending — see
`docs/RISK_QUEUE.md` 28 for what happened when it did not.

**What this leaves open.** An event whose matching instance never appears
again — a session that closed, a store that was deleted — is never consumed.
The outbox grows, and every drain re-walks a lengthening tail.

**Why it is safe to hold now.** The alternative loses invalidations, and a lost
invalidation is a page that is silently wrong. A growing outbox is visible,
bounded by traffic, and costs only work. The dev host restarts with an empty
database, so the growth cannot accumulate across runs.

**Retire when** entries carry a retention horizon. The natural rule is the one
E6 already implies: an event cannot be claimed by an entry that no longer
exists, so consumption should be tied to the entry lifecycle rather than to a
timer. `concurrent_drain.rs::an_event_nobody_listens_for_is_still_consumed`
pins the branch that must NOT start deferring when this changes.

---

## A-015 — a moved node keeps only focus and caret without an atomic move

**Assumed since** 2026-08-07 (E7-P), `spikes/own-renderer/public/pw-runtime.mjs`.

`MoveInstance` moves the existing nodes rather than re-rendering them.
`Element.moveBefore()` does this atomically and preserves focus, `:active`, CSS
animations and transitions, iframe load state, fullscreen, popover and modal
state — verified against MDN `Web/API/Element/moveBefore`, read 2026-08-07,
which also states it is **not Baseline**.

**What this leaves open.** On an engine without `moveBefore`, the runtime falls
back to `insertBefore` and restores the focused element, its caret and its
scroll offset by hand. A running CSS animation restarts; an iframe reloads; a
`:active` element is released; an open popover closes.

**Why it is safe to hold now.** The alternative — refusing to reorder at all
where the atomic move is missing — makes keyed collections unusable on those
engines. The fallback is strictly better than the re-render it replaces, and
the difference is stated rather than hidden. `keyed-list.spec.mjs` asserts the
focus property on all three engines, and `window.__pw.moveKind` records which
path ran, so a test cannot mistake one for the other.

**Retire when** `moveBefore` is Baseline. The fallback then becomes dead code
and should be deleted rather than kept "just in case" — a fallback nothing
exercises is a fallback nobody knows is broken.

---

## A-016 — a retried handler load asks for a different specifier

**Assumed since** 2026-08-07 (E7-L), `spikes/own-renderer/public/pw-runtime.mjs`.

The browser's module map caches **failures** as well as successes. Re-importing
a specifier whose first load failed returns the same rejected promise for the
lifetime of the page: the network is never touched again, and the control that
depends on it is permanently dead. So a retry appends `?attempt=N`.

**What this leaves open.** The identity is the path and the attempt counter is
a query, so an intermediary keying its cache on the full URL sees each retry as
a distinct resource. That is the desired behaviour for a retry and the wrong
behaviour for a cache trying to be helpful.

**Why it is safe to hold now.** The alternative is a page where one failed load
disables a control until reload, which is exactly the "the button does nothing"
failure `lazy-handler.spec.mjs` exists to prevent. The counter only grows on
failure, so a working page never emits a query at all.

**Retire when** handler modules are fetched and instantiated explicitly rather
than through dynamic `import()` — at which point the module map is not involved
and the retry is an ordinary refetch. E9 owns handler code generation and is the
natural place for that to change.

---

## A-017 — RETIRED 2026-08-07, one day after it was written

**Was:** a capability's type argument is judged against every declaration in
the program rather than against what its file imports.

**Retired by** the architect's Effect-prelude ruling, which split the question
the assumption had conflated:

> Effect names resolve through that prelude; their arguments resolve through
> ordinary lexical/module visibility.

and, on the fallback the assumption existed to protect:

> Don't keep a safe-looking fallback simply because it currently prevents tests
> from failing.

All 31 occurrences were enumerated and repaired by adding the import each file
had been doing without — 20 files, including `packages/pw-platform-web/log.pw`
itself, which declared `log<Public>` while importing neither `capability` nor
`Public`. `TypeArgument::Unscoped` and the ontology's program-wide name set are
deleted, so the mode cannot return without someone writing it back.

**What it cost to hold for a day:** nothing, because it was named. The variant
existed precisely so the occurrences could be counted rather than searched for,
and retiring it was a list of 31 rather than an investigation. That is the
argument for naming a compromise instead of leaving it implicit.


---

## A-018 — a `()` body discards its last value

**Status:** `open` (2026-09-24, ADR-0031).

The language has no statement terminator, so a function declared `-> ()` whose
last expression has another type is taken to discard it, not to return it. The
return relation is not applied to unit declarations.

**Validated by** a language decision on statement terminators or on explicit
discard. **If false:** unit bodies need a result relation, and the corpus's
`-> ()` helpers ending in a call become errors unless they end in `()`. `()`
itself lowers to `Expr::Error` today, so that repair needs the unit value
lowered first.

## A-019 — a query bound in a UI declaration is its success value

**Status:** `open` (2026-09-24, ADR-0031).

`let store = query Store(id)` in a page binds what `Store` produces when it
succeeds: the page reads `{store.name}`, and loading and failure are the page's
to render. The value relation types the binding as the `Ok` side of the
declared `Result`, the same reading `infer.rs` gives `{#each}` over a query.

**Validated by** the renderer's semantics for query bindings being written
down (charter §7.6). **If false:** the page must handle the `Result`, and
passing the binding to a function expecting the value is a type error.

## A-020 — a layout snapshot is read explicitly

**Status:** `confirmed`, the owner's ruling 0031-c (ADR-0210), 2026-10-05.

`LayoutSnapshot<T>` is a type of its own and is read through
`browser.measured` (`w.measured`), not treated as `T`. A-016 reads its widths
that way. The accessor was `value` until ADR-0214: in `browser`, `.value` is
the snapshot's representation, and a member of that name was read in its
place (PW0626).

**Validated by** an architect ruling. **Alternative:** a phase rule, owned by
the layout checker, under which a snapshot reads as its value in the phases
after the measure that produced it.

## A-021 — a receiver-only member is a property

**Status:** `open` (2026-09-24).

A callable member whose only parameter is the receiver is read without a call
(`self.style`, `snapshot.value`), as `infer.rs` already resolves it. The value
relations type such a read as the member's result.

**Validated by** the language reference stating property-call syntax.
**If false:** `self.style` must be written `self.style()`, and the corpus
changes accordingly.

## A-022 — captured values are resume metadata, not identity markup

**Status:** `open`, ruling needed (2026-09-25, ADR-0033 §3).

An element with a resumable handler carries the values its handler reads of
its captures (`data-pw-captures`), including a resource key such as a menu
item's id. That is read as charter §8.5's "metadata needed to resume
interactions", which lists resource keys among what a handler may capture. It
is not read as the identity markup that the 2026-08 ruling ("never put raw
domain keys into identity markup merely for renderer convenience") governs.

**Validated by** an architect ruling. **If false:** captures are resolved by
reference, with the browser sending the instance address and the server
re-deriving the render environment to find the value. A compiled handler then
cannot compute its arguments in the browser.

## A-023 — only an escape-free string literal has a value

**Status:** `settled by ADR-0049`, ruling needed on its escape set and on
`"""` strings being raw (2026-09-25). The text below is the assumption as it
stood; one decoder, `pw_syntax::strings`, now gives every string its value,
and every backend reads it.

**Status until 2026-09-25:** `open`, ruling needed (ADR-0033 §6).

`"espresso"` denotes `espresso`. A string with a backslash, and a `"""`
string, denote nothing until the language states its escape rules and its
multi-line rule. Backends that produce a value (the handler backend, the Wasm
lowering) refuse them. The Koka and Marko backends still pass the token through
to their targets' rules.

**Validated by** the language reference stating string escapes. **If false**
(if escapes should follow some existing language's rules): one decoder in
`Literal::string_value`, with every backend reading it.

## A-024 — an element of a labelled collection carries the collection's label

**Status:** `open`, ruling needed (2026-09-26, ADR-0063).

A name bound over a value's elements has that value's label:
- a `for` loop's names, its iterable's;
- an `{#each}` block's name, its collection's;
- a `{#match}` arm's names, its subject's;
- a lambda's parameters, where the lambda is passed to a call, the join of
  the call's other arguments' labels and a piped value's.

It errs toward private. A fold's accumulator is labelled by the list it
folds, and so is every element of a list that holds one secret.

**Validated by** the privacy model stating how a label reaches a collection's
elements (charter §7.8). **If false** (if labels were tracked per element):
each such binding's label would be its element's, and a lambda's parameters
would take theirs from the callee's declared function type.

## A-025 — a database effect's argument names the data it reaches

**Status:** `open`, ruling needed (2026-09-26, ADR-0101).

`database.write<Carts>` changes what `database.read<Carts>` reads, and
nothing else: a write reaches the readers of its argument as written, as a
capability's identity is its argument (`contract.rs`). A row with no argument
or the family alone (`!{ database }`) names no domain, and PW5106 matches
nothing to it.

**Validated by** the ontology stating what a database effect's argument
denotes: a table, a module's data, or a type. **If false** (if two spellings
could name one domain, or a family row were a write of everything): readers
would be matched by resolved identity, and a family row by every domain.

## A-026 — a declared staleness window excuses a command from invalidating

**Status:** `open`, ruling needed (2026-09-26, ADR-0101).

A query that declares `freshness 5.minutes` has said its entries may be five
minutes old. A command writing what it reads need not invalidate it, since the
entries expire. Charter §9.4 admits time-based freshness "as a fallback or
explicit policy". A reader with no window, or a zero one, must be invalidated.

A reader declaring `consistency strong` beside a window is exempted too. The
library's `Order` declares `freshness 2.seconds` with `consistency strong`,
and what `strong` promises beside a window is not stated.

**Validated by** the owner confirming a window is an invalidation source (as
PW0200 already treats one for a shared cache), and saying what `strong`
means beside one. **If false** (if a command must reach every reader of what
it writes): PW5106 would hold every reader, and a public aggregate read by
many would be invalidated by every write.

## A-027 — kiokun's search index holds `is_common` as an integer

**Status:** `open`, the owner asked (2026-10-12).

Track `kiokun` (W6, 2026-10-11). The rewrite's search index and kiokun.com's
oracle hold `is_common` as an integer, as the builder's committed
`output_search_index.sql` writes it. Loaded as text, which is how sqlite3's
`.import` loads a CSV, as kiokun-data's notes describe, 331 of the oracle's
355 answers differ, and every hit is marked Common (6,542 of 6,542, against
2,183 as integers), since `ORDER BY is_common DESC` then orders nothing. How
production loaded it is not known: robots.txt keeps the live site from this
track.

**Validated by** the owner saying how production loaded the index (asked
2026-10-12; one search on the live site shows it: every result Common is the
text loading). **If false** (if production loaded it as text): live
kiokun.com marks every hit Common, a bug of kiokun.com's own for the owner,
and the rewrite's oracle names that as a Difference rather than matching it.



- ADR0388 candidate: scheduled generation ids are not reused; adapter tokens are local and cannot be persisted/transferred. Rebuilding/restarting selects anew. SQL queue state is authoritative even if a commit acknowledgement is missing; no rollback is inferred from String Err. Local seam tests cover after-commit adapter errors, not actual interrupted sockets or process loss. Old workers must be quiesced for protocol1 rollout. Final strict whole campaign and exact-source hosted proof remain pending.
