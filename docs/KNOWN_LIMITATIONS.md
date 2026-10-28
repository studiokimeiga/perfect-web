# Known limitations


## Unmerged session repair, 2026-10-26

ADR-0389's synchronous-reader binding is incremental local work. It does not
complete browser-command transaction/receipt fencing, explicit request-local
admission, existing-document/keyed/background read binding, teardown, or session-
specific PostgreSQL/two-host/browser acceptance. Successful-page delivery checks
do not cover query-derived redirects/errors in the unavailable/not_found paths.
Nine legacy mutation anchors were retargeted; anchor uniqueness is not a claim
that their full campaigns ran. See the
[scoped record](diagnostics/session-readers-2026-10-26.md). No production session
migration, publication or merge is authorized by these local results.

Reviewed 2026-09-15 against master `4a9bcddc095b3b4d0eac0e0c2ddb8a0143de03d9`
and the evidence-gate repair. Current milestone state is owned by
[STATUS](STATUS.md); implementation order is in [NEXT](NEXT.md).

The former file was primarily an E0 snapshot and still said no compiler or Linux
CI existed. Its complete bytes are retained in
[the historical limitations file](KNOWN_LIMITATIONS-history-2026-09-15.md).
Those historical statements must not override current source or test results.

## Current integration qualifications, 2026-10-21

The dated sections below retain historical findings. The train-12 source
`ec15e5b1` passed combined x64/ARM CI, the reached hosted campaigns and all
three browser engines. [STATUS](STATUS.md) and the
[acceptance record](diagnostics/train-12/acceptance-record-2026-10-21.md)
name the source and exact runs. Final record promotion still requires CI
and automatic track verification at that commit.

The original TodoMVC compatibility, invalid device witnesses, missing source
census entry, binding and speculation controls have current passing
replacements. Their original failures remain in the ledger. TodoMVC's
Cypress acceptance uses a pinned compatibility copy that repairs the runner
API and detached alias; it must never be called unchanged upstream execution.

**Pending device input is not yet durable.** A device-signal assignment is
written synchronously to localStorage when its queued handler applies the
assignment (ADR-0366, decisions 1-3). Navigation or reload before that handler
runs can discard the pending interaction. This version does not guarantee
durability of unhandled input. Persistence tests must observe the stored
value before reload. The
[privacy-test synchronization repair](diagnostics/train-12/device-store-marker-race-2026-10-21.md)
changes the test's observation boundary, not production persistence behavior;
it is not evidence of a privacy leak.

The accepted-program corpus still has nine exact backend refusals.
Read-only scoping also found that custom tags such as `<map-container/>`
are parsed as a short tag plus an attribute, and a body-level `use`
annotation is not validated like a `let` annotation. A build-time markdown
value needs a tracked build phase; removing the refusal would currently
expose an empty platform stub. A-018's style refusal likewise cannot be
closed by merely building an unmounted component template. These are open
compiler/execution boundaries, not evidence that the examples work. The
[scoping probes](evidence/tooling/train12-backend-gap-probes.txt) retain
their source, native results and revision qualifications.

## Train-10 integration (2026-10-15 to 2026-10-17)

The each-header and argument-boundary repairs below are integrated on
train-10 as ADR-0345/0346; the train's verify is their record. The browser
command decoder already held opaque invariants; the actual gaps were direct
component calls, page parameters, Map/Set contents and a vacuous PostgreSQL
test. Malformed path escapes and non-UTF-8 decoded path text now return
400 before route matching (ADR-0309/0346); valid unknown paths remain 404.
Generic-result effects are a separate unfinished track (ADR-0345's not
claimed: a member read through a generic call's result performs nothing the
effect typer sees), for train-11.

Scheduled orders (ADR-0347) allow one current scheduled order per reader;
hours are held at choice and placement, not at send; an already-due send
runs when payment settles. The times page and chosen-time checkout wait on
the estimator. Splitting scheduling's words from Checkout is queued. The
per-store estimate N+1 found during integration is repaired by a set read,
with three statements at both fixture sizes (ADR-0348). Whole scheduled,
bounded-checker and statement controls pass, as do 24 combined browser tests.
No production-readiness claim follows.

D1's compiler integration now passes local full CI (ADR-0350), but it does
not supply a runtime table adapter. D2 memory execution, provider admission,
TodoMVC migration and common cost conformance remain separate work. Generated
many reads have the compiler's flat-row/key contract; each caller still
selects its own matching rows. No one_call runtime promise is certified by
compiler acceptance alone.

The guide mutator reports 74/74 at these inputs, but private command-status
injection proves it can accept stopped/contradictory output as a kill. That
instrument must be corrected and rerun before a strict producer-validity
claim. The ordinary 12-test/three-engine guide browser result is separate.
Unrelated inherited browser hosts and overlapping slot port ranges remain
open infrastructure issues; the final run unsets dictionary deployment
variables after setup; older inherited-environment records retain their
qualification.

## Value checking and generated execution

**Open findings, each with the track fixing it** (the integrator's,
2026-10-15):

- (Fixed on train-10: an effectful call in an `{#each}`'s head is charged
  as anywhere else, ADR-0345; and an opaque type's `where` invariant is
  held on a command's arguments, a page's parameters and a map's or a
  set's contents, ADR-0346.)
- **`pw check` passes programs `pw build` refuses**
  (`track/check-is-build`): the build runs analyses the check does not (an
  `{#each}` over a computed list is refused by the build alone), so a
  check is green where the build fails.
- **An `<img>` with no `alt` checks clean** (no track yet; ADR-0343): the
  guide holds it as an example that turns red when the compiler closes it,
  beside its programs the check passes and the build refuses. (`&&` is
  refused with the hint that Pleris writes `&` since train-10, ADR-0322's
  follow-up.)

**What the parser does not read yet** (found with ADR-0237, which made
lowering report what it parses):

- **An `{#each}`'s list is built by path** (ADR-0073): every check reads a
  loop's list and key as expressions since ADR-0345 (train-10), and the
  build still reads the list by path, so a pure computed list checks clean
  and is refused by the build (the check-is-build finding above).
- **A clause, a `return` and a block after a statement are statements**
  (ADR-0243, ADR-0038): a block separates its statements around them, and
  their readers read each pair as one. The grammar has no node for any of
  them.

**E9-V1..V6 are met** (2026-09-24, ADR-0031). What the value relations do not
decide is Undecided, counted by `pw audit-values`, and never reported as
agreement:

- **A member of a value of unknown type is not judged** (ADR-0048). Member
  existence is a relation now (PW0610), and a read whose value's type nothing
  states is undecided, as every relation's is.
- **A sum type's case is typed where it is written through its type**
  (2026-09-26, ADR-0059): `Shape.Circle(3)`, its fields, its arity, and a
  case the type lacks (PW0608). A case written alone, `Empty` or
  `Circle(3)`, is the case of the one visible type with it (ADR-0198), or,
  where several have it, of the type expected where it is written
  (ADR-0201); PW0022 names them where nothing expected chooses. Until
  2026-09-26 no case had a type, and `Shape.Bogus(1)` passed.
- **What the value relations still leave undecided** since ADR-0065, which
  typed a value that holds at every type (`None`, `[]`, `todo`, ..): 1 of the
  store's relations, 1 of kiokun's, 17 of the accepted corpus's. Since
  ADR-0178 a listener's `_` is decided, which had left two of the store's
  undecided and one of the corpus's. Each is its own construct:
  - a `measure { .. }` block;
  - a dimensioned literal (`8.px`);
  - a `derived` value;
  - a painter's context;
  - a declared function named as a value (`Money.add`);
  - a handle from a `use` of a host call;
  - a type named as a value (`Element`).

  A declared function's result that its arguments do not fix stays
  unknown. ADR-0195 (ruling 4) rules it typed from what is expected of it,
  and refused as "type annotation needed" where nothing fixes it; not built
  yet.
- **A record literal whose first field is shorthand** (`P { x }`) parses as
  the name `P` followed by a block `{ x }`. A brace is a record only as `{ }`
  or `{ name:`, so that `if x { .. }` stays a block. `let p = P { x }`
  checks and means something else; `P { a: 1, x }` is a record. Found
  writing ADR-0063. ADR-0195 (ruling 1) rules Rust's rule: a type name and
  `{` open a record everywhere but the head of `if`, `while`, `for` and
  `match`, where a record is parenthesized; not built yet.
- **A `()` body discards its last value** (A-018). A branch mismatch in
  statement position is not refused; as a result, it is.
- **Generic layouts at the boundary.** Phantom parameters map to one WIT
  layout; a generic whose layout depends on its arguments is refused, because
  specialization is not implemented.
- **Named function values are not instantiated.** A generic function named as
  a value (`List.map` passed along) has its type parameters as holes.
- **The unit value `()` lowers to `Expr::Error`.** Its type is therefore
  unknown. This is harmless to the relations, which never guess, but it is a
  lowering gap.

A scope/resource policy is now attached to the resolved type rather than its
spelling, preserving the existing type-level policy. This is not a per-value
ownership proof. Private RPCs now require conditional principal preservation;
that records a runtime obligation, not its execution. Stable nominal capture
identities distinguish modules; complete transitive schema-evolution and host
semantic-ABI validation remain separate work. Foreign code and backend internal
record/variant layouts still need their documented integration proofs.

**E10-I is established** (2026-09-24, ADR-0032): the store's commands compile
to components and run through the E8 host, and the Rust closure path is deleted.
The component backend is narrow, and everything outside it is refused by name
rather than approximated:

- **A generic type does not cross the component boundary** (ADR-0062).
  Inside a component and a module, a generic record, sum type or opaque type
  is laid out per instance, its arguments fixed by its fields or its use. In
  a query's parameter or result it has no WIT form, unless its parameter is
  phantom. An instance only a lambda's branches name is refused by name.
- **A type that contains itself compiles** (ADR-0194, ADR-0202):
  `replies: List<Comment>` is laid out by its type, and one held in place,
  `Option<Node>` or `Add(Expr, Expr)`, is boxed. Each crosses a boundary as
  its nodes. Not yet:
  - **crossing a boundary** through another declaration (two types that hold
    each other), held deeper than one `List` or `Option` of itself, holding
    another such type, or as an opaque type: refused by name where its WIT is
    written. Each compiles inside a component;
  - **on the browser's wire** it crosses as its nodes (ADR-0205): a
    signal's value, which a handler reads and sets, a command's argument,
    and a speculation's value, which the server writes by its query's type
    (ADR-0233). Refused by name: a handler's capture of one, a command's
    declared error holding one, two types that hold each other, and a case
    of one as a browser's argument;
  - **nested deeper than 128 in a host** (`NESTED_DEPTH`): a host reads such
    a value's nodes instead.
- **`==` compares primitives only.** Two records, two sum-type values or two
  lists are not compared by either backend, and the refusal is by name
  (`Eq` on a nominal type).
- **Straight-line bodies, at E10-I.** Import calls and scalar constants were
  supported, and values moved flat or in their canonical layout. Matches over
  `Option` and `Result`, field reads and their cases came after (ADR-0036),
  and then computation (ADR-0039, below).
- **The invocation region is the memory strategy** (ADR-0046, measured). It
  is a bump region reclaimed by each export's post-return, and nothing can
  outlive an invocation. A call's peak is its whole allocation: kiokun's
  Search reaches 3.2 MB for the one-letter query `T`.
- **Each call instantiates afresh** (ADR-0032). At 7–14 µs it costs more than
  most kiokun calls themselves (ADR-0046).
- **An opaque value is built and read inside a component** (ADR-0054), as
  its representation retyped, a generic one too since ADR-0062; it crosses
  the boundary as that representation.
- **The data layer is not Pleris.** `store:data/carts` is the deployment's
  (`owner: external`), as the contract records.

**The backend matches over `Option` and `Result`**, reads fields, and builds
their cases (2026-09-25, ADR-0036). **It computes** (2026-09-25, ADR-0039):
`Int` and `Float` arithmetic, comparisons, `&`, `|`, `!`, `if`, string
literals, interpolation, pipelines, records built, and calls to other
declarations, inlined, or compiled beside the export when they recurse, at
the types a generic callee's arguments give it (ADR-0050). An early
`return`, `?`, `for` loops and `let mut` bindings compile (ADR-0051). Still
refused by name:

- a `Float` literal pattern, and an or-pattern that binds a name (nested
  and literal patterns compile to a decision tree since ADR-0060);
- (resolved by ADR-0076) an arm no case reaches, which the checker
  computed and did not report: it is PW0333 now;
- a `return`, a `?` or an assignment inside a lambda a list operation runs,
  and an assignment to a field (ADR-0051);
- a named argument to a standard-library operation, which the checker
  gives to the parameter of its name and the backend refuses by name: an
  operation takes its arguments in order (ADR-0081);
- `%` on a `Float`, and a `Float` interpolated: their semantics are not
  decided;
- list and string operations beyond the ones the standard library declares
  (ADR-0040, below).

- **A trap's cause is read from a function's name** (ADR-0267). Each trap
  of the component's own calls a function the name section names by its
  cause, and the host reads the cause from the frame the trap stopped in;
  Wasm's own it names by their code. A tool that strips the name section
  takes the causes with it, the traps staying where they were, and an
  engine that inlines functions may omit the frame, which the host's
  engine is configured not to. Until ADR-0267 traps were not distinguished
  by cause: an `Int` overflow and a zero divisor read alike.
- **An operand the checker cannot type is undecided** (ADR-0043). PW0609
  refuses operands of two known types that an operator does not take, and
  counts the rest as undecided; the backend refuses what remains.
- **There is no implicit conversion between `Int` and `Float`.**
  `Float.from_int` converts where a count meets a measurement (ADR-0043).
- **The logical operators are `&` and `|`.** `&&` does not parse. They
  short-circuit.

- **Some matches are not analysed for exhaustiveness.** Each is counted
  Blocked by the match audit, with its reason: neither proven nor refused.
  - A `Float` literal pattern.
  - A scrutinee the value relations cannot type, including a name bound at
    two sites.

  A pattern nested under `Some`, `Ok`, `Err` or a declared case, a literal,
  a `Bool`, and a scrutinee no pattern takes apart (a record, a list) are
  analysed since 2026-09-26 (ADR-0060); until then each was Blocked.

  Until 2026-09-25 matches over `Option`, `Result` and calls were not checked
  at all, and four other shapes were proven exhaustive when they were not
  (ADR-0038).
- **A case written alone is chosen by what is written where it is, not by
  its later use** (ADR-0201). Where two types have `Empty`, `let s = Empty`
  then `take(s)` is PW0022: the `let` states no type, and the rule does not
  infer one from `take`. Write `let s: Shape = Empty`, or `Shape.Empty`.
- **A `let` name or a parameter may begin with a capital** (ADR-0197). A
  pattern's name is a case by its capital and a binding otherwise, and a
  case is declared with one (PW0625); a `let` and a parameter bind a name
  whatever its first letter, since their grammar binds nothing else.
- **`return` is a statement, not an expression.** Its value is the statement
  after it, in a block or on its line in a match arm (ADR-0038).
- **A template arm takes one case apart** (ADR-0042, ADR-0061). A template's
  `{#match}` takes an `Option`, a `Result` or a declared sum type apart, an
  arm binding each field of its case. A nested or literal pattern in an arm
  is not read: the runtime has no decision tree. `{:else}` in `{#each}` is refused;
  `{#if xs}` around the list says the same.
- **An interpolated attribute is refused in a `style`**, and a URL with holes
  must begin with text (ADR-0042).
- **A computed value builds from one value** (ADR-0226 to ADR-0229): a text
  hole, an attribute's whole value or a block's subject, at the top of a page
  from a query's value, which a host computes, or a signal's, which the
  browser computes after the host's first; inside a block a host renders,
  from a query's value; and in a loop's row from the row's item, which a host
  computes for each row, and the speculation module for each row it renders.
  These check and do not build (ruling 0073-a, later):
  - one from several values, from none, or from a page's parameter;
  - one the browser would compute inside a block, or one from a query's
    inside a block a signal decides;
  - one in an arm from the names it binds, or in a view that contains
    itself;
  - an attribute from a speculated value, or a speculated row's value from
    a field of its item (a block's subject from one is the module's since
    ADR-0235);
  - a hole in an attribute's text.

  A directive other than `on:` (`style:width={w}`) does not build either
  (ADR-0073).
- **A value computed from a signal shows its last value until the page's
  module has loaded** (ADR-0227): the first change loads it, once per page.
- **A `Float` is not written by a template** (ADR-0074). It has no format
  yet, so `{price}` over a `Float` is refused (PW0609); the host drops a
  `Float` it is given.
- **A mounted resource does not build** (ADR-0075). The template IR has no
  representation for an element that mounts a resource
  (`resource={StoreMap}`, A-007), and refuses it by name.
- **A stream region's limits** (ADR-0148):
  - **Without JavaScript, outside Chrome 150 and later**, a streamed region
    stays its placeholder. The settled arm is in the document, inert, in its
    `<template for>`.
  - **WebKit paints nothing until a page holds about 200 characters of
    text**, or has loaded. A smaller shell is blank in Safari until its
    regions settle, however it is rendered.
  - **A region is rendered once per document.** A change to its query after
    it settles is not sent. Its keyed rows are addressed in the document's
    own domain, not shared as a public fragment's are.
  - **WebKit paints a streaming page only once it says enough** (ADR-0165,
    ADR-0166). While a document is still arriving, WebKit holds its first
    paint until it has more than 200 non-whitespace characters of text, or
    an image over 32 by 32 pixels. A page that says less is shown in Safari
    only when its streams have settled. The store says enough since its
    descriptions (ADR-0166); a compiler cannot know a page's text before its
    data, so nothing checks another page.
  - **A stream sits at the top of a page, or in a view composed there.** One
    inside a block or a loop's row is refused at build, as is a signal shown
    in one, or a view holding one.
  - **`fallback` is not executed for a stream's query**: the plan refuses a
    page with one.
  - **Every page that binds a query is served and kept current by its own
    plan** (ADR-0190), and speculates from its own module (ADR-0191). The
    store's page alone has its shared list, the menu, kept as one fragment
    for every reader, and a change to public data other than the menu
    reaches no open page. A change the store makes to a session's own
    resource reaches its pages (ADR-0193), for the order alone so far.
  - **A query's budget bounds the region, not the query.** The query runs on
    after its region is given the host's failure, and what it answers is
    kept as its policy says.
- **A template shows each case by name; a function need not** (ADR-0153).
  A function a template calls can map a state to nothing with a catch-all,
  and the template shows what it returns.
- **A command a page sends is keyed by its interaction** (ADR-0154), and the
  runtime sends a request again only where no answer came, as its command's
  `retry` declares and at most that many times (ADR-0173); a command with
  no `retry` is sent once. (This entry said it sent each once until
  2026-10-08, after ADR-0173 had made it resend.)
- **A key a page changes** (ADR-0152):
  - **Only a page's own `let` query is keyed by a signal.** A view's signal,
    and a `<stream>`'s query, are not.
  - **A key is a `String`, an `Int` or a `Bool`** (PW5308). A record or a
    list would need a codec.
  - **The key a page shows is recorded per session**, as what it shows is.
    Two tabs of one session that choose different keys are patched as one.
  - **Keyed reads run in a browser for `cancel` only**: the charter's store
    tests 6-8 in Chromium, Firefox and WebKit (`e2e/keyed.spec.mjs`), and
    T07's controls in Chromium for `keep` and `supersede`. The server's tests
    cover each policy.
- **What a page shows is recorded per session** (ADR-0151, as before it).
  Two pages of one session read at once are both served, and the later is
  the one later changes are derived against.
- **A fragment changed at its source reaches the pages when a document
  reads it** (ADR-0150, ADR-0178). When a shared fragment's query value
  changes with no event, the pages open are told once a new document reads
  the change: within the query's freshness, or not at all for a store no
  one opens.
- **`pw diff`'s limits** (ADR-0149):
  - **Effects are the declared rows.** A query's inferred effects appear as
    its component's capabilities.
  - **Client bytes** are the compiled handlers and speculations; the runtime
    is not counted.
  - **No state machines**, since the language has none; a sum type's cases
    are domain changes.
- **A view composes when its body is its markup, its signals and its
  `provide`s** (ADR-0136, ADR-0144). Refused by name (PW5020):
  - a view with other bindings of its own;
  - a prop that is not a value path, a literal included (`label="Add"`).
- **A view that contains itself is an instance made at run time**
  (ADR-0203), each use rendered in a frame of its own. Refused by name
  (PW5020): one with no `{#if}`, `{#match}` or `{#each}` on the way back to
  it, one that holds or provides a signal, and one that shows a `<stream>`.
  A page nests at most 500 elements, and the renderer refuses one deeper
  (`TooDeep`). A change inside an instance renders it again whole: a list
  in it is not kept in place, and what has focus there loses it. An
  instance at the top of a page given a signal and anything else is
  refused, as the browser holds the signals alone.

  A signal given to a view as a prop is shown, not changed: a view's
  handler that would capture one is refused (PW5301). A view changes a
  signal it names, its own or one provided to it (ADR-0144). A view's
  handler that captures a restricted parameter is refused at the view, even
  where every page using it could hold the value. The Marko adapter composes
  no view that holds or names a signal.
- **A process keeps each component it compiled for its life** (ADR-0317):
  a host that loads many builds would keep each one's components. None
  reloads a build yet; a live reload would bound the cells first.
- **The server patches a page's text parts, its lists and the blocks its
  queries decide, by difference** (ADR-0145, ADR-0146). What a loop's row
  reads of another query is refused by the plan. An interpolated attribute
  renders and is not patched, and neither is a shared list other than the
  menu. A query's error is the server's failure, not a value a page shows.
  kiokun's pages are static.
  A block that renders a query's value is told part by part while it shows
  the arm it showed (ADR-0320), and rendered again whole where it shows
  another, where something inside it that is not set in place changed (a
  view's instance, a raw value, an unkeyed list), or where a speculation
  renders it. A host keeps a copy of a document's values where it has such
  a block. A member of a query read inside any block is refused at build
  (`{cart.line_count}` in `{#if cart.lines}`), though a host renders a block
  at the top of the page once: found on the way (NEXT).
- **A clause's words are the clause's** (ADR-0047). Every name resolves in
  lexical scope, but `scope application`'s `application`, or `load`'s
  `on_first_interaction`, is a word of its clause and not a name. A word
  outside its clause's closed set is left to the analysis that reads the
  clause, and most clauses written inside a block have none.
- **The standard library is small** (ADR-0040, ADR-0055).
  - `List` has `length`, `get`, `take`, `drop`, `slice`, `reverse`,
    `concat`, `map`, `filter`, `fold`, `any`, `all`, `find`, `sort_by`,
    `group_by` (by a `String` key, adjacent runs), `sum` and `maximum` (of
    `Float`s).
  - `String` has `length`, `slice`, `codepoints`, `from_codepoints`,
    `starts_with`, `ends_with`, `contains`, `join`, `trim`,
    `to_lower_ascii`, and `to_lower` and `to_upper` (ADR-0056).
  - `Float` has `from_int` (ADR-0043).

  - `Map` and `Set` (ADR-0057), keyed by an `Int`, a `String`, a `Bool` or
    an opaque type over one (ADR-0248), in ascending key order.

  There is no `split` and no `range`. `enumerate` is removed, since the
  language has no tuple type (ADR-0055).
- **A map's key is an `Int`, a `String`, a `Bool`, or an opaque type over
  one** (ADR-0057, ADR-0248), and another key type is refused at check
  (PW0627), where it is written or where a call instantiates it. A map or set
  a query is given, or a host answers, is sorted on arrival, and a key twice
  stops the invocation (ADR-0259); one inside another value is refused,
  since nothing would sort it. A `for` loop reads a map or set through
  `Map.keys` or `Set.to_list`.
- **Case mapping is per code point** (ADR-0056). A word-final capital sigma
  lowers to `σ`, where Unicode's default conversion gives `ς`. There is no
  case folding and no locale rule. The mapping is Unicode 17.0's, as the
  pinned Rust knows it.
- **A member with one declaration needs no import** (ADR-0254). A function
  of another module whose first parameter takes a type is that type's
  member wherever the type is; only where several modules declare one is the
  module's import what decides it, and a module that sees several or none is
  refused (PW0628). A page's planned reads ask as no module.
- **An affine value has no borrow** (ADR-0045). A function whose row does not
  release a transaction may use it, and one whose row does must end it once
  on every path. There is no way to say "this function reads the value and
  hands it back"; a `use` block releases its value and is the scoped form. A
  release inside a loop is refused even when the loop would run once.
- **An acquisition is followed from a name** (ADR-0250, ADR-0269). One is
  held by a binding, `let x = ..`, `use x = ..` or `let h =
  Maps.create(..)?`; by what takes apart the `Result` or `Option` carrying
  it, an arm's `Ok(h)` or `Some(h)`, or `let h = r?`; ended where it is
  made; given to a caller, by a declaration that answers one; or held by a
  resource's `acquire` clause. Anywhere else it is refused, even where it
  would be ended: a bare handle matched, `match h { x => .. }`, a carrying
  value nested in another or bound by an or-pattern, and `let o =
  Some(Database.begin())`, which no release takes. `use _ = ..` is refused, not read as a scope holding a value it
  never names, and so is an acquisition no name holds inside a keyword's
  block, `unsafe.imperative { Maps.create(..) }`. In a clause other than
  `acquire`, a `release`, a key, a `draw` or a transition, an acquisition
  is refused, not followed into what reads the clause's value (ADR-0251).
- **A function value read from a record field is not called** (ADR-0052).
  A function is a value: a lambda or a declaration's name is stored,
  returned, passed and called. But `r.check(5)` reads as a method call. A
  lambda whose use fixes no parameter types is refused by name.
- **A function passed to a generic declaration is refused by name**
  (ADR-0062, found 2026-09-26). `apply(n, x => x + 1)`, where `fn apply<A,
  B>(a: A, f: fn(A) -> B) -> B`, is "a lambda whose type nothing fixes":
  the backend types an argument by its parameter's declared type, and does
  not carry into it what the earlier arguments fixed. A list operation's
  function, and a non-generic declaration's, are typed.
- **A lambda passed before the argument that fixes its type parameter is
  typed by what came before it** (ADR-0053, amended 2026-10-11). In
  `later(xs => xs + 1, w)`, where `fn later<T>(f: fn(List<T>) -> Int, x: T)`,
  `xs` is a `List<?>` and not a `List<Word>`. It is checked by what is known,
  so `xs + 1` is refused, but `xs`'s elements are not typed. TypeScript types
  a context-sensitive argument after the others. Every callback in `pw-std`
  is its function's last parameter.
- **A statement keyword cannot name a value** (PW0013, ADR-0041, ruling
  needed). `let query = ..` is refused rather than read as a `query ..`
  statement at every use.

**kiokun's search** (ADR-0314) runs kiokun.com's two FTS5 statements in its
data layer, over the whole dictionary's index:
- **Its page is at `/search{?q}`, with kiokun.com's header field as a plain
  GET form** (ADR-0314, amended 2026-10-12), and needs no script. There is
  no `/api/search`: a route that answers JSON has no form yet, and the
  header's suggestions and handwriting input are the client's.
- **Its ranking is FTS5's**, not Pleris's: a program cannot declare a
  full-text index or its ranking, and a platform full-text source waits for
  a second use. Whether production loaded `is_common` as text is the owner's
  question (A-027).

**The kiokun slice** (ADR-0037, ADR-0041) is one shard of kiokun.com, with
its logic in Pleris and its data layer in the host:
- **Search covers the loaded shard only.** A lookup reaches every shard.
  kiokun.com searches in SQLite FTS5, and an index over every shard is the
  deployment's.
- **The index is Rust.** Candidate retrieval, case folding and a stub's target
  are the host's, as the database's are kiokun.com's. The ranking is Pleris.
- **The ranking is held to the slice's port, not to kiokun.com's live search**
  (ADR-0041, corrected). kiokun.com's `/api/search` decides which queries are
  CJK differently (hangul yes, astral Han no) and searches script variants.
- **Korean is looked up, not searched.** An entry shows its Korean words,
  Japanese names and character (ADR-0037, amended). The index has Chinese and
  Japanese rows; kiokun.com's Korean rows need its romanization and ranking
  ported first. Pitch accent is not in kiokun's entries: kiokun.com shows it
  from its app's own files (`sveltekit-app/static/pitch/`, 256 of them),
  which the slice does not read (W6's inventory, 2026-10-08).
- **A call is a fresh instance** (ADR-0032). Bulk work needs a query over a
  list, as `shards.Places` is: a call a word made the whole shard's load three
  times slower.

**Resumable handler bodies are compiled** (2026-09-25, ADR-0033), to one ES
module each, and the page's elements carry what the handlers read. A body
computes (ADR-0058): it is lowered as a query's is, and its commands are
awaited in order. What remains:

- **A handler is given its event's record, as the platform declares it**
  (ADR-0138): `press`, `input`, `change`, `keydown`, `submit`. An element
  does not refine its event: a checkbox's `change` gives `value`, not
  `checked`. `bind:value` and forms wait for ADR-0131's later rulings. A
  function or a command named as a handler, `on:input={save}`, is
  `(e) => save(e)` (ADR-0199).
- **A handler reads what its command answered, never the value**
  (ADR-0157): `Result<(), E>`, whether it committed and its declared error
  if not. A handler takes it apart or discards it by name; one that gives it
  to the runtime is refused (ADR-0159, PW0618), and so is a command named
  as a handler, `on:submit={save}`, that answers one (ADR-0199). A command called
  inside a function value, or a function value that reads what the handler
  captured, is refused, and so is a command parameter that is not a
  primitive or an opaque type over one. A command is called by a page's
  handler only (PW0339).
- **The development server hosts the store's two commands only.** A handler
  that calls another command compiles, and is tested under Node.
- **Captures are not a patched part.** An element carries the capture paths
  its handler reads, as rendered. A patch that changes a captured field without
  re-rendering the element leaves the old value there: for example, E7-P's
  keyed rename, which replaces only the item's text. The store's handler reads
  only `item.id`, the loop's key, which a keyed patch never changes.
- **No invariant relates two fields** (ADR-0180): a delivery estimate's
  least minutes are at most its most because its estimator says so.
- **An opaque type's invariant is bounds on an `Int`, or on a `String`'s
  length** (ADR-0179, ADR-0225). `where value >= 1` and `where
  String.length(value) <= 280` are checked at every construction, by the
  build, and at every boundary, by the host. A length is in code points: no
  grapheme clusters, no normalization. Any other predicate is refused
  (PW0623) until a program needs it. A test narrows what it tests in
  an `if`'s branches and across `&` and `|`; a test followed by an early
  `return` narrows nothing after it. A keyed read's key and a route's
  parameter are not held to an invariant: none of the store's is a type
  that states one.
- **A privacy label follows the control flow since ADR-0129.** A branch a
  secret chooses carries its label, and so does a sink inside it. A name
  bound over a labelled collection's elements carries its label (ADR-0063).
- **A call through an unannotated lambda is related to nothing**
  (ADR-0068). `let f = (x) => x + 1` then `f("a")`: the lambda's parameter
  takes its type from no use, so the call has no function type to check. A
  call through an annotated value or a parameter is checked.
- **The name check keeps its own scope walk** (ADR-0066). `names.rs`
  resolves scopes for PW0021 itself, where every other analysis reads
  `crate::lexical`. They differ in one rule: the name check binds a keyword
  statement's first word, to keep a modifier quiet, except the resource a
  `query` or `subscription` names, which it resolves (ADR-0108).
- **A clause's key is checked for names, count and types, not labels**
  (ADR-0088). A secret passed in `emits` or `invalidates` reaches the graph
  unlabelled, and what a key performs (`current_session()`'s
  `session.read`) is not the declaration's. ADR-0195 (ruling 9) rules a key
  labelled by what built it, a secret refused, and what a key performs the
  declaration's effect; not built yet.
- **`requires` is held at the invocation boundary** (ADR-0115).
  Predicate names belong to the deployment's authorization vocabulary and
  command-parameter arguments are carried by index in the component contract.
  The host refuses an export whose preconditions were not evaluated, and a
  deployment evaluator's false, unknown, error or missing argument refuses the
  invocation before the component runs. **The own-renderer development server
  is not a production identity provider:** it deliberately models every local
  demo session as `SignedIn` and knows no other predicate. Production identity
  verification and application-specific predicate implementations remain
  deployment integrations.
- **A policy's value is checked by its domain wherever it is written**
  (ADR-0089, ADR-0247): heading a declaration, or as a clause in a block,
  `observe .. { scope application }` or a `handler_policy { .. }`. **Some
  clauses are read by nothing**, judged and then unread: no analysis,
  generator or runtime names `respects`, `intrinsic_height` or a
  `handler_policy` (RISK_REGISTER's R5 for the last). Where a block's
  clause may be written is not judged either: `respects` in a resource's
  block is judged by its domain and not refused for its place (0047-a's
  full split). A `requires` predicate is checked as deployment
  vocabulary over command parameters (ADR-0115); a `privacy` label
  constructor's name, a length, or
  a `conflict` strategy's field.
- **A `replicated` declaration is parsed, and nothing else** (A-011): no
  syntax reads one, no runtime keeps one, and its `privacy` clause labels
  nothing. A declaration's label comes from its `session` or `user`
  keyword alone, so `privacy User(consumer)` leaves `OrderDraft` public to
  every analysis. Its values are checked by their domains (ADR-0089).
- **Which declarations a policy belongs to is checked for the four graph
  clauses only** (ADR-0092).
- **A cache key is held to every parameter its body reads** (ADR-0107),
  including one read only for an effect such as a trace, whose value the
  entry does not depend on. Telling the two apart needs the flow of values
  to the result; ADR-0195 (ruling 8) rules it computed by the
  information-flow labels, and the rule here stands until it is.
- **A command's write is matched to its readers by domain, not by key**
  (ADR-0101). PW5106 requires a command writing `Carts` to reach each
  cart reader with no staleness window. It does not check that the event
  it emits carries the key of the entry it wrote, and PW5107 does not check
  that `invalidates Cart(..)` names the entry a command speculates on
  (ADR-0105). A reader with a positive
  `freshness` is exempt (A-026), and a row naming only the family,
  `!{ database }`, matches nothing (A-025).
- **A Marko page refreshes a binding only for a query that a command in
  its own module names in `invalidates`** (`marko.rs`). A query reached only
  by an event, or invalidated by a command in another module, keeps its
  first value on a Marko page, and a speculation on it stays. PW5106 and
  PW5107 accept both kinds of reach, and the dev server honours both
  (ADR-0105).
- **A handler names a function or a command, or is a lambda written in the
  attribute** (ADR-0199). A local's function value, React's
  `onClick={handle}`, is refused when checked (PW0614): a lambda written
  where it is used shows what it captures and the signals it writes, and a
  declaration is how a behaviour is shared.
- **A command computes its events and the entries it invalidates**
  (ADR-0208, ADR-0209). Each goes from the command's component to the
  platform, and is acted on once its writes commit. An event carrying a value
  that is not a key (a record, a list) checks, and is refused when the
  command runs. `invalidates Cart(_)` is every entry (ADR-0256); `_` in
  a `depends_on` key is a name, and resolves to nothing.
- **Accounts are a development provider's, and sessions are in memory**
  (ADR-0258). No production provider adapter: no OIDC discovery, no JWKS, no
  ID token signature verification; `identity::Provider` is the seam. One
  issuer; no account linking, email, recovery or sign-in rate limits.
  Sessions do not outlive the process, expire, or get listed and revoked. A
  stale tab's command refused with 403 shows the runtime's generic failure.
  The program reads who its reader is through a data layer op: since
  ADR-0263 `context.current_user()` is the host's operation,
  `pw:host/principal#read`, which no host answers until track
  `notifications` (W3) lands, and `current_organization()`'s no host
  answers at all.
- **An upload's image outlives its post only where it was copied**
  (ADR-0260, ADR-0261): a deleted post's image is served no more, and its
  blob is collected where no post names it, but a copy a browser or a CDN
  kept under `immutable` stays, and a blob a failed commit put stays in the
  store, served to no one. A budget of uploads is the server's memory, as a
  lease is, so a
  restart or a second host forgets it; the development blob store is a
  directory, and there is no production store, CDN, signed address,
  private image, re-encoding or resizing. A post carries one image,
  attaching it navigates the page and loses a draft typed before it, and
  an image is not part of a post's speculation.
- **An uploaded image is kept without its metadata, not without all that
  can identify it** (ADR-0383, amending ADR-0260). The host rewrites every
  upload before it keeps it and keeps, of its Exif, a JPEG's turn alone; a
  PNG or WebP is shown as stored, never turned by an Exif. An ICC profile
  keeps validated standard colour tags, with reserved bytes canonicalized;
  the numeric colour values remain and can fingerprint a calibrated display.
  Producer signatures, vendor flags, unused fields and trailing payload bytes
  are removed or refused. Unsupported nested ICC transforms (`mAB`, `mBA`,
  `mpet`) cause the whole profile to be dropped, so the image may have
  different colour under sRGB. Undefined header colour identifiers or intent,
  non-D50 header illuminants and malformed supported payloads also drop the
  profile. This is not universal ICC compatibility. Nothing in the pixels is touched
  (a face, a street sign, a screen in a reflection), nor data hidden in a
  JPEG's scan or past a GIF image's LZW end. An Ultra HDR or ISO 21496-1
  gain map goes with MPF and what follows EOI, so such a photo shows as
  its standard-range image; a GIF's ICC profile is dropped, its colours
  then taken as sRGB. Images a deployment kept before this are not
  rewritten: the one-time cleaning pass is described (ADR-0383 §3) and
  queued for the production track, not built.
- **The follows timeline is a query** (ADR-0257): read on each render
  from the posts of those followed, not a materialization fanned out on
  write. A reader is told of its own follow by its session, and another
  session of its user by nothing until it reads again; a user's page lists
  their newest 20 posts. No blocks, mutes, follow requests, private
  accounts or lists of followers.
- **A row shown before the server answers waits, as the feed says**
  (ADR-0275). Its id is the page's, `pending-..`; the feed's timeline row
  links nowhere and acts on nothing until the server's row replaces it. A
  reply shown so still links to itself, `/post/pending-reply-{n}`, not
  found: the thread's replies are a view that contains itself, which
  computes no value in its template yet (ruling 0073-a). An id the client
  makes and the server accepts, with which nothing would wait, is not
  built.
- **`pw fmt` keeps a policy's value as written** (ADR-0276). A clause's
  value is tokens, which the grammar gives no expression's nodes, so the
  formatter keeps their gaps as written, one space where there was any, and
  spaces no value as it spaces an expression. It is a canonical spacing,
  not a layout (ADR-0013): it breaks and joins no line. A `_` out of place in
  a number, `5_` or `5__000`, has no message of its own. `examples/history`
  is held to no format.
- **Notifications tell a superset, and are the feed's own** (ADR-0270,
  ADR-0274). An event naming a user reads again every open page of the
  query, not only that user's; another user's page derives from its own
  kept entry and is sent nothing. A like, a reply and a follow notify; a
  mention does not, an unlike there is none of, and there is no grouping,
  no other language, no push and no email. The feed still reads its
  timelines and relations by the session through `identified_by`, and a
  stream reads no `current_user()`.
- **A materialization is kept public, and made again whole** (ADR-0255,
  ADR-0273, ADR-0277). One that derives its value is compiled, kept as the
  materializer's entry, made again in its chain's order when an event
  reaches it, and read by a page; but only a public one (`partition
  public`): one kept per session or user waits for a partition by
  principal, and the follows timeline is built on queries meanwhile. Each
  making recomputes its body whole, asking it again for each read it
  lacks; `regenerate on_read` is not run. One that declares no type is a
  fragment the host renders, the store's still wired by hand.
- **The clock is read by comparison alone** (ADR-0304): no value of "now"
  is kept or shown, so a countdown ("closes in 12 minutes") is not
  written; a handler reads no clock; a page built once and a public
  materialization are refused a comparison, since nothing would tell them
  when it changes. One host's clock, moved forward only (`/bench/clock`);
  a tz database that changes under a kept value is not noticed until the
  value is read again. A host function whose effect needs no authority is
  never linked (`pw-host`'s `linkable`), so the zone reads are granted.
- **A layout is one, unnested, and given nothing** (ADR-0303): a page names
  one layout; a layout is shown in no other, is given none of the page's
  parameters, has no title template of its own, and holds one slot.
- **A page's canonical address names a page without a query part, and only
  `canonical` is hoisted** (ADR-0311): `alternate`, `icon` and `manifest`
  links stay refused, as does a canonical address with a query. Whether
  the named page's content duplicates this one's, as RFC 6596 asks, is the
  program's to decide.
- **A page's query parameters are text, and given by a link or a form**
  (ADR-0309): a list (`{?tag*}`) or a typed number is not read, `{&name}`
  continuation and a literal query part are refused, and `navigate` gives
  no query parameter. A page whose parameter is a query's is reached by a
  link or a GET form.
- **A reader's store search is computed for that reader** (ADR-0308's
  second amendment): filtered by the stores that reach their chosen
  address, its full-text match is no longer shared between readers. It
  splits into a shared `Search(q)` and a per-reader reach once a page can
  compute from two queries (ADR-0226 reads one) or key a query by another's
  value (NEXT). No toggle shows every store, and none is ranked by distance.
- **The store's search folds nothing, and matches each record by its own
  fields** (ADR-0308): no accent or stem is folded, no word is corrected,
  and a store's dishes do not make its own name match. Its meaning is
  Unicode 16.0's simple lowercase mapping, PostgreSQL 18.6's tables, where
  Rust 1.97's are 17.0's. The 28 capitals 17.0 added (U+A7CE, U+A7D2,
  U+A7D4, U+16EA0 to U+16EB8) are matched as written on both layers, never
  lowered, and `on_postgres_every_character_is_lowered_alike` fails when
  either version moves.
- **A command answered for the reader its page was shown to has two
  windows** (ADR-0318, the integrator's review of 2026-10-12):
  - a retry of a press that committed, arriving while its first attempt
    still runs on a page gone stale, is refused, where waiting would have
    answered it with the outcome;
  - a retry later than two minutes after the host forgot the page's
    session is refused as out of date, though its press committed. The
    runtime retries within seconds.
  - A host restarted numbers its documents from 1 again: a page from
    before is refused, told the sign-in words.
- **The store's accounts** (ADR-0318):
  - a private entry keyed by a user's id that no binding fills with
    `current_user()` reaches every reader of it;
  - a guest's lines whose join failed are reachable by no one;
  - a user's order is their latest.
- **The store's reads are pinned, not bounded by construction** (ADR-0336):
  each operation's, page's and command's statements are counted on
  PostgreSQL and pinned, equal at the seed and with fifty stores more. A
  program's read keyed by a loop's element is refused (PW0405, ADR-0340),
  but no bound is computed or emitted for a page or a command, and the
  store's own `set` pair is not written yet (NEXT 4c). Rows are not counted
  (`stores#list` and the home page read every store, as they must), and a
  cart's lines are written a statement each.
- **What PW0405 does not see** (ADR-0340's Not claimed): a write per
  element (refused only where the call also reads); a key traced through
  another read's answer, a `use` or a function value's call; a recursion
  through a function named as a value, which is its combinator's loop; the
  page's own loops (an `{#each}` and its instances, the render merge's); a
  member call that reads, resolved by its receiver's type; and what a
  source costs under throttling, which the conformance suite measures.
  kiokun-site's four reads per element (`search_reading` three times,
  `read_pitch` once) are warnings, listed in `KNOWN_READS_PER_ELEMENT`,
  until kiokun's layer has set forms and the pair takes a list of key
  records. A set form pairs with its keyed read by a field of the key's
  type, any such field, until track `pair-key-by-name` names it.
- **A store's reviews are public and unmoderated, with no merchant side**
  (ADR-0341): a comment is shown as written (escaped, as all text is), with
  no review, flagging, removal or reply, since the merchant and support
  apps are out of scope. A reader rates only the stores of their latest
  order, once and for good; items and couriers are not rated; a store's
  mean is every rating it has, with no window, recency weighting or badge.
  The checker holds what is read through a reader's handle out of the
  shared reviews; a user's id a data layer answered as plain data would
  not be seen, and the store's layer answers none (a test and a mutant).
- **A store's hours look a week ahead** (ADR-0328): a store that next opens
  later says "Closed". A special date replaces its day's hours, never only
  extends them; item-level hours are not kept. On a day a transition skips
  a period's minute, the instant is RFC 5545's (02:30 read as 03:30) and
  the page says the minute as written.
- **The store's places are a fixture, not a geocoder** (ADR-0319): eleven
  places with approximate coordinates; a courier's minutes are four a
  kilometre in a straight line; an address's label is checked for its
  length alone.
- **A page asks only a predicate with no parameters of its reader**
  (ADR-0307): a control whose command requires one with parameters
  (`OwnsPost(post)`, `MayMessage(to)`) is not held to where it stands, and
  the feed's Delete is shown where the program's own `mine` says. A view's
  control is held in the view's own markup, not by a page's `{#if}` around
  its use. An answer is the document's for its life: a reader signed out in
  another tab is told at the next press (ADR-0302).
- **A navigation keeps a layout, not more** (ADR-0306): between two pages
  of one layout and one build, the layout's regions stay; a page of another
  layout, another build, or none is loaded whole (ADR-0280), so the parts
  two such pages share are rendered again. A loop or a view that contains
  itself in the layout is taken whole from the next document, not merged
  row by row. No view transition is run, and nothing is fetched before the
  press (no prefetch on hover). The document a page left lingers on the
  host until it is forgotten as idle (120 s). A whole load the user stops
  lets the page take presses again only where the browser says it stopped
  (Chromium's Navigation API): Firefox 146 has none, and WebKit 26.0's
  aborts nothing, so there the page stays leaving until it is loaded again.
  A press that never answers holds the navigation as long as the browser
  waits for it.
- **WebKitGTK's network process can crash when a navigation cancels a
  page's requests within milliseconds of their start** (found on train-10's
  verifies, 2026-10-18; ADR-0280's amendment).
  - Mechanism: a use-after-free in libsoup 3.x's send path. glibc aborts
    the process (`malloc_consolidate(): unaligned fastbin chunk
    detected`), and the navigation under way fails ("WebKit encountered an
    internal error") or hangs.
  - Seen in Playwright 1.58.0's WebKit build on libsoup 3.0.7 (Ubuntu
    22.04) and 3.4 (CI's 24.04).
  - It reproduces with this program's server, master's included, and a
    60-line script: 3 of 750 such navigations. It does not reproduce with
    a plain Python server serving the same paths (0 of 1,200). What in our
    traffic trips it is open (NEXT).
  - A person cannot navigate that soon after a page appears. The two tests
    that did, right after a soft navigation, now let the page settle first
    in WebKit.
  - Safari on macOS uses another network stack, and nothing here says it
    is affected.
- **A page's commands are ordered by one host** (ADR-0305): the host that
  answers keeps each page's order, so hosts behind a balancer would share
  it or pin a page to one. A command lost on its way from a page that has
  left is never sent again, so the next is answered early after two seconds
  and is lost with it. An answer made by something between the page and the
  host (a proxy's error) costs the next command the two seconds.
- **A link's hole that is `.` or `..`** (found with ADR-0280): the URL
  parser resolves `/stores/..` to `/`, so such a link goes elsewhere, and
  PW5009 matched its text to the route. `navigate` refuses such a value; a
  link does not.
- **A placement names one world** (ADR-0282): a declaration's `placement`
  listing several worlds is read as no pin, and no program writes one.
- **A fragment's placement is read by no runtime** (ADR-0282): the
  materializer keeps it and never reads it, so a fragment runs where the
  host runs it, at the origin; the checker holds what a program declares
  to what it holds, and no work moves to the edge.
- **A secret laundered through a materialization's body** (ADR-0282):
  ADR-0129's summaries follow functions, queries, commands, subscriptions
  and resources, and not a materialization's body.
- **A call through a local is charged its namesake's effects** (ADR-0283):
  the effect inference's call walk resolves a callee's name without its
  scope, so a local `found` that shadows `fn found` is charged `fn found`'s
  row. That over-states, which refuses work rather than grants authority.
  The contract's imports read the scope, and import nothing of `fn found`.
- **A component's imports mirror the backend's resolution** (ADR-0283):
  they are not read off the lowered IR, which exists only for what the
  backend lowers; the build's audit holds the two to each other.
- **Only a fold's seed takes the checker's type in the backend**
  (ADR-0284): another `[]` its context does not type, an argument to
  `List.concat` first among them, is refused as before, and says why.
- **A frame phase's work is held to a page's declared placement**
  (ADR-0287): `post_paint` and `frame` run in the browser too, but only a
  handler's work is held where it runs. No program places a page with frame
  phases anywhere but the browser.
- **A refusal is told in one language, the first failing predicate's**
  (ADR-0302): a program has one set of words, keyed by predicate, and the
  host stops at the first predicate that fails, in declared order. A
  predicate over another's data may tell, by refusing, that the data is
  there. The message is the control's next sibling, and an author cannot
  yet place it elsewhere; a page without its runtime sends no command to
  refuse.
- **kiokun's word page is its words, its labels, its character header,
  its written forms, its head's title and description, its examples, its
  pitch accent, Contains and Appears in, Homophones, and an inflected
  word's dictionary form** (ADR-0286, ADR-0288, ADR-0289, ADR-0293,
  ADR-0299, ADR-0313, ADR-0315, ADR-0316): the mnemonic's text and
  components, and the canonical redirect (`KiokunError.Moved` is declared,
  and a page can answer it since ADR-0295; kiokun's query does not return
  it yet) are still to come; Contains and Appears in send their whole lists,
  the rest behind a disclosure, not read on demand (ADR-0313); Korean is not
  deinflected (`korean-deinflect.ts`, the owner's own, not yet
  reimplemented), and a form kiokun.com knows that the rewrite's own
  conjugator does not ask is not compared (ADR-0316); its canonical link
  and `og:url` are written with the deployment's `PW_ORIGIN`, where
  kiokun.com uses the request's origin (ADR-0316, amended); no JSON-LD until
  std `Json` lands, and no preview image (`og:image`,
  `twitter:card`) until the owner approves an image renderer; its title and description match kiokun.com's own code on
  5,513 sampled words, read raw, where a stub's or a merged form's page is
  held by the server's tests alone; the route is `/word/{word}` until `/_pw/` lands; the tests
  run on the repository's sample, and no page was rendered beside
  kiokun.com's yet.
- **A page is moved only to another address of itself** (ADR-0295): a
  `redirect_on` case fills the page's own route, of one parameter; a move to
  another page, to a route of several parameters, or one that adds to the
  query (kiokun.com's deinflection adds `from`, `conj` and `alt`) is not
  expressed. A redirect is the document's: a keyed read whose query answers
  the case in the browser fails as any read does. The development server
  keeps no move; a deployment's cache policy for a permanent one is not
  ruled.
- **A source's guarantees are held to the database the feed opens**
  (ADR-0207, ADR-0246): the host measures a PostgreSQL feed's isolation and
  whether it may write, and refuses to serve on a shortfall. The store's
  data, and every other layer, are held to the host's own database's
  guarantees, not measured. Not counted either: a command's several writes
  to a source without transactions, which commit apart, and its reads
  across two sources, which no transaction makes one snapshot.
- **The feed on PostgreSQL is one host's** (ADR-0246): delivery is the
  committing host's, so a second host on the same database hears nothing
  of the first's commits; a host's commands are serialized by its layer's
  lock, so a serialization failure between concurrent writers is refused
  as any refused commit, and never measured; and the browser suite runs on
  the in-memory layer. An interaction's answer commits with its writes
  since ADR-0325.
- **A `style` attribute's value is escaped by refusing what executes**
  (`expression(`, a script scheme in `url(`), then as an attribute. A style
  can still load a URL the value names. A `<style>` element holds text only
  (ADR-0094), and a stylesheet's braces open holes, so none is written.
- **A `<link>` in markup may name any origin** (ADR-0189): a relation the
  body allows, a stylesheet or a preconnect, from anywhere. A link the head
  holds is refused in markup (PW5035), and `http-equiv` is the host's
  (ADR-0186). ADR-0195 (ruling 13) rules an `href` a literal or a build's
  asset, a cross-origin one's origin declared by the program, and the host's
  Content-Security-Policy generated from that list. Both are built
  (ADR-0333): a page loads from its own origin and its deployment's providers
  alone, and the build refuses a load from another origin written in
  markup (PW5052). A program's declaration of another origin is not: none
  can be made, until a program needs one.
- **A single patch that addresses nothing is ignored** (ADR-0168): a patch
  set that does is refused and the page read again, but a lone
  `ReplaceText` or `SetAttribute` is not. It hid E7-P's scrambled list until
  2026-10-03.
- **A member read is computed for a page in two places only** (ADR-0169):
  text at the top of a page, from a query's value (ADR-0125), and a row of a
  loop over a query's list, from its item. Anywhere else it is a build
  refusal:
  - an attribute, or what a block decides by, at the top of a page;
  - a loop over a field of a row's item, or over a stream's answer;
  - a signal's member, which the browser reads by field.
- **`display` writes US dollars as en-US does** (ADR-0169). Other locales
  and currencies have no `display`.
- **A press is sent only once its handler's code has arrived** (ADR-0268).
  A command's request outlives its page, kept alive within 64 KiB, but a
  press whose handler is still loading when the page is left is never
  made, and a request's answer whose page is gone is read by no one: a
  refusal is not shown, and the next page reads what is. A page whose
  other scripts keep bodies alive can find a command refused by the
  browser's bound, which this runtime does not see.
- **A speculation reaches the top of a page** (ADR-0122, ADR-0172): its
  text, and each attribute, block and loop there that reads the speculated
  value, which the browser renders again. A speculated read inside another
  block, or a list inside a row, is a build refusal. A region renders from
  the speculated value, the page's signals and its own names; one that
  reads anything else is refused.
- **A browser sends a record field by field** (ADR-0172), each field named
  as the host finds it: a record with a field whose name would come back as
  another, `opensMinute`, is refused. A variant is not sent.
- **Availability is a `Bool`** (ADR-0178), as charter §15.1 declares: no
  count of what is left. The store's recommendations are not asked again for
  a stock change, and may name an item sold out since; a cart line is not
  marked when its item sells out, and the command refuses a further
  increase.
- **A page does not say what it shows was the last kept** (ADR-0177).
  While a public query's origin fails, its last value is shown, and nothing
  tells the reader it is from before. `fallback empty` is checked and read
  by nothing.
- **A command's send is given up at ten seconds, and what it then knows is
  limited** (ADR-0381, which replaced "a command request that never settles
  remains pending" of ADR-0173). Each send has ten seconds for its whole
  answer, its body included, and is sent again as `retry` allows; a press
  therefore settles within its sends' deadlines, but:
  - a press whose sends are all given up is told it "could not be sent"
    (ADR-0302's words), though it may have reached the host and committed;
    the page shows what its subscription brings, and nothing says the
    outcome was unknown;
  - the host never hears that a page gave up: a command still running, or
    arriving late, can run after the next command its page sent, naming none;
  - the host's own side has no deadline (a command's run, the session's turn,
    the database pool, the wait for a running predecessor), and each resend
    of a request it is still running parks another thread, at most `retry`'s
    `max`;
  - a resend that arrives while its first is still running, from a document
    the host no longer holds, is refused rather than answered (ADR-0318);
  - a command declares no `timeout` (PW5105): the deadline is the platform's,
    and a link's `?deadline=` can lower it to half a second on that page;
  - a refusal's or an early answer's body that stalls (403, 409) is told as
    a refusal, as one cut off was;
  - a hidden page's timers fire late, so the bound is the browser's timer's;
  - a module a press needs is bounded by the same deadline since ADR-0382;
    before it, one that never loaded held its press and the presses after
    it (ADR-0152).
  A rejected request that exhausts its retries is reported on the control
  and by the page's live status announcer (ADR-0173's correction).
- **A page's on-demand modules load within the deadline, and a failed load
  is forgotten** (ADR-0382, which replaced "one failed load of a page's
  speculation module fails every later press that speculates"). What
  remains:
  - a press whose speculation module cannot load is sent unspeculated: the
    reader sees its result only when the server's frames come;
  - a press's failed or given-up load is told "This did not work. Try
    again." (ADR-0302's `failed`), whether the module failed or never came;
  - the computed values have no deadline: a change whose computed values
    failed to load, or have not come, renders them, and the parts after them
    in that change, only once they load.
- **A device value whose migration cannot load the page's computed values is
  lost to the page** (found by the 2026-10-21 review; not repaired; track
  `device-store`'s, ADR-0366). `readKept` marks it `unreadable`; the first
  write then stores the new version's key, and the value under the older
  version's key, which was never migrated, is read by no later page. A
  failed boot load of the computed values (a dropped request) is enough.
- **Each Add carries the whole item** (ADR-0172), its description too,
  though the command reads its id and the transition its name and price. A
  narrower record is the program's to declare.
- **A list inside a shared query's value is rendered once** (ADR-0170). It
  is rendered with its document and not patched again. Only the menu's
  public fragment is patched for every reader (E7-P), its categories' lists
  inside it included (ADR-0181).
- **An unkeyed list inside a row renders the row again** when it changes
  (ADR-0181): its instances have no address. A keyed one is changed where
  it is.
- **A comment in markup is `<!-- -->`** (ADR-0167), and no page renders it.
  HTML's other bogus comments, `<!x>` and `<?x>`, are read as elements. A
  line of markup text that begins with `//` or `/*` is refused (PW5028),
  except in a `<style>` or a `<script>`.
- **A hole cannot hold a string** (ADR-0049). `"{f("a")}"` ends the outer
  token at the inner quote. Escapes are defined, and every backend reads one
  decoder; policy strings (`because`, `route`, `host`) are read as written.

## Research requirements are not implemented guarantees

The census records temporal authority, unknown external outcomes, compatibility
across live versions, composed capacity budgets, optimistic overlap, and unmanaged
browser/foreign behavior. Some related mechanisms already exist, but the census
itself does not prove complete enforcement. Read the per-record status and the
[research/implementation boundary](../research/failures/IMPLEMENTATION.md).

## Evidence and test coverage

**What train-10's first verify left in the evidence's harness** (the
integrator's rulings of 2026-10-17):
- `e14-refusal` runs as two recipes (`e14-refusal-2`, ADR-0302's
  amendment): each of its browser mutants costs about 30 minutes on CI,
  the whole past a job's 345 minutes, until the browser host map
  (train-11) starts only the hosts a spec needs; dealing any script's
  mutants across shards is train-11's.
- `check_latency`'s ceiling per thousand lines is set by CI's slowest
  runner, so on a fast machine it is some six and a half times the
  measurement: a slower checker passes there and fails only on ARM's CI.
- `e14-harness` is local: its sandboxes install offline from the
  machine's pnpm store, which no runner holds (CI runs its Pleris stack,
  `e14-pleris-controls`).
- A browser recipe of three runs fails on one failed run. `e10-browser`'s
  WebKit `store-accounts.spec.mjs:173` failed once in three runs on each of
  verifies 39054436657 and 39091059482. This was not a flake: WebKitGTK's
  network process crashed (the entry under the browsers' navigation above).

**What train-10's landing found in the evidence's harness** (2026-10-19):
- `evidence_is_current` holds 14 of the 20 committed components
  (`docs/evidence/E10/*.wasm`) to the compiler, by a list kept by hand,
  and the development server's unit tests run the committed ones. A
  component left out can go stale unseen: the store's three hours
  components did on train-10, and 107 server tests ran the old code until
  they were regenerated before it landed. The other three left out
  (`store.page.MenuLine`, `store.page.MenuSize`,
  `store.reviews.StoreRatings`) are what the compiler builds at the
  landing. Train-11 derives the list from the build.
- The complete check (check-is-build, ADR-0361) refuses four accepted
  programs, listed with their exact messages in
  `examples/accepted/build-refusals.json`. Three use features the build does
  not support yet: an element that mounts a resource (A-007), a page value
  computed at build time from a workspace file (A-013), and `style:`
  directives (A-018). One is a bug: the WIT generated for A-008 names an
  item twice (`store-recommendations-recommendations-api`); NEXT queues it.
- A binary file is planned by the lines that name it (ADR-0355); one a
  program names only at run time, with its directory nowhere on the line,
  is found only by the line that names the directory.
- Seven recipes train-11's corpus gate reached have no evidence since it
  (`e14-ids`, `e14-clause-heads`, `e14-opaque-value`,
  `e14-resource-clauses`, `e10-callbacks`, `e10-members`, `e10-names`): the
  next verify that plans them records it.
- `stream_boot_mutations.py` runs only where Google Chrome is installed: on
  CI's runners, not in the PC's WSL.

**Findings about the evidence itself, fixed on train-10** (ADR-0344,
ADR-0349): stale Python bytecode no longer decides a mutant's verdict; an
evidence file's counts are held to its script's; the corpus's `@rule`
headers to the codes `pw check` reports; `check_latency` bounds the
checking thread's CPU time, not the wall clock; and a failed browser suite
fails its recipe. What they do not claim stays in those ADRs: a mention of
a decision written in backticks is read as one about the placeholder, and
a decision's path recorded in evidence before its numbering is read as
history (ADR-0344, amended 2026-10-17).

The new recipe tests replace producers in isolated temporary trees. They prove
exit-status propagation for the tested recipe shapes, not compiler correctness,
browser behavior, host isolation, performance, or production readiness.

`errexit` and `pipefail` do not make shell execution universally fail-safe.
Independently invoked scripts and shell contexts that intentionally handle errors
need their own tests. Redirected reports may be incomplete after failure; this
repair does not make report publication atomic or validate every printed claim.

No new real-device, screen-reader, cross-browser, database-fault, or production
rollout test was run locally in the September 15 review. Prior observations keep
their original scope. The false-green recipe defect does not prove old tests
failed, but a recipe's zero exit status alone was insufficient evidence.

**`optimistic` and `idempotent_by` are executed since 2026-10-02**, each
within stated bounds (ADR-0120 found both unexecuted):
- `idempotent_by` (ADR-0121, ADR-0325): once per interaction, its answer
  committed with its writes and answering a day; a retry a day later is a
  new request, and a replayed answer is not marked as one;
- `optimistic` (ADR-0122): in the own renderer only, for a part outside any
  block, where the page binding's key is an invocation-context call. A new
  cart line takes the name and price the page showed (ADR-0172), and the
  server's answer replaces it: a speculation never invents a value
  (ADR-0195, ruling 14).
E14's shared store contract still excludes both, for every stack.

**A signal lives in a page, a use of a view, or a `provide`** (ADR-0133,
ADR-0144). Each use of a view holds its own instance. A view used in a
loop's row holds none yet (PW5307): each row would need its own, kept by the
row's key. A first value is data, not a computation. The browser sets a text part or an
attribute in place outside any loop, at top level or in a block a signal
decides, and renders such a block again for the rest (ADR-0142). The plan
refuses a signal read anywhere else, and a block it renders that reads
anything but the signals and its own names (ADR-0137). `bind:value` binds a
signal of `String`; another type waits for a codec (ADR-0131). So a dialog over a query's value, an
attribute a signal decides, and a signal's list outside its block, wait for
the browser to hold more than the signals. Until 2026-10-02 two of these were
listed here as refused, and built. A block renders again whenever any signal it reads changes, even when
the arm shown does not read it. The browser's renderer is 90 KB gzipped,
loaded on the first block a press renders.

**A modal dialog is a `<dialog>` a signal's block renders** (ADR-0141), the
browser's own, and it handles `close`. A dialog's return value from a
`<form method="dialog">` waits for forms (ADR-0131). A dialog in a view is
shown by the view's own signal or one provided to it (ADR-0144). A page
that reads queries holds signals on the store's route only (ADR-0140); any
other page with queries is not served yet (E14-Q).

**A response's caching is said for a session's responses alone**
(ADR-0184). A session's response says `private, no-store`, and a build's
file names no session but says nothing of how long it may be kept, which is
the deployment's. A page declared `cache shared` is not served yet, so no
response says `public`. The engines in the browser suite ask for a page
again on going back whatever it says, so `no-store`'s effect on a browser
that restores pages is held by its header alone.

**A page states its title only when it is served at a route** (ADR-0183).
PW5029 refuses a routed page that states none. A page without a route is
titled by the host that serves it, as before: the demos' pages by their
names, and the benchmark's store "Store". A title reads a page's parameters
and its queries' values; on a page that binds a query, its parameters since
ADR-0231. Refused at build:
- one that reads a signal;
- one that reads a value a press speculates.

**A page's parameter is read where a host renders** (ADR-0231). Every page
is rendered with its parameters, each one text as its address gives it. A
text part, a title, an attribute, a handler's captures and an instance's
argument read one. The browser holds them only where a speculation renders:
- a region it renders again with a speculation reads them, since a
  speculating document carries them (ADR-0236);
- one read inside a block a signal decides is refused at build (ADR-0137).

A value computed from one waits on ruling 0073-a. Its representation,
`{id.value}`, is read as any opaque value's is (ADR-0232).

**An opaque value's representation is read by its own path** (ADR-0232):
`{p.id.value}` is `p.id`, as the checker types the base. A `.value` neither
the checker's types nor the value relations type the base of is read as a
field, and does not render.

The store has no change event of its own, so its title, like its heading,
changes when the page's values are next read. A title, or metadata, that
reads a member function is refused where the page is planned: a host
computes one in a text part of the page's body, or a loop's row.

**The runtime is sent as it is written** (ADR-0188): comments and all, about
45% of its script's bytes. Its bounds are on Brotli's compression of it, as a
static host sends it. Minifying it waits on the owner (a minifier is a
download). The development server compresses nothing.

**The store contains a menu item only far down its page** (ADR-0187): when
28 items precede it in its list, or 16 lists precede its list. That is at
least 2,400 CSS pixels at 16 px text, in a single column. Not covered:
- a grid of items, which puts the 29th item higher;
- a first screen taller than 2,400 CSS pixels;
- sixteen categories with no items, about 1,300 px;
- text smaller than 16 px.

**A page's metadata is a name and a content** (ADR-0186). A page states its
description and Open Graph properties with `<meta>` at the top of its view.
They are written into the head as the page is served, and set again by
nothing. Not supported:
- `media` and `lang` on metadata, which are refused. So a page has one
  `theme-color` for light and dark alike.
- A `<link>` in the head: a canonical URL, an alternate language, an icon.
- Structured data as JSON-LD.
- Microdata's own rules. A `<meta itemprop>` is written where it is, and
  whether it is inside an item is not read.

No page is required to state a description.

**A page writes a `<link>` only where HTML allows one in the body**
(ADR-0189): its relations body-ok, or an item's property. It cannot state a
canonical or an alternate address. A `<style>` in markup is written where it
is, into the body, where browsers apply it and HTML does not put it. Styling
is charter §8.3's.

**A page that is absent is answered with the host's own page** (ADR-0163).
`not_found_on` names one case of one declared error, and a host answers it
404 with a page that holds nothing of the program's. A program declares no
not-found view yet, for this or for an address no route names, which the
development server answers 404 as plain text. A keyed read that answers the
case after the page is shown fails as any read does.

**Ids and references are read in the declaration that renders them**
(ADR-0185), as a label is (below). Not read:
- a reference or an id across a page and the views it composes;
- what the program computes;
- whether an ARIA attribute is allowed on its element's role, and what a
  role requires: ADR-0182's audit reads these at run time;
- a `<label for>` that names an element that is not a control.

**An element's children are its elements, for nine elements** (ADR-0204).
PW5012 reads what `<ul>`, `<ol>`, `<dl>`, `<table>`, `<thead>`, `<tbody>`,
`<tfoot>`, `<tr>` and `<select>` hold as the page holds it: each element
written there, a block's rows and branches, and what a view renders at its
top. Not read: text in one of them, and the content models of the other
elements.

**A form control is named only in the declaration that renders it**
(ADR-0143). PW5014 matches a `<label for>`, a wrapping `<label>` and an
`aria-labelledby`, written as text in the same view or page. These are not
matched:
- a computed `for`, `id` or IDREF;
- an `id` inside `{#each}`, which names no single row.

Wrapping the control in its label is the repair for each. A name the
program computes, such as `aria-label={x}` or a label's interpolated text,
is taken as a name, though it may be empty when the page runs. A label in a
page for a field in a view is refused, though HTML allows it. `title` and a
placeholder name nothing, though axe-core accepts both.

**Test 14's audit is the store's page, by rules a test decides**
(ADR-0182). The spec reads the store in three engines, as served and after
each kind of change, by axe's and WCAG 2.2 AA's rules that apply to it.
Not covered:
- **axe-core itself.** It is not installed: that needs the owner's OK for a
  network install.
- **A person with a screen reader.** The procedure is in
  `docs/research/screen-reader-smoke-test.md`, and no one has run it yet.
- **Target size at AAA** (44 by 44 pixels). The store's buttons are the
  browsers' own, about 20 pixels tall.
- **Forced colors, text spacing, and text zoomed alone.**
- **The other pages**: the demos' and the kiokun slice's.
- **A phone's layout in Firefox**, which Playwright does not emulate.
- **Duplicate ids and ARIA references at build** are checked since
  ADR-0185, within the declaration that renders them.

**An element left open is reported at the end of the file** (ADR-0200). A
close tag closes the innermost element whatever its name, so in
`<main><div>A</main>` the `</main>` closes `<div>`, `<main>` takes every `}`
after it, and `pw check` says "unclosed block" where the file ends. A
`{#..}` block left open is named where it is (PW5019). Matching a close tag
to its element by name waits for its own ADR, beside HTML's optional end
tags.

**A string's hole cannot hold a string** (ADR-0200). `"{f("a")}"` does not
parse; write the value to a binding first. Swift's and Kotlin's
interpolations read one.

**A label does not follow a value through storage, a later call of a
function value, or time** (ADR-0129). A value's label follows it through
calls, bodies, branches and assignments, and a public log takes only a public
value. Not tracked:
- a database write inside a secret branch: writes are not a sink the charter
  names, and a stored value's label is not carried through storage;
- a function value bound to a name and called elsewhere: it carries what it
  computes (ADR-0079), not the conditions of where it is called;
- how long a branch takes.
No program lowers a label except a host binding, whose signature is its
contract; an audited `declassify` waits for the first program that needs
one.

**The development server honours query policies except one** (ADR-0127,
ADR-0152): `consistency`, which ADR-0207 holds to what its source reads,
and nothing at run time enforces as such. `on_key_change`
runs since ADR-0152, for a binding a page's signal keys. Freshness, cache partition, key, retries,
timeout and one-flight-per-key are `pw-resource`'s, and a commit drops exactly
what it invalidates. **Its sandbox is checked, not
enforced** (ADR-0124, E14-I): no hidden file is copied in, and a process in
it can read the repository by absolute path.

**An entry's generation is still the host's constant** (`BUILD`, ADR-0300's
Not claimed): entries named by the build's name wait for the store's entries
keyed by a user. The static store page `run.sh` renders, which no host serves,
keeps `store-resume.json`'s constants. Until ADR-0300 every page's resume
manifest named the document schema `cart-doc` (found by ADR-0220), and the
decision compared that constant with itself.

**A `<select>` is not bound to a signal, and a `<textarea>`'s value is a
signal or text** (ADR-0221, PW5036). A select's chosen option is marked
`selected={..}` until a computed attribute in a row, from a signal (ADR-0227,
ADR-0228), can mark the one whose value a signal holds. A textarea a query fills waits for a patch the runtime
applies to its default value.

**A pending post is by "You"** (ADR-0222): a transition sees the value it
changes and its command's arguments, not the page's own user. A value of a
type that contains itself is speculated on as its nodes (ADR-0233), and a
view's instance given one is rendered again with it (ADR-0234). A target keyed
by a command's parameter matches a page's binding keyed by the page's parameter
its handlers pass unchanged (ADR-0236); one passed through a name the handler
binds, or from a composed view, is refused by name. An arm whose target's
resource a page does not show is not the page's, and the page waits for the
server on it (ADR-0238); one whose resource it shows under another key
refuses the page.

**A commit reaches another session's open page by the query it reads, not
by its key** (ADR-0219). A like drops one post's `Thread`, and every open
thread page is read again; each other one is sent no change, from the cache.
A reply is a post, and drops every `Thread` (ADR-0231).

**E7 gate 8 is unstable on this machine** (2026-10-02, E7-G8). About half
of runs see one long animation frame of 52-63 ms, with no script attributed
and 4-6 ms blocking, at HEAD and at `6545029` alike. `just e10-bench` stops at
that gate; `just e10-close-bench` records every run instead. The detector
counts a frame no page script made long, and whether it should is not decided
([control](evidence/E10/gate8-control-2026-10-02.md)).

**Ignored tests are not run by CI.** Benchmarks such as
`runtime/pw-host/tests/bench.rs` are `#[ignore]`d and need artifacts `just ci`
does not build. ADR-0115 broke one of them (world-level exports, fixed in
`0ad72c7`), and only re-recording E10's gate found it.

## Historical measurement interpretation

ADR-0027 supersedes the inference that an undefined frame-level
`forcedStyleAndLayoutDuration` establishes browser non-support. That value is
read from script-attribution records by the repaired instrumentation. Missing
observations and observed zero must remain distinct. Old raw measurements are
preserved, not replaced with invented results.
- **A platform's own fees' taxability** (ADR-0329): no CDTFA text addresses a
  delivery platform's own fees directly, so the deployment's tax adviser
  sets each store's flags; CDTFA proposed in 2026 to end the DNC opt-out
  (unconfirmed): if it does, the platform is the retailer and the flags
  change.
- **Payments** (ADR-0332): an order no one pays is taken back thirty
  minutes on (ADR-0334), at a time no deployment chooses; the provider is
  asked inside the
  command's transaction, so the host's commands wait while it answers (ten
  seconds at most); the reader's return is no second trigger; a
  taken-back order's tip and code are chosen again; and a secret does not
  build in a program (`Secret<C>` is laid out as its tag, as a scope
  qualifier is): no program holds one.
- **A page's policy** (ADR-0333): an address a value chooses (`<img
  src={…}>`) loads from this origin alone, and nothing at build says so; no
  `report-to`, so a deployment hears of no refusal; no Trusted Types, as the
  runtime writes its renderer's markup through `innerHTML`; one policy for a
  build's every page, so a page that frames nothing, in a build that frames
  the payment provider, may frame it too.
- **Scheduled commands** (ADR-0334): no recurring schedule; a run starts
  within a second of its time on a host that is running, and one past due
  when a host starts; the in-memory layers keep runs as long as the host
  runs, as they keep their data; an effect outside a run's transaction (the
  provider's cancel) may be asked twice by two hosts, as a press sent twice
  asks it; a run failed for good is kept and said, and runs again only when
  it is scheduled again.


- **Scheduler write ordering is per data-layer instance (ADR-0352).** The
  StorePg/FeedPg scheduled cleanup/retry guard serializes with that instance's
  command staging. Other host instances may still encounter legitimate
  PostgreSQL serializable refusals; this correction adds neither a distributed
  lock nor an automatic payment retry. Successful statement pins wait for the
  actual post-settlement success marker and do not count refused attempts as
  completion. Local strict whole12 and CI passed, and so did the exact
  head's verify 38909882781; landed with train-9.


PNG/APNG upload rewriting now reserves at most 256 MiB of aggregate expected
filtered-scanline extent across the default image and every animation frame.
A malformed excess can fill one fixed 32 KiB output block before refusal;
miniz also owns a 32 KiB internal dictionary. This bounds expected decoded
extent with fixed refusal overhead, not exact actual inflated bytes or CPU
work. It is not a 256 MiB allocation, CPU deadline or replacement for input
and staging budgets. It can refuse large images that fit an application's
compressed-byte and dimension declaration. Ambiguous PNG ordering, duplicate
singleton chunks, extra compressed/decoded stream bytes and malformed APNG
structure refuse the whole image. This does not remove arbitrary encoded-bit,
palette or pixel steganography, calibrated-display fingerprints or metadata
from older stored uploads. ADR-0383 records the specific boundary and gates.


APNG ancillary chunks interleaved between fdAT pieces of one frame are
explicitly unsupported: Chromium renders a demonstrated arrangement
transparently until metadata removal makes its declared pixels visible.
The host refuses it with an unsupported-image response rather than change
that appearance. Split/sequence-only-empty fdAT without interleaving and
ancillary chunks before/after frames remain supported. This is a stricter
supported subset than APNG's ordering rules, not malformed-file evidence.


### Scheduled generation repair (ADR0388, local candidate, 2026-10-26)

The new queue fence is implemented locally, with adversarial PG/materializer tests; hosted source proof, strict whole mutation campaign, independent final review and published-head gates are pending. It does not give exactly-once Wasm evaluation or external HTTP effects. A lost adapter acknowledgement after an actual PG commit is exercised, including hidden/pruned receipts; actual socket loss during COMMIT, process kill and unsettled upload claims remain unproved. Upgrade requires quiescing old workers and reconciling protocol0 quarantine; it does not repair previously duplicated effects or support mixed-version execution. Admission quotas/fairness for pending/failed work and generic D2 business/platform-outbox integration remain unfinished.


ADR0388 follow-up: Clock commit-error reconciliation now uses queue state rather than a result receipt and conservatively refreshes affected local reads after releasing staging. ActualPG adapter-ACK-loss/known-refusal neighbors and concrete server PatchSets are tested; browser DOM delivery under injected ACK loss, private keyed/kept-materialization neighbors, real transport/process failures, upload-claim uncertainty and ordinary-client uncertain-view recovery remain unproved. Earlier bc0 full campaign40342504105 failed and receives no whole-campaign credit; the repaired source still requires full21strict and all reached legacy campaigns plus final published gates.


### Temporary upload authority follow-up (ADR0389, 2026-10-27)

The follow-up binds private leases to the admitted reader and inspects current
source authority for preview/publication. The upload fixtures exercise memory
and the Kept source seam; they do not prove a two-host PostgreSQL upload race.
Remote lease publication is ephemeral cache state, with the business command's
transaction fence still governing committed posts. Already authorized fixed
preview bytes can finish in flight. Pending capacity counts an old lease and
its prepared replacement together, so replacement near the cap can refuse
before the former implementation did. Poisoned mutex/process-abort cleanup is
not established by normal refusal/authority-unwind tests.

A separate preexisting upload lifecycle limit remains: unclaimed lease expiry
is swept on a later upload, while preview/shown/claim do not independently sweep
the nominal sixty-minute lease age. It is not repaired or credited by this
session-revocation change. Public committed images retain ADR0260/0261's policy.
