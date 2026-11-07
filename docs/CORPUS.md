# Corpus versions

The charter §16 corpus is the executable specification. Once a conformance
figure is published against it, it stops being a floating collection of files
and becomes a **versioned artifact** — otherwise a fixture can be edited into
compliance and a previously discovered failure quietly disappears.

Architect ruling, 2026-08-06:

> Keep two distinct measurements. Current-corpus conformance tells you whether
> today's specification passes. Historical-corpus compatibility prevents a
> fixture from being edited into compliance and silently erasing a previously
> discovered failure.

---

## C1 — frozen 2026-08-06

```text
Corpus version:            C1
Accepted programs:         24
Rejected programs:         44
Charter categories:        24/24 accepted, 44/44 rejected
Corpus commit:             324148b
Compiler commit:           324148b
Result:                    24/24 accepted clean, 44/44 rejected enforced
Wrong-reason catches:      0
Modified since C0:         10 fixtures (below)
Generality at freeze:      7/29 invariants generality-tested
```

Reproduce: `just ci` for the gate, `just evidence-corpus` for the per-fixture
table, `just generality` for the second score.

**C1 is frozen. C2 and C3 both opened on 2026-08-06; C4 opened 2026-08-07; C5 opened
2026-08-10; C6 opened 2026-08-11** — see below. A change to
any file under `examples/accepted` or `examples/rejected` opens the next version
and requires a row in its table plus a recorded reason. The rule exists because the path to 44/44 changed ten
fixtures, and a reader who does not know that will read the number as stronger
than it is.

### C0 → C1: every changed fixture

`C0` is, **per fixture**, the text immediately before the change that let the
compiler enforce it — not a single global commit. That distinction was learned
the hard way and is recorded under *Historical compatibility* below. For each
change: what it was, why, whether it repaired **missing program context** or
**altered the invariant**, and the compiler's result before and after.

| fixture | change | kind | before | after |
|---|---|---|---|---|
| R-001 | `import Stores.{ fetch_store }`, and `fetch_store` declared in `examples/lib/Stores.pw` with `!{ network.fetch }` | missing context | silent — the call resolved to nothing, so no effect was inferred | `PW0401` |
| R-004 | `import store.queries`, `import cart.queries` | missing context | caught *through* an ambient module union that E2B removed | `PW5001` |
| R-010 | `import domain.{ DatabaseConnection }`, `import Database`; `DatabaseConnection` declared with `Database.connect` acquiring it | missing context | silent | `PW5008` |
| R-012 | `import Maps`; effect row corrected to `!{ resource.acquire<MapHandle>, dom.mutate, layout.measure }` | **second defect removed** | caught, but for undeclared effects — not for the affine leak it specifies | `PW2005` |
| R-022 | `import events.{ PressEvent }` | missing context | silent | `PW0602` |
| R-023 | a `page StoreOverview` declaring `route "/stores/{id}"` added to the file | **declaration added** | silent — with no declared route, "matches no declared route" is not a statement about anything | `PW5009` |
| R-024 | `import html.{ raw_html }`, and `raw_html` declared with `!{ unsafe.raw_html }` | missing context | silent | `PW5010` |
| R-025 | `import device`, and `device.current_location` declared with `!{ device.location }` | missing context | silent — the placement rule existed and was registered, and had nothing to compare | `PW5005` |
| R-030 | `import domain.{ Cart }`, `import cart.queries` | missing context | silent | `PW5007` |
| R-041 | `import document`, and `document.pw` declared | missing context | silent | `PW0401` |

Nine of the ten are the same repair: **the fixture called something that
existed nowhere**, so the call resolved to nothing, no effect was inferred, and
a rule that in several cases already existed could not fire. The fixture was
silent because the *program* was incomplete, not because the checker was.

Two are not that, and are called out separately because they are the ones a
sceptical reader should look at hardest:

- **R-012** had a second, real defect that masked the one it was written for.
  Removing it made the fixture test its declared invariant instead of an
  unrelated one. This *narrowed* what the fixture proves.
- **R-023** gained a declaration rather than an import. Its invariant is
  relational — a link is dead relative to a route table — and the file had no
  route table.

**No change weakened an invariant.** Every `@expect-error` line in C1 is the
one C0 had; the diffs above add imports and declarations and remove one
unrelated defect. `a_caught_file_conveys_every_fact_its_expect_error_lines_declare`
holds the fixtures to that text, so a future edit that softened an expectation
would have to soften the assertion too, visibly.

### Historical compatibility

`compiler/pw-core/tests/corpus_history.rs` runs the **C0 text** of every changed
fixture against today's compiler and asserts it is still caught **for its
declared invariant** — not merely that it is still red. That distinction is not
theoretical: the first version of this test used a single global baseline
commit that predated the corpus being well-formed, so R-025's old text failed
to *parse*, and the test counted a syntax error as historical compatibility.
The baseline is now per-fixture — the text immediately before the change that
enabled enforcement — and the assertion is on the invariant symbol.

Result: **nine of the ten repairs did not remove the defect.** The old text is
still caught, for the same invariant, without the import or declaration that
was added. That is the evidence the repairs were obstruction-removal.

| fixture | its C0 text today |
|---|---|
| R-001 | `forbidden_effect` |
| R-004 | `private_in_shared_cache` |
| R-010 | `unserializable_capture` |
| R-012 | `affine_not_consumed_once` |
| R-022 | `handler_signature_mismatch` |
| R-023 | **clean** — classified `FixtureDidNotExpressIt` |
| R-024 | `unsafe_audit_incomplete` |
| R-025 | `declared_placement_cannot_grant` |
| R-030 | `private_in_resume_manifest` |
| R-041 | `forbidden_effect` |

The one miss is **classified**, not rounded away. `EXPECTED_TO_PASS` carries
one of four judgements per entry, and they mean different things:

| classification | meaning |
|---|---|
| `FixtureDidNotExpressIt` | the old text did not contain the violation it declared. Benign — the corpus got more precise. |
| `SpecificationChanged` | the language or charter changed. Needs a charter reference. |
| `CompilerRegressed` | it used to be caught and is not. A **defect**. |
| `KnownCheckerGap` | the checker cannot see it. A **defect**, and must also exist as a `slips-through.pw`. |

The last two are rejected by the test rather than accepted as explanations: a
regression must be fixed, and a checker gap must be tracked where gaps are
tracked. Only the two benign classifications may sit in the list quietly.

**R-023 is `FixtureDidNotExpressIt`.** `dead_internal_link` is a *relation*
between a link and a route table, and the old file contained one half of it.
Its silence is correct.

The history suite is kept because it can **disagree** with the current corpus.
Forcing it to 10/10 would remove the only thing it is for.

The C0 texts live in `examples/history/C0/`.

---

## Three separate test bodies

They answer different questions and must not merge. Folding the new programs
into C1 would make the specification drift every time an implementation is
tested for generality — which is precisely the failure mode versioning the
corpus was meant to prevent.

| suite | where | question |
|---|---|---|
| **C1** — executable specification | `examples/{accepted,rejected}` | does the compiler conform to the declared corpus? |
| **G1** — generality challenges | `examples/generality/` | do the same guarantees survive programs the fixtures did not anticipate? |
| **Robustness** | `compiler/pw-core/tests/robustness.rs`, `examples/robustness/` | can an unanticipated program shape crash the compiler or suppress its output? |
| **History** | `examples/history/` | does the pre-change text of a modified fixture still fail? |

C1 is frozen. G1 grows freely — it is not a specification, it is a record of
what has been challenged. Robustness grows by regression: every panic found
keeps its original reproducer, a minimized one, the phase that crashed, the
fix, and a negative control showing the old implementation fails.

---

## C2 — opened 2026-08-06 (E6)

```text
Corpus version:            C2
Accepted programs:         24
Rejected programs:         44
Charter categories:        24/24 accepted, 44/44 rejected
Result:                    24/24 accepted clean, 44/44 rejected enforced
Wrong-reason catches:      0
Modified since C1:         7 fixtures (below)
Generality at open:        29/29 invariants generality-tested
```

Reproduce: `just ci` for the gate, `just evidence-corpus` for the per-fixture
table, `just generality` for the second score.

### Why it opened

**E6 resolved policy clauses for the first time.** `depends_on`,
`invalidates_on`, `emits` and `invalidates` name declarations, and until this
milestone nothing checked that the declarations existed. Seven fixtures named
resources and events that no file in their program declared.

That is the same shape as the C0 → C1 repair — *the fixture referred to
something that existed nowhere* — arriving in a second place, and it went
unnoticed for the same reason: a clause nobody resolved cannot be reported as
unresolved. Nine of ten C1 changes were calls that resolved to nothing. These
are edges that resolved to nothing.

It matters more here than it looks. An edge to nothing is not an error at run
time, it is **silence**: the fragment never regenerates, the page it produces
stays valid, well-formed and permanently out of date, and no test of the page
can tell that apart from a fragment whose inputs never changed.

### C1 → C2: every changed fixture

| fixture | change | kind | before | after |
|---|---|---|---|---|
| A-003 | `import Events.{ StoreChanged }` | missing context | `invalidates_on StoreChanged(id)` named nothing | resolves; `PW5100` clean |
| A-005 | `import Resources.{ Cart }` | missing context | `invalidates Cart(..)` named nothing | resolves |
| A-008 | `import Events.{ MenuChanged }` | missing context | `invalidates_on MenuChanged(id)` named nothing | resolves |
| A-009 | `import Resources.{ Store, Menu }`, `import Events.{ MenuChanged, InventoryChanged }` | missing context | **all four** graph edges named nothing — the fixture for edge materialization described a graph with no edges | resolves |
| A-010 | `import Resources.{ Order }` | missing context | `invalidates Order(order)` named nothing | resolves |
| R-017 | the same imports, plus `code_version included_in_key` | missing context + **second defect removed** | caught for `PW0401`, and would now also emit `PW5102` | `PW0401` alone |
| R-029 | `import Resources.{ Cart }` | missing context | `invalidates Cart(..)` named nothing | `PW0327` alone |

Six of the seven are imports. The two that are not:

- **R-017** gained `code_version included_in_key`, which removes a *second*
  defect the fixture did not declare. Same repair as R-012 in C1, and it
  narrows what the fixture proves: it is about the wall clock, so it must be
  caught for the wall clock.
- **A-009** is the one worth looking at hardest. Its category is "edge
  materialized menu" and every edge it declared pointed at nothing, so the
  fixture demonstrated the SYNTAX of a dependency graph and none of its
  semantics. It passed C1 because no analysis had ever read those clauses.

**No change weakened an invariant.** Every `@expect-error` line in C2 is the one
C1 had.

### New: `examples/lib/Resources.pw`

The library gained the four resources the corpus's policy clauses name —
`Store`, `Menu`, `Cart`, `Order`. Library files are not corpus fixtures and do
not change the 24/44 counts; they are the world the fixtures refer to, the same
role `Stores.pw` and `Carts.pw` have had since C1.

`A-003` and `A-004` keep their own `Store` and `Cart`. They are the fixtures
that demonstrate how such a query is *written*, and a fixture referring to
itself would prove nothing about resolution across files.

---

## C3 — opened 2026-08-06 (E6, on the architect's ruling)

```text
Corpus version:            C3
Accepted programs:         24
Rejected programs:         46
Charter categories:        24/24 accepted, 46/46 rejected
Result:                    24/24 accepted clean, 46/46 rejected enforced
Wrong-reason catches:      0
Added since C2:            2 fixtures (below)
Generality at open:        30/31 invariants generality-tested, 1 known narrow
```

### Why it opened

Architect ruling, 2026-08-06:

> A new user-facing language invariant introduced by E6 should have at least
> one canonical accepted/rejected specification pair. Otherwise your dashboard
> eventually says `generality-tested 29 / 29` while the compiler actually
> contains 31 or 32 semantic invariants. That denominator would have stopped
> meaning "all invariants".

E6 added two invariants that no §16 category named. They had rule fixtures and
generality witnesses but no place in the executable specification, so the
denominator did not move and the score was measuring a smaller compiler than
the one that exists.

### C2 → C3: the two fixtures added

| fixture | category | invariant | why it is canonical |
|---|---|---|---|
| R-045 | private dependency of a shared materialization | `private_in_shared_materialization` | the defect is on the **edge between two individually valid declarations** — `Cart` correctly asks for a private cache, the fragment is correctly entitled to a shared entry, and neither is wrong on its own. No single-declaration rule can see it, and `PW5001` does not |
| R-046 | dependency graph edge to an undeclared target | `graph_edge_unresolved` | invalidation is driven by the graph, so an edge to nothing is not an error at run time — it is silence, and silence is indistinguishable from a fragment whose inputs never changed |

No accepted fixture was added. A-009 is the accepted neighbour for both: it is a
shared materialization whose edges all resolve, and since C2 they do.

### The two lines this changed in the test suite

Both were floors written as constants — `errored >= 44`, `checked == 44` — and
a corpus that grows past a constant floor leaves its new fixtures unenforced
while the assertion still passes. They now count the directory.

`generality-tested` is **30 / 31 with one known narrow invariant**, not 31/31.
`private_in_shared_materialization` has two executable known gaps
(`slips-through-helper.pw`, `slips-through-branch.pw`), and an invariant with a
known gap is not generality-tested. The published triple is asserted, and a
`NARROW` witness is only admissible where `DIMENSIONS.md` has a `## Known gaps`
section — otherwise `NARROW` becomes the escape hatch that turns any failing
witness into an accepted limitation.

---

## C4 — opened 2026-08-07 (E8, on the architect's Effect-prelude ruling)

```text
Corpus version:            C4
Accepted programs:         24
Rejected programs:         46
Charter categories:        24/24 accepted, 46/46 rejected
Result:                    24/24 accepted clean, 46/46 rejected enforced
Wrong-reason catches:      0
Changed since C3:          4 fixtures (below)
Generality at open:        30/31 invariants generality-tested, 1 known narrow
```

### Why it opened

Architect ruling, 2026-08-07:

> Effect names resolve through that prelude; their arguments resolve through
> ordinary lexical/module visibility. […] If `Payments` isn't imported or
> otherwise in scope, that's a source error.

and:

> If `secret<Payments>` appears in `branch-join.pw` and `Payments` isn't visible
> there, the witness was underspecified. Fix the witness. Do **not** weaken
> Pleris's visibility model to preserve it.

**Two specification changes, not checker improvements.** The second, added the
same day: a frame-phase keyword no longer synthesizes an effect. Architect
ruling — *a phase says WHEN work runs; an effect says what it does* — so
`measure { .. }` stopped contributing `layout.measure` and R-042 went silent,
because the effect it tests was never coming from an operation.

The first: before it, an
effect's type argument was judged against every declaration in the program;
after it, against what the file imports. 31 rows across 20 files named an
argument they had not imported — including `packages/pw-platform-web/log.pw`,
which declared `log<Public>` while importing neither `capability` nor `Public`.
The platform library was doing it too.

Three of the 20 files are rejected fixtures, so the version opens.

### C3 → C4: the three fixtures changed

| fixture | change | what it was missing | before | after |
|---|---|---|---|---|
| R-006 | `+ import capability.{ Payments, Public }` | `secret<Payments>` and `log<Public>` named types it never imported | `PW5006` | `PW5006` |
| R-012 | `+ import browser.{ MapHandle }` | `resource.acquire<MapHandle>` named a type it never imported | `PW0310` | `PW0310` |
| R-026 | `+ import capability.{ Payments }` | `secret<Payments>` named a type it never imported | `PW5003` | `PW5003` |
| R-042 | `self.height()` → `el.height()`, `+ import browser.{ ElementRef }` | `self` had no resolved type, so the member named no declaration and the `layout.measure` the fixture is about came from the `measure` KEYWORD | `PW0402` | `PW0402` |

**No `@expect-error` line changed and no verdict moved.** Each fixture is
caught for the same code, by the same rule, for the same reason. What changed
is that the fixture is now a valid Pleris program apart from the defect it
exists to demonstrate — which is what a rejected fixture is supposed to be.

The other 17 files are generality witnesses, rule fixtures and the platform
library, none of which is versioned.

### What this did NOT do

It did not make the corpus stricter about the defects it tests. Every one of
the three was already caught, and would have been caught with or without the
import. The change is that they were relying on a visibility rule the language
no longer has — the same shape as the C0 texts recorded as
`FixtureDidNotExpressIt` in `corpus_history.rs`, caught before it had a chance
to become one.

---

## Opening a corpus version

A new version opens when the *specification* changes — a new charter category, a corrected
`@expect-error`, a fixture that was wrong about the language. It does not open
because a checker got better; that is what `examples/generality/` is for.

To open it:

1. Add the row to the C1→C2 table with the four fields above.
2. Copy the pre-change text into `examples/history/C1/` if the change is to a
   fixture C1 measured.
3. Re-run all three commands and record the new figures here.
4. Say in the commit message which measurement changed and why.

---

## C5 — opened 2026-08-10 (E10, on the architect's semantic-ownership ruling)

```text
Corpus version:            C5
Accepted programs:         24
Rejected programs:         46
Charter categories:        24/24 accepted, 46/46 rejected
Result:                    24/24 accepted clean, 46/46 rejected enforced
Wrong-reason catches:      1 found and repaired (below)
Changed since C4:          6 rejected fixtures, 4 accepted, 1 witness verdict
Unowned call-shaped constructs: 0
```

### Why it opened

`PW0021` had never examined a **bare** call. `unresolved_uses` reads
`path.split_once('.')` and returns if there is no dot, so a call to a name
nothing declares was not merely unreported — it was never asked about. Four
accepted programs had been calling into the void since E4, and the reason
nothing noticed is that every analysis produces an answer for a call it cannot
resolve, and each answer is what a harmless call produces too.

Architect ruling, 2026-08-10:

> Every semantic use in accepted code has an owner. […] Never: call-shaped
> thing → nobody owns it → analyses see nothing. Provenance answers *why did
> the compiler conclude this?* Semantic ownership answers *did the compiler
> ever assign meaning to what the author wrote?*

The rule could only land once the residue was small. Two earlier attempts
reported sixteen and then eleven corpus errors and were reverted;
`tests/semantic_ownership.rs` names every other owner — language constructors,
declared-union constructors, policy operators, CSS value functions, members of
an unresolved receiver — and leaves a residue of four.

### C4 → C5: the fixtures changed

| fixture | change | what it was missing | before | after |
|---|---|---|---|---|
| A-005 | `current_consumer()` → `current_session()`, `+ import context` | the NAME was wrong: `Carts.add` takes a `SessionId` and the command invalidates a session-keyed entry | clean | clean, `+ session.read` |
| A-013 | `+ import build.{ include_markdown, WorkspaceFile }`, row `!{ build.input.read<WorkspaceFile> }` | `include_markdown` was declared nowhere, so a build-determinism claim held against silence | clean | clean, `+ build.input.read<WorkspaceFile>`, placement `build` |
| A-014 | `+ import cart.commands.{ add_to_cart }` | the deferred handler's callee was attributed to nobody | clean, **no contract** | clean, a `view` contract with a component dependency |
| store | `+ import context.{ current_session }` | called in a page and two commands, imported nowhere | clean | clean, `+ session.read` on three components |
| R-011 | `+ import context` | `current_session` | `PW0309` | `PW0309` |
| R-012 | `+ import Maps.{ default_center }` (and the declaration) | `default_center` | `PW0310` | `PW0310` |
| R-020 | `+ import domain`, `+ import cart.commands` | `add_to_cart`, `PositiveInt` | `PW0318` | `PW0318` |
| R-029 | `current_consumer()` → `current_session()`, `+ import context` | A-005's rejected twin, same defect | `PW0327` | `PW0327` |
| R-030 | `+ import Carts.{ checkout }` (and the declaration) | `checkout` | `PW0328` | `PW0328` |
| R-043 | none — `var(--x)` is a CSS value function, now declared as one | the compiler's vocabulary, not the fixture | `PW3009` | `PW3009` |

**No `@expect-error` line changed and no rejected verdict moved.** Every
rejected fixture is caught for the same code, by the same rule, for the same
reason. What changed is that each is now a valid Pleris program apart from the
defect it exists to demonstrate.

### The wrong-reason catch

`examples/generality/resume_capture_schema_unnameable/caught.pw` was caught, and
for the wrong reason. It read `let derived = helper(store_id)` with `helper`
declared nowhere — so `derived` had no type, so its schema was unnameable. The
invariant's subject is a type that **cannot** be named; what was being exercised
was a call that did not exist. Declaring `helper` with a return type made the
witness go silent, which is what surfaced it. It is now an anonymous record,
which is the shape the invariant is about and depends on no resolution failure.

Sixteen generality witnesses and three rule fixtures were repaired the same way.
Neither set is versioned, but the count is recorded because it is the size of
the hole: twenty-three programs across three corpora that exist to be evidence
were calling names that did not exist.

### What this did NOT do

It did not change what any rejected fixture is about, and it did not weaken any
rule to accommodate a fixture. The one verdict that moved, moved because the
witness was invalid evidence and was replaced by a valid one.

---

## C6 — opened 2026-08-11 (E10, on the architect's optimistic-transition ruling)

```text
Corpus version:            C6
Accepted programs:         24
Rejected programs:         46
Charter categories:        24/24 accepted, 46/46 rejected
Result:                    24/24 accepted clean, 46/46 rejected enforced
Changed since C5:          5 files, and ONE INVARIANT RETIRED
```

### Why it opened

**A rejected fixture's invariant was retired**, which no previous version has
done. ADR-0025:

> "an optimistic transition is a function from the current value to the next,
> and rollback is its inverse" — the first half is excellent. The second is
> generally false. […] Pleris already knows the exact pre-optimistic resource
> value/version. Making the programmer describe the inverse is precisely the
> kind of redundant mechanism the project is trying to eliminate.

`optimistic_no_rollback` said an optimistic transition must declare how it is
reversed. It is gone. A cart holding `Apple × 3`, optimistically `+2`, is
`Apple × 5`, and a written `remove(apple)` does not restore `Apple × 3`.

What replaced it is what makes automatic restoration meaningful:
`optimistic_not_pure`. The transition runs on the client and is abandoned by
restoring the value that was held, and an externally visible effect cannot be
abandoned that way.

### C5 → C6

| fixture | change | before | after |
|---|---|---|---|
| store | `optimistic Cart(current_session()) as cart => Carts.with_line(..)`, `rollback` deleted | clean | clean |
| A-005 | the same | clean | clean |
| R-029 | **subject changed**: the impure transition, not the missing rollback | `PW0327` *optimistic without rollback* | `PW0330` *optimistic not pure* |
| `generality/optimistic_no_rollback/` | renamed `optimistic_not_pure/`, both witnesses rewritten | — | — |
| `examples/lib/Carts.pw` | `+ fn with_line(cart, item, quantity) -> Cart !{}` | — | — |

**R-029's `@category` is unchanged** — *optimistic state with no rollback path* —
because the failure mode is unchanged: an optimistic update that cannot be
cleanly abandoned leaves the UI permanently inconsistent with the server. Only
the mechanism moved. The `@rule` and `@invariant` both changed and the header
records why.

`PW0327` was **reused rather than retired**: same code, same declaration,
opposite verdict. It required a `rollback`; it now refuses one. A reader looking
the code up finds what replaced it instead of a dead entry.

### The finding the repair exposed

`optimistic cart.add(item, quantity)` called a member `Cart` does not have —
`Cart` is a record of `lines` — and the `database.write<Carts>` the analysis
attributed to it came from `Carts.add` through `effects::resolved`'s scoped
last-segment member fallback. A three-argument origin write standing in for a
two-argument client-side transformation.

Nothing was wrong with the fallback's own rule, which is scoped-and-unique and
exists for `it.style.set_padding`. What was wrong is that the corpus depended on
it for a call that should never have resolved. `Carts.with_line` is the pure
`Cart -> Cart` the transition actually needed, and it is declared `!{}`.

### Reformatted, 2026-08-11

`A-007`, `A-009`, `A-011` and `A-021` are canonically formatted differently
now, and no verdict moved. Their clauses became real policies when
data-operation declarations began parsing their policies inside their braces, so
they are column-aligned like every other policy block — and a multi-line policy
value keeps its indentation, which `A-009`'s `invalidates_on` continuation had
been missing since E6.

### The declaration-level coverage floor moved 4 → 3

Recorded here because a floor going down is the shape `docs/RISK_QUEUE.md`
warns about. `R-029` was caught by `rules.rs`, which reads declaration HEADERS;
its new defect is an effect question that only `check.rs` can answer. The
compensating assertion is in `rules::corpus_tests`: R-029 must still be caught,
by the whole checker, for its declared code.


## C7: resolved-signature fixture repairs, 2026-09-16

No invariant is retired. The current 24 accepted / 46 rejected corpus remains
clean / rejected for its declared rule. These changes remove accidental reliance
on undeclared type names and avoid testing two independent defects at once.

| Fixture | Repair | Preserved obligation |
|---|---|---|
| R-008 and option-field rule | Import the Store type explicitly | Unchecked Option field access is refused |
| R-009 and cast generality witness | Import Unknown and Store | Unchecked external cast is refused |
| R-032, R-037, effect-oracle fixtures | Import ElementRef | Layout reads still violate their effect/phase context |
| R-029 and optimistic generality witness | Effectful helper returns the actual Cart, not Result<Cart, Error> | Optimistic transition remains impure and is rejected for that rule only |
| stdlib list | Import Decoder and Unknown from decode | Platform signatures resolve completely |

C0 raw texts are not edited. R-010 and R-030 have unimported parameter types;
they now fail as unnameable captures instead of borrowing the resource/privacy
meaning of globally same-spelled types. R-022's unimported PressEvent is blocked
rather than a known mismatch. Current imported counterparts retain their original
rules. `corpus_history.rs` tests those boundaries explicitly and keeps the known
ordinary-type diagnostic gap visible. New `signature_behavior.rs` tests cover
same-spelled but genuinely different nominal types, including imported handlers.
The platform signature hash changes only for the explicit list imports.

## C8: the value relations' first run, 2026-09-24

Opened because the specification changed: written types must now resolve
(`PW0026`) and every value must have its declared type (`PW0604`–`PW0607`).
[ADR-0031](DECISIONS/ADR-0031-value-relations.md) is the decision;
[the evidence](evidence/E9/value-relations-2026-09-24.md) lists the defects the
first run found. No invariant is retired and no `@expect-error` line changed:
the current 24 accepted / 46 rejected corpus remains clean / rejected for its
declared rule only. Pre-change text of every modified fixture is in
`examples/history/C8/`.

| Fixtures | Repair | Kind |
|---|---|---|
| 25 rejected, 55 generality, 7 rule fixtures; A-005, A-006, A-007, A-015..A-019, A-022..A-024 | Import the domain/platform types they name (`StoreId`, `OrderId`, `ElementRef`, ...) | missing context |
| A-006, A-007/A-019/A-024/R-044, A-010, A-015..A-017, A-023, R-025 | Declare what they name and nothing declared: `stream.Stream`, `domain.LatLng`, `domain.PaymentReceipt`, `browser.LayoutSnapshot`, `domain.InventoryRow`, `domain.DeliveryEstimate`/`EstimateError` | missing declaration |
| A-001, A-010, R-014, retry witnesses | `type Money<C>`: the corpus wrote `Money<USD>`; the argument was being dropped | declaration corrected |
| A-004, store, `Carts`, `Resources` | `Session<SessionId>` where `current_session()` is passed (ADR-0031 §7) | signature corrected |
| store `Menus` | `for_store` returns `List<MenuItem>`, which the page renders | library corrected |
| A-012, `StoreId`, `decode` | `from_string` fails with `DecodeError`; `field<T>` returns what its decoder produces | library corrected |
| A-016 | Reads snapshot values through `.value`; `List.sum`/`maximum` are over `Float` | fixture and library corrected |
| A-017, R-034 | `List<BadgeId>` (declared nowhere) is `List<ElementRef>`: the badges are styled elements | fixture corrected |
| A-005, optimistic witness | `domain.Cart` qualified: `Cart` is also the imported query | name made unambiguous |
| R-005, cache-key witnesses | `MenuError` (declared nowhere) is `StoreError`, what `Menus` returns | fixture corrected |
| R-012, affine witnesses and rule | Map containers are `ElementRef`, as components hold them | fixture corrected |
| payment witnesses | `Money(100)` rather than `100`; results are `PaymentReceipt` | fixture corrected |
| sink witnesses | `log.public<T>` records any value; the record witness declares its field `Secret<Payments>` | library and fixture corrected |
| `undeclared_effect` row witnesses | `Telemetry.record_visible(id)` for a store id, same `network.fetch` effect | fixture corrected |
| transaction witnesses | `Database.rollback` returns `Result<(), CartError>`, as `commit` does | library corrected |

The history question is unchanged: the rejected fixtures' old texts in `C8` still
fail for their declared invariant, now also with `PW0026` for the missing
import that each repair added. That is the evidence the repairs removed an
obstruction, not the defect.

One exception, found by ADR-0200: R-019's C8 text writes a comment as `//` in
markup. Since ADR-0167 that is text, so the `<ul>` it mentions opens an
element that never closes, and the text no longer parses. The fixture itself
moved to `<!-- -->` with ADR-0167. Until ADR-0200 the suite checked what
recovery made of the old text; it now holds that the text does not parse,
and is refused for that.

## C9: a page states its title, 2026-10-04

Opened because the specification changed. A page served at a route states
its title (`PW5029`), and a title is the page's: once, at the top of its
view, written as text and values (`PW5030`).
[ADR-0183](DECISIONS/ADR-0183-a-page-states-its-title.md) is the decision. No
invariant is retired, and no `@expect-error` line changed.

```text
Corpus version:            C9
Accepted programs:         25
Rejected programs:         48
Charter categories:        25/25 accepted, 48/48 rejected
Generality at open:        33/33 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-025 | added: a page served at a route states its title, from its values | new category, `page states its title` |
| R-047 | added: a page served at a route states none (`PW5029`) | new category |
| R-048 | added: a title in a view (`PW5030`) | new category |
| R-023 | its page, served at a route, gained `<title>Store</title>` | missing context |
| `dead_internal_link`'s GENERAL and NEIGHBOUR witnesses | their pages, served at routes, gained a title | missing context |
| `rules/routes/link-to-declared-route.pw` | its page gained a title | missing context |
| `title_missing`, `title_misplaced` | a GENERAL and a NEIGHBOUR witness each | new invariants |

The old texts of the four changed fixtures are in `examples/history/C9/`.
`corpus_history.rs` checks them:
- R-023's is still caught for `dead_internal_link`, now beside
  `title_missing`;
- the other three are refused for the title alone.

R-023's C0 entry in `EXPECTED_TO_PASS` is retired. Its old text declared no
route, so its link was dead relative to nothing. A-025 now declares a route
in the program every old text is checked in, so the old link is dead
relative to it, and the C0 text is caught for its invariant again.

## C10: ids and ARIA, checked at build, 2026-10-04

Opened because the specification changed: an id names one element of its
page (`PW5031`), a reference names an element its page shows whenever the
referrer is shown (`PW5032`), and ARIA is what WAI-ARIA defines (`PW5033`).
[ADR-0185](DECISIONS/ADR-0185-ids-and-the-aria-that-names-them-checked-at-build.md)
is the decision. No invariant is retired, and no `@expect-error` line
changed.

```text
Corpus version:            C10
Accepted programs:         26
Rejected programs:         51
Charter categories:        26/26 accepted, 51/51 rejected
Generality at open:        36/36 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-026 | added: ids, references and roles written right, an id in both arms of a block among them | new category, `ids and the ARIA that names them` |
| R-049 | added: two elements with one id (`PW5031`) | new category |
| R-050 | added: `aria-describedby` naming nothing (`PW5032`) | new category |
| R-051 | added: `aria-labeledby` (`PW5033`) | new category |
| `control_without_label/labelledby-nothing.pw` | moved to `reference_names_nothing`: its defect is the reference, and PW5014 no longer reports the field it leaves unnamed | invariant moved |
| `control_without_label/label-for-another.pw` | its label names another control's id, not a missing one | missing context |
| `duplicate_id`, `reference_names_nothing`, `aria_unknown` | a GENERAL and a NEIGHBOUR witness each | new invariants |

The two witnesses' old texts are in `examples/history/C10/`.
`corpus_history.rs` checks them:
- `label-for-another.pw`'s is reported for both its defects, each where it
  is;
- `labelledby-nothing.pw`'s for the reference alone.

## C11: a page states its description, 2026-10-04

Opened because the specification changed: a page's metadata is the page's,
at the top of its view, named once as text, its content text and values,
and a name HTML allows once stated once (`PW5034`).
[ADR-0186](DECISIONS/ADR-0186-a-page-states-its-description.md) is the
decision. No invariant is retired, no `@expect-error` line of an earlier
fixture changed, and no fixture moved.

```text
Corpus version:            C11
Accepted programs:         27
Rejected programs:         52
Charter categories:        27/27 accepted, 52/52 rejected
Generality at open:        37/37 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-027 | added: a page's description and Open Graph title, from the values it reads | new category, `page states its description` |
| R-052 | added: a `<meta>` in a view (`PW5034`) | new category |
| `metadata_misplaced` | a GENERAL witness, a page that states its description twice, and a NEIGHBOUR one, a description and two Open Graph images after `<main>` | new invariant |

## C12: a `<link>` is written where HTML allows it, 2026-10-04

Opened because the specification changed: a `<link>` in markup is one HTML
allows in the body, each relation body-ok or an item's property
(`PW5035`).
[ADR-0189](DECISIONS/ADR-0189-a-link-is-written-where-html-allows-it.md) is
the decision. No invariant is retired, no `@expect-error` line of an earlier
fixture changed, and no fixture moved.

```text
Corpus version:            C12
Accepted programs:         28
Rejected programs:         53
Charter categories:        28/28 accepted, 53/53 rejected
Generality at open:        38/38 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-028 | added: links the body allows, a stylesheet, a preconnect and an item's property | new category, `links the body allows` |
| R-053 | added: a canonical link in a page's markup (`PW5035`) | new category |
| `link_not_in_body` | a GENERAL witness, an icon among body-ok relations in a view, and a NEIGHBOUR one, body-ok relations in another case and an item's property | new invariant |

## C13: a type that contains itself, and one no finite value has, 2026-10-05

Opened because the specification changed: a type may contain itself
through a list, and one each of whose values holds another value no finite
one fills is refused where it is declared (`PW0624`).
[ADR-0194](DECISIONS/ADR-0194-a-type-that-contains-itself-crosses-as-its-nodes.md)
is the decision. No invariant is retired, no `@expect-error` line of an
earlier fixture changed, and no fixture moved.

```text
Corpus version:            C13
Accepted programs:         29
Rejected programs:         54
Charter categories:        29/29 accepted, 54/54 rejected
Generality at open:        39/39 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-029 | added: a reply thread, a comment and a list of comments, built and walked | new category, `a type that contains itself through a list` |
| R-054 | added: a thread whose every value holds its first reply (`PW0624`) | new category |
| `no_finite_value` | a GENERAL witness, two sum types holding each other in every case through a generic record, and a NEIGHBOUR one, six types each with a way to be built without itself | new invariant |

## C14: a pattern's name by its capital, 2026-10-05

Opened because the specification changed: a pattern's name is a case by its
capital and a binding otherwise, and a case is declared with one (`PW0625`);
a misspelt case is a case its type lacks (`PW0608`).
[ADR-0197](DECISIONS/ADR-0197-a-pattern-tells-a-case-from-a-binding-by-its-capital.md)
is the decision. No invariant is retired, no `@expect-error` line of an
earlier fixture changed, and no fixture moved.

```text
Corpus version:            C14
Accepted programs:         30
Rejected programs:         56
Charter categories:        30/30 accepted, 56/56 rejected
Generality at open:        41/41 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-030 | added: cases and bindings in one match, nested, each told by its capital | new category, `cases and bindings told apart by their capital` |
| R-055 | added: a case named in lowercase (`PW0625`) | new category |
| R-056 | added: a misspelt case in a pattern (`PW0608`) | new category |
| `case_name_capitalized` | a GENERAL witness, a lowercase case with a payload in a generic type, and a NEIGHBOUR one, cases with capitals, digits and underscores | new invariant |
| `pattern_constructor` | a GENERAL witness, a misspelt case nested in an `Option`'s, and a NEIGHBOUR one, a binding named like a case in lowercase | new witnesses |

## C15: a form control's value where HTML reads it, 2026-10-05

Opened because the specification changed: a form control's value is written
where HTML reads it, a `<textarea>`'s as its text and a `<select>`'s by the
option it marks `selected` (`PW5036`).
[ADR-0221](DECISIONS/ADR-0221-a-form-controls-value-is-written-where-html-reads-it.md)
is the decision. No invariant is retired, no `@expect-error` line of an
earlier fixture changed, and no fixture moved.

```text
Corpus version:            C15
Accepted programs:         31
Rejected programs:         57
Charter categories:        31/31 accepted, 57/57 rejected
Generality at open:        42/42 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-031 | added: a draft bound to a `<textarea>`, and a `<select>` whose chosen option is marked | new category, `a form control's value where HTML reads it` |
| R-057 | added: a `<select>` bound to a signal (`PW5036`) | new category |
| `form_control_value` | a GENERAL witness, a view's `<textarea>` given its value by a parameter, and a NEIGHBOUR one, a textarea's value as a signal and as its text, and a select's option marked `selected` | new invariant |

## C16: a value the template computes, 2026-10-05

Opened because the specification changed: a value a template computes
compiles (ruling 0073-a), and performs nothing (`PW0334`, revision 2).
[ADR-0226](DECISIONS/ADR-0226-a-value-the-template-computes-compiles.md)
is the decision. No invariant is retired and no `@expect-error` line of an
earlier fixture changed. Three earlier files changed, each text as it was
kept in `examples/history/C16/`, and `corpus_history.rs` holds what each old
text reports:

- `forbidden_effect`'s valid neighbour read the wall clock in its template,
  which a value computed there may not. It reads it in the page's body, as
  the page renders: the same effect, recomputed per reader.
- `task_detached`'s nested witness called `List.each`, and the cast rule's
  clean control `decode.run`. Neither module declares one: the qualified
  call check took an import's name for a local, and passed both. They call
  `List.map` and a decoder function now.

```text
Corpus version:            C16
Accepted programs:         32
Rejected programs:         58
Charter categories:        32/32 accepted, 58/58 rejected
Generality at open:        43/43 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-032 | added: a page computing a count in words, a `class` and a `disabled` from a query's value | new category, `a value the template computes` |
| R-058 | added: "3m ago" computed from the clock in a template (`PW0334`) | new category |
| `derived_not_pure` | a GENERAL witness, a view's attribute computed through a helper that reads the clock, and a NEIGHBOUR one, the clock read in the page's body and by a handler, and values computed by a helper that performs nothing | new invariant |
| `forbidden_effect/page-may-read-clock.pw` | the clock read in the page's body, not its template | witness corrected |
| `task_detached/nested.pw` | `List.map`, not `List.each`, which no module declares | witness corrected |
| `rules/annotations/decoded-not-cast.pw` | a decoder function, not `decode.run`, which `decode` does not declare | rule fixture corrected |

## C17: a template condition that tests a number, 2026-10-05

Opened because the specification changed: a template's condition is a
`Bool`, or a `List` or a `String` tested non-empty, where a number and a
record were the renderer's truth (ruling 0071-a; `PW0609`, revision 2).
[ADR-0230](DECISIONS/ADR-0230-a-condition-is-a-bool-or-tested-non-empty.md)
is the decision. No invariant is retired, no `@expect-error` line of an
earlier fixture changed, and no fixture moved: none tested a number or a
record.

```text
Corpus version:            C17
Accepted programs:         32
Rejected programs:         59
Charter categories:        32/32 accepted, 59/59 rejected
Generality at open:        44/44 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-059 | added: `{#if post.likes}` over an `Int` (`PW0609`) | new category |
| `operand_type` | a GENERAL witness, a view's boolean attribute given a record, and a NEIGHBOUR one, a count compared, a list and a string tested non-empty, and a `Bool` | new invariant |

## C18: a secret a query answers, and placement reading what a declaration holds, 2026-10-08

Opened because the specification changed: a secret a declaration answers
is held where one it only uses is not, a materialization reads what it
depends on, and placement reads what a declaration holds, as the cache
rules read it, in the checker and the contract alike.
[ADR-0282](DECISIONS/ADR-0282-what-a-value-holds-is-one-label.md) is the
decision. Three of the four findings of 2026-10-03 it fixes became exhibits.
No invariant is retired, no `@expect-error` line of an earlier fixture
changed, and no fixture moved.

```text
Corpus version:            C18
Accepted programs:         32
Rejected programs:         62
Charter categories:        32/32 accepted, 62/62 rejected
Generality at open:        44/44 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-060 | added: a `public query` answering `Secret<Payments>`, kept in a `partition public` fragment (`PW5101`) | new category |
| R-061 | added: a fragment of the session's cart placed at build (`PW5002`) | new category |
| R-062 | added: a page placed at build reading the session through a query of its own (`PW5002`) | new category |

## C19: a page states its canonical address, 2026-10-12

Opened because the specification changed. A page states its canonical
address with `<link rel="canonical" href={Page(args)} />` once, at the top of
its view: a page's address, checked as a navigation's is (`PW5051`). Anywhere
else a canonical link is still refused (`PW5035`).
[ADR-0311](DECISIONS/ADR-0311-a-page-states-its-canonical-address.md) is the
decision. No invariant is retired. One fixture changed:

- **R-053**: its canonical link, which was at the top of the page's view, is
  now inside `<main>`. At the top it would be the page's own address,
  refused only for its text (R-063's defect).
- **R-053's `@expect-error` lines**: they now say where the link is, and
  quote PW5035's invariant as it names the exception.

```text
Corpus version:            C19
Accepted programs:         33
Rejected programs:         63
Charter categories:        33/33 accepted, 63/63 rejected
Generality at open:        45/45 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-033 | added: a page's own canonical address, beside its title and its description | new category, `page states its canonical address` |
| R-053 | changed: its canonical link inside `<main>` (`PW5035`) | its category kept |
| R-063 | added: a canonical address written as text (`PW5051`) | new category |
| `canonical_address` | a GENERAL witness, an address that calls a view, and a NEIGHBOUR one, another page's address given by name in another order | new invariant |

## C20: a private query's record the user's, and `user`, 2026-10-09

Opened because the specification changed, twice, by the integrator's
rulings of 2026-10-09 (track `store-accounts`, ADR-0318):

- a record a `private` query produces is its user's (ADR-0270), as a
  record a `session query` produces is the session's (R-030), and a record
  several scoped queries produce holds each of their scopes, joined. Until
  then `private` gave the record no scope, so a view's parameter of it
  crossed into the public shell unrefused, and the scope a record carried
  was its last producer's in the units' order;
- `user` is a visibility, `session`'s peer: the user's, as `private` is, and
  importable, as `session` is and `private` is not. It is one only before a
  declaration's keyword, so a name `user` is still a name.

No invariant is retired, no `@expect-error` line of an earlier fixture
changed, and no fixture moved. The library gains `Saved`, a `user` query
and the record only it produces, which A-034 and R-065 import.

```text
Corpus version:            C20
Accepted programs:         34
Rejected programs:         65
Charter categories:        34/34 accepted, 65/65 rejected
Generality at open:        45/45 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-064 | added: a `private` query's record captured into the public shell from a view's parameter (`PW5007`) | new category |
| A-034 | added: a `user` query imported from the library and read, and captured, on a `user page` | new category |
| R-065 | added: the same `user` query's record captured into a public page's manifest (`PW5007`) | new category |

## C21: `private` says a declaration is not importable, 2026-10-12

Opened because the specification changed, by the integrator's ruling of
2026-10-12 (NEXT 3a, W8's finding; ADR-0324): `private` says a declaration
is not importable, and nothing more. A declaration that holds data says
whose with `user`, `session` or `public`, and `private` on one is refused
(`PW0029`). No invariant is retired. One fixture changed:

- **R-064**: its query says `user` where it said `private` for the user's.
  Its case, a record carried into the public shell by a view's parameter,
  is unchanged, and is R-065's at a view's parameter rather than a page's.
  Its `@category` names a `user` query; its `@expect-error` line is
  unchanged.

```text
Corpus version:            C21
Accepted programs:         34
Rejected programs:         65
Charter categories:        34/34 accepted, 65/65 rejected
Generality at open:        45/45 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-064 | changed: its query says `user` (`PW5007`) | its category kept |

## C22: a page loads from its own origin, 2026-10-14

Opened because the specification changed. The host sends each page a
Content-Security-Policy generated from its build and its deployment
(ADR-0195's ruling 13, the host's half), so an address a page's markup
fetches that names another origin built and was refused in the browser. An
address a page's markup fetches, written as text, is a path, and a
`<link>`'s address is written as text (`PW5052`). No invariant is retired.
Two fixtures changed:

- **A-028**: its `preconnect` to another origin is a `preload` of an image
  of its own; its item's property, a value no browser fetches, still names
  schema.org.
- **`link_not_in_body`'s NEIGHBOUR witness**: its `preconnect` names this
  origin.

```text
Corpus version:            C22
Accepted programs:         35
Rejected programs:         66
Charter categories:        35/35 accepted, 66/66 rejected
Generality at open:        46/46 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-028 | changed: its preload is this origin's (`PW5052`) | its category kept |
| A-035 | added: an image, its candidates and a stylesheet at paths, and a link to another origin | new category, `a page loads from its own origin` |
| R-066 | added: an image from another origin (`PW5052`) | new category |
| `load_from_another_origin` | a GENERAL witness, a `//` address a view's `<link>` preloads, and a NEIGHBOUR one, a navigation and an item's property to another origin, and a frame and an image built with a value at paths | new invariant |

## C23: a command runs when the clock passes its time, 2026-10-14

Opened because the specification changed. A command `invoked_by Clock` is
run by the host when the clock passes the time a command scheduled it for:
`schedule cmd(args) at when` (an `Instant`) or `after duration` (a
`Duration` from the scheduling command's commit), and `unschedule
cmd(args)` (the scheduled-commands track). A schedule names such a command,
written as a call (`PW6501`); it is a command's statement (`PW6502`); its
time is an instant or a duration (`PW6503`). No invariant is retired; no
fixture changed.

```text
Corpus version:            C23
Accepted programs:         36
Rejected programs:         69
Charter categories:        36/36 accepted, 69/69 rejected
Generality at open:        49/49 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-036 | added: a command schedules one the clock invokes, after thirty minutes, and another takes it back | new category, `a command schedules one the clock invokes` |
| R-067 | added: a schedule that names a command the clock does not invoke (`PW6501`) | new category |
| R-068 | added: a schedule written in a function (`PW6502`) | new category |
| R-069 | added: a run scheduled `at` a duration (`PW6503`) | new category |
| `schedule_names_no_clocked_command` | a GENERAL witness, an `unschedule` of a command a payment's outcome invokes, and a NEIGHBOUR one, a clocked command of another module | new invariant |
| `scheduled_outside_a_command` | a GENERAL witness, an `unschedule` in a query, and a NEIGHBOUR one, a schedule inside a command's `if` | new invariant |
| `schedule_time` | a GENERAL witness, `after` given an instant, and a NEIGHBOUR one, `at` an instant `clock.at` computes | new invariant |

## C24: a command the platform invokes, specified, 2026-10-14

Opened because the corpus did not specify a rule the language holds. NEXT
6c's `invoked_by` (ADR-0332, the payments track; ADR-0334 for the clock)
added five codes and no fixture: a command the platform invokes holds no
reader to a predicate (`PW6401`); a payment's command is given one
`payments.Settled` (`PW6402`); one command answers a payment's outcome
(`PW6403`); no page presses a command the platform invokes (`PW6404`); and
each is idempotent (`PW6405`). Exercising them found that `PW6402` and
`PW6405` named no boundary span, which charter §16.3 requires: each names
its `invoked_by` clause now. No invariant is retired; no fixture changed.

No accepted program is added: `PW6403` is the whole program's, and a
fixture's program is the accepted corpus with it, so an accepted command
`invoked_by Payments` would be every rejected fixture's second. The store's
`settle_payment` is the accepted case, checked with the store.

```text
Corpus version:            C24
Accepted programs:         36
Rejected programs:         74
Charter categories:        36/36 accepted, 74/74 rejected
Generality at open:        54/54 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-070 | added: a payment's command that `requires SignedIn` (`PW6401`) | new category |
| R-071 | added: a payment's command given an order's reference (`PW6402`) | new category |
| R-072 | added: two commands a payment's outcome invokes (`PW6403`) | new category |
| R-073 | added: a page's button that sends a payment's command (`PW6404`) | new category |
| R-074 | added: a payment's command with no `idempotent_by` (`PW6405`) | new category |
| `invoked_command_requires` | a GENERAL witness, the clock's command held to a predicate over its parameter, and a NEIGHBOUR one, the predicate held by the command that schedules it | new invariant |
| `invoked_command_parameters` | a GENERAL witness, the outcome and a second parameter, and a NEIGHBOUR one, the clock's command given two | new invariant |
| `invoked_twice` | a GENERAL witness, the second apart from the first, answering another type, and a NEIGHBOUR one, two of the clock's | new invariant |
| `invoked_command_pressed` | a GENERAL witness, the clock's command pressed before its time, and a NEIGHBOUR one, a page pressing the command that schedules it | new invariant |
| `invoked_command_not_idempotent` | a GENERAL witness, the clock's command keyed by nothing, and a NEIGHBOUR one, a command a reader presses keyed by nothing | new invariant |

## C25: a read keyed by its loop's element, 2026-10-15

Opened because the specification changed: a source read inside a loop over
data whose key comes from the loop's element is refused (`PW0405`), its set
read the repair, and the set form a keyed read's `set` names is held to the
pair's shape (`PW0407`): NEXT 4c's checker, track `bounded-checker`.
[ADR-0340](DECISIONS/ADR-0340-a-read-is-bounded-by-construction-the-checkers-half.md)
is the decision, and ADR-0337 the design. No invariant is retired, no
`@expect-error` line of an earlier fixture changed, and no fixture moved.
Three generality witnesses of `undeclared_effect` (`caught`,
`complete-row`, `via-callback`) read a store per element, incidental to
their subject, the effect row: each reads by a parameter now, its shape and
its verdict kept. `PW0406`, a warning, has no fixture: the rejected corpus
holds errors, and `tests/bounded_reads.rs` holds it.

```text
Corpus version:            C25
Accepted programs:         37
Rejected programs:         78
Charter categories:        37/37 accepted, 78/78 rejected
Generality at open:        56/56 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-075 | added: a list's prices read one at a time in `List.map`'s callback (`PW0405`) | new category |
| R-076 | added: a total summed in a `for` that reads each item's price, its key bound by a `let` (`PW0405`) | new category |
| R-077 | added: a function that recurses over its list and reads a price at each step (`PW0405`) | new category |
| R-078 | added: a set form that answers the cents alone, with no key (`PW0407`) | new category |
| A-037 | added: a list's prices read by one set read before the loop, in each of R-075's, R-076's and R-077's forms, and a read in a loop keyed by no element | new category, `a list read together before its loop` |
| `read_per_element` | a GENERAL witness, a generic callback with a match arm's key, a function named as the callback, and another module's read, pair and source, and a NEIGHBOUR one, a key no element's and a callback applied once | new invariant |
| `set_pair_shape` | a GENERAL witness, a set form that drops a reader the keyed read passes along, and another module's set form of another source, and a NEIGHBOUR one, the shapes a set form may take | new invariant |
| `undeclared_effect` | changed: its three witnesses read by a parameter, not per element | its invariant kept |

## C26: what a loop's head reads, 2026-10-15

Opened because the corpus did not specify a rule the language holds, and
the checker did not hold it there. A view's effect row is empty (charter
§7.4), and an `{#each}`'s list and key are what the view evaluates while
it renders, as a text hole, an attribute or an `{#if}`'s condition is.
They were text no walker of a view's expressions read (ADR-0242's "not
claimed"), so a database read in a loop's list checked clean, where the
same call anywhere else in the view was `PW0401`; written as a member of
the store it built too (ADR-0345, the each-head-effects track). No
invariant is retired; no fixture changed.

```text
Corpus version:            C26
Accepted programs:         38
Rejected programs:         79
Charter categories:        38/38 accepted, 79/79 rejected
Generality at open:        56/56 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-038 | added: a page's loop over the value of a query it names, the query reading the database | new category, `a loop over a query's value` |
| R-080 | added: a view's loop over `Stores.list()`, which reads the database (`PW0401` at the list) | new category |
| `forbidden_effect` | two GENERAL witnesses, a loop's list read through a member that reads the database and a nested loop's key that does, and a NEIGHBOUR one, the list read through a pure member | new witnesses |
| `secret_to_browser` | a GENERAL witness, a loop over a list computed from secrets, each rendered | new witness |

## C27: an app's data, declared as tables, 2026-10-15

Opened because the specification changed (track `tables`, ADR-0338's D1;
C25 is track `bounded-checker`'s). A program declares its data: `table T`
in `module T`, its `row` a record declared beside it, its `key`, `order`,
`index` and `unique` the row's fields, its `refers` to other tables, its
`bounded`, and its `migrate` from the schema released. Its reads and writes
are the functions the declaration gives the module, and nothing else
reaches its rows. Five codes: a malformed table (`PW0361`), a table's rows
reached otherwise than through its functions (`PW0362`), `all()` of a table
with no bound (`PW0364`), a warning for any reference across sources (`PW0365`), and a change from the schema released that loses
or reinterprets data with no `migrate` (`PW0366`). A fixture of `PW0366`
names the schema it is held to as released, `// @schema-lock: <file>`,
beside it. No published invariant is retired. The unpublished D1 fixtures were corrected before integration: PW0365 is a warning, not a refusal; A-052 names it with @expect-warning, and the harness compares exactly those warning codes.

```text
Corpus version:            C27
Accepted programs:         42
Rejected programs:         87
Charter categories:        42/42 accepted, 87/87 rejected
Generality at open:        60/60 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-050 | added: a table keyed by two fields, indexed and bounded, read by its key, a prefix, an index and whole, and written, only through its functions | new category |
| A-051 | added: a table that refers to itself, in one source | new category |
| A-052 | added: a reference to a table another source holds, accepted with its explicit PW0365 warning | new category |
| A-053 | added: a field renamed, one dropped and one backfilled from the schema released, each stated | new category |
| R-100 | added: a table keyed by a `List` (`PW0361`) | new category |
| R-101 | added: an index on a field its row does not have (`PW0361`) | new category |
| R-102 | added: `all()` of a table with no `bounded` (`PW0364`) | new category |
| R-103 | added: a read by an index the table does not declare (`PW0362`) | new category |
| R-104 | added: a table's rows read through a `host` binding of the app's own (`PW0362`) | new category |
| R-105 | added: a reference whose field holds no key of its target (`PW0361`) | new category |
| R-106 | added: a field dropped from the schema released, unstated (`PW0366`) | new category |
| R-107 | added: a field renamed from the schema released, unstated (`PW0366`) | new category |
| `table_malformed` | a GENERAL witness, a reference whose field holds no key of the table it names, and a NEIGHBOUR one, the same reference by the key's type | new invariant |
| `data_not_declared` | a GENERAL witness, a write by a binding of the app's own, and a NEIGHBOUR one, a function with a body over a read the table gives | new invariant |
| `all_unbounded` | a GENERAL witness, `all()` called bare in the table's own module, and a NEIGHBOUR one, the same call of a bounded table | new invariant |
| `refers_across_sources` | a GENERAL witness, the referring table in no source, and a NEIGHBOUR one, a self-reference within one source | new invariant |
| `migration_unstated` | a GENERAL witness, a field's type changed, and a NEIGHBOUR one, the same change `backfilled with` a value | new invariant |

## C28: a value the reader's device keeps, 2026-10-20

Opened because the specification changed (track `device-store`, ADR-0366:
charter §9.1's fourth kind of state). A module's signal with a value and
`storage device "name"`, `privacy Device` and `conflict last_write_wins` is
kept in the reader's browser, read before what it decides is shown, and
never sent: a read of one carries `Device` (charter §7.8), which follows it
as every label does, and the browser's ways to the server refuse it: a
command's argument or a call it decides (`PW6801`), a page query's key
(`PW6802`), and an address, a navigation or a form's control (`PW6803`).
The declaration's own rules (`PW6804` to `PW6808`) are held in
`tests/device_store.rs`. No invariant is retired; no fixture changed.

```text
Corpus version:            C28
Accepted programs:         43
Rejected programs:         89
Charter categories:        43/43 accepted, 89/89 rejected
Generality at open:        62/62 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| A-054 | added: a value the device keeps, read by a page and written by its handler, shown and never sent | new category |
| R-108 | added: a value the device keeps given to a command as its argument (`PW6801`) | new category |
| R-109 | added: a page's query keyed by its own signal, assigned a value the device keeps (`PW6802`) | new category |
| `device_value_to_a_command` | a GENERAL witness, a kept list summarized by a helper, put in a record and its field given to a command from a key handler's match arm, and a NEIGHBOUR one, the command given the page's own value with the device's shown | new invariant |
| `device_value_keys_a_query` | a GENERAL witness, a query keyed beside the session by a page's signal a key handler assigns, trimmed, from the device's value, and a NEIGHBOUR one, keyed by what is typed with the device's value shown | new invariant |

## C29: generic results retain member types, effects and labels, 2026-10-16

Opened under ADR-0351 after C28, alongside the device-store integration. The canonical value solver instantiates each
generic call; effects and privacy read members through that solved receiver
type. Rigid type parameters declare no members (PW0610), and an untyped
receiver is refused (PW0408). Existing D1 and bounded-read categories remain.

```text
Corpus version:            C29
Accepted programs:         44
Rejected programs:         95
Charter categories:        44/44 accepted, 95/95 rejected
Generality expected:       65/65 invariants generality-tested
Integration validation:    pending combined gates
```

| Fixture | Change | Kind |
|---|---|---|
| A-039 | generic call results retain pure member types | new category |
| R-090, R-091 | generic-result view/function member effects remain visible | two new categories |
| R-092 | a member receiver needs a known type, PW0408 | new category |
| R-093 | a rigid parameter declares no member, PW0610 | new category |
| R-094 | a function-valued receiver retains the function's effects | new category |
| R-095 | a recursive generic invocation substitutes its own argument types | new category |

R-028 now states progress: OrderProgress to preserve its existing scope
refusal without introducing an unrelated unknown receiver; C0 remains frozen.
The worker's complete before/after program comparison and negative histories
are linked from ADR-0351. This entry does not certify backend specialization.

R-091 now takes one element from the reversed result through List.get/Option
matching. Its original loop also has the now-detected per-element read defect;
the revised fixture retains only PW0400. The complete old program and actual
new diagnostic remain in train10-generic-call-composition-before.txt. Separate
joint tests preserve that property's PW0405 and its explicit-call neighbours.

## C30: a key that changes is asked where it changes, 2026-11-06

Opened because a fixture was wrong about the language. R-027 was a query
alone that declares a `concurrency` and no `on_key_change`, refused by
PW0325. Nothing in it changes the key, so no read of it goes stale. A key
changes while its read runs only where a page's signal keys the query
(ADR-0152), and PW5309 asks for the policy there. The stale-key ADR
(`ADR-XXXX-a-key-that-changes-is-asked-where-it-changes.md`, numbered when it
lands) removes PW0325's rule and makes PW0325 PW5309's old number. No
category changes.

```text
Corpus version:            C30
Accepted programs:         44
Rejected programs:         95
Charter categories:        44/44 accepted, 95/95 rejected
Generality at open:        65/65 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-027 | a page whose search box's signal keys the query, which still declares `concurrency one_per_key` and no `on_key_change`; `@rule` PW0325 → PW5309, `@invariant` `stale_key_policy` → `signal_key_stale_work`, both `@expect-error` lines restated | fixture wrong about the language: it changed no key |
| `generality/stale_key_policy/` | removed; `generality/signal_key_stale_work/` holds the invariant's GENERAL witness, a page whose `Int` signal keys a query with no `concurrency`, and its NEIGHBOUR | invariant folded into PW5309 |

The history question has a different answer for R-027, and the suite says
so rather than dropping it:

- its C8 text is classified `FixtureDidNotExpressIt` in `corpus_history.rs`,
  and reports only the two unresolved types C8's repair imported;
- its text before this version, and the removed GENERAL witness, are in
  `examples/history/C30/`. Both check clean, which is the change, and
  `the_c30_texts_change_no_key_and_check_clean` holds it.

## C31: an image says what it shows, 2026-11-06

Opened because a charter category was added: an image with no text
alternative, under §8.2's "missing labels" and §17.4's "labels". PW5053
refuses an `<img>` with no `alt`. The image ADR
(`ADR-XXXX-an-image-says-what-it-shows.md`, numbered when it lands) is the
decision. No fixture changes, and no invariant is retired.

```text
Corpus version:            C31
Accepted programs:         44
Rejected programs:         96
Charter categories:        44/44 accepted, 96/96 rejected
Generality at open:        66/66 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-096 | a page whose photo has no `alt`, refused with PW5053 | new category |

## C32: a command says who may run it, 2026-11-06

Opened because the specification changed, and a charter category was added
under §7.5's "explicit mutation with authorization". A command a reader can
run declares `requires` (PW0367); `requires Anyone` says anyone may. The
requires ADR (`ADR-XXXX-a-command-says-who-may-run-it.md`, numbered when it
lands) is the decision.

```text
Corpus version:            C32
Accepted programs:         44
Rejected programs:         97
Charter categories:        44/44 accepted, 97/97 rejected
Generality at open:        67/67 invariants generality-tested
```

| Fixture | Change | Kind |
|---|---|---|
| R-097 | a command no page sends, `delete_post`, with no `requires`, refused with PW0367 | new category |
| A-036, A-050, R-067, R-069, R-108 | each command with neither `requires` nor `invoked_by` gains `requires Anyone`: anyone could run it, and it now says so | specification changed, meaning kept |
| nine generality witnesses (`device_value_to_a_command`, `invoked_command_not_idempotent`, `invoked_command_pressed`, `invoked_twice`, `schedule_names_no_clocked_command`, `schedule_time`, `scheduled_outside_a_command`) | the same | specification changed, meaning kept |

The texts under `examples/history/` keep what they were. A command in one
that says nothing of who may run it is now refused for that too, beside the
invariant its history test asks about.
