# Decisions

Index over `docs/DECISIONS/ADR-*.md`. One ADR per consequential decision, written
**before** the change it authorizes (PROJECT_CHARTER.md §3.1 step 3).

Status values: `Proposed` · `Accepted` · `Superseded by ADR-NNNN` · `Rejected`

| ADR | Decision | Status | Strategy | Deletion / revisit condition |
|---|---|---|---|---|
| [0001](DECISIONS/ADR-0001-koka-as-temporary-semantic-compiler.md) | Koka 3.2.3 is a temporary semantic oracle, not the permanent compiler | Accepted | tape | Milestone 9 checker reaches corpus parity |
| [0002](DECISIONS/ADR-0002-marko-as-temporary-renderer.md) | Marko 6.3.32 is the temporary renderer behind an adapter | Accepted | tape | Milestone 7 renderer passes the golden suite |
| [0003](DECISIONS/ADR-0003-rust-as-permanent-implementation-language.md) | Rust 1.97.1 is the permanent compiler/host language | Accepted | build | Dependency MSRV exceeds the pin |
| [0004](DECISIONS/ADR-0004-preserve-html-css-http.md) | Preserve standard HTML, CSS, URLs and HTTP | Accepted | reuse | Never wholesale; narrow additions at M13 |
| [0005](DECISIONS/ADR-0005-sqlite-first-data-layer.md) | SQLite first, PostgreSQL when nodes separate | Accepted | reuse | Milestone 11 |
| [0006](DECISIONS/ADR-0006-wasmtime-capability-host.md) | Wasmtime 47.0.3 capability host; effects become WIT imports | Accepted | reuse | A capability cannot be expressed in WIT |
| [0007](DECISIONS/ADR-0007-explicit-invalidation-before-inference.md) | Explicit typed invalidation events before any inference | Accepted | build | After Milestone 6, as an auditable optimization only |
| [0008](DECISIONS/ADR-0008-wasi-0.2-not-0.3.md) | Target WASI 0.2; 0.3 not reachable through the stable toolchain | Accepted | reuse | Rust ships stable `wasm32-wasip3` |
| [0009](DECISIONS/ADR-0009-parser-and-diagnostics-stack.md) | Hand-written parser + `annotate-snippets` 0.12.16 diagnostics | Accepted | build | Milestone 2 lossless-tree design |
| [0010](DECISIONS/ADR-0010-provisional-pw-extension.md) | `.pw` is the provisional source extension | **Proposed** | build | Milestone 2 task 1 owns the real decision |
| [0011](DECISIONS/ADR-0011-koka-is-an-effects-only-oracle.md) | Koka is an effects-only oracle; `pw` owns value semantics | Accepted | tape | E9 checker reaches parity |
| [0012](DECISIONS/ADR-0012-adopt-rowan-before-body-parsing.md) | Adopt `rowan` 0.17.0 before body parsing; analyses consume HIR, not syntax nodes | Accepted | reuse | rowan cannot express a needed property |
| [0013](DECISIONS/ADR-0013-formatter-design.md) | Canonical formatting rules; implementation deferred until ADR-0012 lands | Accepted (design) | build | canonical examples drafted |
| [0014](DECISIONS/ADR-0014-hir-representation.md) | HIR is id-indexed arenas per body, with a span on every node; only `lower.rs` sees syntax | Accepted | build | name resolution needs to interleave with lowering |
| [0015](DECISIONS/ADR-0015-koka-backend-scope.md) | The Koka backend lowers a pure subset only, and states what that does not prove. Amended 2026-10-11: Koka checks a module as a library (`koka -l`); as a program it needed a `main` no module has, so none could agree, and a positive control now shows one can. | Accepted | tape | the `pw` checker reaches corpus parity (ADR-0001) |
| [0016](DECISIONS/ADR-0016-e2a-r-runtime-shape.md) | E2A-R is a thread-scoped runtime on `std::thread::scope`; its results are behaviour, not guarantees | Accepted | build | E8 selects the host execution model |
| [0017](DECISIONS/ADR-0017-marko-adapter-boundary.md) | The Marko adapter is a one-way lowering from HIR; generated files are build output, never authored | Accepted | tape | E7-R passes the golden suite (ADR-0002) |
| [0018](DECISIONS/ADR-0018-manifest-is-the-compiler-runtime-boundary.md) | The resource manifest is a data artifact; neither compiler nor runtime depends on the other | Accepted | build | E8 selects the host execution model |
| [0019](DECISIONS/ADR-0019-materializer-store-and-outbox.md) | The materializer's state and its outbox share one SQLite database, so a command writes both in one transaction | Accepted | build | E8 selects the host execution model |
| [0020](DECISIONS/ADR-0020-component-contract-is-the-compiler-host-boundary.md) | The compiler hands the host a six-field `ComponentContract`, one per declaration, and actual Wasm imports must be a SUBSET of what it allows | Accepted | build | E9 lowers capabilities and effects into it |
| [0021](DECISIONS/ADR-0021-the-language-is-called-pleris.md) | The language is **Pleris**; Perfect Web stays the project, `.pw` the source format, `pw-*` the internal crates | Accepted | prose only | a public launch needs a formal trademark/organization clearance pass |
| [0022](DECISIONS/ADR-0022-causal-analysis-validation.md) | **Coincidental correctness** has a name and a countermeasure: important conclusions must explain which resolved facts caused them, and fixtures grow from three checks to five | Accepted | E8 slice 2 is the first consumer | a provenance DAG that RECOMPUTES rather than RECORDS would be a second answer to the same question |
| [0026](DECISIONS/ADR-0026-a-capability-authorizes-an-operation-it-does-not-identify-one.md) | A host dependency is a **callable import** — `ImportId` + ABI + a SET of capabilities + who defined it — and a capability is never the callable identity. A host implementation is explicit declaration metadata, never inferred from a `todo` body or an effect row. The audit is three separate layers, and operation authority ⊆ component authority. Amended 2026-10-11: the artifact audit builds the guests it reads, which no runner had. | Accepted | E10 | the Wasm encoder could not encode `add_to_cart`: `Carts.add(s, item, qty)` and `Carts.clear(s)` share `database.write<Carts>` and a core import has one signature, so a capability-keyed import had no honest arity |
| [0025](DECISIONS/ADR-0025-an-optimistic-clause-targets-a-resource-entry.md) | An optimistic clause names the resource **entry** it updates, binds its current value, and applies a **pure** transition. There is no written `rollback`: the platform restores the value it held | Accepted | E10 | two entries can share a type, so a transition naming only the type does not say what it changed; and a hand-written inverse is generally false — `Apple × 3` optimistically `+2` is not restored by `remove(apple)` |
| [0024](DECISIONS/ADR-0024-optimistic-and-rollback-bind-with-a-lambda.md) | *(rejected)* `optimistic` / `rollback` bind their subject with a bare lambda | Rejected, superseded by 0025 | E10 | the lambda said what transformation to perform and not which entry it applied to; and it kept a written inverse, which describes an operation the runtime will not use |
| [0023](DECISIONS/ADR-0023-milestone-gates-prove-one-layer.md) | A milestone gate proves **one layer's** contract; a cross-layer end-to-end proof belongs at the milestone where both sides exist, as a recorded deferred obligation | Accepted | build | a SECOND gate item deferred for the same reason — one mistake is a patch, two is a structural inversion in the milestone order |

| [0028](DECISIONS/ADR-0028-recursive-written-types-before-signature-cutover.md) | Preserve recursive written types and complete return annotations before the resolved-signature cutover; resolve only valid type identities | Accepted | reopened E9 | no nested type reparsing or partial resolution; ordinary call/return compatibility remains a separate gate |

## Decisions the charter asked for and where they landed

Charter §14 Milestone 0 task 5 lists eight required ADRs. All eight exist:

| charter item | ADR |
|---|---|
| Koka as temporary semantic compiler | 0001 |
| Marko as temporary renderer | 0002 |
| Rust as permanent compiler/host language | 0003 |
| standard HTML/CSS/HTTP preservation | 0004 |
| SQLite-first data layer | 0005 |
| Wasmtime capability host | 0006 |
| no browser fork before profiling | see below |
| explicit invalidation before automatic dependency tracking | 0007 |

ADRs 0008–0010 were added because Milestone 0's spikes forced decisions the
charter left open (the WASI version) or that the corpus needed immediately
(the diagnostics stack, the file extension).

**"No browser fork before profiling"** is deliberately *not* an ADR. It is a
standing constraint in the charter itself (§11.4, §14 M13) and there is no
decision to record until Milestone 13 produces profiling data. Recording an ADR
that says "we did not do the thing we were told not to do" would add
ceremony without content. It is tracked in `docs/vision/non-goals.md`.

## Decision log

Short entries for choices that shaped the repository but are too small for an ADR.

**2026-08-05 — `docs/`-scoped STATUS/DECISIONS/ASSUMPTIONS.**
The charter mandates `docs/STATUS.md` and `docs/DECISIONS/ADR-*.md`. The three
requested filenames live under `docs/` rather than at the root so there is one
source of truth. Recorded as assumption A-006.

**2026-08-05 — The original prompt file became `PROJECT_CHARTER.md` and was removed.**
Byte-identical copy verified by SHA-256 (`d37dfa2d…a90e4c`) before deleting the
duplicate, so there is exactly one constitution in the repository.

**2026-08-05 — `wasm32-wasip2` spike crates are excluded from the Cargo workspace.**
They target wasm and must not be pulled into a host-target
`cargo test --workspace`. `spikes/wasmtime-component/run.sh` builds them.

**2026-08-05 — Node 22.21.1 rather than Node 24.**
Both `vite@8.2.0` (`^20.19.0 || >=22.12.0`) and `marko@6.3.32` (`>=22`) are
satisfied. Pinning what was actually tested, per charter §3.6. Assumption A-002.

**2026-08-05 — one diagnostic code per invariant, not per detector.**
`PW2004` ("a resource cannot outlive the scope that owns it") is emitted by both
the declaration rule and the scope graph, distinguished by a `reason` and
`detector` field. `PW0326` is a deprecated alias resolving to it. Two permanent
codes for one invariant would be wrong; aliasing during migration is fine.

**2026-08-05 — semantic rules may not live in the CLI.**
`pw-syntax` owns syntax, `pw-core` owns meaning and emits one `Diagnostic` type,
`pw-cli` renders. This is what lets a language server, test harness, build
system, playground, AI loop and PR analyser share one checker.

**2026-08-05 — E7 is subdivided.**
E7-R (resumption/DOM), E7-P (patch semantics), E7-L (lazy loading). Marko is the
accepted oracle for E7-R only; it **fails** E7-L. The undifferentiated sentence
"Marko is the E7 oracle" is forbidden.

**2026-08-05 — four small syntax decisions forced by the body grammar.**
None is large enough for an ADR; all four are load-bearing for the tree shape.
*Compound comparisons* (`==` `!=` `<=` `>=`) lex as one token, while `<` and
`>` stay separate because they also delimit type arguments — the joined forms
are unambiguous since a type argument list is never followed directly by `=`.
*An infix operator that can also begin an expression* (`<`, `-`, `!`) may not
begin a line; without the rule, `let s = f(id)` followed by `<main>` parses as
one comparison. *Dotted paths are not absorbed* in expression position: the
parser cannot distinguish `Stores.get` from `store.name` and must not pretend
to, so every `.ident` is a field access and name resolution folds the segments
that turn out to be a module path. *Triple-quoted strings* carry wrapped
`because "..."` justifications, leaving the single-quoted form ending at the
newline, which is the error-recovery property worth keeping.

**2026-08-05 — silent parser recovery is a defect, not a convenience.**
Three closers were consumed with a bare `eat` whose `false` was discarded.
Turning them into diagnostics moved coverage from 56/68 corpus files to 68/68,
because the silence was hiding four real defects — each of which reported its
error one line *after* the cause. Recovery must continue parsing; it must not
continue quietly.

**2026-08-05 — `just ci` runs `clippy -D warnings`.**
Dead code is treated as a signal, not noise: the first `-D warnings` failure
surfaced genuinely unused model surface, which was resolved by *using* it
(`--explain`, a warning-level rule) rather than by silencing the lint.

## 2026-09-16: resolved signature authority

[ADR-0030](DECISIONS/ADR-0030-resolved-signature-authority.md): replace written
signature types atomically with resolved identities, share semantic keys and
stable projections, and keep missing/blocked/known slots distinct. Boundary
privacy and WIT decisions consume the same signature. Does not close ordinary
call/return typing or the component execution gate.

## 2026-09-24: the value relations

[ADR-0031](DECISIONS/ADR-0031-value-relations.md): one module decides every
place a value meets a declared type. That covers arity, arguments, fields,
results (including `?`), annotated bindings and unresolved written types, with
diagnostics projected from a queryable, three-valued analysis. Adds:
- callable type parameters instantiated per call;
- `type` parameters kept;
- function types;
- `?` in the HIR;
- declared-constructor arity.

Decides without a ruling, for reversal:
- privacy qualifiers are not value-transparent;
- function types exist;
- snapshots are read explicitly.

Closes E9-V1..V6. Does not close E10-I.

## 2026-09-24: compiled components (E10-I)

[ADR-0032](DECISIONS/ADR-0032-compiled-components.md): Canonical ABI adapters
from the world, with every number from `wit-parser`, one encoder, and upstream
`wit-component` wrapping. Component identity is checked twice: during encoding
and by decoding the artifact. The host runs a component with the deployment's
operations, compiled once. The contract locates each export (ruling needed). The
dev server's command closures are deleted. Closes E10-I.

## 2026-09-25: compiled resumable handlers (E10)

[ADR-0033](DECISIONS/ADR-0033-compiled-handlers.md): a resumable handler's body
compiles to an ES module (`pw emit-handlers`). Each fact the backend needs is
read from the stage that owns it: identity, name, callee, component id and
capture paths. The component id now has one derivation instead of three. An
element carries exactly the capture paths its handler reads (ruling needed).
The browser's arguments are typed by the command component's own parameters.
The dev server has one command path, which fixes `clear_cart`'s uncommitted
state. A string literal has a value only where no escape rule is involved
(ruling needed). Supersedes ADR-0032 §8.

## 2026-09-25: `pw build` (E10 gate item 1)

[ADR-0034](DECISIONS/ADR-0034-pw-build.md): one command builds every artifact
of a checked program: the template IR, the handler modules, every component
(audited), and the contracts and WIT. It composes the stages that own each
artifact, and is all or nothing. Each contract gets one outcome: component, no
body, placeholder, or refused. A `todo` placeholder fails the build only when
something depends on it (ruling needed). The gate's evidence runs the build
with neither Koka nor Node on the PATH.

## 2026-09-25: bounded subscribers (E10 gate item 3)

[ADR-0035](DECISIONS/ADR-0035-bounded-subscribers.md): a consumed outbox event
is deleted. A subscriber holds at most 256 frames, and past that gets one
`Recovery::Reload`. One that has not asked for 120 s is forgotten, with its
cached cart fragment, and its next poll is told to reload. A served document's
cursor is never zero, and the runtime reloads on `Reload`. Measured before:
3,000 retained events, 600,000 frames, 1,001 entries.

## 2026-09-25: matches, fields and variants in the backend (E10)

[ADR-0036](DECISIONS/ADR-0036-matches-fields-variants.md): `match` over
`Option` and `Result` is a structured IR instruction whose arms are regions.
Lowering is bidirectional, so `None` takes the type its use fixes. The encoder
types a constructed variant from its uses and checks every move with
`same_type`, and every layout number comes from `wit-parser`. The store's
components are byte-identical afterwards.

## 2026-09-25: the kiokun slice (E10)

[ADR-0037](DECISIONS/ADR-0037-kiokun-slice.md): a second application, entry
lookup and search over one real shard of kiokun.com, compiled by `pw build`.
Its data is kiokun's own files, copied with attribution (owner decision needed
on share-alike data). The data layer is the deployment's, as kiokun's search is
its database's. Building it found the platform package depending on the store
example, and interpolated attribute strings rendering literally; both fixed.

## 2026-09-25: every match is analysed (E10)

[ADR-0038](DECISIONS/ADR-0038-every-match-typed.md): the exhaustiveness
analysis reads every match. Scrutinees are typed by the value relations,
`Option` and `Result` are sum types to it, and each pattern is read against its
own type. A constructor its type lacks is the new PW0608. `return` is a
statement in an arm too, and an arm with no `=>` is an error. Five gaps, each
of which reported a match as exhaustive when it was not; the kiokun slice
found the first. Two decisions need a ruling: a bare name is a constructor
whenever some type has one of that name, and no `return` expression is built.

## 2026-09-25: pure computation in the component backend (E10)

[ADR-0039](DECISIONS/ADR-0039-pure-computation.md): arithmetic, comparisons,
`&`, `|`, `!`, `if`, string literals, interpolation, records, and calls to
other declarations (inlined). `Int` traps where its exact result does not fit,
and `/` and `%` are Euclidean, as Koka's are; a zero divisor traps where Koka
answers 0 (ruling needed). Types the world never names are defined in a
private copy of its `Resolve`. Building it found a query body the parser
dropped, a backend that compiled programs that did not parse, a Koka backend
that emitted invalid negation, and an import missing when called only in an
arm; all fixed.

## 2026-09-25: the standard library's lists and strings, compiled (E10)

[ADR-0040](DECISIONS/ADR-0040-standard-library.md): `List` and a new `String`
module are declarations the compiler supplies, each marked `intrinsic`, read
as `host` is. `map`, `filter`, `fold`, `any`, `all`, `find` and a stable
`sort_by` compile their function argument where they run it; `length`, `get`,
`take`, `concat`, and the string operations are routines in the component.
`to_lower_ascii` maps `A`–`Z` only (ruling needed). Building it found that
`(a, b) => e` had never parsed.

[ADR-0041](DECISIONS/ADR-0041-kiokun-in-pleris.md): kiokun's shard rule and
its search ranking are Pleris, compiled to `shards.Place`, `shards.Places` and
`kiokun.page.Search`; the host keeps the index and the files. The Rust rule and
ranking stay as the references every compiled answer is compared with, on
every word kiokun has. Building it found that the E9 value relations left a
generic call's `let` binding uninstantiated, that a statement keyword could
name a value, and that kiokun's file names are not its words.

[ADR-0042](DECISIONS/ADR-0042-template-branches-and-attributes.md): a
template block's markers are kept and checked (PW5019), `{:else}` and
`{:else if}` are branches, `{#match}` takes an `Option` or a `Result` apart,
and an attribute interpolates, each value escaped for its context (a URI
component in a URL). It fixes a silent miscompile: `{:else}` was dropped, and
both branches rendered together.

[ADR-0043](DECISIONS/ADR-0043-operands-are-typed.md): an operator's operands,
and an `if`'s condition, are related to the types they take (PW0609), so
`1 == "a"` no longer checks. `Float.from_int` converts a count. Building it
found two accepted fixtures dividing a `Float` by an `Int`, which ADR-0039 §2
refuses.

[ADR-0044](DECISIONS/ADR-0044-pure-computation-in-javascript.md): a query
that reaches no host also compiles to an ES module, from the same backend IR
as its component. Every place JavaScript's semantics differ from Pleris's
(BigInt, Euclidean division, code point order, White_Space) is encoded
explicitly. The two backends agree on 6,800 generated calls under Node,
kiokun's shard rule among them.

[ADR-0045](DECISIONS/ADR-0045-affine-exactly-once.md): PW2005's invariant,
"exactly once, in the scope that acquired it", is checked on every path:
the end of a scope and a failing `?` are exits, a second release is
counted, and a release in a loop is refused. A declaration whose row
promises to release a parameter must. No syntax is added.

[ADR-0046](DECISIONS/ADR-0046-memory-strategy-measured.md): E10 task 4.
Every compiled kiokun query was measured per call on the whole shard:
instructions, peak linear memory, and instantiation beside the call.
Invocation regions stay the strategy; the other six the charter names are
judged against the numbers. Per-call instantiation, not memory, dominates.

[ADR-0047](DECISIONS/ADR-0047-every-name-resolves.md): every name resolves,
in lexical scope, not only a call's. A `let` binds after itself, a pattern
for its own body, and a template's blocks for their children. Clauses
written as statements are read by position from the policy table. The walk
found `derived` parsed as a bare name, so A-016 and A-018 computed nothing,
and a statement's named argument read as an assignment.

[ADR-0048](DECISIONS/ADR-0048-members-exist.md): a read or a call through a
value names a member its type has (PW0610): a field, or a declaration whose
first parameter takes the type. An opaque type's `.value` is its
representation in its own module only. It found eight reads of members no
type has, among them the store page's `{cart.line_count}`.

[ADR-0049](DECISIONS/ADR-0049-string-escapes.md): a string's escapes are the
language's, settling A-023. One decoder in `pw_syntax::strings` reads every
string token; an undefined escape is PW0014; `"""` strings are raw; each
backend encodes the value in its own syntax.

[ADR-0050](DECISIONS/ADR-0050-recursion-and-generic-callees.md): recursion
and generic callees compile. A call is inlined until it recurses; the
recursion is compiled beside the export, once per instance, and called, in
the component and the JavaScript module. A generic callee is instantiated
from its arguments. Existing artifacts are byte-identical.

[ADR-0051](DECISIONS/ADR-0051-early-return-and-loops.md): an early `return`,
`?`, and `for` loops with `let mut` bindings compile, in the component and
the module. A callee that returns early is compiled beside its export. The
checker refuses an assignment to a binding that is not `let mut` (PW0611),
and relates the assigned value to the binding's type (PW0607).

[ADR-0052](DECISIONS/ADR-0052-function-values.md): a function is a value. A
lambda or a declaration's name compiles to a closure of its captures, is
stored, returned, passed and called through: in the component an
environment in the region naming its code's slot in a `funcref` table,
called with `call_indirect`; in the module a JavaScript function.

[ADR-0053](DECISIONS/ADR-0053-callback-parameters.md): a lambda's parameters
take the types its use declares. `infer.rs` reads them from the callee's
declared function type, where it gave the first parameter the element type
of any list beside it; `values.rs` keeps the types a solved call, an
annotation or a declared result gives them. `fold`'s accumulator was typed
as the element and refused. Amended 2026-10-11: a parameter known only in
part (`List<?>`) is checked by that part. Since ADR-0063 the guard against
keeping it only left the parameter unchecked, and two mutants survived it
unseen for two weeks.

[ADR-0054](DECISIONS/ADR-0054-opaque-values.md): an opaque value is built and
read inside a component, as its representation retyped (`Instr::Retype`),
in the component and the module. An opaque type's representation is a type
tree, where it was a spelling that never resolved with type arguments.

[ADR-0055](DECISIONS/ADR-0055-slices-and-placeholders.md): `List.drop`,
`List.slice`, `List.reverse` and `String.slice`, each bound clamped, a
slice a view. `sum` and `maximum` are Pleris in `list.pw`, where their
placeholders answered 0.0; `maximum` is an `Option`. `enumerate`, which
answered `[]`, is removed: the language has no tuple type.

[ADR-0056](DECISIONS/ADR-0056-unicode-case-mapping.md): `String.to_lower`
and `String.to_upper` map each code point as Unicode 17.0 does, from
tables generated from the compiler's own `char` mapping. The component
holds them in its data segment and the module as constants, so neither
reads a platform's mapping. A final sigma lowers to `σ`.

[ADR-0057](DECISIONS/ADR-0057-maps-and-sets.md): `Map<K, V>` and `Set<T>`,
keyed by an `Int` or a `String`, in ascending key order: in the component
`list<tuple<K, V>>` and `list<T>`, searched in binary and changed by
copying; in the module sorted arrays. A map or set from outside is checked
on arrival. `List.sort_by`'s merge sort takes a comparison now.

[ADR-0058](DECISIONS/ADR-0058-handlers-that-compute.md): a resumable
handler's body is lowered as a query's is and written by the
pure-computation emitter, its commands awaited in order: bindings,
arithmetic, branches, loops and several commands. A captured `Int` is read
into a `BigInt`, and one sent past ±2^53 traps before it is sent.

## 2026-09-26: declared sum types, typed, built and matched (E10)

[ADR-0059](DECISIONS/ADR-0059-declared-sum-types.md): a sum type's case is
typed where it is written through its type, `Shape.Circle(3)`: its fields,
its arity, and a case the type lacks (PW0608). A pattern may name its type
too, and an arm's fields are typed in its body. The backend builds and
matches declared cases, in the component as WIT variants and in the module
as `{ $case, value }`, with `_`, name and `A | B` arms. Writing it found
four ways a wrong program passed `pw check`: a case's fields were
unchecked, an undeclared case passed, `Shape.Empty` in a pattern bound a
name and matched everything, and an arm's bindings were unknown in its
body.

## 2026-09-26: nested and literal patterns (E10)

[ADR-0060](DECISIONS/ADR-0060-nested-and-literal-patterns.md): the
exhaustiveness analysis types each match's subject, an ADT per instance, so
a pattern nested under `Some` or a declared case is read against its own
type, and `true`, `false` and each literal are constructors. The backend
compiles any match that is not one level deep to a decision tree of nested
matches and tests. It fixes a silent miscompile: `Some(Empty)` bound every
payload to a name `Empty`.

## 2026-09-26: a declared sum type in a template's match (E10)

[ADR-0061](DECISIONS/ADR-0061-template-matches-over-sum-types.md): a
template's `{#match}` takes a declared sum type apart, checked as any match
is. An arm binds each field of its case, `{:Rect(w, h)}`, and may name its
type. The template IR names a declared case by its WIT name, as the value a
component gives does, and a case of several fields is a list the renderer
binds field by field.

## 2026-09-26: generic types in the backend (E10)

[ADR-0062](DECISIONS/ADR-0062-generic-types-in-the-backend.md): a nominal
type carries its arguments, so a generic record, sum type or opaque type is
laid out per instance inside a component and a module. An instance's
arguments come from its fields, as a generic callee's do, or from its use; a
parameter nothing fixes is refused by name. A generic type still does not
cross the boundary. Writing it found a type and a query of one name failing
the whole WIT package.

## 2026-09-26: every name means one binding (E10)

[ADR-0063](DECISIONS/ADR-0063-every-name-means-one-binding.md): each local
name is resolved once, to the binding in scope where it is written, by the
scopes the backend lowers with. The value relations, the declared-type
environment and the privacy labels keep their facts by binding, where each
kept them by name. It fixes type errors that passed through any name bound
twice, a handler capture typed by another binding of its name, and secrets
logged or rendered through a `for` loop's, a lambda's or an `{#each}` block's
name.

## 2026-09-26: a label carried through a call (E10)

[ADR-0064](DECISIONS/ADR-0064-a-label-through-a-call.md): a declared call's
result joins its declaration's label with the label of each argument whose
type mentions a type parameter the result mentions, so a secret passed
through `List.get`, `List.map`, `List.fold` or a program's own generic
function stays secret. A lambda is labelled by what it computes, and a call
no declaration answers by all that goes into it, its receiver included.

## 2026-09-26: a value that holds at every type (E10)

[ADR-0065](DECISIONS/ADR-0065-a-value-of-any-type.md): `None`, `[]`, `Ok` and
`Err`, `todo`, an early return, and a generic value whose parameter no field
mentions hold at every type their hole could be, and the value relations
decide them. A variable such a value meets is not fixed by it, a function's
link between its parameter and its result is kept, and a `let mut` holds one
type, completed by an assignment.

## 2026-09-26: a nested declaration sees the bindings around it (E10)

[ADR-0066](DECISIONS/ADR-0066-a-nested-declaration-sees-around-it.md): a
declaration nested in another sees the enclosing parameters and the bindings
in scope where it is written, typed and labelled as the enclosing declaration
has them, and is in its module. A stream's parts and a release clause's name
are resolved; a call's callee is the binding in scope or a declaration;
`(x) =>` binds. It fixes a secret logged publicly through a nested function,
and a nested declaration's annotations, which never resolved.

## 2026-09-26: a record is built with each of its fields, once (E10)

[ADR-0067](DECISIONS/ADR-0067-a-record-is-built-with-its-fields.md): a record
built by its fields' names is related to the fields its type declares, each
once and no other (PW0612), where each field was related alone and a field
left out, one its type lacks, or one given twice passed. An `if` without
`else` is the unit value, so a body declaring another result cannot end in
one.

## 2026-09-26: what each construct takes, checked (E10)

[ADR-0068](DECISIONS/ADR-0068-what-each-construct-takes.md): a call through a
function value is checked against its type (arity, arguments, result), and a
value called that is not a function is PW0614; `for`'s list and `?`'s operand
are related; the branches of an `if` or a `match` whose value is used produce
one type (PW0613). It fixes a silent miscompile: every `elif` and `else if`
chain lowered to its first branch and, for its `else`, the next condition.

## 2026-09-26: a list's items share one type, and an Int literal fits an Int (E10)

[ADR-0069](DECISIONS/ADR-0069-list-items-and-int-literals.md): a list's items
are related to each other (PW0615), where two of different types made the
element a hole and `[1, "a"]` passed as a `List<Int>`; an `Int` literal is
related to the 64-bit range (PW0616), which the backend was the first to
enforce.

## 2026-09-26: an assignment to a field has the field's type (E10)

[ADR-0070](DECISIONS/ADR-0070-an-assignment-to-a-field.md): `b.value = e`
relates `e` to the field's type (PW0607), where an assignment to a field
related nothing.

## 2026-09-26: what a template's blocks and events take (E10)

[ADR-0071](DECISIONS/ADR-0071-what-a-template-takes.md): an `{#each}`'s
collection is a list, an `{#if}` or `{:else if}` condition is not a declared
sum type (PW0609), and an event attribute is given a function (PW0614). Each
of the three passed `pw check`, and the first two failed only when rendered.

## 2026-09-26: an element named with a capital letter is a view (E10)

[ADR-0072](DECISIONS/ADR-0072-an-element-named-with-a-capital-is-a-view.md):
`<Money value={p} />`, a view used in another view as the charter writes it
(§8.1), built as an unknown HTML element, its markup never rendered and its
props checked by nothing. It is refused (PW5020) until views compose, and so
is a tag that names no view. How a view composes needs a ruling.

## 2026-09-26: a template reads each value by path (E10)

[ADR-0073](DECISIONS/ADR-0073-a-template-reads-each-value-by-path.md): a
computed hole checks and does not build, where it built with an empty path
and failed every render; `pw build` refuses a part the renderer refuses; a
`style:` directive no longer builds as an attribute a browser ignores; and a
loop's key is read from its element (PW5021) along its whole path, where
`(k.r.id)` keyed on `k.id`.

## 2026-09-26: what a template writes has a text form (E10)

[ADR-0074](DECISIONS/ADR-0074-what-a-template-writes-has-a-text-form.md): a
value a template writes as text has a text form (PW0609); one that may be
absent is taken apart (PW0600), in `{:else if}` too, which ADR-0071 left to
nothing; a boolean attribute has a truth; and a loop's list and key are read
through fields their values have (PW0610). Each passed `pw check` and failed
when rendered.

## 2026-09-26: a stream and a mounted resource do not build (E10)

[ADR-0075](DECISIONS/ADR-0075-a-stream-and-a-mounted-resource-do-not-build.md):
the template IR refuses a `<stream>` and an element that mounts a resource
by name, where each lowered as a literal element: a stream was refused for
its `query` attribute, and A-007's mounted resource built a page that fails
when rendered.

## 2026-09-26: an arm no value reaches is refused (E10)

[ADR-0076](DECISIONS/ADR-0076-an-arm-no-value-reaches.md): the
exhaustiveness analysis always found unreachable arms, and nothing reported
one. `_ => 0` before `Circle(r) => r` checked, and a literal matched twice
built with a dead arm. PW0333 refuses each.

## 2026-09-26: a call through a field holding a function is checked (E10)

[ADR-0077](DECISIONS/ADR-0077-a-call-through-a-field.md): `r.f(x)`, where
`f` is a field holding a function, resolved to nothing, so its arguments,
its arity and its result passed unchecked. ADR-0068's relations check it
now.

## 2026-09-26: an effect is performed where its function is named (E10)

[ADR-0078](DECISIONS/ADR-0078-an-effect-is-performed-where-its-function-is-named.md):
a view declared `!{}` read the clock through `List.map(xs, stamp)`, a
local, a helper's parameter or a record field, since only a call counted;
R-037's invariant held only for a lambda. A declaration named as a value
performs its effects where it is named, and a helper declaring no row
carries its members' and its values' effects too.

## 2026-09-26: a function value carries the label of what it makes (E10)

[ADR-0079](DECISIONS/ADR-0079-a-function-value-carries-its-label.md): a
`Secret<Payments>` logged publicly passed through `let f =
secrets.payments` then `f()`, or a record field holding it: a name meaning
a declaration was public, and a call through a value left out its callee.
Both carry the label of what the function makes now.

## 2026-09-26: the affine rule follows bindings, not names (E10)

[ADR-0080](DECISIONS/ADR-0080-the-affine-rule-follows-bindings.md): PW2005
found a transaction's uses by its name, so an outer `tx` never ended passed
where each branch ended an inner `tx`, correct programs were refused, and
an end through `let end = Database.rollback` counted nothing. It follows the
binding a name means now; a local bound to a declaration is that
declaration, and a value given to any other function value is refused.

## 2026-09-26: a named argument is given to the parameter of its name (E10)

[ADR-0081](DECISIONS/ADR-0081-a-named-argument-is-its-parameters.md): the
backend passed arguments in written order, so `g(b = 1, a = 10)` computed
`g(1, 10)`, a silent miscompile; the checker related no argument of a call
that named one; and a generic result's label was carried from the argument
in the wrong place. Signatures carry parameter names now, and one
arrangement serves the checker, the labels and the backend (PW0617). Amended 2026-10-11: a test names a secret key out of its place, the only call that tells one parameter from another since ADR-0129.

## 2026-09-26: a `derived` value performs no effect (E10)

[ADR-0082](DECISIONS/ADR-0082-a-derived-value-performs-no-effect.md): the
charter's `derived` is a pure value computed from other values, and nothing
held it to that: `let t = derived clock.now()` checked. PW0334 refuses an
effect inside one, whether called, read through a member or named.

## 2026-09-26: a call's arguments open on the callee's line (E10)

[ADR-0083](DECISIONS/ADR-0083-a-call-opens-on-its-callees-line.md): a `(`
at the start of a line continued the expression before it, so `g(n)` then
`()` parsed as `g(n)()` and a correct program was refused for "`` does not
resolve". It begins a new statement now, as `<`, `-` and `!` do.

## 2026-09-26: what a string interpolates has a text form (E10)

[ADR-0084](DECISIONS/ADR-0084-what-a-string-interpolates-has-a-text-form.md):
`"{xs}"` over a list, a record or an `Option` checked, and the backend was
the first to refuse it. A string's holes are related to a text form now, as
a template's are (ADR-0074); three fixtures interpolated a value with none
and are corrected.

## 2026-09-26: a call carries what it is given (E10)

[ADR-0085](DECISIONS/ADR-0085-a-call-carries-what-it-is-given.md): a
secret logged publicly passed once it went through `String.trim`, or any
declared function over plain values: ADR-0064 carried a label only where the
result mentions a type parameter. A declared call carries every argument
given to a parameter that states no label now, and a parameter declared
`Secret<C>` keeps its contract. Corrects ADR-0064 §1.

## 2026-09-26: a function crosses no boundary (E10)

[ADR-0086](DECISIONS/ADR-0086-a-function-crosses-no-boundary.md): a
resumable handler that captured a function checked, and the build refused it
for a name that "names no declaration". A value that is or holds a function
crosses no boundary now: PW5008 for a capture, untransferable for a call to
another placement.

## 2026-09-26: a call names a term (E10)

[ADR-0087](DECISIONS/ADR-0087-a-call-names-a-term.md): a call to a view, a
page, an event or an effect checked, answered for as a call to nothing, and
`fn f() -> Int { Badge(1) }` returned a view where an `Int` is declared. A
call names a function, a data operation, or a type it builds (PW0027).

## 2026-09-26: a clause names a declaration of its kind, and gives it its key (E10)

[ADR-0088](DECISIONS/ADR-0088-a-clause-names-a-declaration-and-gives-it-its-key.md):
`depends_on`, `invalidates` and `emits` were text. The graph drew an edge to
whatever a name found, and nothing resolved, counted or typed a key, so
`invalidates Cart(item)` and `emits Cart(..)` checked. A clause names a
declaration of its kind (PW5103), and its key's arguments are terms related
to what it names. The store's own `emits` gave its event the wrong type.

## 2026-09-26: a policy's value is one its domain has (E10)

[ADR-0089](DECISIONS/ADR-0089-a-policy-value-is-one-its-domain-has.md):
nothing held a policy's value to the table that says what it is. `cache
Shared` checked on R-004's page and escaped the rule that keeps a session's
data out of a shared cache; `placement originn`, `retry nope(..)` and `key
nope` checked; and three readers were bypassed by a prefix or a substring.
A value is one its domain has now (PW0335), and each reader reads it exactly.

## 2026-09-26: `pw build` checks what `pw check` checks (E10)

[ADR-0090](DECISIONS/ADR-0090-the-build-checks-what-pw-check-checks.md):
the declaration rules ran only in the `pw check` command, so `pw build`
compiled R-015's `retry forever` query into a component and built R-014.
One checker runs them now, and their diagnostics meet the checker's
standard: the registry's invariant, a boundary span, an explanation.

## 2026-09-26: a listener binds its entry's key (E10)

[ADR-0091](DECISIONS/ADR-0091-a-listener-binds-its-entrys-key.md): the
materializer read an event's values as a set, so `InventoryChanged(47, item
3)` never reached store 47's menu, and nothing checked what a listener
wrote. Each argument of `invalidates_on` is a parameter the entry's key
binds, or `_` (PW5104), related to the event's values, and the materializer
compares them position by position.

## 2026-09-26: a dependency-graph clause belongs to a declaration that can mean it (E10)

[ADR-0092](DECISIONS/ADR-0092-a-graph-clause-belongs-to-a-declaration-that-can-mean-it.md):
a query that `emits`, a command that listens, a `fn` that emits and a page
that invalidates each checked, and nothing read what they said. A command
emits and invalidates, a resource or a materialization listens, and a
materialization depends (PW5105).

## 2026-09-26: an element handles an event the platform declares (E10)

[ADR-0093](DECISIONS/ADR-0093-an-element-handles-an-event-the-platform-declares.md):
`on:clik={go}` checked, skipped by the rule that reads the platform's
`events`, and ran by accident, since the runtime listens for a click
whatever the name. An `on:` attribute names a declared event now (PW5022).

## 2026-09-26: a template writes no code (E10)

[ADR-0094](DECISIONS/ADR-0094-a-template-writes-no-code.md):
`<script>{msg}</script>`, `onclick={msg}`, `srcdoc={msg}` and
`<style>{msg}</style>` checked and built, escaped as text or an attribute,
which makes none of them inert: `msg` ran; so did `href="javascript:go({id})"`.
A template writes no script, no inline handler, no script URL, no `srcdoc`
and no value into a stylesheet (PW5023).

## 2026-09-26: an attribute's context is read as HTML reads its name (E10)

[ADR-0095](DECISIONS/ADR-0095-an-attributes-context-is-read-as-html-reads-its-name.md):
the template IR chose a value's escaping from the attribute's name as
written, so `<a HREF={msg}>` was escaped as an ordinary attribute and
`msg = "javascript:alert(1)"` ran. The name is read as HTML reads it now,
lowercased.

## 2026-09-26: a template moves no URL (E10)

[ADR-0096](DECISIONS/ADR-0096-a-template-moves-no-url.md): `<base
href={msg}>` chose where the platform's runtime loaded from, since the page
writes the view before the runtime's script, and an SVG `<animate>` set a
link's `href` to any URL. A template writes no `<base>` and animates no link
or handler now (PW5024).

## 2026-09-26: data embedded in a page cannot end its script element (E10)

[ADR-0097](DECISIONS/ADR-0097-embedded-data-cannot-end-its-script.md): the
parts manifest's JSON broke only a lowercase `</script`, and `</SCRIPT>` or
`<!--<script>` in it would end or swallow the element. It holds no `<` now:
`escape::json_in_script` writes `\u003c`.

## 2026-09-26: a name is written once where it is declared (E10)

[ADR-0098](DECISIONS/ADR-0098-a-name-is-written-once-where-it-is-declared.md):
a parameter taken twice, a field or a case declared twice, a policy written
twice and an attribute given twice each checked, and one of the two was
dropped: `cache private` then `cache shared` decided by its order whether
a session's data was refused a shared cache. Each is written once now
(PW0028), an effect's `impact` excepted.

## 2026-09-26: a failure is handled (E10)

[ADR-0099](DECISIONS/ADR-0099-a-failure-is-handled.md): a statement's
`Result` was dropped without a word, and nine corpus fixtures dropped one, a
rollback's or a clear's, one of them `@expect: clean`. A `Result` whose value
nothing uses is refused (PW0618): `?` returns it, `match` handles it, and a
binding that says so discards it by name.

## 2026-09-26: a query reads (E10)

[ADR-0100](DECISIONS/ADR-0100-a-query-reads.md): a query whose body cleared
a cart checked, and the platform caches, deduplicates and retries a query as
a read. A query or a subscription that writes or opens a transaction is
refused (PW0401); a mutation is a command's.

## 2026-09-26: a command invalidates what it writes (E10)

[ADR-0101](DECISIONS/ADR-0101-a-command-invalidates-what-it-writes.md): a
command that wrote the cart and declared neither `invalidates` nor `emits`
checked, and nothing told the queries reading the cart that it had changed.
A-005 told the library's stub and not A-004's cart. A command reaches each
reader of what it writes that has no staleness window: by name, or by an event
the reader listens for (PW5106).

## 2026-09-26: an event reaches what reads what it invalidates (E10)

[ADR-0102](DECISIONS/ADR-0102-an-event-reaches-what-reads-what-it-invalidates.md):
the materializer invalidated an event's direct listeners and nothing built
from them, although the compiler's graph follows reads. A-009's fragment
depends on a store that listens for `StoreChanged`, and kept the old store
after one. An event now reaches each entry that reads what it reaches, at
the key the read supplies, and a key the read does not supply matches any
value.

## 2026-09-26: a write reaches the fragments built on it (E10)

[ADR-0103](DECISIONS/ADR-0103-a-write-reaches-the-fragments-built-on-it.md):
a fragment is rebuilt only when an event reaches it, and one built from a
query with a staleness window kept a renamed store for good, since the
command emitted nothing and PW5106 left the query to expire. A command now
emits an event that reaches each fragment built on what it writes (PW5106);
`invalidates` does not reach a fragment.

## 2026-09-26: the dev server commits the events a command declares (E10)

[ADR-0104](DECISIONS/ADR-0104-the-dev-server-commits-what-a-command-declares.md):
the dev server committed `CartChanged` after any cart write, whatever the
command declared, which hid ADR-0101's finding at run time. It now reads the
command's `emits` edges from the compiler's graph, commits those events, and
refuses a key it cannot compute before the command runs.

## 2026-09-26: a command invalidates the entry it speculates on (E10)

[ADR-0105](DECISIONS/ADR-0105-a-command-invalidates-the-entry-it-speculates-on.md):
a command speculating on `Cart` with neither `invalidates` nor an event
`Cart` hears checked, and nothing replaced the speculation with what the
command committed (ADR-0025's reconciliation). A command now reaches each
entry it speculates on (PW5107).

## 2026-09-26: `pw check` reports each file by its place (E10)

[ADR-0106](DECISIONS/ADR-0106-each-file-is-reported-by-its-place.md):
`pw check` kept each file's diagnostics under its file name, so of two files
named `app.pw` the second's replaced the first's, and `pw check a/app.pw
b/app.pw` passed with an error in `a/app.pw`. Every `just ci` check named two
files `effects.pw`. Diagnostics are paired with their files by position now,
and a file whose name is shared is named by its path.

## 2026-09-26: a cache key names each parameter its entry depends on (E10)

[ADR-0107](DECISIONS/ADR-0107-a-cache-key-names-what-its-entry-depends-on.md):
`query Other(id, other)` with `key id` and a body reading `other` checked, and
two calls differing in `other` shared one cache entry. A `key` or `dedupe_by`
names each parameter the body reads now (PW0336).

## 2026-09-26: a query names a resource that exists (E10)

[ADR-0108](DECISIONS/ADR-0108-a-query-names-a-resource-that-exists.md): the
name check bound a keyword statement's first word, so `let menu = query
Nonexistent(id)` in a page checked, and the graph kept the read as a dangling
edge no rule reports for a page. The resource a `query` or `subscription`
statement names is resolved now (PW0021).

## 2026-09-26: a timeout is a budget above zero (E10)

[ADR-0109](DECISIONS/ADR-0109-a-timeout-is-a-budget-above-zero.md):
`timeout 0.seconds` checked, being a duration, and the resource runtime ends a
flight once its budget is spent, so the query could never answer. A
timeout's domain is a duration above zero now (PW0335); a freshness of zero
is still a promise.

## 2026-09-26: a resumable handler reads what it captures (E10)

[ADR-0110](DECISIONS/ADR-0110-a-resumable-handler-reads-what-it-captures.md):
`resumable() => add_to_cart(item.id, ..)` inside an `{#each}` checked, and
`pw emit-handlers` refused it, "`item` is not bound here": `pw check` passed
a program its build refuses. A handler reads what it captures and what it
binds itself now (PW5025).

## 2026-09-26: a shorthand field reads its capture (E10)

[ADR-0111](DECISIONS/ADR-0111-a-shorthand-field-reads-its-capture.md): a
handler capturing `item` and building `Pick { n: 1, item }` was refused by
PW5017, a program with nothing wrong in it. The capture paths, the handler
artifact and the backend each read a capture through a name or a field path
only. A shorthand field reads its capture whole in all three now.

## 2026-09-26: a call's privacy is the declaration it resolves to (E10)

[ADR-0112](DECISIONS/ADR-0112-a-calls-privacy-is-the-declaration-it-resolves-to.md):
the privacy rules read a callee by its fully qualified spelling, so a bare
imported name carried no label: R-003's secret in markup and R-006's secret in
a public log passed `pw check` written with `import secrets.{ payments }` and
`import log.{ public }`. A callee is resolved as the unit sees it now, and a
value PW5001 refuses a shared cache is not reported again by PW5004.

## 2026-09-26: a resumable handler runs in the browser (E10)

[ADR-0113](DECISIONS/ADR-0113-a-resumable-handler-runs-in-the-browser.md): a
handler calling `Carts.add` itself passed `pw check`, and `pw emit-handlers`
refused it. The page's contract leaves its handlers out, and a handler has
none, so nothing asked where its effects run. What a handler performs itself,
beside the commands it calls, is the browser's to grant now (PW5005). R-010
and a witness flushed a connection from the browser, a second defect, and
are corrected.

## 2026-09-26: what is built before any request reads no request's value (E10)

[ADR-0114](DECISIONS/ADR-0114-what-is-built-before-any-request-reads-no-requests-value.md):
a `placement build` page rendering its `id` passed `pw check`, and the file
it is built into exists before any request supplies one. A build-placed
declaration reads none of its parameters now (PW5026). Whether a build may
enumerate a parameter's values needs a ruling.

## 2026-10-01: a command's `requires` is held before its body runs (E10)

[ADR-0115](DECISIONS/ADR-0115-a-commands-requires-is-held-before-its-body-runs.md):
`requires SignedIn` was parsed and then ignored: no contract or host read it,
so the store command ran for any session. Authorization is now an invocation
precondition on the compiled export. The deployment evaluates every predicate
over the typed command arguments before execution; unknown, denied, malformed
or unevaluated requirements fail closed. Static capabilities remain separate
from per-request authorization.

## 2026-10-01: Wasmtime 48.0.3 replaces the vulnerable 47.x pin (E8/E10)

[ADR-0116](DECISIONS/ADR-0116-wasmtime-48-0-3-replaces-the-vulnerable-47-x-pin.md):
the supply-chain gate found RUSTSEC-2026-0315 and RUSTSEC-2026-0316 in Wasmtime
47.0.4. The host, conformance engine, standalone spike and bootstrapped CLI now
pin 48.0.3. The advisories are not suppressed, and historical measurements keep
their original engine labels.
 
## 2026-10-01: patch brace expansion below Marko's glob layer

[ADR-0117](DECISIONS/ADR-0117-patched-brace-expansion-stays-below-markos-glob-layer.md):
the Node advisory gate found three denial-of-service advisories in
`brace-expansion 5.0.9` through Marko's glob dependency. The 5.x transitive
dependency is pinned to 5.0.12 and the lockfile was regenerated; no advisory
exception was added.

## 2026-10-02: what a declaration reads, it reads through what it calls (E10)

[ADR-0118](DECISIONS/ADR-0118-what-a-declaration-reads-it-reads-through-what-it-calls.md):
the privacy rules read a declaration one call deep, so a public query reading
the session through a helper or another query passed PW5101, PW5004 and
PW5001, and its page's contract allowed `build`. Labels are joined through
what each declaration reads, to a fixed point, and the checker and the
contract share that derivation. A command's reads stay its own.

## 2026-10-02: E10 closes, and the AI benchmark comes next (E10, E14)

[ADR-0119](DECISIONS/ADR-0119-e10-closes-and-the-ai-benchmark-comes-next.md):
E10's five gate items were re-recorded at `bff437c` and pass. Re-recording
found world-level exports refused by ADR-0115's authorization lookup (fixed),
and E7 gate 8 unstable on this machine at HEAD and at the recorded tree alike
(open, E7-G8). Tasks that are not gate items are carried to E15. E14 runs
before E11-E13, harness and tasks first, agents after the owner chooses
models and budget.

## 2026-10-02: three stores, one contract (E14)

[ADR-0120](DECISIONS/ADR-0120-three-stores-one-contract.md): the canonical
store in Next.js 16.3.8 and SvelteKit 2.70.3 (not the day-old 3.0.0) beside
the Pleris store, held to one behavioural contract with mutant stores as
negative controls. Found that the Pleris store's `optimistic` and
`idempotent_by` are checked and not executed; the contract excludes both. Amended 2026-10-11: svelte-check is asked for its machine output, which it wrote by default only in an agent's session (`CLAUDECODE=1`), so every runner failed the recipe. Amended 2026-10-22: Next.js 16.4.1, for three advisories in 16.3.8 (two unauthenticated remote code executions, one denial of service) that no 16.3.x fixes.

## 2026-10-02: an idempotent command runs once per interaction (E14)

[ADR-0121](DECISIONS/ADR-0121-an-idempotent-command-runs-once-per-interaction.md):
`idempotent_by InteractionId` was checked and read by nothing, so a retried
request added twice. The contract carries it, the runtime sends one
interaction per press, and the host runs an idempotent command once per
interaction through `pw-resource`'s reservation, keeping a bounded number per
session. A request without an interaction, or one reusing an interaction with
other arguments, is refused.

## 2026-10-02: an optimistic transition runs in the browser (E14)

[ADR-0122](DECISIONS/ADR-0122-an-optimistic-transition-runs-in-the-browser.md):
`optimistic` was checked and executed by nothing, its store transition was a
stub, and the backend could not compile the page's count. The compiler emits a
speculation module per page, the browser holds the speculated entry's value
from `entry_value` frames, and a speculation is reconciled by version or
restored exactly. An unpriced optimistic line and the new frame are offered
for reversal.

## 2026-10-02: the development server runs what `pw build` built (E14)

[ADR-0123](DECISIONS/ADR-0123-the-development-server-runs-what-pw-build-built.md):
the server read its commands and contracts from `docs/evidence/` and compiled
the graph in, so a rebuilt program kept running the repository's commands.
`pw build` writes the graph and speculations too, and the server loads one
build directory. Query values remain the server's (E14-Q).

## 2026-10-02: the benchmark harness, and the controls a score needs (E14)

[ADR-0124](DECISIONS/ADR-0124-the-benchmark-harness-and-its-controls.md):
a sandbox per run, isolation checked, grading by checker, build, the shared
contract and hidden tests against the sandbox's own server, and four controls
per task and stack, each requiring tests that ran. T08 holds all four on
three stacks; Pleris's unsafe patch is refused by `pw check`.

## 2026-10-02: scoped signals (proposed)

[ADR-0126](DECISIONS/ADR-0126-scoped-signals.md), **proposed; accepted with changes by ADR-0130**:
UI state as `signal`s declared in a scope, read by name and resolved by the
compiler, written only by handlers, updating only what reads them, dropped
with their scope, typed and resumable. Four rulings are needed before it is
built.

## 2026-10-02: a page shows what its queries return (E14)

[ADR-0125](DECISIONS/ADR-0125-a-page-shows-what-its-queries-return.md): the
development server computed the store page's values itself and ran none of
its queries. The compiler now plans each page (its bindings' queries, each
part's field and member reads), compiles each member function a plan names
as a component, and the server follows the plan. Query policies are the next
slice.

## 2026-10-02: queries run by their declared policies (E14)

[ADR-0127](DECISIONS/ADR-0127-queries-run-by-their-declared-policies.md): the
development server ran every query on every render. Each binding's policy now
travels in the page plan and `pw-resource` applies it: freshness, cache
partition, key (every argument when no `key` is written), retries, timeout,
one flight per key; a commit drops exactly the entries it invalidates. Found
on the way: a page served after a menu change showed the old menu, and two
presses at once rolled both back.

## 2026-10-02: a shared cache holds no one reader's value (E14)

[ADR-0128](DECISIONS/ADR-0128-a-shared-cache-holds-no-one-readers-value.md):
charter §7.8's first must-fail example, `SharedCache<Cart@Session>`, passed
when the cart query was declared `public` and given the session as a
parameter (T12's wrong fix), and when any reader's value was keyed by its
reader. A declaration now observes what it is given, every cache rule reads
that whole label, and no key makes a session's, user's or device's value
shareable; a tenant's, keyed by its organization, is the one a shared cache
may hold. Closes generality's last known gap (31 / 31).

## 2026-10-02: a value's label follows it (E14-L)

[ADR-0129](DECISIONS/ADR-0129-a-values-label-follows-it.md): the sinks read a
value's label, and it was lost in seven ways: through a parameter that states
a label (ADR-0085's contract), through a helper's body, through a branch's
condition, at a sink a branch decides, into an assigned binding, at a public
log taking a session's value, and at PW5003 reading one restriction. An
argument now comes out of a call less a secret its parameter states (a key
the call uses), a call carries what its callee's body makes, a branch carries
its condition to its value and to every sink it decides, an assignment
labels its binding, and a public log takes only a public value. Settles
ADR-0085's open ruling: inference over bodies, keys only for stated secrets,
and declassification only at a host binding until a program needs more.

## 2026-10-02: UI state is a signal; a handler's event is plain data (rulings)

[ADR-0130](DECISIONS/ADR-0130-ui-state-is-a-signal-provided-where-it-lives.md)
accepts ADR-0126 with its four rulings: the keyword `signal`; views composed
at compile time and instantiated at run time (ADR-0072's question, Marko 6's
model); a signal ephemeral or in the URL; and a handler may not assign a
body's `let mut`. It changes two rules: a shared signal is a declaration a
scope `provide`s and views read by its own name, and an inner `provide`
shadows an outer one explicitly.
[ADR-0131](DECISIONS/ADR-0131-a-handler-is-given-its-event-as-plain-data.md)
settles ADR-0058's two rulings: a handler takes its event as a closed record
of plain data read synchronously by the runtime, synchronous behaviour is a
static modifier (`on:submit|prevent`), `bind:value` is shorthand with a codec,
a form submits one typed record, and a handler may read its command's answer.
Neither is built yet.

## 2026-10-02: the resume decision knows the build; a page holds its own UI state

[ADR-0132](DECISIONS/ADR-0132-the-resume-decision-knows-what-the-build-compiled.md):
the browser's resume decision knew the store page's two handlers by name, so
every other handler built and was refused in the browser. It now knows the
build's handlers by identity, from the build the runtime was served with.
[ADR-0133](DECISIONS/ADR-0133-a-page-holds-its-own-ui-state.md): `signal x: T =
v` in a page, changed by handlers that need call no command, read in text and
blocks, held by the browser and rendered again there by the server's renderer
built for the browser. Three rules (PW5300-PW5302) keep it written by
handlers and read where the browser reads it again. ADR-0130's first step.

## 2026-10-02: every handler is resumable

[ADR-0134](DECISIONS/ADR-0134-every-handler-is-resumable.md): a handler not
written `resumable(captures = { .. })` built into a button with no code
behind it, silently. Every `on:` lambda is a handler now, and one derivation
says what it captures: what it lists, or else what it reads that the body
around it binds, never a signal or its own binding. `() =>` takes no
parameter. A handler that is not a lambda is refused at build until the
event is passed (ADR-0131).

## 2026-10-02: a handler's identity is its own file's

[ADR-0135](DECISIONS/ADR-0135-a-handlers-identity-is-its-own-files.md): the
build's map from a handler's place to its identity was keyed by indices
counted within one file, so two pages of one shape in two files shared a key
and one page's button ran the other's handler. Keyed by the file as well.

## 2026-10-02: a view is written where it is used

[ADR-0136](DECISIONS/ADR-0136-a-view-is-written-where-it-is-used.md): a view
used in another was refused (ADR-0072), so no page could be built from views.
A view now composes. Its markup is lowered in place, in the page's one
numbering, each parameter read as the path its prop gives, and a name it
binds renamed where it would hide one. A view's handler keeps its own module,
and the template says where the page holds what it captures. Props are
checked as arguments (PW0619). What a view's handler captures is checked
where the page gives it, and a signal it would capture is refused.
ADR-0130's second step.

## 2026-10-02: what the browser renders again, it can (a correction)

[ADR-0137](DECISIONS/ADR-0137-what-the-browser-renders-again-it-can.md):
ADR-0133's limits said a signal read inside a query's `{#each}`, and a
query's `{#each}` inside a block a signal decides, were refused by the plan.
Each built: the first would have shown its first value forever, and the
browser could not render the second again. The plan now refuses every part a
signal decides that the browser does not render again, and every part of a
block it renders that reads what it does not hold: three more of the same
kind, which ADR-0133 did not list, with them.

## 2026-10-02: a handler is given its event

[ADR-0138](DECISIONS/ADR-0138-a-handler-is-given-its-event.md): ADR-0131's
first slice. A handler takes nothing, or its event's record, `(e:
InputEvent) => ..` or `(e) => ..`, read in the browser's listener before any
code loads. `on:submit|prevent` stops the form's submission there. Seven
defects found building it, each checked or built clean:
- every handler listened for a click;
- a lambda's written parameter type was dropped, in any lambda;
- `(e: T) =>` did not parse, and a handler with a parameter was refused;
- a modifier parsed as two more attributes;
- two handlers on one element wrote two capture attributes, and the second
  was never bound.

## 2026-10-02: a frame is forgotten when the page says it applied it (a correction)

[ADR-0139](DECISIONS/ADR-0139-a-frame-is-forgotten-when-the-page-says-it-applied-it.md):
the stream adapter dropped each frame 25 ms after writing it, though its own
comment said it waited for the page's next request. A stream held for a page
that had been reloaded wrote the new page's frames into a socket nobody read
and dropped them, so a change made within two seconds of a reload never
arrived. Found as the keyed-list suite's intermittent failure. A frame is
dropped now when a page says it applied it.

## 2026-10-02: a page that reads queries holds signals too

[ADR-0140](DECISIONS/ADR-0140-a-page-that-reads-queries-holds-signals-too.md):
the store's route rendered its page from its queries and ignored its signals,
so a store page with one did not render, and T11's dialog could not be
written. It renders each signal's first value now, and its document carries
the signals' manifest, as a page of signals alone does.

## 2026-10-02: a dialog a signal shows is the browser's modal dialog

[ADR-0141](DECISIONS/ADR-0141-a-dialog-a-signal-shows-is-the-browsers-modal-dialog.md):
a `<dialog>` without `open`, in a block a signal decides, is shown with
`showModal()` and closed with `close()`. The browser then focuses it, makes
the page behind it inert and closes it on Escape. The dialog handles
`close` (PW5303), so the signal that shows it hears of every closing,
Escape's included. A dialog nothing shows is refused. Focus goes back to
what invoked it in every engine, WebKit included.

## 2026-10-02: an input bound to a signal

[ADR-0142](DECISIONS/ADR-0142-an-input-bound-to-a-signal.md): `bind:value={s}`
is lowered to `value={s}` and an `on:input` handler setting `s`, and binds a
signal of `String` by its name on a field (PW5304). The browser sets an
attribute a signal decides in place, never from the field's own typing. A
block a signal decides is rendered again only for what it cannot set in
place, so a field bound inside one keeps its focus. Until then a block was
rendered again for every signal read anywhere in it.

[ADR-0143](DECISIONS/ADR-0143-what-names-a-form-control.md): a correction to
PW5014. A form control is named only by something in the same declaration
that reaches it, as the HTML standard and accname 1.2 define:
- a `<label for>` with text, naming the first element with that `id`;
- a `<label>` with text wrapping it;
- an `aria-labelledby` reaching text;
- a non-blank `aria-label`. Amended 2026-10-11: an `id` every row has reaches the first row's element, as HTML's reference does, and is PW5031's alone; an `aria-labelledby` through one was reported twice.

Until then any `id` counted as a name, unchecked, so T06's unsafe store and
`tricky.pw`'s field passed with no label, and a wrapping label was refused.

[ADR-0144](DECISIONS/ADR-0144-a-view-holds-signals-and-a-page-provides-them.md):
ADR-0130's third step.
- A view holds its own signals, and each use of it holds its own instance.
- A signal a module declares, `signal drawer: Bool`, has no value: a page
  or view gives it one with `provide`, for everything that body contains.
- A view names it as it names anything it imports, so sibling views share
  UI state with nothing passed by hand.
- A page that needs a signal nothing provides is refused (PW5305). So is a
  second `provide` of one signal in a body, or a `provide` of anything but a
  module's signal (PW5306), and a view holding a signal in a loop's row
  (PW5307).
- One compiled handler serves every instance: the element says which
  instance each signal it names is.
- An instance in a block starts again when the block shows another arm.

[ADR-0145](DECISIONS/ADR-0145-a-page-keeps-what-its-queries-decide-current.md):
E14-Q's third slice. The server renders every list a session's queries fill,
and keeps a page current by difference.
- It records what each document shows, and after a command derives a text
  patch for each part that changed, and keyed list operations: remove,
  insert after the one before, move, and set in place inside an instance.
- One change's patches travel as one `patch_set` frame, held once whole.
- A set the document cannot apply reloads it.

Until then the store patched its cart count and its menu by name, and a page
listing a private query's values did not render. T03 is gradable on Pleris
since.

[ADR-0146](DECISIONS/ADR-0146-a-block-a-query-decides-is-rendered-and-kept-current.md):
E14-Q's fourth slice.
- The page plan names each block a query's value decides at the top of the
  page.
- The server gives the renderer each binding's whole value, and a
  component's case as a case.
- After a command, the server sends `ReplaceRange` for each block whose
  rendering changed, and none for one that renders the same.
- What a loop's row reads of another query is refused, since a row is
  rendered again for its own item alone.

This replaces ADR-0145's refusal of such a block.

[ADR-0147](DECISIONS/ADR-0147-a-query-binding-is-the-query-s-value.md): what
T04 needed, and two corrections it found.
- A page's query binding is the query's value, the `Ok` value of its
  declared result. A `{#match}` over it takes that value apart and covers
  every case. Taking its `Result` apart waits for T10.
- Loops and arms are typed together until nothing more is learned.
- A page whose queries fail is answered 503, and a served one is told to
  reload, where the server had panicked holding its subscriber table and
  failed every request after.
- The development server holds a session's order, which the kitchen's
  benchmark hook sets.

[ADR-0148](DECISIONS/ADR-0148-a-stream-region-shows-its-query-s-state.md):
what T05 and T10 needed.
- A `<stream>` is compiled where ADR-0075 refused it. Its query's delivery
  says when its region is filled:
  - `delivery streamed`: in the same response, after the document, as the
    WHATWG's `<template for>` patch. Chrome 150+ applies it itself; the
    runtime applies it elsewhere.
  - otherwise: with the document, which waits for it.
- PW5400: a streamed query is read only by a `<stream>`.
- PW5401: a stream shows each state its query can be in, and no other.
- PW5402: a streamed query declares a `timeout`.
- The failed arm is given `Option` of the declared error; `None` is the
  host's failure.
- A declared error is given to the readers of its flight and not kept.
- The runtime starts from an inline `import()`, since a deferred module waits
  for the whole response.

Found on the way: the server's handler table missed a handler inside a
stream, and WebKit paints nothing until a page holds about 200 characters of
text.

[ADR-0149](DECISIONS/ADR-0149-pw-diff-what-a-change-means.md): E14-D, gate
item 1.
- `pw diff OLD NEW` checks and builds two programs, and reports what the
  change means in charter §19.2's sections: domain, effects, capabilities,
  privacy, placement, cache, invalidation, pages, client bytes, server
  components, obligations, unsafe and diagnostics.
- A side that does not check is refused, naming its errors.
- A policy no section names is reported, never dropped.

Its first use found that a page streaming a query was charged the query's
capability. The stream's query is now excluded from the page's authority, as
a handler's body is.

[ADR-0150](DECISIONS/ADR-0150-one-change-reaches-a-page-whole.md): two
development-server corrections.
- One change's frames reach a page in one hold of the subscriber table, the
  entry's value from the snapshot the patches came from. They had gone in
  two holds, which an intermittent browser failure exposed.
- A materialized fragment is kept while it shows its query's value, not
  until a command invalidates it.

[ADR-0151](DECISIONS/ADR-0151-a-page-s-values-are-read-outside-the-subscriber-table.md):
a development-server correction, found designing T02.
- A document's values are read, and it is rendered, outside the subscriber
  table. Pages are read at once, and pages asking while a query's flight is
  under way share it, as `concurrency one_per_key` says. They had been read
  one after another, server-wide.
- A change that reaches a session while its page is read is not lost. The
  page is read again, and the third attempt is read inside the table.
- The cart's count is read from the cart's binding alone, where a command
  asked every query on the page for it.

[ADR-0152](DECISIONS/ADR-0152-a-key-a-page-changes.md): what T07 and the
charter's §15.6 tests 6-8 needed. Settles ADR-0089's open ruling.
- A page's query may be given a page signal. The browser reads the binding
  again, for the new key, when the signal changes. The server applies only
  the latest read for a page that is still the session's, and sends what it
  shows as one patch set in the session's frames.
- PW5308: a signal that keys a query is a `String`, an `Int` or a `Bool`.
- PW5309: a query a signal keys declares `on_key_change`.
- `on_key_change`:
  - `cancel` stops the old key's read: the browser aborts it, and the
    server lets go of its flight, which `pw-resource` stops when nobody else
    holds it;
  - `supersede` lets the old read finish, and drops its answer;
  - `keep` reads the new key once the old read has finished, dropping keys
    passed over meanwhile.

  In none is an old key's answer shown for a new key.

Found on the way: the browser ran presses in the order their code arrived.
Each handler now starts after the press before it.

[ADR-0153](DECISIONS/ADR-0153-a-template-tests-a-case-with-match.md): the
first form gate item 5 found open. In a template a case is tested with
`{#match}`, which PW0305 holds to every case: an `{#if}` comparing a value
with one of its cases is refused (PW0337). A template's catch-all arm was
refused already (PW5019).

[ADR-0154](DECISIONS/ADR-0154-a-command-a-page-sends-declares-idempotent-by.md):
the second. A command a page's handler calls declares `idempotent_by`
(PW0338): its request can be delivered twice whatever the program does, and
each press carries an interaction. RFC 9110 lets a client retry only what it
knows to be idempotent.

[ADR-0155](DECISIONS/ADR-0155-the-page-keeps-its-subscription-and-recovers-a-refused-handler.md):
three runtime defects an audit of the store against charter §15 found.
- A failed subscription request ended the subscription: it is asked again,
  with a growing pause, from the cursor the page holds.
- A press on a handler from another build did nothing: it reads the page
  again, once, and never replays the press (§15.6 test 16).
- The recovery codes were read one place off.

[ADR-0156](DECISIONS/ADR-0156-the-benchmark-s-pleris-store-is-its-own-copy.md):
the benchmark's Pleris store is `benchmarks/baselines/pleris`, frozen as the
other two stacks' are. The canonical `examples/store` can then grow toward
charter §15 without moving the 108 patches the tasks are anchored on.

[ADR-0157](DECISIONS/ADR-0157-a-handler-is-answered-what-its-command-did.md):
the first gap the charter §15 audit found, item availability, end to end.
- A handler's call to a command is typed `Result<(), E>`. It learns whether
  the command committed, and the error it declares if not, never the value,
  which reaches the page from the query the command invalidates.
- The server answers `Ok` without its value, or the declared `Err` whole.
- PW0339: a command is called only by a page's handler.
- PW0620: a handler that binds a command's value is refused where it is
  written.
- The canonical store revalidates availability inside `add_to_cart`, and its
  page says when an item sold out (§15.6 test 10).

Found on the way:
- a compiled handler dropped a command whose answer decodes as `()`;
- a kept answer lost `null`;
- a handler matching on its command's answer had no name;
- ADR-0154 named its rule PW0339 for PW0338;
- E14-A's contract ran against the canonical store;
- the value relations behind PW0605 typed a command's call by its declared
  result.

[ADR-0158](DECISIONS/ADR-0158-a-test-leaves-nothing-behind.md): the
repository's tests had left 4,809 directories, 673 MB, in the temporary
directory, and made about 500 more each `just ci`. A Rust test's directory is
a `tempfile::TempDir` now, removed when the test is done with it. The
development server's test store keeps its directory as long as its server.
The harness removes its sandboxes and copied specs however a step ends.

[ADR-0159](DECISIONS/ADR-0159-a-handler-handles-what-its-command-answers.md):
what ADR-0157 left open. A handler's value goes to the runtime, which drops
it. So PW0618 refuses a `Result` a handler gives the runtime: its last value,
a value it returns, or the failure a `?` returns. A handler matches its
command's answer, or discards it by name. A block ending in a binding
compiles, so that discard can end a handler. The canonical store shows every
refusal. The benchmark's store discards each answer by name, its behaviour
unchanged, with T11's patches re-based.

[ADR-0160](DECISIONS/ADR-0160-a-page-s-route.md): the compiler's half of
the audit's route gap.
- The page plan carries a page's route, read by one reader with the link
  check.
- PW0340: a route is `/` and segments, a word or a `{parameter}` each, and
  names each of its page's parameters once.
- PW0621: a route's parameter is text.
- PW0341: one route is one page's.
- The canonical store declares `/stores/{id}`. Amended 2026-10-11: two pages of one name at one route are each named by its module.

[ADR-0161](DECISIONS/ADR-0161-each-document-is-its-own-subscriber.md): the
second step of the route gap.
- The development server kept one subscriber per session, and serving a
  document cleared it, so a change waiting for one tab was lost when
  another was served (a server test failed before).
- Each document is now numbered and has its own subscriber, record of what it
  shows, and keyed reads.
- A change reaches every document of its session, and the browser names its
  document in its subscription.
- This revises ADR-0152's "a page that is still the session's".

Found on the way:
- the browser suite served a build made with an older runtime;
- a stopped mutation run could leave its mutant unseen;
- two earlier mutants survived and are dealt with.

[ADR-0162](DECISIONS/ADR-0162-each-store-at-its-route.md): the third step of
the route gap.
- The development server routes a path by the plans' routes, so the store is
  served at `/stores/{id}`.
- Each document reads its own parameters.
- A second store, 48, has its own menu fragment.
- A change to a store's menu reaches that store's pages only: §15.6 test 11,
  end to end.

[ADR-0163](DECISIONS/ADR-0163-a-page-says-when-it-is-absent.md): the last step
of the route gap.
- A page declares the error that means its address names nothing:
  `not_found_on StoreError.NotFound`.
- PW0342 holds the clause to a case that a query the page reads can fail with.
- The plan carries the case, and the development server answers it 404, any
  other failure still 503.

Found on the way: every response said `OK`, whatever its status.

[ADR-0164](DECISIONS/ADR-0164-a-menu-change-drops-what-declares-it.md): a
correction to ADR-0162's test 11.
- A change to store 47's menu dropped every store's kept menu, by the query's
  name, in the server's code.
- The store's `Menu` now declares `invalidates_on MenuChanged(id)`, and the
  change is the event `MenuChanged(47)`, which drops store 47's entry only.

[ADR-0165](DECISIONS/ADR-0165-the-store-s-estimate-and-recommendations.md):
the audit's third gap.
- The canonical store streams its delivery estimate and its recommendations,
  each in a named region, after its own content (§15.6 tests 3 and 17).
- The recommendations are A-008's: shared, kept ten minutes, dropped by
  `MenuChanged(id)`, which now reaches a stream's kept answer.

Found on the way:
- WebKit paints the store only when its slots are filled, since it holds
  under 200 characters of text: tests 3 and 17 hold in Chromium and Firefox.
  §15.1's descriptions and prices are next.
- A `//` line in markup is text.

[ADR-0166](DECISIONS/ADR-0166-the-store-and-its-items-say-what-they-are.md):
part of the audit's eighth gap, and a correction to ADR-0165.
- `Store` and `MenuItem` declare a `description` (§15.1), and the store's
  page shows them: WebKit now paints it before its slots are filled.
- A host's answer is read through the type the importing component
  declares: fields it does not name are not passed in, and one it names
  that is missing is refused by name. One data layer serves the canonical
  store and the benchmark's frozen copy.
- Correction: ADR-0165's browser suite ran on a stale build. The suite now
  refuses a build whose sources have changed since.

[ADR-0167](DECISIONS/ADR-0167-markup-text-is-text.md): markup text is text,
and a comment in markup is `<!-- -->`.
- The lexer read `//` in markup as a comment. A `//` line between elements
  became page text. `<p>http://example.com</p>` did not parse. An unquoted
  `href=http://x.y` built `href="http"` and broke the next element. An HTML
  comment built a nameless element. None of these was reported.
- The parser now reads markup text, `<!-- -->` and unquoted attribute
  values from the source, as HTML reads them.
- PW5028 refuses a line of markup text that begins with `//` or `/*`.
- Seven rejected fixtures had shown their `// ERROR:` notes as page text;
  they are `<!-- -->` now.

[ADR-0168](DECISIONS/ADR-0168-a-change-reaches-every-part-that-reads-it.md): a
change reaches every part that reads it.
- The store names each Add by its item and says its count in a live region.
  Naming the buttons showed a rename left an attribute stale.
- The renderer now says what changed in an instance, text and attributes
  alike, and a host sets them in place with `SetAttribute`, which the
  runtime now applies.

Found on the way: E7-P moved an instance already in place, which scrambled
the list's anchors; a single patch that addressed nothing hid it.

[ADR-0169](DECISIONS/ADR-0169-a-member-read-is-computed-or-refused.md): what a
template reads through a member function, a host computes or the build
refuses.
- A menu row's `{item.price.display}` was neither planned nor refused. The
  store built, then failed at its first render. Values outside text
  (attributes, block subjects, loop lists) were not looked at by the plan at
  all.
- A loop's row now reads members of its item. A host computes each one for
  each row of a query's list. Every other member read no host computes is a
  build refusal, wherever it is.
- The store shows each price, `$3.50`, written as en-US writes dollars, and
  exact for every amount.

Found on the way: a transport test assumed the cart's frame came before the
menu's, and read one entry under load.

[ADR-0170](DECISIONS/ADR-0170-a-list-inside-a-querys-value.md): a loop over a
list inside a query's value, and a part a speculation would not reach.
- `{#each cart.lines as line}` built, and the development server failed at
  the cart's first change: the plan recorded `cart` as the list. A row's
  member read over it was refused, and one of a number, `quantity.count`,
  had nowhere to go.
- A list is recorded by its path. Its rows read members of their item, and
  a computed value is set in the row whole, by its path.
- The speculation module refuses an attribute, a block's subject or a loop's
  list that reads a speculated value. The page would have shown two values
  of one thing at once.

[ADR-0171](DECISIONS/ADR-0171-an-attribute-that-reads-a-query-is-set-again.md):
an attribute at the top of the page that reads a query's value is set again
when it changes.
- `hidden={cart.lines}` kept its first value for the document's life: a host
  set a text part, a list's rows and a block again, and no attribute at the
  top of a page.
- The plan names each one, and a host sets it with `SetAttribute`, or
  removes a boolean one that goes, as the page's render writes it.

[ADR-0172](DECISIONS/ADR-0172-the-cart-lists-its-lines.md):
the cart lists its lines, and a speculation reaches every part that reads it.
- Each line shows its name, − and + around its quantity, its total and
  Remove, and the cart its subtotal (§15.3). `increase_in_cart`,
  `decrease_in_cart` and `remove_from_cart` are optimistic and idempotent,
  and `add_to_cart` takes the item the page showed, so a new line has its
  name and price before the server answers.
- The browser renders again each attribute, block and loop that reads a
  speculated value, with the server's renderer, rows set where they are and
  focus kept. A region reading what the browser does not hold is refused.
- Corrections: a speculation whose value came before its answer was shown
  twice, and two changes of one session could be sent at once.

[ADR-0173](DECISIONS/ADR-0173-a-command-is-sent-again-where-no-answer-came.md):
a command is sent again where no answer came, and a command that is retried
is idempotent.
- The compiled handler passes the command's `retry` clause where it sends
  it, and the runtime sends the request again, with the same interaction,
  as many times as the clause allows. An answer of any kind is never sent
  again. The store's commands declare `retry transport_only(max = 2,
  jitter = true)` (§15.4).
- Correction: PW0312 let `transport_only` retry a command that is not
  idempotent, though a browser cannot tell a request never sent from one
  whose answer was lost.

[ADR-0174](DECISIONS/ADR-0174-a-store-delay-a-cart-delay-and-a-one-shot-database-error.md):
a store delay, a cart delay, and a one-shot database error.
- `/bench/store?delay=`, one for every reader; `/bench/cart?delay=`, one
  session's; `/bench/fail?next=write|read`, the session's next cart write
  or read failing once as a database that is down does (charter §15.5).
- Correction: the one test of a rolled-back command posted an add of the
  server's own from another session, so it could not fail. It is the page's
  own press now.

[ADR-0175](DECISIONS/ADR-0175-a-network-error-and-a-forced-reconnect-the-server-makes.md):
a network error and a forced reconnect, made by the server.
- `/bench/drop?next=command&at=before|after` closes the session's next
  command connection with no answer, before the command runs or after it
  commits; `/bench/reconnect?for=ms` ends its subscriptions now and refuses
  new ones for a while (charter §15.5).
- A press survives either drop as one mutation, and a page cut off hears,
  once back, what changed meanwhile, in three engines.

[ADR-0176](DECISIONS/ADR-0176-a-regeneration-that-fails-sends-nothing-and-is-tried-again.md):
a regeneration that fails sends nothing, and is tried again.
- `/bench/materializer?fail=next` fails the session's next regeneration
  (charter §15.5). A failed one sends nothing, its stale entry is tried
  again at the session's next drain, which a page's subscription request
  makes, and the commit's answer names a version a later one passes.
- Correction: a failed regeneration's frames went out at a version that had
  not moved, and the page ended a line short, with no error.

[ADR-0177](DECISIONS/ADR-0177-a-public-read-whose-origin-fails-is-answered-with-the-last-value-kept.md):
a public read whose origin fails is answered with the last value kept.
- Charter §15.6 test 18. PW0343 keeps `fallback last_known_good` to public
  data; the query runtime answers a public read whose origin failed with
  the last value kept, checking the manifest's privacy and the kept
  value's; the store's `Store` and `Menu` declare it.
- `/bench/store?fail=next` fails the store's origin once; the page is shown
  with the last store kept, and a session's cart never is.

[ADR-0178](DECISIONS/ADR-0178-whether-an-item-can-be-ordered-is-shown-before-the-press.md):
whether an item can be ordered is shown before the press, and a change to it
reaches every page open.
- Charter §15.1 and §15.2. `MenuItem.available`; a sold-out row says so and
  has no Add; a stock change is `InventoryChanged`, which the `Menu` query
  hears; `/bench/stock?tell=true` tells.
- Every new version of the shared menu fragment reaches the pages that show
  it, as the whole difference, amending ADR-0150: a change at the source
  that a document read had left the pages open a version behind.
- The renderer looks into a block that decides as it did; the value
  analysis types a listener's `_` as any value.

[ADR-0179](DECISIONS/ADR-0179-an-opaque-type-states-its-invariant.md):
an opaque type states its invariant, and every construction and every
boundary holds it.
- `opaque type PositiveInt = Int where value >= 1`: bounds on an `Int`'s
  `value`, joined by `&`; PW0623 refuses any other predicate.
- PW0622: a construction the build cannot show holds it is refused; the
  value analysis bounds the `Int` it is given, and a test narrows it.
- The contract states each boundary's checks, as paths into the value; the
  host holds a command's arguments to them as it decodes them, and a data
  layer's answer before the component reads it.
- Measured before: a forged quantity of 0 committed a line of nothing, and
  −3 a line of minus three.

[ADR-0180](DECISIONS/ADR-0180-a-delivery-estimate-is-a-range.md):
a delivery estimate is a range, and says when it was made.
- Charter §15.1: `DeliveryEstimate { min_minutes: PositiveInt, max_minutes:
  PositiveInt, generated_at: Instant }`, and the platform's `clock` declares
  `Instant`. An estimate of 0 minutes is a failed read (ADR-0179).
- The slot says "Delivery in 25 to 35 min", in words: an en dash in a range
  is read unreliably by screen readers.

[ADR-0181](DECISIONS/ADR-0181-a-menu-is-grouped-by-its-category.md):
a menu is grouped by its category, and a list inside a row is changed where it
is.
- Charter §15.1: `MenuItem.store_id` and `category`; the store's `Menu` answers
  `MenuSection`s, and the page gives each category a heading and its items.
- The plan reads a loop inside a loop (`menu.*.items`); the renderer derives a
  keyed list's change, a list inside a row diffed where it is; every menu
  change is derived from the menu's values, E7-P's operations included.
- A menu's version and a cart's identity stay the runtime's, not fields.

[ADR-0182](DECISIONS/ADR-0182-keyboard-and-screen-reader-semantics-remain-valid.md):
keyboard and screen-reader semantics remain valid.
- Charter §15.6 test 14: the store read by axe's and WCAG 2.2 AA's rules in
  Chromium, Firefox and WebKit, as served and after each kind of change; the
  keyboard's order and presses, the accessibility tree, live regions, reflow,
  a phone's width and reduced motion.
- The runtime writes a part only when what it shows changes: one Add said
  "Items in cart: 1" up to four times. Every page has a viewport.
- The store's title, "Store" for every store, is the page's to declare
  (ADR-0183); axe-core itself is the owner's to install.

[ADR-0183](DECISIONS/ADR-0183-a-page-states-its-title.md):
a page states its title.
- `<title>{store.name}</title>` at the top of a page's view: the host writes it
  into the document's head, and a change to what it reads is set as text,
  as `document.title`. Store 47's page is "Blue Bottle", not "Store" (WCAG
  2.4.2, F25).
- PW5029: a page served at a route states its title. PW5030: a title is the
  page's, once, at the top of its view, written as text and values. A title
  that reads a signal or a speculated value is refused at build.
- Corpus C9: A-025, R-047 and R-048; four routed fixtures given a title;
  generality 33 / 33.

[ADR-0184](DECISIONS/ADR-0184-what-a-cache-may-keep-holds-nothing-of-a-session-s.md):
what a cache may keep holds nothing of a session's.
- Charter §15.6 tests 2 and 13 against the running store. Found: the
  store's page went out with no `Cache-Control`, and a fresh session's
  cookie went on a build's files. A cache could have handed one person's
  cart, or session, to the next.
- A session's response says `private, no-store`; a build's file names no
  session. What the query runtime keeps for every reader, and the
  materializer's public fragments, are the same whether or not anyone
  pressed.

[ADR-0185](DECISIONS/ADR-0185-ids-and-the-aria-that-names-them-checked-at-build.md):
ids, and the ARIA that names them, checked at build.
- Charter §8.2's "duplicate IDs" and "invalid ARIA relationships". PW5031: an
  id names one element of its page, none in a loop. PW5032: a reference
  names an element its page shows whenever the referrer is shown. PW5033:
  ARIA attributes, values and roles are WAI-ARIA's; `aria-labeledby` is
  refused, with `aria-labelledby` offered.
- One mistake, one report: a field left unnamed by such a reference or id is
  not reported again by PW5014. Corpus C10: R-049 to R-051, A-026;
  generality 36 / 36.

[ADR-0186](DECISIONS/ADR-0186-a-page-states-its-description.md): a page
states its description.
- Found by Lighthouse on the store's page: SEO 75, with no meta
  description. A page writes `<meta name="description" content={…} />` or
  Open Graph's `<meta property="og:title" …>` at the top of its view, and
  its host writes it into the head as the page is served. Optional: a
  description is not an accessibility requirement.
- PW5034 follows HTML: one description, `color-scheme`, `application-name`
  and `theme-color`, compared ignoring case; no `media` or `lang`, which the
  head would drop; the host's charset and viewport refused. A
  `<meta itemprop>` is microdata, written in the body where it is, as React
  19 reads it.
- A correction to ADR-0183: a static page that stated its title shipped the
  browser runtime. Corpus C11: R-052, A-027; generality 37 / 37.

[ADR-0187](DECISIONS/ADR-0187-nothing-contained-is-on-screen-when-the-page-is-first-laid-out.md):
nothing the store contains is on screen when its page is first laid out.
- Found by Lighthouse on a phone: CLS 0.136. Every menu item was contained
  with a 42 px placeholder, and items are 125 px, so the cart moved 249 px
  when the browser rendered the three on screen. That was before the first
  paint, and the Layout Instability API, which field data reads, reported it
  all the same.
- An item is contained when 28 items precede it in its list, or 16 lists
  precede its list: at least 2,400 px down, below any first screen up to a
  4K display's. At most 448 items are laid out uncontained, whatever the
  menu. The placeholder is 7.75em, within 5% of an item in three engines,
  and the style is in the head.

[ADR-0188](DECISIONS/ADR-0188-a-page-s-runtime-is-bounded-as-it-is-sent-in-every-run.md):
a page's runtime is bounded as it is sent, in every run.
- A correction to E7's record: gate item 7b's bound, 128 KiB of what the
  store's page downloads to run, had been passed by ADR-0172 at the latest
  (141,339 bytes on 2026-10-04), unseen, as its test ran only in `just e7-performance`, last run on
  2026-08-07.
- Each file is bounded compressed with Brotli at quality 11, as a static
  file is sent: 64 KiB for activation (44,129 bytes), 128 KiB for the
  renderer's WebAssembly (82,888). The byte counts run in every browser
  suite; controls grow each file past its bound.
- For the owner: minifying the runtime, which needs a minifier this session
  did not download.

[ADR-0189](DECISIONS/ADR-0189-a-link-is-written-where-html-allows-it.md): a
`<link>` is written where HTML allows it.
- `<link rel="canonical">` and `rel="icon"` in markup checked, built, and
  were written into the body, where HTML does not allow them and nothing
  reads them. PW5035: a `<link>` in markup has only body-ok relations
  (`stylesheet`, `preload`, `preconnect`...) or is an item's property.
- Corpus C12: R-053, A-028; generality 38 / 38.

[ADR-0190](DECISIONS/ADR-0190-every-page-that-binds-a-query-is-served-at-its-route.md):
every page that binds a query is served at its route.
- E14-Q's next slice. Only the store's page could bind a query: the server
  read the store's plan in 21 places. Now each document records its page,
  and is read, rendered and kept current by its own plan and template.
- The menu's fragment and the speculation stay the store's page's. The store
  gains a second page, its cart at `/cart`: a change made on either page
  reaches the other while both are open.

[ADR-0191](DECISIONS/ADR-0191-every-page-speculates-from-its-own-module.md):
every page speculates from its own module.
- `pw build` wrote every page a speculation module; the server read the
  store's alone, so the cart's page showed a press only when the server
  answered. Now each page carries its own, and each document whose page
  speculates on a value is sent it. Amended 2026-10-11: each page is held to its own entry's value, which a layout's binding of the same query had masked.

[ADR-0192](DECISIONS/ADR-0192-the-stores-as-the-home-page.md): the stores,
as the home page.
- A delivery site starts with its stores. `Stores.list()`, a host operation
  of the store's data layer, a public `StoreList()` query over it, and a
  `HomePage` at `/`: each store linked to its page, and the session's cart
  counted beside them. `/` was store 47's page, an alias from E7.

[ADR-0193](DECISIONS/ADR-0193-an-order-is-placed-and-its-page-follows-it.md):
an order is placed, and its page follows it.
- The cart's page places its cart as an order: one commit writes the order
  and the emptied cart, and an empty cart places nothing. The order's page at
  `/order` shows its status, kept current as the store moves it along.
- A change the store makes, not a command, reaches the session's open pages
  (`session_changed`), against the order's own entry at an advancing version.

[ADR-0194](DECISIONS/ADR-0194-a-type-that-contains-itself-crosses-as-its-nodes.md):
a type that contains itself compiles, and crosses a boundary as its nodes.
- PW0624 refuses a type no finite value has, `type Loop = Loop { again: Loop }`.
- Through a list, such a type is laid out by its type inside a component. At
  a boundary it is `list<node>` in level order, each list of itself a
  `list<u32>` of node indices: canonical, checked in one pass, encoded and
  decoded with no recursion, and decoded in place. Malformed nodes trap.
- A host holds the value nested, no deeper than 128, and an invariant inside
  a tree holds at every node. Held in place, through another declaration,
  or on the browser's wire, it is refused by name.

[ADR-0195](DECISIONS/ADR-0195-the-owners-rulings-on-fifteen-open-questions.md):
the owner's rulings on fifteen open questions.
- Relayed 2026-10-05: record shorthand by Rust's rule; a case uppercase and
  a lowercase pattern name always a binding; only statement-starting words
  reserved; "type annotation needed" where nothing fixes a result; a bare
  case with a payload resolved like a payload-free one; Float `%` as Rust's
  `rem_euclid` and a Float's text as ECMAScript's `Number::toString`;
  `return` stays a statement.
- Cache keys by information flow; labels on clause keys; `Cart(_)` written
  out, materializations that read others; events returned by the command
  and written in its transaction; a named function as a handler; `<link>`
  origins declared and the CSP generated from them; a speculation never
  invents a value; E7's gate 8 counting attributed long frames.
- Each checked against a primary source and confirmed, `%` refined by
  `rem_euclid`'s own contract, and ordered behind the app layer except
  where it fixes a wrong value.

[ADR-0196](DECISIONS/ADR-0196-only-a-word-that-begins-a-statement-or-an-expression-is-reserved.md):
only a word that begins a statement or an expression is reserved (ADR-0195,
ruling 3).
- PW0013, revision 2: no such word names a binding (a `let`, a parameter, a
  loop's or a pattern's, a signal), and a declaration is not named by a word
  that begins an expression. `let return = n` had checked, its use read as a
  `return`.
- Every other keyword is contextual. The platform's `query`, `measure` and
  `mutate` stand, since a call by a statement word reads as a call.
- The repair suggests a name: `match_`, PEP 8's trailing underscore.

[ADR-0197](DECISIONS/ADR-0197-a-pattern-tells-a-case-from-a-binding-by-its-capital.md):
a pattern tells a case from a binding by its capital (ADR-0195, ruling 2).
- The parser decides once, `pw_syntax::pattern_kind`; six analyses had each
  guessed, and a misspelt case, `Circel`, bound a name that matched every
  value (rustc's E0170).
- A capitalized name its type lacks is PW0608 (revision 3); a case named in
  lowercase is PW0625; `{:Some(Draft)}` is no arm. `check::Env` is gone.
- Corpus C14, generality 41 / 41.

[ADR-0198](DECISIONS/ADR-0198-a-bare-case-with-a-payload-is-the-one-type-that-has-it.md):
a bare case with a payload is the case of the one type that has it
(ADR-0195, ruling 5, its first half).
- `Circle(3)` alone resolves as `Empty` alone does: the name check accepts
  it, the typer relates its payload, the backend builds it, and PW0022 names
  every type where several have it. It was PW0021.
- Resolution from the expected type, for both forms, is the ruling's other
  half, not built yet.
[ADR-0199](DECISIONS/ADR-0199-a-function-or-command-named-as-a-handler-is-the-lambda-that-calls-it.md):
a function or a command named as a handler is the lambda that calls it
(ADR-0195, ruling 12, its remainder).
- `on:submit={save}` is `(e) => save(e)`: compiled as that lambda, and held
  to its event, its answer, idempotency and the browser, as it is.
- A local's value, a page, a type or a case named as a handler is refused
  when checked (PW0614), and a handler named through a module is checked
  against its event (PW0602). Each checked before.
[ADR-0200](DECISIONS/ADR-0200-unit-is-a-value-and-nothing-unread-checks.md):
`()` is the unit value, and nothing the compiler cannot read checks (found
building ADR-0199).
- `()` was an expression that did not parse, typed as anything: `fn f() ->
  Int !{} { () }` checked. It is typed `Unit` now, and built.
- `check_sources` reports a file's syntax errors and keeps the file out, as
  `pw check` does; four tests had passed on programs that do not parse.
- An error node in a file that parses is refused (PW0015), and every `.pw`
  file in the repository is held to that.
- A `{#..}` block left open ends with its element, so PW5019 names it where
  `pw check` had reported the end of the file, twice.
[ADR-0201](DECISIONS/ADR-0201-a-case-written-alone-is-the-expected-types.md):
a case written alone is the case of the type expected where it is written
(ADR-0195, ruling 5, its second half).
- Where several types have `Empty`, the type expected where it is written
  chooses: a result, an annotation, an argument, a field, an assignment, a
  comparison's other side, a list's element, and through branches and arms.
- The typer decides it once and owns PW0022 for it; the backend asks the
  same function, so it builds the case the checker typed.
[ADR-0202](DECISIONS/ADR-0202-a-type-that-holds-itself-in-place-is-boxed.md):
a type that holds itself in place is boxed, and crosses as its nodes.
- `next: Option<Node>` and `Add(Expr, Expr)` compile: each value of such a
  type is the address of its cell, a `u32` of its own in the private
  layout, built, read and matched through the cell. The IR is unchanged.
- At a boundary a node's slot is a `u32` for a box and an `option<u32>` for
  an option of one, beside ADR-0194's `list<u32>`, in the same level order,
  encoded and decoded in place without recursion.
[ADR-0203](DECISIONS/ADR-0203-a-view-that-contains-itself-is-an-instance-made-at-run-time.md):
a view that contains itself is an instance of its own template, made at run
time (ADR-0130, ruling 2).
- A reply thread's view shows each reply as itself, as deep as the data:
  each use is an `instance` part, rendered in a frame of its own, its
  template compiled once. One with no block on the way back to it, or
  holding a signal, is refused (PW5020).
- The renderer writes each instance after the markup around it, not on its
  stack, and refuses a page nesting more than 500 elements, under what
  Blink and WebKit's parsers nest (512).
- The browser reads each instance in its own template: its parts, its
  range, and its handlers, bound in every instance.
[ADR-0204](DECISIONS/ADR-0204-an-element-holds-the-children-html-permits-as-the-page-holds-them.md):
an element holds only the children HTML permits, as the page holds them
(found building ADR-0203).
- PW5012 read only the elements written inside: `<ul><Thread /></ul>` was
  refused though `Thread` renders an `<li>`, and a `<div>` row of an
  `{#each}` in a `<ul>` passed. A block's rows and what a view renders are
  read now.
[ADR-0205](DECISIONS/ADR-0205-a-value-that-contains-itself-crosses-the-browsers-wire-as-its-nodes.md):
a value of a type that contains itself crosses the browser's wire as its
nodes (ADR-0194's next step).
- `{ "$graph": [node, ...] }`, each value of the type inside a node
  `{ "$node": k }`, in ADR-0194's level order: a signal's first value, what a
  handler reads and sets, and a command's argument, which the host passes to
  the component as its nodes.
- The renderer's values are dropped, cloned and compared without recursion.
[ADR-0206](DECISIONS/ADR-0206-a-case-has-one-name-where-values-are-rendered.md):
a case has one name where values are rendered, its WIT case's (found
building ADR-0205).
- The template and a host's values named the language's four `Some`,
  `None`, `Ok`, `Err`, and the browser's wire `some`, `none`, `ok`, `err`, so
  a `{#match}` on an `Option` a signal holds found no arm. One name now, the
  wire's.
[ADR-0207](DECISIONS/ADR-0207-a-data-source-states-what-it-guarantees.md):
a data source states what it guarantees, and nothing asks it for more (the
owner's priority 21; ADR-0195's ruling 11).
- `source X  holds A, B  transactions …  reads …  changes …`, stated by the
  program: what a database gives depends on how it is deployed.
- A query's consistency, a command's isolation, events and idempotency, and
  its writes in one source are held to it (PW0344-PW0349). A resource no
  source holds is the host's database's.
[ADR-0208](DECISIONS/ADR-0208-a-command-computes-its-events.md): a command
computes its events, and the outbox commits them with its writes
(ADR-0195's ruling 11's other half).
- Each event is a function of the platform's outbox,
  `pw:host/outbox#cart-changed`, which the command calls before its body
  with the values it computed. Emitting is `outbox.write`, which a node
  grants where it keeps an outbox.
- The server stages them and commits them with the writes; it evaluates no
  key's text for an event. Found: an `Int` key missed its entry, and an
  empty value reached every entry.
[ADR-0209](DECISIONS/ADR-0209-a-command-computes-the-entries-it-invalidates.md):
a command computes the entries it invalidates (ADR-0195's ruling 11,
completed).
- Each query a command invalidates is a function of the platform's
  invalidations, `pw:host/invalidations#store-page-cart`, called before its
  body with the values it computed, under `outbox.write`.
- The server drops the entry by that key once the writes commit, and reads
  no key's text: until then anything but `current_session()` dropped every
  entry of the query.
[ADR-0210](DECISIONS/ADR-0210-the-owners-rulings-on-the-pre-delegation-marks.md):
the owner's rulings on the pre-delegation marks, ADR-0031 to ADR-0122,
relayed as ADR-0195's were.
- 85 marks: 26 overruled, 30 confirmed, 29 already settled. Ten soundness
  defects the sweep found come first.
- Each is built in its own ADR, which may overrule it with research.
[ADR-0211](DECISIONS/ADR-0211-a-path-that-leaves-a-loop-owes-its-releases.md):
a path that leaves a loop's body leaves the function, and owes its releases
(ruling 0045-a; ADR-0210's urgent defect 1).
- A `return` or failing `?` in a `for` body left a transaction open and
  checked; the correct roll-back-then-return was refused. Both are fixed.
[ADR-0212](DECISIONS/ADR-0212-a-query-reads-a-query.md): a `query` reads a
query or a resource, and a `subscription` a subscription (ruling 0108-a;
urgent defect 6).
- `let v = query helper(n)` over a `fn` checked. PW5108 refuses the name
  where it is read, by its kind.
[ADR-0213](DECISIONS/ADR-0213-the-maximum-of-zeros-is-plus-zero.md):
`List.maximum` gives +0 over −0 (urgent defect 7).
- It kept the first, and its test compared by value, under which the two
  are equal. It compares bits now, as IEEE's `maximum` orders them.
[ADR-0214](DECISIONS/ADR-0214-an-opaque-types-value-is-its-representation.md):
an opaque type's own module reads its representation as `.value`, and
declares no member of that name (urgent defect 8).
- `LayoutSnapshot`'s accessor was `value`, which shadowed the representation
  in `browser`. It is `measured`, and PW0626 refuses the shape.
[ADR-0215](DECISIONS/ADR-0215-a-querys-retry-reaches-its-runtime.md): a
query's `retry` reaches its runtime as declared, and `fixed` is no strategy
(ruling 0089-b; urgent defect 4).
- The plan carried a query's attempts alone, so `jitter = false` ran with
  jitter, and `fixed` ran as exponential. The plan carries its jitter, and
  `fixed` left the language.
[ADR-0216](DECISIONS/ADR-0216-every-clause-belongs-to-a-declaration-that-reads-it.md):
every clause belongs to a declaration that reads it, and a code body admits
none (rulings 0092-b and 0047-a's interim; urgent defect 5).
- PW5105 placed four heads and let 61 check anywhere: `freshness` on a
  command and `retry` on a function were read by nothing. Every head has a
  place now, and the table fails closed.
- A policy word in a function's body was a clause whose words were never
  resolved. It is a name now.
[ADR-0217](DECISIONS/ADR-0217-a-handlers-captures-are-set-again.md): what
a handler at the top of the page captures is set again when it changes
(urgent defect 2).
- A row's captures were patched with the row. A top-level handler's were in
  no plan, so a button capturing the cart sent the cart the page was first
  rendered with. The plan, the server and a speculation set them again now.
[ADR-0218](DECISIONS/ADR-0218-a-host-serves-any-program-its-data-is-the-deployments.md):
a host serves any program, and its data is the deployment's (the feed
reference app's first step).
- The development server served the store alone: its machinery and the
  store's data were one. The split is made in place, in steps, each keeping
  every test green. First the store's state, reads, command staging and
  grants move into `StoreData`, and a build importing what no layer supplies
  is refused at start.
[ADR-0219](DECISIONS/ADR-0219-what-a-commit-drops-reaches-every-session-that-reads-it.md):
what a commit drops reaches every session that reads it (the feed reference
app's second step).
- A commit told the session that made it. Another reader's open timeline
  saw a post when the page was next loaded. Each other session whose open
  page reads a query the commit dropped, whole or by a shared key, is read
  again and sent the change, after the author is answered.
[ADR-0220](DECISIONS/ADR-0220-the-feed-is-served-in-browsers.md): the feed
is served in browsers by the host that serves the store (the feed's third
step).
- A browser opened its stream at once, and the drain regenerated the store's
  cart for every program: the feed's page was told to reload, for ever. A
  layer with no session entry drains nothing, a page's style is its layer's,
  and a guest is named for its session. The feed runs in three engines on
  hosts of its own.
[ADR-0221](DECISIONS/ADR-0221-a-form-controls-value-is-written-where-html-reads-it.md):
a form control's value is written where HTML reads it (found by the feed).
- `bind:value` on a `<textarea>` was written as an attribute it does not
  have: a first value showed nothing until a script ran. It is written as
  the textarea's text now. PW5036 refuses a `<select>`'s value, which is the
  option it marks `selected`, and what no change would set again.
[ADR-0222](DECISIONS/ADR-0222-a-post-is-shown-before-the-server-answers.md):
a post is shown before the server answers (the feed's optimistic posting;
ruling 0105-a's unnamed key).
- Speculation was the store's cart's: the server sent no other value. A
  target may leave a key unnamed, `_`, and the server sends any value a page
  speculates on, read with what the page shows, when it changes. The feed's
  post shows first, by "You", and is the server's when its value arrives.
[ADR-0223](DECISIONS/ADR-0223-a-streamed-region-is-filled-when-its-whole-arm-has-arrived.md):
a streamed region is filled when its whole arm has arrived (found by an
intermittent `slots.spec.mjs`).
- The runtime applied a `<template for>` as soon as it saw one, and a
  response that arrived in parts gave a region the part of it parsed: one
  recommendation of two. It waits for the comment the renderer writes after
  each template now. The slots' tests hold the recommender instead of
  outwaiting a delay half a second short of the query's timeout.
[ADR-0224](DECISIONS/ADR-0224-a-longer-read-is-not-applied-over-a-commit-it-did-not-see.md):
a longer read is not applied over a commit it did not see (found by
ADR-0222).
- A keyed read read outside the session's hold and applied its value after,
  so a commit between the two was sent first and then undone by the older
  value. It applies in the hold now, and reads again when a change reached
  its document while it read.
[ADR-0225](DECISIONS/ADR-0225-a-strings-length-is-an-invariant.md): a
`String`'s length is an invariant (the feed's typed length limit; extends
ADR-0179).
- `opaque type PostText = String where String.length(value) >= 1 &
  String.length(value) <= 280`: bounds on a length in code points, as
  `String.length` counts it. Every construction is shown to hold them at
  build, and a host holds a browser's post to them. The feed's post is one.
[ADR-0226](DECISIONS/ADR-0226-a-value-the-template-computes-compiles.md): a
value the template computes compiles (ruling 0073-a, the host's part; the
feed's counts).
- A text hole or an attribute's whole value may compute from one query's
  value at the top of a page. The compiler lifts it into a function, a
  component of its own, which the host runs when the page renders and again
  when the value changes. It performs nothing (PW0334, revision 2). Found:
  a member an imported module lacks resolved, and checked clean.
[ADR-0227](DECISIONS/ADR-0227-a-value-computed-from-a-signal-is-the-browsers.md):
a value computed from a signal is the browser's (ruling 0073-a, the browser's
part; the feed's draft).
- A text hole or an attribute's whole value may compute from one signal. The
  host renders its first value with the component ADR-0226 lifts; the build
  compiles the same function into the page's module, which the browser loads
  when the signal first changes. The feed's draft says what is left, and its
  post button is disabled while there is no post to send.
[ADR-0228](DECISIONS/ADR-0228-a-value-computed-in-a-row-is-the-rows.md): a
value computed in a row is the row's (ruling 0073-a in a loop's row and from
a speculated value; the feed's likes).
- A value computed from a row's item is named by a path from the item, and a
  host computes it for each row as a member read; the speculation module
  computes it for each row it renders. The feed's rows say their likes in
  words, and a like is shown before the server answers. Found: a field of a
  value was charged a same-named function's effects, and an opaque value did
  not compare.
[ADR-0229](DECISIONS/ADR-0229-a-computed-condition-decides-its-block.md): a
computed condition decides its block (ruling 0073-a for a block's subject and
inside a block; 0071-a's repair).
- `{#if}`, `{:else if}` and `{#match}` read a computed subject by a path the
  compiler names: a host's from a query's value, its block rendered again
  when it changes, and the browser's from a signal's, its block rendered
  again as the signal changes. A value computed inside a block a host renders
  is the host's. The feed's draft too long says so; a thread with no reply
  says that.
[ADR-0230](DECISIONS/ADR-0230-a-condition-is-a-bool-or-tested-non-empty.md):
a condition is a `Bool`, or tested non-empty (ruling 0071-a; answers
ADR-0071's mark).
- A template's condition, and a boolean attribute, is a `Bool`, or a `List`
  or a `String` tested non-empty. A number has no truth, "say what it tests:
  `n > 0`", which ADR-0229 builds; a record and anything else none. Corpus
  C17, generality 44 / 44.
[ADR-0231](DECISIONS/ADR-0231-a-pages-parameter-is-rendered-on-every-page.md):
a page's parameter is rendered on every page (found building the feed's
replies).
- A page that binds a query is rendered with its parameters, as one that
  binds none is: its document, each block and row a host renders again. A
  title, an attribute or a handler's captures that read one built, then
  every request was answered 503; a text part is no longer refused. The feed
  replies: the thread page's form passes the page's `id`. Found: an opaque
  value's representation does not render in a template (ADR-0232).
[ADR-0232](DECISIONS/ADR-0232-an-opaque-values-representation-is-read-in-a-template.md):
an opaque value's representation is read in a template (found by ADR-0231).
- `{p.id.value}` was read as a field of the text, and its page was answered
  503. A template reads an opaque value's `.value` by the opaque value's own
  path now, as the checker types it, in each body it lowers; a record's
  field named `value` is a field. Where the checker's types leave a base
  untyped, the value relations type it.
[ADR-0233](DECISIONS/ADR-0233-a-speculation-on-a-value-that-contains-itself.md):
a speculation on a value of a type that contains itself (lifts ADR-0205 §5
for a speculation's value).
- The server writes each value a page speculates on by its query's declared
  type: a value of a type that contains itself as its nodes, in the level
  order the host reads, each such value inside another a graph of its own.
  The page's module reads it from its nodes, as a handler reads a signal's.
[ADR-0234](DECISIONS/ADR-0234-a-speculated-views-instance-is-rendered-again.md):
a view's instance given a speculated value is rendered again with it (found
building the thread page's optimistic reply).
- A view that contains itself, given a value a page speculates on, was no
  region of the speculation: a press changed the page's count and left the
  thread as the server rendered it. Its instance is a region now, rendered as
  a block is; one given what the browser does not hold, or nested where no
  region renders it, is refused by name.
[ADR-0235](DECISIONS/ADR-0235-what-a-speculated-value-computes-the-module-computes.md):
what a speculated value computes, the page's module computes (ruling 0073-a,
and a gap beside it).
- A block whose subject is computed from a speculated value is a region now,
  its subject and each value computed inside it the module's, given to the
  renderer with the value. A value computed from one inside a block no region
  renders was skipped, leaving the server's beside the speculation's; it is
  refused by name.
[ADR-0236](DECISIONS/ADR-0236-a-speculation-on-the-entry-a-pages-parameter-keys.md):
a speculation on the entry a page's parameter keys (ruling 0122-d).
- A target's key may name a command's parameter; it matches a page's binding
  keyed by the page's parameter that every handler on the page passes it,
  unchanged. A speculated entry is named by the page's parameters its key reads.
  The feed's thread page shows a reply before the server answers, in three
  engines.
[ADR-0237](DECISIONS/ADR-0237-what-lowering-parses-it-reports.md):
what lowering parses, it reports (a correction).
- A clause's value, a string's hole and a block marker's expression are parsed
  by lowering, and every error those parses made was dropped:
  `invalidates Cart(s) Order(s)` invalidated no order, and `{#if flag other}`
  was decided by `flag`. They are reported now, at their place, as PW0016 and
  PW0017; a missing comma or arrow is reported and read past, as rustc does.
[ADR-0238](DECISIONS/ADR-0238-a-command-speculates-on-several-entries.md):
a command speculates on several entries, a page on those it shows.
- An `optimistic` clause has an arm for each entry it changes, separated by
  commas, each arm's transition typed against its own entry. A page
  speculates on the arms whose targets it shows; one whose resource it does
  not show is not the page's. The feed's thread page shows a like before the
  server answers, in three engines.
[ADR-0239](DECISIONS/ADR-0239-every-code-the-compiler-writes-is-registered.md):
every code the compiler writes is registered, once (a correction).
- The declaration rules wrote PW0101 and PW0102 unregistered, and the parser
  PW0102 for a `for` with no `in`: one number, two meanings. A reader's
  value in a shared cache was two errors, PW0100 and PW5001. The registry's
  tests now read what the compiler writes, both ways, as rustc's `tidy`
  does.
[ADR-0240](DECISIONS/ADR-0240-a-clause-is-read-once.md):
a clause is read once (a correction).
- The resource graph split a clause's text at its commas while lowering read
  it with the grammar, and the two disagreed: `emits Searched(")"), Other(1)`
  had no edge to `Other`, which nothing declares. The graph reads the keys
  now. An interface's clause terms are lowered into an arena of their own,
  and every rule that reads a term reads them.
[ADR-0241](DECISIONS/ADR-0241-a-transitions-value-is-the-value-typers.md):
an optimistic transition's value is the value typer's (a correction).
- PW0331 read a transition's type through the older typer, which has no
  answer for a literal, so `optimistic Thing(x) as t => "no"` over an `Int`
  checked. The value typer relates each transition to its target's value
  now, and the older reading is retired: one fact, one detector.
[ADR-0242](DECISIONS/ADR-0242-an-each-head-is-read-once.md):
an `{#each}`'s head is read once, by the grammar (a correction).
- Five places split the directive at ` as ` and `(`: `{#each xs ys as x
  (x)}` and an unclosed key checked, and PW5011 took any `(` for a key. The
  grammar parses the head now, a missing `as` is PW0019, and every reader
  reads its parts.
[ADR-0243](DECISIONS/ADR-0243-a-blocks-statements-are-separated.md):
a block's statements are separated, by `;` or a line (a correction).
- `fn f(a: Int, b: Int) -> Int !{} { a b }` was two statements and checked.
  Two on one line are PW0030 now, except what a block's readers read as
  one: a `return` and its value, a clause's head and the rest of its line,
  and a block after a statement. A clause's word that is no clause where it
  is written, a binding's name or a word in a code body, is refused by the
  names check, and a `;` ends a clause's value there as in the grammar.
- Amended 2026-10-10: markup written on one line is one region, and no
  operand. Two roots on a line at a view's top were PW0009 (found writing
  ADR-0303's refusals), and `<p>a</p> + 1` checked, the sum dropped: a root
  followed on its line by markup goes on as one region, and an operator
  over markup is PW5047.
[ADR-0244](DECISIONS/ADR-0244-wasmtime-48-0-5-and-a-host-that-enables-only-what-it-runs.md):
Wasmtime 48.0.5, and a host that enables only what it runs (a correction).
- CI's advisory job had failed since 2026-10-05 on three Wasmtime
  advisories. Every pin is 48.0.5; the host's engines turn off GC,
  exceptions and the component model's async, each with when to revisit it;
  the Node audit behind it is fixed or accepted with its reason.
[ADR-0245](DECISIONS/ADR-0245-the-heavy-verification-runs-on-github-actions.md):
the heavy verification runs on GitHub Actions.
- The chain and the mutation controls ran serially on one machine. A
  `verify` workflow runs them in parallel shards on Linux, a push the
  recipes it touches and each night all of them, and a recorded command
  fetches a run's evidence, each file naming the run.
[ADR-0246](DECISIONS/ADR-0246-a-feed-on-postgres.md):
the feed's data in PostgreSQL, held to what its source states.
- A `DataLayer` on PostgreSQL beside the in-memory one: a command is one
  serializable transaction, and its events commit in its outbox with its
  writes. The host compares what each source states with what the database
  it opens provides, and refuses to serve on a shortfall.
[ADR-0247](DECISIONS/ADR-0247-a-clause-written-in-a-block-is-judged-by-its-domain.md):
a clause written in a block is judged by its domain (a correction).
- PW0335 judged only a policy heading a declaration; `scope bogus` in a
  resource's block checked. The names check returns the clauses it reads,
  the same table judges each, and a length is a CSS length.
[ADR-0248](DECISIONS/ADR-0248-a-maps-key-is-ordered-and-refused-at-check.md):
a map's key is an `Int`, a `String`, a `Bool`, or an opaque type over one,
refused at check where it is not.
- The owner's ruling 0057-a. `Bool` and opaque keys compile in the
  component and the browser's module; another key is PW0627 where it is
  written or a call instantiates it, not first at build.
[ADR-0249](DECISIONS/ADR-0249-a-verification-shard-sets-up-only-what-its-recipes-need.md):
a verification shard sets up only what its recipes need.
- Setup was most of a shard's time. Recipes of a kind share shards, only
  browser recipes' shards install browsers and only build readers' build,
  the disk is freed in the background, and a run is at most 16 shards.
[ADR-0250](DECISIONS/ADR-0250-let-discards-and-an-acquisition-is-held-or-refused.md):
`let _` discards a value, and an acquisition is held or refused.
- The owner's ruling 0099-a. `let _ = e` binds nothing, and satisfies PW0618;
  a resource bound to `_` is PW2005. Its probes found PW2005 followed only
  `let x = acquire()`: a statement, an argument, a branch, `let h = ..?` and
  a function value's result each passed, leaking what they acquired.
[ADR-0251](DECISIONS/ADR-0251-a-resources-clauses-are-held-to-pw2005.md):
a resource's clauses are held to PW2005 (a correction, found with ADR-0250).
- A `resource` declaration's `acquire` and `release` are terms no statement
  reaches, and PW2005 walked none of them; a component's were walked, but a
  `release` owed nothing. Now what `acquire` makes the resource holds, and
  what `release` is given it ends exactly once on every path.
[ADR-0252](DECISIONS/ADR-0252-a-value-returned-early-carries-its-label.md):
a value returned early carries its label to the caller (a correction).
- A body's label was its last statement's and its `?`s': a secret returned
  from a branch or a loop was public to the caller, and could be logged.
  Each `return` counts now, with the conditions it runs under, and each `?`
  with its own.
[ADR-0253](DECISIONS/ADR-0253-two-tracks-are-built-in-parallel-each-in-files-of-its-own.md):
two tracks are built in parallel, each in files of its own.
- The owner's approval of parallel workers (charter R10). Accounts and
  uploads each get an imported justfile, a block of diagnostic codes, and a
  module of the development server reached at marked seams; the protocol is
  `docs/PARALLEL.md`, and `verify` runs on a track's push.
[ADR-0254](DECISIONS/ADR-0254-a-member-names-what-its-module-sees.md):
a member names the declaration its module sees (a correction).
- A second declaration of a member replaced the first, and `h.destroy()`
  named whichever was registered last, `VendorSdk.destroy` in a module that
  imports only `Maps`. Of several, a member is now the one the module
  declares or imports, or that is declared beside its type; a module that
  sees several or none is refused, PW0628.
[ADR-0255](DECISIONS/ADR-0255-a-materialization-may-read-another.md):
a materialization may read another (ADR-0195's ruling 10, its compiler half).
- `depends_on` names a resource or a materialization, whose arguments are
  checked; a cycle is refused (PW5109), a shared one reading a private one
  is PW5101, and a write reaches each materialization of a chain. A clause
  naming a function or a view, no node of the graph, was taken as it was,
  and is refused (PW5103).
[ADR-0256](DECISIONS/ADR-0256-an-entry-written-with-a-wildcard-is-every-entry-at-the-rest.md):
an entry written `_` is every entry at the rest (ADR-0195's ruling 10, its
other half, and a correction).
- `invalidates Timeline(current_session(), _)` drops the session's
  timeline at every limit: the command computes the values it gives, and
  the host drops the entries at them, whatever the rest. `_` in `emits` is
  refused, and a bare key's repair says how every entry is written. And an
  invalidation dropped a private entry in the committing session's
  partition alone; it drops it in every session's, and tells the others.
[ADR-0257](DECISIONS/ADR-0257-the-follows-timeline.md):
the follows timeline (the integrator's track, ADR-0253).
- `follow` and `unfollow`, idempotent as Mastodon's are; the timeline of
  those you follow, and your own posts, at `/following`; a user's page at
  `/user/{id}`, counting who follows them and whom they follow, with a
  Follow or Unfollow that shows before the server answers. A follow is a
  row the feed's source holds, so PW5106 holds each command that writes one
  to every reader of one.
[ADR-0258](DECISIONS/ADR-0258-accounts-and-sign-in-pleris-is-the-relying-party.md):
accounts and sign-in, Pleris the relying party and the provider the
deployment's (track `identity`, W1; merged by the integrator).
- The relying party is Pleris's: state, nonce, PKCE S256, the callback, a
  session rotated at sign-in and sign-out, `HttpOnly` cookies, a same-origin
  check on every request that changes something, and `requires` evaluated
  (`SignedIn`, `OwnsPost(post)`), a refusal answered 403 with its
  predicate. The provider is the deployment's; a development one, with
  Argon2id passwords, starts only in development on loopback. The feed's
  posts, replies and likes are their principal's, and its author may delete
  a post.
[ADR-0259](DECISIONS/ADR-0259-a-map-or-set-from-outside-is-sorted-on-arrival.md):
a map or set from outside is sorted on arrival (ruling 0057-c).
- A query's map or set, or a host's answer, given out of order is sorted
  by key as it arrives, in the component and in the browser's module, and
  a key twice still stops the invocation. Refusing it tied every host to
  code point order: a database's collation, an order by UTF-16 unit, a
  hash map's. The divergences from the Component Model's `map` and from
  `Map.from_lists`, both keeping the last of a repeated key, are recorded.
[ADR-0260](DECISIONS/ADR-0260-an-image-on-a-post-a-typed-upload-served-safely.md):
an image on a post, a typed upload served safely (track `uploads`, W2;
merged by the integrator).
- A program states its upload, `upload PostImage`, its route, what it
  serves, its bytes, its types and its size, checked (PW5601-PW5603) and
  held by the host; a deployment may lower a limit and never raise one. A
  file's kind and size are read from its bytes, never from what the browser
  says; a signed-in user attaches within an hour's budget, a command claims
  the upload and publishes it with the post or discards it (PW2005 holds
  each path), and only what a post committed is served, `nosniff`,
  sandboxed and immutable.
- Amended 2026-10-25 by ADR-0383: an upload had been kept and served as
  sent, a photo's GPS position among it; it is now kept without its
  metadata.
[ADR-0261](DECISIONS/ADR-0261-a-deleted-posts-image-is-not-served-and-its-blob-is-collected.md):
a deleted post's image is not served, and its blob is collected (ADR-0260's
first question at its merge).
- A blob is served while a committed row names it, as the data layer says
  (`DataLayer::names_blob`), and a layer that cannot say is answered 503.
  Once a delete commits, a blob no post names is deleted, unless a command
  in flight holds a claim to the same bytes, which its post will name.
[ADR-0262](DECISIONS/ADR-0262-a-host-binding-is-an-operation-a-host-provides.md):
a host binding is an operation a host provides (found by the uploads track).
- `host "namespace:package/interface#name"`, each part a WIT identifier,
  held to that where it is written (PW0335). Any string was taken, and one
  with no interface broke WIT generation for every component with an error
  naming no line.
[ADR-0263](DECISIONS/ADR-0263-a-handle-is-the-platforms-to-make.md):
a session's, a user's or an organization's handle is the platform's to make
(a soundness defect, found by W3).
- `Session("…")` checked clean, and so did a command whose session the
  browser supplies, so a program could read anyone's data. No program
  constructs a handle (PW5037), no data layer answers one (PW5038), and
  nothing the browser supplies holds one (PW5039); `current_user()` and
  `current_organization()` are the host's operations, where their bodies
  made one user and one organization of every reader.
[ADR-0264](DECISIONS/ADR-0264-a-sessions-handle-never-reaches-the-browser.md):
a session's handle never reaches the browser (a soundness defect, found
writing ADR-0263).
- A session's id is its `HttpOnly` cookie's value, and a page could print
  it. No query's, subscription's or command's answer, and nothing markup
  prints, holds one now (PW5040); a user's or an organization's id is a
  name, and may.
[ADR-0265](DECISIONS/ADR-0265-a-form-goes-where-something-answers-it.md):
a form goes where something answers it (the identity track's second
question, and the uploads track's fourth).
- The route table states what each route answers: a page a `get`, the
  relying party its four (`/sign-in`, `/sign-up`, the callback, a `get`;
  `/sign-out`, a `post`), an upload a `post` of a file. A form's action
  answers its method or is PW5041, and a link reaches what answers a `get`:
  `/sign-in` no longer PW5009, `/sign-out` PW5009.
[ADR-0266](DECISIONS/ADR-0266-an-exports-parameters-past-the-flat-limit-arrive-in-memory.md):
an export's parameters past the flat limit arrive in memory (found by the
uploads track).
- Past the Canonical ABI's 16 flat values, the host stores an export's
  parameters as a tuple and passes one pointer; each is held where it sits,
  aligned as its type is. Such an export was refused, so a row's record
  could not grow past it.
[ADR-0267](DECISIONS/ADR-0267-a-components-trap-says-why-it-stopped.md):
a component's trap says why it stopped (ADR-0259's "Not claimed", for
ruling 0057-c's "by name").
- Each trap of the component's own calls a function the name section names
  by its cause, `pw-trap: <its words>`, and the host reads the cause from
  the frame the trap stopped in, and Wasm's own by their code: `stopped:
  <cause>: ...`. Every trap had read as Wasm's `unreachable`.
[ADR-0268](DECISIONS/ADR-0268-a-command-outlives-the-page-that-sent-it.md):
a command outlives the page that sent it (found by a WebKit failure on CI).
- A press's speculation is shown before its request leaves, and a link
  followed at once ended the request with the page. Each command's request
  is kept alive now, within the Fetch standard's 64 KiB of such bodies in
  flight, counted in bytes; past it, sent as before.
[ADR-0269](DECISIONS/ADR-0269-a-resource-is-held-by-what-takes-it-apart.md):
a resource is held by what takes it apart (found by ADR-0250).
- A `Result` or an `Option` carrying what a call acquires is held by the
  arm or the `?` that takes it out, `Ok(h)`, `Some(h)` or `let h = r?`,
  which must end it; an arm or a failure that carries none owes nothing.
  A match on it was refused however its arms ended it.
[ADR-0270](DECISIONS/ADR-0270-the-readers-user-is-the-hosts-and-a-listener-binds-a-users-id.md):
the reader's user is the host's, and a listener's handle binds a user's id
(track `notifications`, W3; amends ADR-0091).
- `context.current_user()` is answered by the host from the session's
  principal (`pw:host/principal#read`), and a user's id is the platform's
  `capability.UserId`. In `invalidates_on` alone, a parameter of the
  platform's `User<T>` binds an event's value of type `T`, so an event
  naming a user drops that user's entries in each of their sessions.
[ADR-0271](DECISIONS/ADR-0271-a-reader-is-told-once-per-burst-and-served-in-turn.md):
a reader is told once per burst, and served in turn (found by WebKit's
"Load more" failures).
- Each commit read every other reader's documents again, and a session's
  hold was taken in no order, so under a burst of posts a page's own read
  waited seconds. A reader is told once for every commit waiting, one
  telling at a time, and a session's hold is taken in the order asked.
[ADR-0272](DECISIONS/ADR-0272-a-region-the-browser-fills-while-the-runtime-boots-is-bound.md):
a region the browser fills while the runtime boots is bound (found by the
notifications track on CI).
- The runtime indexes a page and then boots across the network; Chrome 150
  can fill a streamed region meanwhile, which was indexed pending, never
  read again, and its buttons bound to nothing. One that settled while the
  runtime booted is read again and bound now.
[ADR-0273](DECISIONS/ADR-0273-a-materialization-is-a-value-its-body-derives.md):
a materialization is a value its body derives (ruling 10's last piece,
first part).
- One that declares its type derives it in its body, reading what it
  depends on as a page does, `query R(..)`, which the graph takes as its
  edges; `depends_on` beside a body is PW5110. A materialization is a term,
  read by `query`; inside a block, a clause's value ends with its line
  unless it cannot have.
[ADR-0274](DECISIONS/ADR-0274-notifications-a-row-an-act-writes-private-to-its-user.md):
notifications, a row a like, a reply or a follow writes, private to its user
(track `notifications`, W3).
- A notification is a row its act writes in its own transaction, in memory
  and on PostgreSQL; one's own act notifies no one, and a deleted post's go
  with it. Its queries are keyed by the reader's handle, `cache private`;
  an unread count on the home page, a page of them, and mark-read reaching
  each of the reader's sessions.
[ADR-0275](DECISIONS/ADR-0275-a-row-shown-before-the-server-answers-waits.md):
a row shown before the server answers waits.
- A row the page made, `pending-..`, links nowhere and acts on nothing until
  the server's row stands in its place: its author and handle are
  placeholder links, its Like and Delete disabled. The program's own, by
  `waits(id)`; a reply's link waits on ruling 0073-a.
[ADR-0276](DECISIONS/ADR-0276-pw-fmt-changes-no-programs-meaning.md):
`pw fmt` changes no program's meaning (amends ADR-0013).
- A `_` between two digits groups them, `5_000_000`; a policy's value keeps
  its gaps as written; `pw fmt` refuses to write a program whose tokens, or
  a value's gaps, it would change; `} else {` and a clause's continued
  value indent as the tree says; and the feed, the store, the demo and the
  generality cases are held to `pw fmt --check`.
[ADR-0277](DECISIONS/ADR-0277-a-materialization-is-kept-and-a-page-reads-it.md):
a materialization is kept, and a page reads it.
- One that derives its value is a component; each `query R(..)` it reads is
  the platform's read, `pw:host/reads#<R>`, a dependency with no capability,
  answered by the host from R, the body asked again for each read it lacks.
  The host keeps its value as the materializer's entry, makes a chain again
  in its order when an event reaches it, and tells each document that reads
  an entry made again at its key; a page reads a public one as a query.
[ADR-0278](DECISIONS/ADR-0278-a-recipe-run-against-a-database-is-a-shard-of-its-own.md):
a recipe run against a database is a shard of its own (amends ADR-0246 and
ADR-0249).
- Each recipe in `NEEDS_DATABASE` has a shard and a PostgreSQL of its own,
  which the `recipes` job's matrix starts only for it; the database job is
  gone; such shards count among a run's 17 (a track's 9), leaving at least
  one for the rest.
[ADR-0279](DECISIONS/ADR-0279-direct-messages-a-conversation-read-from-each-side.md):
direct messages, a conversation read from each side (track `messages`, W4).
- A conversation is read through private queries keyed by the reader's
  handle and the other's id, so no label names two principals; X's rule
  decides who may message whom (`MayMessage`); a message is a row in the
  command's transaction, in both of the feed's layers (migration 0007),
  shown before the server answers and waiting by ADR-0275's rule; a third
  user, signed in or not, sees none of it, by page, `/pw-read`, cache and
  stream.
[ADR-0280](DECISIONS/ADR-0280-a-handler-navigates-after-its-command-commits.md):
a handler navigates after its command commits.
- `navigate Page(args)`: the page by its declaration, its parameters typed
  as a call's are (PW5042), last in the `Ok` arm of the command's answer
  nearest it, in a handler (PW5043). The runtime refuses an `Ok` that did
  not commit, takes no press and no second navigation once the page
  leaves, answers each press made before it first, and loads the page's
  address, each value one encoded segment: read after the commit, never
  from a cache, with no cache-busting parameter and no reload.
- Amended 2026-10-18: in WebKit, the two tests that navigated as soon as
  the address changed (the chosen address's, the order page's audit) now
  let the page it showed settle first. Their `goto` cancelled requests the
  page had just started, which can crash WebKitGTK's network process (a
  use-after-free in libsoup 3, reproduced on master). The corrections come
  first: it was no flake, and no session's turn held across the estimate.
[ADR-0281](DECISIONS/ADR-0281-a-merge-is-held-to-ci-s-verification-run.md):
a merge is held to CI's verification run.
- A branch is merged when its tip's `ci` and `verify` runs are green,
  failures already open named; locally `just ci`, the changed tests and the
  change's own mutation script once; CI plans a recipe from a changed
  browser spec too; `evidence-fetch` brings the record into docs/evidence
  once the owner approves the download in the integrator's session; what CI
  cannot run stays local and says so.
- Amended 2026-10-12: a fetch takes no file whose recipe reads what
  changed between its run's commit and the tree it joins (the planner's
  rule): the branch merges master and is verified again.
[ADR-0282](DECISIONS/ADR-0282-what-a-value-holds-is-one-label.md):
what a value holds is one label (amends ADR-0118, corrects ADR-0128).
- A secret a declaration answers is held, and one it only uses as a key is
  not; a materialization reads what it depends on; placement reads what a
  declaration holds, as the cache rules read it, in the checker and the
  contract alike; and a placement refusal names the label that rules each
  world out. Four soundness findings of 2026-10-03, each fixed.
[ADR-0283](DECISIONS/ADR-0283-a-components-contract-is-what-its-code-does.md):
a component's contract is what its code does (amends ADR-0078's readers and
E8-0's handler rule).
- A component imports what the functions compiled into it call; placement,
  the source checks and the contract read what a body performs as the row
  check does, function values and members included; and only a handler's
  work is deferred. W6's finding, and a query that read the database
  through a function value or a lambda and could be placed in the browser,
  both fixed.
[ADR-0284](DECISIONS/ADR-0284-a-value-of-any-type-is-fixed-by-the-call-that-meets-it.md):
a value of any type is fixed by the call that meets it (amends ADR-0065).
- A variable bound to a type with parts of any type gets a variable for
  each, so a fold's function fixes its accumulator: the checker refuses
  ill-typed folds it passed, and the backend builds a seed its context does
  not type at the fold's solved type. W6's finding.
[ADR-0285](DECISIONS/ADR-0285-kiokun-parity-inventory.md):
kiokun.com's parity inventory, every route marked (track `kiokun`, W6).
- Every route and feature of kiokun.com's SvelteKit app is a row, marked
  built, partial or missing, held to the app by `scripts/kiokun_inventory.py`:
  102 routes (0 built, 4 partial, 98 missing) and 63 features; the gaps by
  kind, and the order the integrator ruled.
[ADR-0286](DECISIONS/ADR-0286-kiokun-word-page-on-the-development-server.md):
kiokun's word page, a third program on the development server (track
`kiokun`, W6).
- `examples/kiokun-site` serves kiokun's entry for a word as kiokun.com's
  page shows it, from a read-only layer over kiokun's own files; Pleris finds
  the file (the shard rule, kiokun's escape) and answers only for the word it
  records; a stub is followed one hop; what kiokun.com does not show is not
  shown; differences from kiokun.com are stated, its label bug not copied.
[ADR-0287](DECISIONS/ADR-0287-a-handler-is-held-where-it-runs.md):
a handler is held where it runs.
- A page's declared placement grants what the page renders; its handlers'
  work is held where handlers run, in the browser, by ADR-0113's rule, where
  a command they call is a request the command performs. A page built ahead
  may have a button that sends a command.
[ADR-0288](DECISIONS/ADR-0288-kiokun-labels-and-character-header.md):
kiokun's labels for JMdict's codes, and the character header (track
`kiokun`, W6).
- The word page shows a sense's part of speech, field, misc and dialect, and
  a form's note, by kiokun.com's label table read from `KIOKUN_APP`, a code
  the table lacks as it is; the character header shows the learner gloss,
  its levels, the mnemonic's meanings and the readings, each rule named for
  the kiokun.com function it is read from. Found: a host record's field is
  named as its world names it, in kebab case.
[ADR-0289](DECISIONS/ADR-0289-kiokun-loader-merges-and-written-forms.md):
kiokun's loader merges, and the header's written forms (track `kiokun`,
W6).
- A word's files in one batched read; kiokun.com's loader rules in Pleris
  (a stub followed, its variants and related forms merged, an equivalent
  target found), each named for the function it is read from; the header's
  written forms, each with its roles, language, label and meaning, the
  component glosses read from `KIOKUN_APP`.
[ADR-0290](DECISIONS/ADR-0290-a-verification-run-is-dealt-by-the-seconds-its-recipes-last-took.md):
a verification run is dealt by the seconds its recipes last took (amends
ADR-0249 and ADR-0278).
- A recipe's cost is the seconds it last took to its end on CI, kept in
  `scripts/ci_seconds.json` by every evidence fetch (`--times-only` from any
  completed run); one not yet measured costs its mutants at the upper
  quartile of the measured recipes of its kind, browser, host or core
  (amended 2026-10-09: `e14-refusal`, planned at the median, ran its shard
  out of time). Each recipe, the longest first, goes to the shard where it ends
  soonest, what it adds to that shard's setup counted; one that needs less
  may run where more is set up. The recipes run against a database keep
  shards of their own, as many as end the run soonest. Amended 2026-10-10:
  no shard of more than one recipe is dealt past a ceiling under a shard's
  345-minute limit, 10,350 s, half of it since 2026-10-11 (a branch ran its
  recipes up to twice their recorded seconds); a run takes more shards than
  it asks for where its own would (a track's 9 were dealt 21,900 s, and a
  shard stopped at the limit throws the run away). Amended 2026-10-16
  (train-10): of the deals that end as soon, the one whose shards of more
  than one recipe go least past the ceiling; a long recipe alone in its
  shard had hidden three PostgreSQL bundles dealt past it.
[ADR-0291](DECISIONS/ADR-0291-a-hosts-record-is-written-as-the-program-names-its-fields.md):
a host's record is written as the program names its fields (extends
ADR-0172 to the host).
- A host's record is read field by field by the WIT name, or else by the
  program's (`-` read as `_`); a field under neither is refused, naming
  both. W6's finding: kiokun's layer wrote `chinese_char`, as the program
  names it, and the query trapped.
[ADR-0292](DECISIONS/ADR-0292-a-mutation-scripts-processes-are-bounded-in-memory.md):
a mutation script's processes are bounded in memory.
- Every process a mutation script starts, and theirs, holds at most 4 GiB
  (a quarter of the machine's memory where less), watched from
  `mutation_baseline`; one past it is stopped and said before the mutant's
  verdict. A kill by the bound is a kind of its own: each script's last line
  says whether any mutant's run had a process stopped, and the run's summary
  lists those apart. Only the script's own processes are read, and one is
  stopped only where two readings agree it is the script's.
[ADR-0293](DECISIONS/ADR-0293-kiokun-page-head-held-to-kiokuns-own-code.md):
kiokun's page head, held to kiokun.com's own code (track `kiokun`, W6).
- The word page describes itself as kiokun.com's does, its title and
  description made in Pleris by kiokun.com's rules and cut as JavaScript
  cuts, never inside a surrogate pair. kiokun.com's own `seo.ts`, copied
  from the owner's checkout into a temporary directory each run, is the
  oracle: 5,513 sampled words, every title and description the same; CI
  holds a fixture of its answers. A sample of the whole dictionary, served
  and timed, found the renderer quadratic in a list's length.
[ADR-0294](DECISIONS/ADR-0294-a-list-renders-in-its-length.md):
a list renders in its length (W6's finding on kiokun's sample).
- An item's scope shares the page's values by pointer: `Env` holds each
  binding's value, its fragments, its settled streams and its capabilities
  by `Arc`, and entering a scope copies the map of pointers and adds the
  item's own. Each item had copied the page's whole value, the list among
  it, so `n` items over a page of size `m` rendered in `n × m`: kiokun's
  128 names took 215 ms, and 2,000 names took 7.1 s where they now take
  18 ms on CI.
[ADR-0295](DECISIONS/ADR-0295-a-page-at-another-address-of-the-page-is-moved-there.md):
a page at another address of the page is moved there (for track `kiokun`'s
`KiokunError.Moved`).
- `redirect_on Type.Case permanent|temporary` on a page (PW0350): a case of
  one value, of the type of the one parameter the page's route carries, that
  a query the page reads can answer. The host answers it 308 or 307 to the
  page's own route with that value, encoded as a link's hole is, the query
  kept, no origin named, kept by no cache; a move to itself or to a value no
  segment carries is the program's fault (500), and a case the page does not
  name a failure (503).
[ADR-0296](DECISIONS/ADR-0296-a-change-is-derived-outside-the-table.md):
a change is derived outside the table, and what reaches a document is
recorded (WebKit's "Load more", failing on CI).
- A telling and a keyed read derive a document's change outside the host's
  subscriber table, against what the document shows, and push it only while
  the document still shows exactly that, the last attempt inside. Each
  document keeps a trail of what reached it, answered to its own session
  (`/bench/records`), and a failing feed test prints its records, which a
  red mutation baseline shows.
[ADR-0297](DECISIONS/ADR-0297-a-commit-tells-the-pages-that-ask-at-once.md):
a commit tells the pages that ask, and tells them at once (the feed's
"a follow reaches another reader", failing on CI).
- A telling derives only documents whose pages asked in the last three
  seconds (`LIVE`); a document passed by is derived when its page next
  asks, before its frames. The other sessions are told at once, eight at a
  time, each in its own hold. A commit had told every session one after
  another, the documents of pages closed up to two minutes before among
  them.
[ADR-0298](DECISIONS/ADR-0298-the-store-on-postgresql.md):
the store's data behind the seam, in memory and on PostgreSQL (track
`store-pg`, W5).
- The store's operations run through its data layer, in memory or on
  PostgreSQL (`PW_STORE_DATABASE_URL`), with its guarantees stated as a
  source's (ADR-0207), so each DoorDash gap after it is built and tested on
  both. Found: an order dropped its lines, `OrderChanged` was never consumed
  in memory, a menu change's event was committed nowhere, `/bench/stock`
  set a stock no menu knew, and store 48's items were answered not found.
[ADR-0299](DECISIONS/ADR-0299-kiokun-examples-and-pitch-accent.md):
kiokun's examples and pitch accent (track `kiokun`, W6).
- A sense's examples as kiokun.com's `SenseExampleList` shows them, the
  first shown and the rest behind a disclosure; a word's pitch accent from
  kiokun.com's own pitch files, its morae high and low and its pattern
  named, each rule named for the function it is read from. The Japanese
  examples are held to kiokun.com's own code over every 256th entry (an
  oracle run locally, its CI fixture committed). Found: kiokun.com hashes
  its entries' files over code points and its pitch files over UTF-16
  units, which differ past U+FFFF.
[ADR-0300](DECISIONS/ADR-0300-a-build-is-named-by-what-it-built.md): a build
is named by what it built, and the decision holds a document to what its
build says of its page (the integrator's; the first step to the parts two
pages share kept in place).
- `pw build` names a build by an FNV-1a hash over every file it writes
  (`build-id`, `b` and sixteen hex digits); a host serves the build it was
  given, refuses one that names none, and says its name on every document
  and on its handler table (`pw-build`), with each page's document schema
  and scope (`#page|...`); a page's plan carries its scope. The resume
  decision holds a document to what the serving build says of its page,
  never to the build's name, so a deployment that changed nothing a page
  shows keeps the page working. Found: a runtime-recovery mutant survived
  once pages had real scopes; a stale press's recovery is now held to its
  name.
[ADR-0301](DECISIONS/ADR-0301-kiokun-word-page-moves.md): kiokun's word page
moves an equivalent simplified form (track `kiokun`, W6).
- `redirect_on KiokunError.Moved permanent`, ADR-0295's clause, declared by
  the program: where a word's own file is a simplified form that equals its
  one traditional form in meaning, the page is answered 308 to that form's
  page, the query kept, as kiokun.com's `equivalentTraditionalTarget` says.
  Held to kiokun.com's own function over 1,643 words: 3 moves, none
  unexplained, each target a page. ADR-0295's five tests keep every
  assertion, their helpers asserting the clause before they patch it. And
  the word page's tests compile each distinct program once per test process.
[ADR-0302](DECISIONS/ADR-0302-a-refusal-is-told-where-the-press-was.md): a
refusal is told where the press was (the owner's finding, using the feed by
hand).
- A predicate has words: the deployment's (`SignedIn`: "Sign in to do
  this."), or the program's, where it declares them (`predicate SignedIn
  says "…"`, PW0351: one string, not empty, no hole; a `requires` held to a
  declared one's parameters; declared once). The host answers a refusal 403
  with them, keeps it with its press, and refuses to start for a predicate it
  cannot evaluate. The runtime tells it beside the control, named by its
  `aria-describedby`, and says it through the page's announcer, a
  `role="status"` every document holds from its first byte; an unreachable
  command, a stale page and a failed handler are told in the platform's
  words. The feed's composer is a signed-in reader's.

[ADR-0303](DECISIONS/ADR-0303-a-page-is-shown-in-its-layout.md): a page is
shown in its layout, which the pages that name it share (the integrator's,
the second step to the parts two pages share kept in place).
- `layout Name { bindings; view { … <slot /> … } }` and a page's `layout
  Name` clause, each only where a name follows the word (`layout.measure` is
  an effect's). Composed into each page as a view is (ADR-0136), the
  layout's parts and elements numbered first, so the same on every page that
  names it, the page's markup between `<!--pw-slot-->` markers; its bindings
  and signals under `~` names, its handlers one module, its speculation the
  page's with it. A page declares at least its layout's audience (PW5046),
  its privacy label and its graph reads join the layout's, and a query a
  page and its layout both bind is read once for the document. PW0352,
  PW0353, PW5044, PW5045. Found: two roots on one line did not parse; a
  page and its layout read one query twice; a speculation found one binding
  per entry. The store's four pages share `StoreLayout`.

[ADR-0304](DECISIONS/ADR-0304-what-a-page-shows-by-the-clock-is-told-when-the-clock-passes-it.md):
what a page shows by the clock is told when the clock passes it (the
integrator's; the first of the store hours' steps).
- `clock.passed(at)` and `clock.today_in(zone)`, effect `clock.compare`: the
  wall clock read only by comparing it. The host notes the earliest instant
  each read compared, holds a kept entry until then, and reads each live
  document that shows one again at that instant and tells it what changed.
  One clock (the system's, moved by `/bench/clock`, forward only). Refused
  where nothing tells (a page built once, a public materialization). Time
  zones from the host's own tz database (`pw-time`: TZif, RFC 8536, and
  RFC 5545's reading of a skipped or repeated time), `TimeZone`,
  `LocalDate`, `LocalTime`, `Weekday`, effect `clock.zone`. Found: the clock
  was a stub answering 0; a host function whose effect needs no authority is
  never linked.

[ADR-0305](DECISIONS/ADR-0305-a-pages-commands-run-in-the-order-it-sent-them.md):
a page's commands run in the order it sent them (the integrator's, found by
ADR-0306's evidence run).
- Each command names the latest its page sent and has not been answered
  (`pw-after`). The host runs it once that one has run, waits two seconds
  for one not yet arrived, and past that answers it early (409), running
  nothing. What follows a command answered early is answered early too, and
  one closed unrun is waited for again. The runtime sends an early-answered
  command again, naming none, once each before it is answered. This is
  Bayou's monotonic writes; Next.js orders a client's actions one at a time,
  and Replicache by `lastMutationID`. Found: "Clear", pressed after "Place
  order", reached the server first, and the order was refused as nothing to
  order.

[ADR-0306](DECISIONS/ADR-0306-a-navigation-keeps-the-layout.md): a navigation
keeps the layout, and shows the next page in it (the integrator's, the third
step to the parts two pages share kept in place).
- This covers a link's navigation, a handler's `navigate` after its commit
  (ADR-0280), and back and forward between the entries it made, from a page
  that names a layout. The next page is read as a load reads it, and kept
  only where its build (`pw-build`) and its layout's path and schema are this
  page's; otherwise it is loaded whole.
- The swap is one step: the slot's content, each layout region the next
  document renders differently, the layout's elements' attributes, the
  title, description, style and manifest, the address pushed, and scroll and
  focus placed. The page then goes to the next document's runtime, and its
  own life ends.
- Found:
  - a page from the back-forward cache listened to nothing;
  - a soft navigation stopped the browser's own load;
  - three of ADR-0280's tests raced the page;
  - two presses' commands ran out of order (ADR-0305);
  - the verify of `b6e98ef` found two navigate mutants surviving in a
    rule that had a copy on each path out of a page; each rule is now one
    function both paths call.


[ADR-0307](DECISIONS/ADR-0307-a-control-is-shown-where-its-commands-predicates-hold.md):
a control is shown where its command's predicates hold (the integrator's,
the third step of "a refusal is never silent").
- A page asks a predicate of its reader by naming it, `{#if SignedIn}` or
  `hidden={!SignedIn}`. The deployment answers it as it answers `requires`
  (`Identity::answers`), once a document and for the document's life. The
  plan names what a page asks, and the document carries the answers to the
  browser.
- A control whose command requires a predicate with no parameters stands
  where the page asks it and it holds (PW5048). `|refusable` marks one no
  reader's answer decides. A page served to everyone asks its reader nothing
  (PW5049). A name that holds no value is refused (PW0629).
- Found:
  - a name holding no value checked and built, and failed where it was
    served;
  - the feed showed fourteen controls a signed-out reader was refused for;
  - a block decided by a value static for its document was never rendered
    again;
  - an answer read again at each read hid some controls after a sign-out
    elsewhere and kept the rest.

[ADR-0308](DECISIONS/ADR-0308-the-store-searched-in-its-data-layer.md): the
store's stores and dishes, searched in its data layer to one meaning (W9's,
track `store-search`, accepted at the merge).
- A search's terms are its words, each lowered once by Unicode 16.0's simple
  lowercase mapping. A record matches where every term is part of one of
  its fields. A store is listed where it matches or holds a dish that does.
  A tier says where its terms were found. The order, the cut (20 stores,
  5 dishes each) and the bound (100 characters) are `search.rs`'s alone.
- Each layer answers what matched through `Rows::search`: in memory a scan,
  on PostgreSQL two statements with escaped `LIKE` patterns bound as one
  parameter. So the two layers cannot rank differently, and a test holds
  them to answer alike.
- The program's `public query Search(q)`, dropped by any store's menu or
  stock change, and its `SearchPage` at `route "/search/{q}"` until the
  query part lands.
- Found: the menu route's drop told only the store's own pages, and now
  tells each open page that reads what it dropped. The verify found a
  survivor whose dish moved tier on PostgreSQL alone, which the test's table
  did not show: the test now compares every record whole.
- Amended 2026-10-12 (2, W9's): a store's cuisine is its own field, shown on
  its page and the home page's; the home page has a GET search box; and a
  reader's search lists the stores that reach their chosen address, by
  default, saying how many more match and don't (`user query Search(q,
  reader)`); a search is two statements on PostgreSQL whatever the number
  of stores, the layer's statements counted.

[ADR-0309](DECISIONS/ADR-0309-a-pages-query-parameters-are-typed.md): a page's
query parameters are typed, and a GET form makes them (the integrator's; for
W6's and W9's searches).
- A route's query part is RFC 6570's form-style expansion at its end,
  `route "/search{?q}"`. Each name is a page parameter, an `Option` of text
  (PW0630), as an address may leave it out.
- The host reads a query as the URL Standard reads a form's: `+` and `%20`
  a space, the first of a name taken, and a foreign name ignored. It refuses
  bytes that are no UTF-8 (400), where the URL Standard would replace them.
- A link's query and a GET form's fields give only what the route names
  (PW5050). Two routes that differ only in their query are one (PW0341). A
  page is at its route's path: a move (ADR-0295) fills the path and keeps
  the query, and PW5602 compares an upload's paths with a page's path.
- Found:
  - `navigate` cannot carry a form (ADR-0280 keeps it for a commit), so
    W6's first search ruling could not be written. The forms are GET forms,
    with no script.
  - The host's move read a route's query part as part of its path: a move
    to itself became a 308 loop. PW5602 missed a page at a query route.

[ADR-0310](DECISIONS/ADR-0310-a-name-means-its-binding-in-every-analysis.md):
a name means its binding, in every analysis (the integrator's; the first of
"the language's, found on the way").
- `<ready as={x}>` and `<failed as={e}>` bind where they are written.
- A call through a local is that local's. Effect inference, privacy's
  reads and a handler's requests charge no declaration of its spelling.
- Each name a template reads is one binding's. A block binding a name the
  template could already read, and every binding a layout makes, gets a
  name no source writes.
- Found:
  - W8's `<ready as={estimate}>` was charged a module function's row
    (PW0401);
  - a call through a `let` was charged its namesake's row (ADR-0283's
    limitation);
  - the page plan read a loop's `line` as the page's binding `line`
    (ADR-0277's limitation). The store's binding is `line` again.

[ADR-0311](DECISIONS/ADR-0311-a-page-states-its-canonical-address.md): a page
states its canonical address (the integrator's; W6's inflected word page,
and the ruling of 2026-10-09, Q2).
- `<link rel="canonical" href={Page(args)} />`, once at the top of a page's
  view: a page's address, checked as `navigate`'s is (PW5051, PW0604,
  PW0605). PW5035 still refuses one anywhere else, and says where it goes.
- The host writes it into the head, absolute with the deployment's origin
  (`PW_ORIGIN`), never the request's `Host`, and `og:url` from it. A page's
  own `og:url` is refused (PW5034). A production deployment whose pages
  state one is refused at start without an origin, and `PW_ORIGIN` must be
  an origin.
- An address is its own HIR node, never a call.
- Found:
  - lowered as a call, an address was charged its page's effects (the
    store's page did not build) and joined its page's privacy label (a
    shared page naming a session page was refused, PW5001). Exempting
    analyses one by one missed the label; an address is now no call to
    any of them;
  - a soft navigation kept the last page's Open Graph and `theme-color`
    (ADR-0306, amended).
- Corpus C19: A-033, R-063, R-053 changed; generality 45 / 45.

[ADR-0312](DECISIONS/ADR-0312-a-page-and-its-layout-show-one-entry-by-one-value.md):
a page and its layout show one entry by one value (the integrator's; found
2026-10-11, ADR-0191's amendment).
- Where a page's binding and its layout's read the entry a speculation
  targets, the page's is the entry's binding and the layout's its alias: no
  decoder, no transition and no value of its own, its parts, computed values
  and regions rendered from the binding's value under its own name.
- The module exports `aliases`; the host sends the entry once; the runtime
  holds and reconciles it once, and renders it by every name.
- Found: each press ran the cart's transition twice, and each change was
  sent twice, at two versions of one entry.

[ADR-0313](DECISIONS/ADR-0313-kiokun-contains-and-appears-in.md): kiokun's
word page shows Contains and Appears in (W6's, track `kiokun`).
- The layer reads each entry's four lists as `Preview` records. kiokun.com's
  ordering rules are ported as it writes them, an equivalent form's lists
  merged as its loader merges them, and the component glosses read for the
  previews too.
- The first ten of each list are shown, and the rest are behind a
  `<details>` disclosure, as Japanese Names are.
- Held to kiokun.com's own code (`just e14-kiokun-contains`, local): 5,513
  sampled words, the same. CI holds each list's length and its first 20 and
  last 5 items.
- Amends ADR-0285: kiokun-data is read at HEAD. Records the repository's
  kiokun data, cleared.
- Found: a test reader decoded each TCP read on its own, splitting a
  character (the shared helper's fix taken).

[ADR-0314](DECISIONS/ADR-0314-kiokun-search.md): kiokun's search, over the
whole dictionary (W6's; the integrator's rulings B1 to B5 of 2026-10-10).
- kiokun.com's two FTS5 statements run in the data layer as written, over
  migration 0005's schema (`source KiokunSearch`, host operations
  `search#cjk`, `#latin`, `#aliases`, `#variants`, `#canonical`). The
  program keeps the rest: detection, expansion, grouping, the score, the
  cut.
- Ranking stays FTS5's. A platform full-text source waits for a second use.
- A search does not fail on what a reader types: each term is bound as an
  FTS5 string.
- The index is built once per process into `target/kiokun-search/`, keyed
  by the CSV's digest. `is_common` is read as an integer.
- Held to kiokun.com's `GET` handler in Node over `node:sqlite`. CI holds a
  4,375-row sample from cleared sources.
- Amended 2026-10-12 (W6's second packet): the page at `/search{?q}`, with
  kiokun.com's header field as a plain GET form, its empty state, and a CJK
  query that is a word leading with a link to it.

[ADR-0315](DECISIONS/ADR-0315-kiokun-reading-index-and-homophones.md):
kiokun's reading index and the word page's Homophones (W6's).
- The reading statement is the layer's (`search#reading`). The program keeps
  the readings, the limit of 20, the groups and the common mark.
- The section is rendered with the page, needs no script, and groups by the
  readings' order.
- Held to kiokun.com's endpoint, run in Node.

[ADR-0316](DECISIONS/ADR-0316-kiokun-deinflection.md): kiokun's
deinflection, our own rules (W6's; the owner's answer of 2026-10-11, rules
of our own).
- The facts come from three cited CC BY-SA 4.0 revisions (Wikipedia's
  "Japanese conjugation", Wiktionary's verb and adjective appendices).
  Facts only: the structure, names, reason labels and algorithm are the
  rewrite's. Nothing is taken from 10ten-ja-reader's GPL-3.0 table.
- Word classes with their bases, closings and auxiliaries, and up to six
  rounds read back. The resolution is kiokun.com's own: the candidates'
  files, part of speech checked, hiragana retried through the reading
  index, at most five forms.
- The inflected address answers 200 with the first form's entry, a note
  "asked → form (how)" and the other forms, and keeps the address (as
  MediaWiki's redirect note does), not kiokun.com's 307.
- Held to kiokun.com as a black box in Node, never read and never
  committed: 3,873 forms, 0 unnamed differences.
- Amended 2026-10-12: the word page states its canonical link (ADR-0311),
  self-referencing on a dictionary form and naming it on an inflected one,
  with `og:url` from it (ADR-0293's Differences 10 and 12 closed).

[ADR-0317](DECISIONS/ADR-0317-a-component-is-compiled-once-a-process.md): a
component is compiled once a process (the integrator's; W6's finding of
2026-10-09, and the memory bound's stops of 2026-10-12).
- `Prepared::compile` keeps one cell for each component's bytes, for the
  process's life. A second caller takes it or waits for it, and a failed
  compile stays failed.
- Authority is never cached: a call's linker is built from its own grants.
- A component's imports are read from that compile (`Prepared::imports`), by
  the development server, `imports_of` and pw-conformance.
- Found: each server compiled every component twice, and every test's
  server again. The development server's suite, whole, four tests at once,
  held more than ADR-0292's bound after kiokun's merge. It now runs in
  302 s at a 1.56 GiB peak on CI, from about 950 s and over the bound.

[ADR-0318](DECISIONS/ADR-0318-a-cart-and-an-order-are-a-users.md): a cart and
an order are a user's (W8's, track `store-accounts`, milestone 1).
- `user query Cart(reader)` and `user query Order(reader)`, read by
  `current_user()`. Their commands emit `UserCartChanged` and
  `UserOrderChanged`, carrying the reader's handle.
- Anyone fills a cart; a reader signed in places an order. A guest's cart
  joins its user's at sign-in, in one store transaction.
- `user`, a visibility: the user's, and importable, as `session` is the
  session's. `private` is unchanged (the integrator's ruling, queued).
- A record's scope is its scoped producers', joined.
- A user's entry is told to that user's sessions alone (telling by
  principal).
- A command is answered for the reader its page was shown to: a document
  served to another session, or not held, is refused before it runs; a
  press already answered is answered from what was kept.
- Found:
  - a correction to the integrator's ruling of Q1: the superset it accepted
    sent another user's activity to every reader, a timing channel;
  - a record's scope depended on the order of the files;
  - `private` was two things.

[ADR-0319](DECISIONS/ADR-0319-a-delivery-goes-to-the-readers-chosen-address.md):
a delivery goes to the reader's chosen address (W8's, milestones 2 and 3).
- Eleven fixed places, and each store's zone a radius. One rule
  (`places::delivers_to`, haversine) read by the estimate, the page,
  `add_to_cart` and `place_order`, on both layers.
- Out of reach is the program's declared error (`CartError.OutOfRange`,
  `OrderError.NoCoverage`), told in the page's words.
- Addresses are the reader's, at most ten. The estimate is the reader's,
  kept for no time.
- The address page: choosing an address and returning to the store shows its
  estimate with no reload (the owner's Next.js bug).
- Found:
  - a `<ready as>` name resolved as a module function (fixed by ADR-0310);
  - the place table first told the two stores' radii apart nowhere.

[ADR-0320](DECISIONS/ADR-0320-a-block-is-told-part-by-part-while-its-arm-stays.md):
a block is told part by part while its arm stays (the integrator's; W9's
finding, ADR-0308's amendment; ADR-0146 amended).
- The plan names each block at the top of a page that renders a query's
  value, whatever decides it: a query, a value a host computes from one, or
  anything inside that reads one.
- A host tells a block whose rendering changed part by part while it shows
  the arm it showed: text, attributes, captures, keyed lists, inside a
  conditional that decides as it did or a `{#match}` that shows the arm it
  showed (`pw_render::block_changes`). It renders the block again whole
  where it shows another arm, or where something not set in place changed.
- A block a speculation renders is rendered again whole (ADR-0172).
- Found:
  - W9's sentence was never told;
  - a block was replaced whole for any change inside it, its status region
    and fields with it;
  - the feed's unread counts were sent as their whole block;
  - a member read inside any block is refused (NEXT 3c).
[ADR-0321](DECISIONS/ADR-0321-a-related-note-is-shown-in-its-own-file.md):
a related note is shown in its own file (the integrator's; W8's finding).
- A note names its file where it is not the diagnostic's own (`Related::
  in_unit`, resolved to its path by the checker), and `pw check` shows it by
  that file's lines. A span outside its file is told as a note, never an
  internal error. PW5046, PW0338, PW5106, PW0351 and PW0341 note what they
  found in another file; five explanations read as written.
[ADR-0322](DECISIONS/ADR-0322-or-and-and-are-parenthesized-where-they-mix.md):
`|` and `&` are parenthesized where they mix (the integrator's; W9's
finding).
- A chain mixing them without parentheses is refused at parse (PW0031),
  naming both groupings; a chain of one is left to right, as before.
[ADR-0323](DECISIONS/ADR-0323-a-member-read-is-computed-wherever-a-page-reads-it.md):
a member read is computed wherever a page reads it (the integrator's; NEXT
3c).
- A member read of a query's value outside a loop's list is a part a host
  computes once a change, by its path, at the top and inside a block, and
  the browser computes it where it renders a speculated block again.
[ADR-0324](DECISIONS/ADR-0324-private-says-a-declaration-is-not-importable-and-nothing-more.md):
`private` says a declaration is not importable, and nothing more (the
integrator's; W8's finding, NEXT 3a).
- The user's scope is `user`'s alone; `private` on a declaration that holds
  data is refused by name (PW0029); PW0023's repair drops `private`; a
  `user` opening a declaration ends the clause before it.
[ADR-0325](DECISIONS/ADR-0325-an-interactions-answer-commits-with-its-writes.md):
an interaction's answer commits with its writes (the integrator's; NEXT 5).
- The record of an idempotent command's interaction (its key, the arguments
  first sent, its answer) commits in the command's transaction, on both
  layers; a command recalls it before running, a second host's commit that
  meets it answers what the first recorded, and a record answers a day.
- ADR-0281 amended (3), 2026-10-12: tracks ready at once go on a train
  (`track/train-<n>`), each its own merge commit, verified once; master
  moves to the train when its run is green, and a track whose recipe fails
  leaves it, as GitHub's merge queue does.
[ADR-0326](DECISIONS/ADR-0326-a-name-imported-by-name-is-the-files-before-a-whole-modules.md):
a name imported by name is the file's, before a whole module's (the
integrator's).
- An explicit import shadows a whole module's import of the same name; a
  bare call two whole modules declare is refused at check (PW0022), where
  only the build had refused it.
[ADR-0327](DECISIONS/ADR-0327-money-is-rounded-only-where-a-program-names-how.md):
money is rounded only where a program names how (the integrator's; NEXT
6a).
- `Money<C>` opaque; exact `cents`, `plus`, `minus`, `times`, `sum`; a part
  only by `at_rate` with a `Rate` in millionths and a named `Rounding`; a
  phantom generic opaque type crosses as its representation (the
  2026-08-20 ruling amended for that case).
[ADR-0328](DECISIONS/ADR-0328-a-store-is-open-by-its-hours-and-the-clock.md):
a store is open by its hours, read by the clock (W9's; NEXT 4, step 2).
- Hours as data on both layers (zone, cut-off, weekly periods as their
  union, special dates); open or closed computed by the program against the
  clock and told when it changes; an order refused while closed or past the
  last order, as data the page words.
- ADR-0026 amended, 2026-10-13: a failed artifact audit prints its tests'
  output, then exits with their status and writes no evidence.
- ADR-0281 amended (4), 2026-10-13: a push to master plans from the newest
  commit on its first-parent line a push's or the nightly's verify run passed
  at; a train's evidence then plans nothing.
[ADR-0329](DECISIONS/ADR-0329-an-order-is-priced-by-the-program-from-the-stores-data.md):
an order is priced by the program from the store's data (W9's; NEXT 6b).
- Fees, tax, a tip and promo codes from each store's schedule, priced per
  store in named roundings; the shown total held at `place_order`; each
  store's part kept with its rate.
- ADR-0327 corrected: `is_less` was named and not declared (found by W9).
[ADR-0330](DECISIONS/ADR-0330-an-arms-binding-is-read-through-a-member.md):
an arm's binding is read through a member, as a block's value is (the
integrator's; W9's finding in NEXT 6b).
- A case's payload is a step of a planned path; a value of another case
  sets nothing; two arms that would set one path refused.
[ADR-0331](DECISIONS/ADR-0331-a-case-crosses-from-a-browser.md):
a case crosses from a browser (the integrator's; W9's finding in NEXT 6b).
- `Option`, `Result` and declared cases as command arguments, sent as the
  browser holds them and read by the parameter's type.
- ADR-0281 amended (5), 2026-10-13: a shell script a recipe runs plans it;
  master's verify skips the browser suites where only records changed since
  the run that passed.
[ADR-0332](DECISIONS/ADR-0332-a-payment-is-the-providers-word-held-to-its-signature.md):
a payment is the provider's word, held to its signature (the integrator's;
NEXT 6c).
- A signed webhook, read again; a command `invoked_by Payments`, on the
  reader's behalf; an order awaiting its payment, framed, placed by the
  provider's word, taken back by its reader or the provider.
[ADR-0333](DECISIONS/ADR-0333-what-a-page-may-run-is-generated-from-its-build.md):
what a page may run is generated from its build (the integrator's;
ADR-0195's ruling 13).
- Every HTML answer carries a Content-Security-Policy, from the build's
  templates and the deployment; a document's script and style by their
  hashes; every answer `nosniff`; the build refuses a load from another
  origin (PW5052).
[ADR-0334](DECISIONS/ADR-0334-a-command-runs-when-the-clock-passes-its-time.md):
a command runs when the clock passes its time (the integrator's; ADR-0332's
sweep, NEXT 4).
- `invoked_by Clock`; `schedule cmd(args) at|after t` and `unschedule
  cmd(args)`, a command's statements (PW6501 to PW6503), written with its
  writes; each run made as a press is, on its scheduler's behalf, keyed so
  it commits once; the store's abandoned orders taken back at thirty
  minutes.
[ADR-0335](DECISIONS/ADR-0335-wasmtime-48-0-6.md):
Wasmtime 48.0.6, for RUSTSEC-2026-0353 to 0364 (the integrator's; the
downloads approved by the owner, 2026-10-14).
- CI's advisory job failed on every branch from 2026-10-14 on eleven
  Wasmtime advisories. Every pin is 48.0.6; the engine stays as ADR-0244
  configured it; no advisory ignored. Two are reachable by a hostile
  component, and the version closes them.
[ADR-0336](DECISIONS/ADR-0336-a-read-is-a-bounded-number-of-statements.md):
a read is a bounded number of statements on PostgreSQL (W9's; NEXT 4b).
- The store's rows read by set (`= ANY($1)`): stores by id, the stores of
  a set of items, the hours and the coverage of a set of stores. Every
  operation, page and command counted at the seed and with fifty stores
  more, pinned and equal at both; a command's commit counted with it;
  `place_order` pinned by scenario.
[ADR-0337](DECISIONS/ADR-0337-a-read-is-bounded-by-construction.md):
a read is bounded by construction (W9's design note for NEXT 4c, ruled by
the integrator 2026-10-14; nothing built yet).
- A read keyed by a loop's element refused above the seam (PW0405), its
  set read the repair; each source operation's cost per call stated below
  it (`sets`, PW0406) and measured by one conformance suite; the store's
  composites moved above the seam. Built in order: the render merge, the
  checker's rules, the suite, the composites.
[ADR-0338](DECISIONS/ADR-0338-three-databases.md):
one app on three kinds of database, each held to what it gives (a design
note, ruled by the integrator 2026-10-15; nothing built yet).
- An app's data declared in Pleris (`table`), one generic adapter per
  database implementing every app's, no app-specific Rust; one conformance
  suite, the 4c note's; built as tracks D1 to D5 and T1 to T6, the owner's
  decisions 1 to 7 before T3, T4 or T5.
[ADR-0339](DECISIONS/ADR-0339-a-collaborative-document-app.md):
a collaborative document is merged by Automerge, edited in a mounted
ProseMirror, and told on the stream (a design note, ruled by the
integrator 2026-10-15; nothing built yet).
- NEXT's Google Docs-shaped app: Automerge behind `replicated`'s
  `rich_text` policy; Pleris renders the document and an editor's page
  mounts ProseMirror; presence and sync on the stream. Its data declared
  as tables (ADR-0338); its downloads wait for the owner's approval.
[ADR-0340](DECISIONS/ADR-0340-a-read-is-bounded-by-construction-the-checkers-half.md):
a read keyed by its loop's element is refused (NEXT 4c's second step,
ADR-0337's checker; corpus C25).
- PW0405 refuses a source read inside a loop over data whose key comes
  from the loop's element, its set read the repair; `set` pairs a keyed
  read with its set form, held by PW0407; `sets` states a source's cost per
  set read, and PW0406 warns of one that cannot batch. kiokun-site's four
  reads per element are warnings until its layer has set forms and key
  records.
[ADR-0341](DECISIONS/ADR-0341-ratings-and-reviews.md):
a store is rated through its delivered orders, and its reviewers go
unnamed (W9's; the DoorDash customer side's ratings and reviews).
- Each store of a reader's latest delivered order rated once, 1 to 5 stars
  and a comment, final; a store's mean said from ten ratings; its reviews
  shown without their reviewer, public and unmoderated, the shared cache
  holding no reader's value (PW5001); read by set, one statement for a
  list of stores.
[ADR-0342](DECISIONS/ADR-0342-the-claim-measured.md):
the claim, measured: the bug museum and the head-to-head (track `proof`).
- An exhibit is a census leaf with a dated primary source, its bug shown in
  Next.js or SvelteKit with its repair as the control, and Pleris's answer,
  misses first; five exhibits, H07's miss closed by ADR-0340's PW0405 on
  the same train. The head-to-head's protocol, TodoMVC first; its
  downloads wait for the owner.
- Amended 2026-10-22: its findings were read in Next.js 16.3.8; on 16.4.1
  the museum shows the same five, J01's stale page and its repair among
  them.
[ADR-0343](DECISIONS/ADR-0343-a-guide-whose-examples-compile.md):
a guide whose examples compile (track `guide`; NEXT's developer
experience).
- `docs/guide/`, every Pleris block checked by the real `pw` (clean,
  formatted and built, or refused with exactly its codes), the tutorial
  byte for byte the guestbook a browser test drives, the codes' reference
  generated from the registry, and each gap in the checks an example held
  open until the compiler closes it.

[ADR-0344](DECISIONS/ADR-0344-evidence-integrity.md):
six ways evidence could be wrong, or a gate fail for the wrong reason
(track `evidence-integrity`; train-10).
- Stale Python bytecode is discarded; evidence counts and corpus headers
  are checked against their producers; latency bounds thread CPU time;
  suites require complete builds; trains and master hold no unnumbered
  ADR (amended 2026-10-17: a decision's path that evidence recorded
  before its numbering is history). Reports are published atomically.
[ADR-0345](DECISIONS/ADR-0345-each-head-effects.md):
an `{#each}`'s list and key are expressions every walker of a view reads
(track `each-head-effects`; train-10).
- A loop's list and key are semantic expressions. One exhaustive
  enumeration feeds view walkers; effects, privacy, signal and name checks
  cover all seven positions.
[ADR-0346](DECISIONS/ADR-0346-argument-invariants.md):
a value from outside is held to its type's invariant at every boundary it
crosses (track `argument-invariants`; train-10).
- The host holds component arguments before predicates and execution,
  page parameters at arrival, and invariants within maps and sets;
  malformed path text is a 400.
[ADR-0347](DECISIONS/ADR-0347-scheduled-orders.md):
an order is placed for a time its reader chooses, and sent to its stores
then (W9's, track `scheduled-orders`; train-10).
- Paid orders are sent when preparation is due, with windows held to
  every store's as-soon-as-possible admission rule at send time.
[ADR-0348](DECISIONS/ADR-0348-a-set-read-keeps-each-keys-result.md):
a set read keeps each key's result (train-10).
- Set pairs can preserve each key's exact Result/Option, and are held to
  the key by its name (track `pair-key-by-name`, ADR-0340's answers).
[ADR-0349](DECISIONS/ADR-0349-evidence-integrity-2.md):
a failed browser test fails its recipe, and five more rulings on the
evidence (track `evidence-integrity-2`; train-10).
- One browser-suite runner propagates failures; recipes and builds are
  checked behaviorally; the planner follows suite configuration and helper
  imports; mutation program checks cover every root; diagnostic evidence
  is held current.
[ADR-0350](DECISIONS/ADR-0350-tables-declare-an-apps-data.md):
a table declares an app's data and gives its reads and writes (ADR-0338's
D1, track `tables`; train-10).
- Generated reads and writes, schema metadata and migration locks; the
  bounded checker consumes generated pair metadata and source costs.
  Runtime adapters (D2) are next.

[ADR-0352](DECISIONS/ADR-0352-bounded-statements-wait-for-completed-payment.md):
statement counts wait for the completed payment transaction.
- Real delayed-webhook and staged PostgreSQL overlap witnesses establish the
  completion and same-instance write-order defects; exact success pins are
  retained. Strict Cargo producer classification refuses invalid/build/stopped
  evidence with source restoration and no next control. Local strict12/12 and
  fresh CI passed; verify 38909882781 green at the exact head; landed with
  train-9.
[ADR-0353](DECISIONS/ADR-0353-repeated-store-hours-preserve-each-occurrence.md):
a repeated store keeps all of its opening hours (the integrator's local
repair, track `legacy-hours-duplicates`; train-10).
- PostgreSQL's `hours#for-stores` took a store's periods out of its maps
  for the store's first occurrence, so a store asked for twice came back
  the second time with none; they are read by lookup now, the caller's
  order, duplicates and misses kept, on both layers. Two more controls,
  and a Cargo result that is no result is refused, not counted.
[ADR-0354](DECISIONS/ADR-0354-a-render-reads-each-call-once.md):
a render reads each call once, in one snapshot, in a bounded few seats
(NEXT 4c, track `render-merge`; train-11).
- One render's identical calls are made once and read as of one snapshot; on
  PostgreSQL its reads take one of a deployment's seats (`PW_RENDER_SEATS`,
  `PW_RENDER_SEAT_WAIT`: 8 and 30 s where unset, refused at start); the
  table's and a turn's lock-order checks run in every debug build.
[ADR-0355](DECISIONS/ADR-0355-the-committed-components-are-the-builds.md):
the committed components are the build's, every one (the integrator's
defect; train-11).
- `evidence_is_current` holds `docs/evidence/E10/*.wasm` to exactly the
  components the store's build emits (55; three were stale on train-10's
  head), with a control; the planner and the evidence fetch read a binary
  file's change by the lines that name it.
[ADR-0356](DECISIONS/ADR-0356-browser-host-map.md):
a browser run starts only the hosts its selected tests need (track
`browser-host-map`; train-11).
- A registry of the suite's hosts (stride 256), the selection asked of
  Playwright itself, only its hosts started; a spec an optional build cannot
  serve is left out and said, and a guard holds that each runs where its
  build is made.
[ADR-0357](DECISIONS/ADR-0357-mutants-run-in-parallel-slots.md):
a mutation script's mutants run in parallel slots (track
`parallel-instrument`; train-11).
- `PW_MUTANT_SLOTS` deals a script's mutants among worktrees of their own,
  their ports from the browser registry, and says what the script says in
  order; off on CI for now (the integrator's ruling, 2026-10-20).
[ADR-0358](DECISIONS/ADR-0358-kiokun-cache.md):
a Kiokun search cache is published whole, and a reader keeps the version
it opened (track `kiokun-cache`; train-11).
- Each builder stages in a private directory and publishes a complete,
  validated one; final connections are read-only; a builder cleans only its
  own failures and losing stages.
[ADR-0359](DECISIONS/ADR-0359-ci-reliability-traces-latency-incremental-builds-and-the-nightly.md):
a failure's trace is kept, the latency ceiling bounds the fastest check, a
mutant is built incrementally, and the nightly does not repeat a verified
head (track `ci-diagnostics`; train-11).
- `kept-traces/` (32 MiB in all) for each failing browser run and red
  baseline; `check_latency` bounds the fastest of seven or more checks (800
  ms a thousand lines); `PW_MUTANT_INCREMENTAL` on verify's recipe step; the
  nightly skips a head a run passed at, its recipes and browsers, never on
  Sunday.
[ADR-0360](DECISIONS/ADR-0360-the-coverage-guided-fuzzer-measures-or-fails.md):
the coverage-guided fuzzer measures, or it fails (track `fuzz-floor`;
train-11).
- `llvm-profdata` is found through the toolchain's sysroot (`llvm-tools` in
  `rust-toolchain.toml`); a target that covers no region fails the run;
  `e14-fuzz-floor` records E9C's evidence.
[ADR-0361](DECISIONS/ADR-0361-check-is-build.md):
check and build validate one executable program (track `check-is-build`;
train-11).
- `pw check` and `pw build` share one complete validation, the backends a
  build runs, D1 tables composed in; the accepted corpus's four backend gaps
  are refused by both (PW6601); the check's cost is held to the unchanged
  800 ms ceiling by its fastest of seven or more runs.
[ADR-0362](DECISIONS/ADR-0362-guide-producer-validity.md):
guide mutation kills require valid producer outcomes (track
`guide-producer-validity`; train-11).
- The guide's Cargo, Python and browser producers are each validated before
  a mutant counts as killed; an empty, inconsistent or stopped result is
  refused, not a kill.
[ADR-0363](DECISIONS/ADR-0363-wasmtime-48-0-7.md):
Wasmtime 48.0.7, for RUSTSEC-2026-0378 and 0379 (the integrator's, the
downloads the owner's; train-11).
- Every pin is 48.0.7 (28 crates in each lockfile, the CLI on three
  platforms by GitHub's digests); 0378 needs host-made streams the host
  never makes; 0379 (aarch64 Cranelift, a br_table of 256k targets) no
  emitted component reaches, a hostile one could on an aarch64 host; the
  engine's configuration stays.
[ADR-0364](DECISIONS/ADR-0364-a-train-is-pre-verified-locally.md):
a train is pre-verified here, in lanes, before it is pushed (track
`preverify`; train-12).
- `just preverify [BASE]` plans the recipes verify would run and runs them
  in lanes of their own on this machine (a worktree, a target and ports
  each, 12741-14740; the PostgreSQL shared), says a failure or a survivor
  within 15 s of its being written, and records 179 tests and 242 of 242
  mutants killed (`e14-preverify`).
[ADR-0365](DECISIONS/ADR-0365-tests-wait-on-events.md):
a test waits on an event, never on a timer (track `flake-hunt`; train-12).
- The server's tests wait on the query runtime's events (`LIVE` at 30 ms: 41
  of 662 failed, now 0 of 663), the Playwright specs on what the page says,
  and every browser host ends with the runner that started it
  (`serve-while-owned.sh`); the week's flakes ranked, and 293 of 293 mutants
  of 19 scripts killed.
[ADR-0366](DECISIONS/ADR-0366-device-local-storage.md):
device-local storage: a module's signal the device keeps, read before it
is shown, never sent (track `device-store`; train-12).
- `storage device` with `privacy Device` and `conflict last_write_wins`; the
  `Device` label follows the value to each way to the server
  (PW6801-PW6808); versions with pure migrations; tabs agree; decision 7,
  the browser computes from several values and inside a block it renders,
  kept apart from a host's value by a test and three mutants (the
  integrator's ruling 3).
[ADR-0367](DECISIONS/ADR-0367-todomvc.md):
TodoMVC in Pleris, held to its own spec and its own tests (track
`todomvc`; train-12).
- A page binds a signal to its address's fragment (PW6101-PW6106); the
  platform declares a double press and a control's losing focus; an element
  a block shows with `autofocus` takes focus; TodoMVC's official Cypress
  sources remain unchanged; execution uses the explicitly identified pinned
  compatibility copy, and each result is judged by what Cypress printed.
[ADR-0368](DECISIONS/ADR-0368-todomvc-acceptance-means-passing.md):
TodoMVC acceptance requires every official test to pass (the integrator's;
train-12).
- A judge that called a run of 3 of 29 a success was no acceptance;
  acceptance requires all 29 official cases passing in full on the reviewed
  Cypress 15.21.1 pin. The October 20 amendment identifies the exact
  two-patch compatibility execution and preserves every failed old run.
[ADR-0369](DECISIONS/ADR-0369-hosts-share-one-database.md):
hosts share one database: each hears every commit from the outbox, and a
session is the database's (track `second-host`; train-12).
- The outbox is a log each host reads by its own cursor, `NOTIFY` only
  waking them; a command's idempotency is written with its writes; a session
  is the database's, kept as its hash, with an idle and a lifetime expiry; a
  page is numbered by its host's instance, and another host's page is
  answered `another-host`.

[ADR-0351](DECISIONS/ADR-0351-generic-effects.md):
Generic calls preserve solved result types and performed effects (landed with train-12 at `e7664d2e`, 2026-10-21).
- Type checking, member resolution, affine checking and lowering share the solved generic facts; unknown member receivers are refused rather than treated as pure.

[ADR-0370](DECISIONS/ADR-0370-vite-security-update.md):
Vite uses the patched editor launcher on every dependency path (landed with train-12 at `e7664d2e`, 2026-10-21).
- Pin Vite 8.3.3 and launch-editor 2.14.2 across direct and Marko dependency paths, with the existing reviewed audit allowance unchanged.

[ADR-0371](DECISIONS/ADR-0371-node-audit-fails-closed.md):
Dependency audits require a complete, internally consistent producer result (landed with train-12 at `e7664d2e`, 2026-10-21).
- Reject empty, malformed, duplicate-key or inconsistent audit reports and producer failures; the pinned package manager's exit status must agree with its advisory report.

[ADR-0372](DECISIONS/ADR-0372-ci-test-reach.md):
Changed test witnesses reach their verification recipes (landed with train-12 at `e7664d2e`, 2026-10-21).
- Root Rust tests reach package-wide commands, Python unittest selectors retain their tests, and evidence counts only scripts actually executed by a recipe.

[ADR-0373](DECISIONS/ADR-0373-component-namespaces.md):
Component identities preserve declaration namespaces (landed with train-12 at `e7664d2e`, 2026-10-21).
- UI identities use the reserved ~ui suffix across contracts, artifacts, renderer paths and semantic diffs; noncomponent declarations cannot replace executable bodies.

[ADR-0374](DECISIONS/ADR-0374-keyed-retry.md):
Only a current keyed read retries after newer frames (landed with train-12 at `e7664d2e`, 2026-10-21).
- A superseded keyed read ends before frame revalidation can start an unnecessary flight; current reads retain their protection against stale results.

[ADR-0375](DECISIONS/ADR-0375-effect-convergence.md):
Inferred effects reach a fixed point before checks and contracts (landed with train-12 at `e7664d2e`, 2026-10-21).
- Remove the silent eight-sweep limit; deep and cyclic helper chains preserve their effects regardless of declaration order. Bounded scale measurements record the additional work.

[ADR-0376](DECISIONS/ADR-0376-producer-process-ownership.md):
Mutation producers own their child through launch, cancellation and reaping (landed with train-12 at `e7664d2e`, 2026-10-21).
- Defer cancellation while acquiring and cleaning the child; reap its owned process group before source restoration. Native launch-window, running, repeated-signal and timeout controls hold the lifecycle.

[ADR-0377](DECISIONS/ADR-0377-todomvc-cypress-compatibility-copy.md):
Execute an identified TodoMVC Cypress compatibility copy (landed with train-12 at `e7664d2e`, 2026-10-21).
- Preserve the seven upstream blobs, verify exactly two compatibility patches and the runner pin, and require all 29 cases with native status zero. The launcher owns and reaps its actual host; failed and ineffective probes remain recorded.

[ADR-0378](DECISIONS/ADR-0378-bind-capture-control.md):
Binding capture controls exercise diagnostic ownership (landed with train-12 at `e7664d2e`, 2026-10-21).
- A nonserializable invalid target distinguishes the binding exemption from canonical signal filtering, while ordinary handlers retain capture checks. All 14 fault models remain; native builds and timeouts no longer count as mutation kills.

[ADR-0379](DECISIONS/ADR-0379-speculated-arms-proof.md):
Speculation absence names an existing page, and controls require native assertions (landed with train-12 at `e7664d2e`, 2026-10-21).
- Derive the canonical page identity before asserting absence; keep all eight fault models and require complete Cargo/browser receipts. Feed cases remain sequential and continue after failures.

[ADR-0380](DECISIONS/ADR-0380-effect-inference-queue.md):
Effect inference revisits readers only when dependency rows grow (landed with train-12 at `e7664d2e`, 2026-10-21).
- Preserve canonical effect rows, performed metadata and convergence while replacing repeated whole-program sweeps with a deduplicated reader queue. Bounded alternating measurements establish the deep-chain improvement, not an application-wide speedup.

[ADR-0381](DECISIONS/ADR-0381-a-commands-send-is-answered-within-its-deadline.md):
A command's send is answered within its deadline, or it is no answer (track `command-deadline`; verified at `bfda9ae5`, not landed).
- Each send has ten seconds for its whole answer, its body included, and is then sent again as `retry` allows; a send held for the command before it waits its turn, as an early answer does. Every press settles in bounded time; an outcome the page cannot know is still told as "could not be sent".

[ADR-0382](DECISIONS/ADR-0382-a-page-loads-its-modules-within-the-deadline.md):
The modules a page loads when it first needs them are loaded within the deadline, and a failed load is not kept (track `press-modules`; not landed).
- A handler's module, the page's speculation module, its renderer and its computed values load within ADR-0381's deadline; a failed or given-up load is forgotten and asked for again under a new specifier, as a handler's is; a press whose speculation cannot load is sent unspeculated.

[ADR-0383](DECISIONS/ADR-0383-an-uploaded-image-is-kept-without-its-metadata.md):
An uploaded image is kept and served without what names a place, a device, a person or a time (track `upload-metadata`, amending ADR-0260; reviewed 2026-10-26; PR #54 holds its landing record).
- The host rewrites every upload before it keeps it: each format's container is walked, and what decoding, colour, animation and a JPEG's turn need is copied byte for byte (the turn as an Exif holding `Orientation` alone); an ICC profile is kept for its colour without its descriptions, device fields, time or ID; everything else is dropped, and a container that does not parse is refused whole. Not claimed: the pixels, a profile's measured colour tags, data hidden in a JPEG's scan, and images a deployment kept before.

[ADR-0384](DECISIONS/ADR-0384-a-dating-app.md):
A dating app: whose a row is, who else may read it, and what a precise value reveals (track `hinge-design`; accepted 2026-10-25 as the fifth reference app's plan, docs only).
- Every capability a Hinge-shaped app needs that Pleris lacks, as generic platform tracks: rows with owners and per-principal sharing, predicates written in Pleris, table policies that reveal no precise value, bounded rows with host-made cursors, coarse location, ordered private media, push, declared limits, erasure, moderation, verification, subscriptions and typed form bindings; then the app in slices in `examples/dating/`, with no app-specific Rust. Nothing is built by this note, and the three published Hinge privacy failures become museum exhibits, Pleris misses until their protections are real.
