# ADR-0367: TodoMVC in Pleris, held to its own spec and its own tests

Status: accepted under the owner's delegation of 2026-10-02 (a worker's, the
integrator's to number and index). Date: 2026-10-14, closed 2026-10-15 with
the integrator's rulings (below). Milestone: E14, track `todomvc` (W7), its
first milestone. Builds on the integrator's rulings in docs/PARALLEL.md,
"2026-10-09, W7's plan: TodoMVC", and of 2026-10-14 on the fragment (below).

## Amendment: live TodoMVC witnesses and strict mutation evidence, 2026-10-21

Verify 39538607183 at `34f3` exposed four survivors among the 43 semantic
controls. The empty-list test did not observe the toggle-all state. Two
checkbox controls changed the old host patch path, while this device-backed
application now uses `setAttributePart`. The focus control removed only a
nested descendant lookup, while the current morph inserts the edit field
itself. The failed run remains failed.

The original empty-toggle operator is unchanged. Existing tests now assert
that the toggle-all is unchecked at the host default and at three browser
transitions: initial empty, removing the last todo, and clearing all completed
todos. The two runtime controls omit only the false or true `checked`
property assignment at the live boolean-attribute authority, preserving other
boolean attributes. The insertion control omits the observer's added-node
focus lookup. Existing dirty-checkbox and focused-edit tests observe these
faults directly. All 43 model names and the four host/50 Chromium test IDs
remain; the application and runtime production behavior are unchanged.

The semantic campaign delegates process ownership, source restoration and
native Cargo/browser validation to the shared producers. Server runs require
all four exact IDs and native assertion-site provenance. Chromium freezes all
50 IDs on a green baseline and requires a fresh nonce, native exit agreement,
complete body-assertion results and no skips, hooks or timeouts. Program
mutants must pass native `pw check`; only exact, complete PW6805 output may
kill the explicitly named privacy control. Build failures, unrelated compiler
or runtime errors, stops and incomplete results refuse and stop further
controls. Both restored native suites must pass. Its separate producer suite
has 15 tests held by 17 assertion controls, including real child SIGTERM
restoration and final restored-suite gating.

The targeted final native proof ran in 62.5 seconds: host 4/4 and Chromium
50/50 baseline; the unchanged empty-list operator caused one of four host
assertions to fail; the three live runtime operators caused 2, 1 and 3 body
assertions among the same 50 Chromium IDs; restored host 4/4 and browser
50/50 passed. The strict checker also observed clean 23 files/native 0,
exact PW6805/native 1 under the original privacy fault, then restored native
0. These targeted results do not substitute for the whole 43-control hosted
campaign. Commands, original failed-run identity and native receipts are
recorded in [the diagnostic proof](../diagnostics/train-12/todomvc-witnesses-and-report.txt).

## Amendment: explicit Cypress compatibility execution, 2026-10-20

The integrator accepts a versioned execution copy under the owner's current
instruction to continue the work. The seven vendored upstream blobs remain
byte-for-byte unchanged. Execution on the pinned Cypress 15.21.1 applies
exactly two identified compatibility edits: invoke the spy's `resetHistory`
method, and finish `createTodo` with a fresh query after retaining its
existing logging callback. The latter lets the persistence alias query the
new document after reload. No application/runtime behavior, test case,
storage assertion, persistence assertion or known-issue table is changed.

Every report must identify this as **official TodoMVC cases with pinned
Cypress compatibility patch v1**. It is not unchanged upstream execution.
The source hashes, runner pin, unique patch anchors, output hashes and
staged manifest are checked before launch, as specified by
[ADR-0377](ADR-0377-todomvc-cypress-compatibility-copy.md). The runner must own and reap
its host; another process answering the chosen port is not readiness proof.

Acceptance still requires all 29 cases to execute and pass, no pending,
failed or skipped cases, and native Cypress status zero. Crashes, timeouts,
missing results and source drift fail the gate. Historical runs remain
failed, including the unchanged 24/29 run in verify 39526960864. A passing
compatibility diagnostic does not replace the complete recipe or its
negative controls. The server-backed version and old unchanged-suite
requirements described below are the historical decision; this amendment
supersedes their execution method and does not retroactively change results.

## Amendment: the toggle-all witness reaches a valid browser receipt, 2026-10-21

Verify 39545311229 at a0602009 exposed an invalid native browser witness for
"the toggle-all does what clearing the completed does": the toggle-all fault
left todos active, so the affected test clicked a hidden Clear completed button
and timed out before the browser producer could issue its complete receipt.
The original raw run remains at
C:/Users/haki/code/perfect-web-handoff/tracks/evidence/train12-eight/verify-39545311229/evidence-0/evidence/docs/evidence/E14/todomvc.txt
(SHA-256 5266673e48698fef68472d42c2d2aef5b8b5f2342daed183232e567a2b59da1b).

The existing test now asserts Clear completed is visible before clicking it.
The other clear-completed test that clicks after toggling a todo now asserts the
same visible precondition; the test that already checked visibility remains
unchanged. These assertions describe the UI state required for the action and
keep all 50 test IDs and titles. No producer, parser, model, or runtime behavior
changed.

The focused current-source receipt has the unchanged four server IDs and 50
Chromium IDs: server baseline 4/4, browser baseline 50/50, the unchanged
operator fault passed pw check and produced a complete valid browser result
with 46 passes and four assertion failures (including the repaired witness),
then restored server 4/4 and browser 50/50. The same 50 IDs were reported at
each browser run. Raw focused logs and structured receipts are in
/tmp/train12-toggle-visible-proof7; this local focused proof is not a whole
43-control campaign or hosted verification.

## Amendment: edit actions have visible-field preconditions, 2026-10-21

The original program operators 16 (a row never enters editing) and 30 (a
label double-press never begins editing) can leave the label visible while the
edit input is absent. Several tests double-pressed a label and immediately
filled or pressed the edit locator; those locator actions timed out before the
browser producer could write a complete receipt. Each such edit interaction now
asserts that its input is visible before filling or pressing it. The completed-
todo edit test also asserts input visibility: its existing row-class assertion
was insufficient because the mutation removes the row's editing data while the
row class can still say editing. No test title or ID, operator, helper,
producer, parser, or runtime behavior changed; the same 50-test inventory and
all 43 application controls remain.

On the current source, server baseline and restored receipts each passed the
exact four host IDs. The Chromium baseline and restored receipts each passed the
same exact 50 IDs. Each unchanged program mutant passed pw check (23 files,
no diagnostics) and produced a complete, valid 50-ID browser receipt with 35
passes and 15 assertion failures. Neither mutant produced a locator action or
test timeout in the completed proof. The proof completed in 49.7 seconds. Raw
receipts, output, build logs, and exact ID inventory are in
[the focused diagnostics](../diagnostics/train-12/todomvc-edit-preconditions/final/).

An earlier focused attempt caught one missed visibility precondition in the
completed-todo edit test: operator 16 then produced a 30-second fill timeout
and no complete receipt. That invalid attempt is retained separately under
[preliminary diagnostics](../diagnostics/train-12/todomvc-edit-preconditions/preliminary-incomplete/)
and is not counted as a kill. The corrected proof above is the accepted local
result; whole 43-control hosted verification remains due.

## Amendment: clear actions wait for completed model state, 2026-10-28

PR62 verify40617180819 at `f1d19c8e` returned actual `e14-todomvc`
status1 despite its enclosing shell job succeeding. The unchanged control
"the clear button counts what is left" produced one genuine hidden-state
assertion and a clear-button click timeout in the dirty-toggle-all test.
The strict reporter correctly refused its incomplete assertion receipt:
14/43 controls killed, one refused. That campaign remains failed.

The test already asserted the native toggle-all checkbox and clear visibility.
The native checkbox can change before the device's completed state renders;
the faulty done count can show the clear button while its todo is still active,
then hide it before the click. This interleaving is an inference from source
and the actual hidden-element timeout: the failed trace was over the retained
trace budget. A local original-source control did not reproduce that timeout.

The test now awaits the existing completed-row invariant after checking the
native checkbox, before its unchanged clear visibility assertion and click.
The same 50 test identities and all43 operators remain. No application,
runtime, producer, reporter, timeout, retry, skip or acceptance rule changes.
Under the exact done-count control, the new row assertion passes before the
existing visibility assertion fails through the complete nonce-bound reporter.

Focused before and after diagnostics each ran50/0 baseline,48/2 valid mutant
assertions,50/0 restored Chromium results with identical inventories. They are
uncommitted-source diagnostics, not final-head or whole-campaign acceptance.
The initial external helper callback error is retained as refused tooling;
it reached no mutation. Successful build child output is discarded by the
existing builder, so its empty wrapper logs are not native build transcripts.
Raw hosted failure, native receipts, source snapshots and hashes are in
[the focused record](../evidence/E14/todomvc-completed-precondition-2026-10-28/manifest.json).
`just e14-todomvc`, all planned recipes, browsers, CI and security gates still
must pass at the next exact published head before merge.

## Context

- **The owner's reference app, familiar to everyone** ("something people are
  familiar with", relayed 2026-10-08): TodoMVC, with an objective suite, its
  spec (`app-spec.md`) and its official behavioural tests (one Cypress spec),
  and, for the comparison and the tutorial, a first lesson.
- **What it asks of Pleris that no app has asked**: the filters' routes,
  `#/`, `#/active` and `#/completed`, which reach no server; editing a todo on
  a double press, with the field focused, saved on Enter and on blur and
  discarded on Escape; a counter that says "1 item left" and "2 items left".
- **The plan's ruling on the first of these** (docs/PARALLEL.md, 2026-10-09):
  "a page binds a signal to its fragment, whose type names the forms it takes;
  the runtime keeps it to `location.hash` at start and on each `hashchange`;
  the server renders its declared default", and the integrator's of
  2026-10-14: build it as its own commit, with its own checker and runtime
  tests and mutation controls, minimal where the language needs more, and
  write down each choice the ruling did not specify.

## Rulings (the integrator, 2026-10-15)

The worker put nine questions; each was ruled by the integrator, 2026-10-15,
as below, and the section that built it says so.

1. **Signals in rows**: ruled by the integrator, 2026-10-15: keep the
   page-signal workaround for the first milestone; its cost is stated under
   "Not claimed". Per-row signal instances (ADR-0137, PW5307) are a separate
   language track, not this one.
2. **`todomvc-app-css` is CC BY 4.0**, not MIT as the plan says: ruled by the
   integrator, 2026-10-15: acceptable with the attribution kept (the page's
   footer and the licence file copied beside the sheet); the correction to the
   plan is recorded here. (No repository licence allow-list covers npm packages:
   `deny.toml`'s is cargo's, and `tools/node-audit-allow.txt` lists advisories.
   Nothing was added.)
3. **The fragment signal as a `String`** with clause-named forms: ruled by the
   integrator, 2026-10-15: accepted; a sum-typed fragment signal is a later
   question.
4. **`dblclick`, `blur` and `BlurEvent { value }`**: ruled by the integrator,
   2026-10-15: accepted.
5. **`autofocus` on insertion**: ruled by the integrator, 2026-10-15: a runtime
   behaviour, accepted; section 4 says how it differs from HTML's and why.
6. **The checkbox property sync**: ruled by the integrator, 2026-10-15:
   accepted as built (its own commit, its tests, its mutants).
7. **Cypress**: ruled by the integrator, 2026-10-15: in the evidence recipe
   only, not in `just ci`. No vacuous pass: where the recipe runs as evidence
   it fails if Cypress cannot run (no binary, no Xvfb), and never prints a skip
   and passes. Passes are recorded out of the spec's total and, for each
   failure, the spec rule and why; a test that asks `localStorage` fails by
   design for an app whose data is on the server, and says so per test. The
   upstream test is never changed. Section 5.
8. **The 500-todo cap** (`full`): ruled by the integrator, 2026-10-15:
   accepted, as a stated limit with its own test.
9. **A title of 1 to 1000 characters after trim**: ruled by the integrator,
   2026-10-15: accepted.

Downloads, approved by the owner on 2026-10-08 for this track and nothing
else: the Cypress npm package and its binary, `todomvc-app-css` and
`todomvc-common`. Their versions and sizes are in section 3 and section 5.

## Decision

### 1. A page binds a signal to its address's fragment

```
session page Todos() {
    route    "/"
    fragment filter "/" => "all", "/active" => "active", "/completed" => "completed"

    signal filter: String = "all"
    ..
```

1. **A page clause**, `fragment <signal> "<form>" => "<value>", ..`: a signal
   of the page, and each form the fragment takes, as `location.hash` reports
   it without its `#`, with the value the signal takes then. It is a policy
   head (`pw_syntax::POLICY_KEYWORDS`), a page's alone (`policy::declared_by`), read
   from the clause's text as a `route` is (`address_fragment::read`), and
   carried to the browser in the page's plan and its document's manifest
   (`fragment: { signal, forms: [{ form, value }] }`).
2. **The runtime keeps the signal in step with the address**
   (`pw-runtime.mjs`, `bindFragment`): it reads `location.hash` once when the
   page starts, after the document's subscription, and again on each
   `hashchange`, the one a link makes, the back and forward buttons, and a
   script's `location.hash = ..`. The signal is set as any signal's change is:
   what reads it is set or rendered again, and a query it keys is read again
   for the new key (ADR-0152), which is how TodoMVC's model filters.
3. **The server renders the declared default**: a request carries no fragment,
   so the document is the same bytes whatever the address's fragment was. A
   page opened at `#/completed` shows its first value until the runtime has
   read the address and the page's keyed read has answered (the deep-link
   test), which is a flash of the default, and the price of a fragment that
   no server sees.
4. **A fragment that is no form, or none, is the signal's first value**, the
   declared default; the first value is held to be one a form gives (PW6104),
   so the default is a page the address can reach. A signal already at the
   value an address gives is not set again, so a page opened at its default
   renders nothing more.
5. **The address is the signal's only writer** (PW6105): no handler assigns
   the signal, so the signal and `location.hash` cannot differ. A page changes
   it by a link, `<a href="#/active">`, which the browser follows, in place,
   and whose history entry is the back button's.
6. **A link to a fragment the page does not take is refused** (PW6106): on a
   page that binds its fragment, a static `href="#..."` in its own view names
   a form it lists, so a reader who follows one is never set back to the
   default by a typo.

**Checks** (`compiler/pw-core/src/address_fragment.rs`, owner `Todomvc`, the
block PW61): PW6101 the clause names a signal the page declares; PW6102 the
signal is a `String`; PW6103 the forms are `"form" => "value"` pairs, each form
once, of the characters a fragment holds; PW6104 the signal starts at a value a
form gives; PW6105 no handler assigns it; PW6106 a link names a form. A clause
written twice, and one outside a page, are the generic rules'.

### The choices the ruling did not specify

Each was the integrator's to confirm or reverse; none is built past what the
ruling says, and each is cheap to change. The first, the signal as a `String`,
was ruled by the integrator, 2026-10-15: accepted; a sum-typed fragment signal
is a later question.

- **The forms are named by the clause, and the signal is a `String`, not a sum
  type.** The ruling reads "the signal's type names the forms". Two limits of
  the language today stopped that: a query's key must be a `String`, an `Int`
  or a `Bool` (PW5308), and TodoMVC's filter keys its read; and a signal of any
  other type needs a codec, which is not built (ADR-0131). So a form's value is
  the text the signal holds, and the clause is where the closed set is written.
  When keys and signals may be a sum type, the clause can name its cases
  instead of texts, and nothing else changes.
- **A clause on the page, not a modifier of the signal, and not the route's.**
  The signal's declaration stays the one every signal has. RFC 6570 has a
  fragment expansion, `{#var}`, which would have put it in the route; a route's
  parameter is read once per request, by a server that never sees a fragment,
  and a signal is what the browser changes, so the route is the wrong place.
- **One-way: the address sets the signal, and nothing sets the address.** "At
  start and on each `hashchange`" is what the ruling asks. Writing the address
  back from a handler (`history.pushState`) would make two writers; PW6105
  refuses it instead, and a link is how a page moves.
- **A form is compared exactly**: the text after `#`, as `location.hash`
  reports it, case-sensitive, not decoded. A form is written in the characters a
  fragment holds as they are (RFC 3986 §3.5's `pchar`, `/` and `?`, less `%`),
  so a form is what the browser reports and never a percent-encoding of it.
- **Several forms may give one value, and the empty fragment is a form a page
  may list**: aliases (`"!/" => "all"`, as TodoMVC's spec allows `#!/`) and
  the address with no fragment (`"" => "all"`). A form written twice is
  refused. A value is any text; a first value is a literal.
- **One binding a page**, and the page's own view only: PW6106 reads the links
  of the page's template, not of the views it composes.
- **The compiler's change is one policy head and one module.** No syntax form
  was added: a policy's value is the text of its line, and the clause's
  grammar is read from it, as `route`'s and `says`'s are.

### Alternatives

- **A signal modifier** (`signal filter: String = "all" in fragment ..`): a new
  form of the statement, whose forms would have no place to be written.
- **A sum-typed signal with its cases named by the type**: not today, as above.
- **Functions** (`fn filter_of(fragment: String) -> Option<Filter>`) written in
  Pleris and called by the runtime: a codec, ADR-0131's question, for one page.
- **A route's `{#filter}`**: above.
- **Writing the address back** from handlers: two writers, above.

### 2. A double press and a control's losing focus are events the platform declares

TodoMVC begins an edit on a double press of a todo's label, and saves it on the
field's blur. `events.pw` declared `press`, `input`, `keydown`, `change`,
`submit` and `close`; ADR-0138 listed `focus` and `blur` among the "events the
platform does not declare", and a double press was not thought of. "Declaring a
new event is a change to this file rather than to a checker" (`events.pw`), so:

- **`dblclick`**, whose handler is given a `PressEvent`, where the pointer was:
  a double press is read as a press is. The two presses of the pair are
  presses still, and run the handlers an element has for them.
- **`blur`**, whose handler is given a `BlurEvent { value }`, the value the
  control held as it lost focus: the field's edit is saved from it, as an
  `input`'s record is read. It does not bubble, so it is the element's own.
- **The runtime** maps each to the browser's event of the name and reads its
  record in the listener, before any code loads (ADR-0138, `DOM_EVENTS`,
  `eventRecord`). The platform contract's hash moves
  (`tests/platform_contracts.rs`), for `events.pw` alone.

The names are the browser's, `dblclick` and `blur`, not `doublepress`: a double
press has no other form (no key, no touch), and a reader of the program meets
the name they know. Ruled by the integrator, 2026-10-15: accepted.

Not built, and not needed: `focus` (the pair of `blur`), `focusin` and
`focusout`, a refinement of a checkbox's `change` to `checked` (ADR-0131), and a
keyboard's way to begin an edit (TodoMVC's own is the double press, which the
spec names, and a keyboard user cannot make one: a gap the spec has).

### 3. TodoMVC, built

A fourth program on the development server, beside the store, the feed and
kiokun: `examples/todomvc/app.pw`, built by `spikes/own-renderer/todomvc.sh`
into `dist-todomvc`, served on a host per engine (`TODOMVC_PORTS`, `PORT+140`
to `PORT+142`, the plan's ports).

- **Its data layer is a seam of its own** (`TRACK SEAM (todomvc)` in
  `from_build_layers`): chosen where the program imports `todomvc:data/…`, as
  kiokun's is. `todomvc.rs` keeps a session's todos in memory, oldest first,
  under one lock from a command's call to its commit, as the feed's layer
  does. Every operation names its session, and one that names another than
  the command's or the query's own is refused, so no write or read reaches
  another session's list. A session holds at most 500 todos (`full`), which
  TodoMVC's spec does not state and a host guests write to needs: a stated
  limit with its own test (`a_session_holds_at_most_a_bounded_list`), ruled by the
  integrator, 2026-10-15: accepted. A todo's title is 1 to 1000 characters after
  trim (the spec says only "trim"; an empty title does not add, and removes the
  todo on edit; `the_host_holds_a_title_to_its_invariant`): ruled by the
  integrator, 2026-10-15: accepted.
- **Its markup and class names are the template's**, and its stylesheets are
  TodoMVC's own: `todomvc-common`'s `base.css` and `todomvc-app-css`'s
  `index.css`, from the npm packages the owner approved on 2026-10-08, copied
  as they are by `todomvc.sh`. The official sheet draws a todo's checkbox, a
  circle and a ticked circle, as `data:` images, which a page's generated
  policy refuses (`img-src 'self'`, ADR-0333). The spec allows an app's own
  sheet over the official ("if you need to change some styles, use the app.css
  file"), so `todomvc-styles.mjs` reads the two drawings out of the official
  sheet as it ships, writes each as an SVG file of this origin, and writes
  `todomvc-pleris.css`, the two rules that point at them, loaded after.
  Nothing is drawn by hand; the host serves `.svg` as `image/svg+xml` (a
  one-line change at its static route, marked as the track's), which a browser
  asks for under `nosniff`.
- **The licences.** `todomvc-common` is MIT. `todomvc-app-css` is **CC BY 4.0**,
  at its source and on npm, not MIT as the plan says (a correction to the
  plan): it asks for credit, which the page gives in its footer and the licence
  file served beside the sheet (`/todomvc-app-css.license.txt`) carries, and
  nothing in it is changed. Ruled by the integrator, 2026-10-15: acceptable with
  this attribution. The two packages, approved by the owner on 2026-10-08, from
  the npm registry through pnpm: `todomvc-app-css` 2.4.3 (4 files, 26,872 bytes
  unpacked) and `todomvc-common` 1.0.5 (5 files, 10,641 bytes unpacked).
- **The filters are the model's.** The page's read, `Board(session, filter,
  editing)`, is keyed by the signals `filter`, which the fragment sets (§1), and
  `editing`; the program's own pure functions filter (`shown`) and count
  (`board_of`), and a change of the address is a read for a new key (ADR-0152).
  The footer, the counter, the toggle-all and the clear button read counts of
  every todo, which no filter changes.
- **The editing mode is the page's signal, and the model says which row shows
  its field.** TodoMVC's spec: "Editing mode should not be persisted", so the
  mode is UI state (charter §9.1's first kind): the signal `editing` names the
  todo, `edit_text` holds the field's text, and a double press sets both. A
  signal read inside a query's loop, `{#if editing == row.id}` in a row, is not
  built (ADR-0137 refuses it; ADR-0144's per-row instances, PW5307, are not
  built either), so `editing` keys the page's read too and the model returns
  each row with `editing: true` for the one: the row is shown again, with its
  field. The cost is a read for each begin and end of an edit, and a row
  re-rendered whole, which a change to its text by another tab of the session
  would take the field from. Ruled by the integrator, 2026-10-15: keep this
  workaround for the first milestone; this is what the missing piece costs, and
  the proper fix, a signal a row can read, is a separate language track. The
  cost is stated under "Not claimed".
- **The new todo is cleared at Enter, and given back if it was not added.** The
  spec says Enter "creates the todo, appends it to the todo list, and clears
  the input". Cleared when the command answers, what a person typed while the
  command was in flight was lost (found in WebKit, where the answer is slower);
  cleared at once, it is not, and a todo the host refuses (`full`, a network
  failure) is given back to the field where nothing has been typed since.
- **A save ends only its own edit.** A blur saves the todo being edited and,
  once the host answers, ends the edit unless another todo has been begun
  since: a double press on a second todo begins it by leaving the first's
  field, and the first's answer must not end the second's edit (found by the
  suite: "editing another todo saves the first").
- **Persistence is the framework's own**, the spec's allowance in place of
  `localStorage`: a session's todos, kept by the host, outliving a reload, not a
  restart of the host, and not a device that clears its cookies. **The version
  on a device's own storage waits for charter §9.1's fourth kind of state**, which
  starts with the Docs app (docs/NEXT.md), and is TodoMVC's second version then.
  What the official Cypress suite says of an app that does not use
  `localStorage` is the next section's.

### 4. Two things TodoMVC needed of the runtime

Building the page found two things the runtime did not do, which no page had
asked of it. Each is its own commit in the shared runtime, with its tests and
its mutants.

- **A patch that checks or unchecks a box sets the box's state.** The runtime's
  `setAttributeAt` set the `checked` attribute and nothing else, which moves a
  checkbox only until a person has pressed it: from then the property is the
  box's own. The toggle-all, pressed, stayed checked when one todo was cleared
  (the official suite asks it: "complete all checkbox should update state when
  items are completed / cleared"), and a row another tab cleared stayed ticked in
  the tab that had ticked it. A signal's attribute already set the property
  (`setAttributePart`); a patch's now does, for `checked`. `selected` is the same
  and is not done: no page needs it, and none tests it. Three tests in three
  engines, each failing without the change, and two mutants. Ruled by the
  integrator, 2026-10-15: accepted as built.
- **An element that arrives with `autofocus` takes focus.** *How it differs
  from HTML's, and why.* HTML honours `autofocus` once per document, and the
  page's load is its time: an inserted element that has the attribute is only a
  *candidate*, queued while the top-level document's "autofocus processed flag"
  is false; the first focus it gives, or any focus the document already has when
  the candidates are flushed, sets the flag, and from then an inserted element's
  `autofocus` is ignored (HTML Standard, 6.6.7, read 2026-10-15). TodoMVC's page
  opens with its new-todo field focused by the attribute, which sets the flag,
  so a browser leaves the edit field a double press inserts unfocused
  (Chromium and Firefox do; WebKit focuses an inserted field the first time and
  not the second, `e2e/focus.spec.mjs`). The spec asks for the opposite, "the
  edit field should be focused": the field a person asked for is the one they
  type in, and the attribute is the only way a page says so. So the runtime
  honours it for each thing put into the page, not once: it watches what is put
  in from its start (`followAutofocus`, a `MutationObserver` on the body, which
  sees a block's rendering and a server's patch alike) and gives the first
  element in it that asks for focus, if it does not have it. What the page was
  served with is left alone (the browser focused it), and a modal dialog's
  `autofocus` element is the one `showModal` focuses. A runtime behaviour, not a
  language rule: the attribute is HTML's and the page says nothing new. Ruled
  by the integrator, 2026-10-15: accepted. Its cost, under "Not claimed": a
  patch that inserts an element with `autofocus` takes the focus from wherever a
  person is. `examples/demo/focus.pw` and `e2e/focus.spec.mjs` show it in three
  engines (7 of the 12 fail without the change), and TodoMVC's own test holds
  it on the edit field a patch inserts.

### 5. TodoMVC's own Cypress suite, unchanged

The plan's acceptance for the first milestone is the official suite, run
unchanged. `spikes/todomvc-cypress` holds the seven files of
github.com/tastejs/todomvc, at `ff43b02e59dfa604386bb382034b2cd07c2bcd8a`, that
the suite needs: the spec (29 tests, named for TodoMVC's own Selenium tests),
its support files, its `knownIssues.js`, its Cypress config and its licence
(MIT). Each is held to the git blob upstream has
(`scripts/todomvc_cypress_vendored.py`), so "unchanged" is a hash. The plan
names the spec `tests/cypress/e2e/spec.cy.js`; upstream today has
`cypress/e2e/spec.cy.js` (a correction to the plan). It runs on Cypress 15.21.1,
headless in Electron 138, under Xvfb.

**Downloads** (approved by the owner, 2026-10-08, for this track): the npm
package `cypress@15.21.1` (7.6 MB installed) and its binary (a zip of 248,957,235
bytes, 813 MB unzipped, in Cypress's own cache). Not the 15.14.2 that upstream's
lockfile pins: its dependency tree carried six advisories (`uuid`, three of
`qs`, two of `extract-zip`) that `scripts/audit-node.sh` refuses, and 15.21.1's
carries none (nothing is added to `tools/node-audit-allow.txt`); the suite
printed the same outcomes on both. It is the last 15.x because the spec reads
`Cypress.env()`, which Cypress 15 warns a future major will drop. The package is
a member of the workspace, so `just audit` scans its tree; `pnpm exec cypress
install` fetches the binary, and `just e14-todomvc` runs it (it does nothing
where the binary is there).

**Two runs, and a judge.** `spikes/todomvc-cypress/run.sh` serves the build on a
host of its own and runs the suite twice, and `scripts/todomvc_cypress_result.py`
reads what Cypress printed and judges it.

- **The acceptance**: `cypress run --env framework=page/todomvc.app.Todos`, the
  suite as it ships. The suite visits `baseUrl + '/' + framework`, so the name
  is the page's own path on the host and nothing proxies or rewrites an
  address. It is in none of the suite's tables, so the suite asks of this app
  what it asks of an app it knows nothing about, `localStorage` included.
- **The diagnostic**, labelled in every report as not the acceptance: the same
  files under `--env framework=puremvc`, a name the suite's own table
  (`noLocalStorageCheck`) takes as keeping nothing in `localStorage`, reaching
  the page through an address fragment it ignores. It shows what the rest of
  each test says of this app.
- **No skip passes** (ruling 7). `run.sh` exits 1 where Cypress cannot run (no
  Xvfb, no binary, no build, no host) and where a run printed no results
  table; its status is the judge's, which holds each test to the outcome the
  app's design gives it. `just e14-todomvc` records both runs and fails with
  them. The suite is not part of `just ci`.

**What the suite asks of `localStorage`**, read from the vendored spec: of its
29 tests, 23 ask it, in their own bodies or in their context's `beforeEach`
(`checkItemSaved` spies on `localStorage.setItem`; the others poll it for the
todo's text, for n todos, for n completed todos, for the word `complete`), and
6 ask it nothing (1, 2, 3, 21, 22, 23). TodoMVC's spec asks an app to persist its
todos to `localStorage`, and to use the framework's own persistence where it has
one (read at the pinned commit, 2026-10-15); this app's are kept by the host
(section 3) and it never writes `localStorage`. So in the acceptance a test
that asks `localStorage` must fail and one that asks it nothing must pass, and
the judge refuses a run in which either does otherwise.

**The result**, Cypress 15.21.1 and 15.14.2, each run at least twice
(`scripts/tests/fixtures/todomvc-cypress/` holds real outputs of both). The
acceptance column is a run whose hook failed at test 7, as in three of five
runs, and the diagnostic column is one in which test 11 won its race (below):

| # | spec section | test | asks localStorage | acceptance | diagnostic |
|--:|---|---|---|---|---|
| 1 | When page is initially opened | should focus on the todo input field | no | pass | pass |
| 2 | No Todos | starts with nothing | no | pass | pass |
| 3 | No Todos | should hide #main and #footer | no | pass | pass |
| 4 | New Todo | should allow me to add todo items | setItem, n todos | FAIL | pass |
| 5 | New Todo | should clear text input field when an item is added | n todos | FAIL | pass |
| 6 | New Todo | should append new items to the bottom of the list | n todos | FAIL | pass |
| 7 | New Todo | should trim text input | n todos | hook fails | pass |
| 8 | New Todo | should show #main and #footer when items added | n todos | not run | pass |
| 9 | Mark all as completed | should allow me to mark all items as completed | setItem, n completed, n todos | not run | pass |
| 10 | Mark all as completed | should allow me to clear the complete state of all items | setItem, n completed, n todos | not run | pass |
| 11 | Mark all as completed | complete all checkbox should update state when items are completed / cleared | setItem, n completed, n todos | not run | pass or races |
| 12 | Item | should allow me to mark items as complete | n completed | not run | pass |
| 13 | Item | should allow me to un-mark items as complete | n completed | not run | pass |
| 14 | Item | should allow me to edit an item | text | not run | pass |
| 15 | Editing | should hide other controls when editing | n todos | not run | pass |
| 16 | Editing | should save edits on blur | n todos, text | not run | pass |
| 17 | Editing | should trim entered text | n todos, text | not run | pass |
| 18 | Editing | should remove the item if an empty text string was entered | n todos | not run | pass |
| 19 | Editing | should cancel edits on escape | n todos | not run | pass |
| 20 | Counter | should display the current number of todo items | n todos | not run | pass |
| 21 | Clear completed button | should display the correct text | no | not run | pass |
| 22 | Clear completed button | should remove completed items when clicked | no | not run | pass |
| 23 | Clear completed button | should be hidden when there are no items that are completed | no | not run | pass |
| 24 | Persistence | should persist its data | keyword, n completed, text | not run | skipped by the suite |
| 25 | Routing | should allow me to display active items | n completed, text | not run | pass |
| 26 | Routing | should respect the back button | n completed, text | not run | pass |
| 27 | Routing | should allow me to display completed items | n completed, text | not run | pass |
| 28 | Routing | should allow me to display all items | n completed, text | not run | pass |
| 29 | Routing | should highlight the currently applied filter | text (in the hook) | not run | pass |

- **Acceptance: 3 of 29 pass**, the three that ask nothing of `localStorage` and
  come first. Where a test asks it and ran, it failed: 4 on `expected setItem
  to have been called` (this app never calls it), 5 and 6 on `cy.then() timed
  out` over a poll that never ends. Then a hook fails and Cypress skips the
  rest. **Per test, why each did not pass**: every row above that asks
  `localStorage` and is FAIL or not run fails, or would fail where it was
  not run, at its first `localStorage` check, *by design*: the app keeps its
  todos on the host. The rows that ask nothing and are not run (21, 22, 23) are
  not run only because a hook failed before them; the diagnostic runs them,
  and they pass. The judge prints this per test, with the spec's line, in
  `docs/evidence/E14/todomvc.txt`.
- **Diagnostic: 28 of 29 pass, 1 skipped by the suite itself** (24, which the
  suite skips for a name in `noLocalStorageCheck`: it reloads the page and
  asks `localStorage`; this app's reload is held by its own test, "persistence").
  The tests that fail in the acceptance pass here, so what they failed on there
  was `localStorage`, and what the rest of each says holds. Test 11 passes
  or loses a race, below.

**Why the acceptance stops early.** For an app that never writes `localStorage`
the suite's own helpers poll for ever: `checkNumberOfTodosInLocalStorage`
re-arms `setTimeout(checkItems, 0)` until `localStorage` holds something. The
test that called it fails at Cypress's 4 s `cy.then()` timeout and the loop goes
on, reading a `Storage` of the window that Cypress clears away between tests (it
navigates the page to `about:blank`); the read throws `SecurityError: Failed to
read the 'length' property from 'Storage': Access is denied for this document`
in the spec's own window; Cypress charges the throw to the next test's
`beforeEach`, whose `cy.document()` then finds the blank page ("Cannot
determine what kind of selectors this app uses"), and, as for any failed hook,
skips the rest of the suite. Which test that is varies with timing (test 7 in
three of five runs, test 6 in two), so the judge holds a run to rules and not to
a list. The throw is on record in Cypress's output (`ELECTRON_ENABLE_LOGGING`,
which `run.sh` sets), and the judge refuses a failed hook without it. It is
the suite's doing and not the app's: the diagnostic, where the suite does not
poll, passes the same hook 28 times.

**Test 11 races the host.** "complete all checkbox should update state when
items are completed / cleared" presses toggle-all and, at once, unchecks the
first row's box (`uncheck({force: true})`, which does nothing to a box that is
not checked). The press checks toggle-all's own box and nothing else; the rows'
boxes are checked by the host's answer. Where the test's next step comes before
the answer, the first row's box is not yet checked, `uncheck` does nothing, and
toggle-all stays checked as every row completes (the screenshot of a failure
shows all three rows completed, "0 items left", and toggle-all checked). The
answer reaches the page after a median 24 ms on a debug build (min 20.5, max
33.9; 80 presses at load 4 to 7) and 24 ms on a release build (min 4.6, max
28.5; 80 presses), because the stream is not pushed: `stream_open`
(`spikes/own-renderer/server/src/main.rs`) reads the session's queue every 25
ms (`std::thread::sleep`). Under load 60 to 70 the median was 50 ms (24 to
101). In 8 diagnostic runs the test failed 3 times (two when the machine was
loaded, load 30 to 70, and one at load 12) and passed 5 (load 11 to 36). It is neither a bug of the app nor a
design choice but a finding about the transport (below). The judge lets the
test pass or lose this race, and says which it did.

**What this says of the app, and what it does not.** The official suite, run
unchanged, cannot show that tests 4 to 29 pass for this app, because 23 of the
29 ask `localStorage`, which this app does not use: it is not claimed that the
unchanged suite passes. The diagnostic shows that, with that one question
removed by the suite's own table, each test's other assertions hold (28 of 29,
the 29th skipped by the suite). Adding this app's name to `noLocalStorageCheck`
would make the acceptance the diagnostic, and is a change to the upstream spec,
which is never made here.

## What was found

Corrections first.

1. **The plan says `todomvc-app-css` is MIT**: it is CC BY 4.0 (section 3).
2. **The plan names the spec `tests/cypress/e2e/spec.cy.js`**: upstream has
   `cypress/e2e/spec.cy.js` (section 5).
3. **HTML's `autofocus` is once per document, not "at load"**, as the ruling's
   text and the first draft of section 4 had it: an element inserted later is a
   candidate until a focus has set the Standard's "autofocus processed flag"
   (read 2026-10-15). What the runtime does is the same either way (section 4).

Then what the build and the suite found:

4. **The stream transport is a poll** (framework finding). `stream_open` reads
   a session's queue every 25 ms, so every server-driven update waits for the
   next tick: a median 24 ms from press to patch on a debug and a release build
   alike, 50 ms under load. A queued frame could wake the stream (a `Condvar`
   beside `pending`, notified where a frame is queued, with 25 ms kept as the
   bound), about thirty lines that would change every page's delivery
   timing. Not done here: a question for the integrator, below.
5. **A server-driven page races a test that acts on the result at once**
   (Cypress test 11). The same property: a client-state app updates its rows in
   the press's own turn, and a host-state app after the host's answer.
6. **The official suite cannot run to its end against an app that writes no
   `localStorage`**: its polling helpers never end, and the loops they leave
   behind fail the next hook (section 5).
7. **Two runtime bugs, found by TodoMVC**: server patches set the `checked`
   attribute and not the property, and nothing focused an element that arrived
   with `autofocus` (section 4); each is its own commit with tests and mutants.
8. **The page's content policy refuses `data:` images** (`img-src 'self'`,
   ADR-0333), which `todomvc-app-css` draws its checkboxes with; the build
   writes them to files of the app's own origin (section 3).
9. **Two races in the page's own program**, found by the browser suite and
   fixed: WebKit lost fast typing when the field was cleared after the
   command's answer (the field is cleared at Enter and given back on a
   refusal), and a blur's save ended a newer edit (a save ends only its own).
10. **The language's limits met**: a keyed read allows only `String`, `Int`
    and `Bool` keys (PW5308, ADR-0131); a signal read inside a query-loop row is
    refused (ADR-0137); a `resumable(captures)` handler must be fully typed
    lambdas or use signals only (a `row.id.value` capture is refused, hence
    `Row.id: String` and `TodoId(row.id)`).
11. **Flakes**: one in the browser suite, mine and fixed (the toggle-all test
    cleared a row before the host's answer; 2 of 141 under load 60 to 80, now
    waits for the rows, 54 of 54 in a repeat of six). None other.

## Acceptance

### 1. The fragment

- `compiler/pw-core/tests/address_fragment.rs`, 8 tests: the plan carries the
  binding and a page that states none has none; each of PW6101 to PW6106 with
  its control (the same page written well checks clean); a clause written twice
  and a clause on a query are refused. The policy table's own tests hold the
  clause to a domain and a place (`policy::tests`, `clause_places`), and
  `codes::tests` hold PW61 to its owner.
- `spikes/own-renderer/server/src/tests/address_fragment.rs`, 3 tests: the
  document of a page that reads queries and of a page of signals alone name
  the binding, a page that binds none says none, and the host renders the first
  value whatever the address's fragment.
- `spikes/own-renderer/e2e/fragment.spec.mjs` over `examples/demo/fragment.pw`,
  10 tests in Chromium, Firefox and WebKit: the document names the binding; an
  address with a form shows it at start, and what reads the signal follows (a
  text, an attribute, a block); the default is no change; a link, the back and
  forward buttons and a script each change it; a fragment that is no form, and
  the empty one, are the default; a form is its characters exactly; the
  signals it does not bind keep their values; a page that binds none ignores
  its address's.
- `scripts/address_fragment_mutations.py`, 23 mutants, `just
  e14-address-fragment`.

### 2. The two events

- `compiler/pw-core/tests/declared_events.rs` (PW5022 lists both, and each is
  accepted on an element) and `events.rs`
  (`a_double_press_and_a_blur_are_given_their_records`: each handler given its
  record, run under Node, and another event's record refused, PW0602).
- `spikes/own-renderer/e2e/events.spec.mjs` over `examples/demo/events.pw`, 2
  new tests in three engines: a single press runs the press's handler and
  not the double press's, a pair runs the double press's once, given where
  the pointer was; a control losing focus runs its handler, given its value,
  typing is not leaving, and one with no handler for it runs none.
- `scripts/dom_events_mutations.py`, 8 mutants, `just e14-dom-events`.

### 3. TodoMVC

- `spikes/own-renderer/server/src/tests/todomvc.rs`, 14 tests of the host:
  `.main` and `.footer` hidden, not absent, with no todos, and shown with one;
  todos appended to the bottom, ids never reused; the host holding a title to
  its invariant (empty and over 1000 refused, 1000 added); the counter's words
  and its `<strong>`; toggling one and every todo, and the toggle-all's checked
  state following them; clear completed, and its button hidden with none; an
  edit saved trimmed and one emptied destroying the todo; a todo that is not
  there not found; a session's todos its own, by the page, by a command and by
  the layer; the bound; the model filtering by the page's key, and only what
  changes sent; the key naming the row being edited; the template's classes,
  the new-todo field's `autofocus`, the filters' links and TodoMVC's
  stylesheets.
- `spikes/own-renderer/e2e/todomvc.spec.mjs`, 47 tests in Chromium, Firefox
  and WebKit (141 runs), in the spec's own order: no todos; the new todo (Enter, the
  field cleared and focused, appended, trimmed, never empty, bounded, markup
  shown as text); mark all as complete; an item (its checkbox, hover for
  `.destroy`); editing (a double press, Enter, blur, clicking away, trimmed,
  emptied, Escape, not persisted, another todo's begin saving the first,
  nothing changed, a completed todo being edited); the counter; clear
  completed; persistence (a reload, another visitor, a second tab live);
  routing (each filter and its `selected` link, the model filtering, an item
  updated under a filter, the back button, a page opened at a route and kept
  on reload, a fragment that is no route); the template's markup; and
  TodoMVC's stylesheets, served and applied, and the licence beside them. Since
  the first draft: a row's box and toggle-all's following the page after a
  person pressed them (3 tests), and the edit field focused as the patch
  inserts it (1).
- `scripts/todomvc_mutations.py`, 52 mutants: 38 of the program, each
  checked to be a well-typed program before it runs, 10 of the layer, 1 of the
  host and 3 of the runtime (the box's state, the edit field's focus);
  `just e14-todomvc`.

### 4. The Cypress suite

- `scripts/tests/test_todomvc_cypress.py`, 44 tests. The vendored files are
  upstream's blobs, and a file changed by a byte is not. The spec's 29 tests,
  and which ask `localStorage`, are read from it (6 ask nothing; a context's
  `beforeEach` counts). Cypress's output is read: the counts, each test's line,
  each failure's first error line, the browser's record of the polling loops'
  throw, colour codes, the same lines from 15.14.2 and from 15.21.1. Each rule
  of the judgement is broken in a copy of a real output: a test that asks
  `localStorage` passing, one that asks nothing failing, a test not run with no
  failed hook before it, a failed hook with no cause on record, the first tests,
  the table against the lines, the exit status, a test the spec lacks, the
  diagnostic's persistence test, its racy test failing for another reason or
  skipped, any other test not passing. A suite that did not run is not a pass
  (no table, an empty file, a missing file, a missing binary's message). And
  `run.sh` is run whole, with a stand-in Cypress and a stand-in host: the
  acceptance and the diagnostic under their names, the browser's console on, the
  machine's display off; a run with no results, an outcome the design does not
  give, and an exit status that is not the failing count each exit 1; a missing
  Xvfb, binary, build and host exit 1 and run nothing.
- `scripts/todomvc_cypress_mutations.py`, 46 mutants of the vendored check,
  the judge and `run.sh`, each failing at least one of those tests (run twice:
  46 of 46 killed each time); `just e14-todomvc`.
- The runs: `just e14-todomvc` records both in `docs/evidence/E14/todomvc.txt`
  and fails where either is not the outcome the app's design gives. Locally,
  2026-10-15, on Cypress 15.21.1: the acceptance 3 of 29 passed, the diagnostic
  28 of 29 passed and 1 skipped by the suite.

## Not claimed

- **A signal of another type than `String`**, or a form that names a case.
- **The signal writing the address**, or a handler that moves the page by
  setting a form (PW6105 refuses it).
- **A page shown in a layout** (ADR-0303): a navigation that keeps the layout
  hands the document to the next runtime, whose listener replaces this one's
  (`life.signal`); no test holds a fragment-bound page inside one.
- **The links of the views a page composes** (PW6106).
- **That the official suite, unchanged, passes this app.** In the acceptance 3
  of 29 pass; 23 tests ask `localStorage`, which this app never writes, and each
  fails or would fail there by design (section 5). The official persistence test
  (24) is never run for this app: it asks `localStorage`, and the suite skips it
  for the name the diagnostic uses. What the diagnostic shows is that each
  test's other assertions hold, for one name's table: not the acceptance.
- **That Cypress test 11 passes**: it passes or loses a race with the host's
  answer, by timing (section 5), and the judge lets it do either.
- **Editing is the page's signal, and its cost.** Another tab's change to the
  row being edited drops the edit in progress, and each begin and end of an
  edit is a read the server renders (ruling 1: kept for the first milestone;
  per-row signal instances, ADR-0137 and PW5307, are a separate language
  track).
- **A session's list beyond 500 todos, or a title beyond 1000 characters**
  (rulings 8 and 9): both are limits, each with its test.
- **`autofocus` that leaves a person's focus alone.** A patch that inserts an
  element with `autofocus` takes the focus from wherever a person is, as the edit
  field must; a page that inserts one for another reason would take it too, and
  nothing refuses that. The `selected` property of an `option` is not synced as
  `checked` is.
- **The device's own storage, and a keyboard way to begin an edit.** The
  version on `localStorage` waits for charter 9.1's fourth kind of state; the
  spec's way to begin an edit is a double press, which a keyboard cannot make.
- **A browser other than Chromium, Firefox and WebKit** for the page, and any
  engine but Electron's Chromium for Cypress.

## Questions for the integrator

1. **Wake the stream when a frame is queued** (section 5, "What was found" 4).
   `stream_open` reads a session's queue every 25 ms, so a server-driven update
   takes a median 24 ms to reach the page, on any build. A `Condvar` beside
   `pending`, notified where a frame is queued, with the 25 ms kept as a bound,
   would deliver in a few milliseconds; about thirty lines, and a change to the
   timing of every page, which the track did not make (no suite other than this
   track's could be run to check it). Without it, Cypress test 11 passes only
   when the machine is quiet.
2. **A verification shard for `e14-todomvc` needs Cypress's binary and Xvfb.**
   The recipe fetches the binary itself (`pnpm exec cypress install`, a zip of
   about 249 MB; nothing where it is installed) and fails without Xvfb. The
   GitHub image `ubuntu-24.04` lists `xvfb` among its installed apt packages
   (2:21.1.12-1ubuntu1.8, read 2026-10-15). Not checked: that Cypress's own
   libraries (GTK 3, NSS, ALSA, GBM) are all there; the image's list names none
   of them, and they may come with the browsers it carries. Provisioning the
   binary in `verify.yml` instead is the other choice.
3. **Cypress 15.21.1, not 15.14.2** (section 5): a deviation from upstream's
   lockfile, for the six advisories; the owner's approval names the package and
   its binary, not a version.

## Next

- **TodoMVC's second version**, on the device's own storage (charter 9.1's
  fourth kind, which starts with the Docs app), where the official suite's
  `localStorage` tests become questions the app can answer.
- **Per-row signal instances** (ADR-0137, PW5307), so that editing a row is the
  row's signal and not a page read; then the workaround here goes.
- **The tutorial's first lesson and the comparison** the plan names, which this
  milestone's program and evidence are the ground of.
- **A keyboard way to begin an edit**, where the spec has none.

## Numbered on train-12 (2026-10-20)

- **Numbered ADR-0367** (the integrator's ruling of 2026-10-20: a train is
  numbered before it is pushed, after master's ADR-0363, in train order),
  merged from `a8bab6a2` (`track/todomvc`, merged inside
  `track/device-store`). `ADR-XXXX` reads `ADR-0367` in the track's code,
  demo pages, recipes, tests and runtime (30 mentions in 20 files), as in
  this file's name and title.

## Native TodoMVC filter-boundary witness (2026-10-21)

The unchanged original mutation operators 10 and 11, followed by a bounded
strict sweep of operators 12–20, were run against the same 50 native browser
tests. The filtered-update test now starts with one completed row and
checks exact visible row titles after each filter transition before taking an
action through a row locator. This makes the Active and Completed boundaries
observable before an action can target a stale list.

The initial focused run had 50/50 browser tests pass on baseline and
restoration, and 4/4 host tests pass on both. Operators 10 (`the active
filter shows the completed`) and 11 (`the completed filter shows the active`)
were each killed by native assertion failures; neither was refused. After the
empty Completed-filter transition was restored, the final-source 10–11 rerun
again passed baseline/restoration 50/50 browser and 4/4 host tests and killed
both operators by native assertions, with no refusals. The diagnostic
sweep's baseline and restoration also passed 50/50 browser and 4/4 host tests;
all nine unchanged original operators 12–20 were killed by native assertions,
with none refused or surviving. Both bounded campaigns used the existing
strict receipt validators and retained their native JSON receipts. This is a
focused witness, not a rerun of all 43 operators or the broader recipe.

The original 12–20 sweep used source SHA-256
`0397aca273bdf2a49e95a90a063428def7af403d08b5ff1e25795ba6b695ea93`. The
final 10–11 source SHA-256 is
`260d8f30037942c17d8d936087ced87603044a11da38e18eac7291f1b83b95c2`. Its
raw summary, harness, and receipts are recorded separately under
`docs/diagnostics/train-12/todomvc-filter-boundaries/final-source-operators-10-11/`.
The selection and strict receipt producers were unchanged.


## Landing (2026-10-21)

Landed on `master` with train-12 at `e7664d2eb01aa8c6e91df594bd7270a84a8bf8de`
after exact-commit CI ([39584219053](https://github.com/studiokimeiga/perfect-web/actions/runs/39584219053))
and automatic track verification ([39575516650](https://github.com/studiokimeiga/perfect-web/actions/runs/39575516650)).
Dated statements above that call landing or combined gates pending describe
the candidate before it landed and are kept as written. The
[acceptance record](../diagnostics/train-12/acceptance-record-2026-10-21.md)
states what the landing's evidence does and does not prove.
