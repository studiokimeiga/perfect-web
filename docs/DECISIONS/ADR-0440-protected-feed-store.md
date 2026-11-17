# ADR-0440: protected feed and store data use the declared-table gate

Status: accepted in part 2026-11-13, the integrator's ruling under the
owner's delegation of 2026-10-02 and order of 2026-10-25: L3 of the
declared data layer's landing (its plan of 2026-11-12) lands the first
three prerequisites below, the platform's (the shared outbox, the
Identity lifecycle, owned Clock actions; see
[Landing](#landing-l3-2026-11-13)); the fourth (uploads, L5), the fifth
(bounded reads, L4) and the apps' port (L7) stay proposed, each accepted
as its train lands. Proposed 2026-11-04 as a local migration on the
integration branch (`fix/row-data-integration`), starting at `a9a8d79e`,
the slice L2 landed. Milestone: E14. Builds on ADR-0338, ADR-0350,
ADR-0385, ADR-0386, ADR-0389, ADR-0393, ADR-0395 (owned tables), ADR-0397
(durable tables) and ADR-0369. The decision was recorded before
implementation.

## Decision and preservation contract

Application rows, projections, transitions and application predicates belong
in Pleris. The canonical feed and store use the same generated table gate
and generic memory/PostgreSQL adapters as the owned-row integration. Identity
provider verification, blob leases, payment signatures/secrets and clock
delivery remain generic platform responsibilities. Existing browser routes,
command results, account isolation, event declarations and transaction
outcomes remain contracts of the migration.

The current startup guards require one data layer per program and refuse
uploads that generic tables cannot retain. They continue to hold until their
specific generic capability is implemented and proved. Source inspection
found the following prerequisites; migration will close them explicitly.

1. Generic PostgreSQL outbox delivery must preserve the apps' existing
   cross-host behavior: commit-ordered IDs, host skipping, polling after a
   lost wakeup, age-based trim with a watermark, reload after missed rows,
   and host-specific page numbers. Reuse the existing Tail protocol and
   the generic adapter's existing transaction lock. NOTIFY is a wakeup;
   retained rows are authoritative. A local delivery does not delete rows
   that another host still needs.
2. Store guest-cart joining needs a generic Identity-origin lifecycle
   command. Its source/target handles come from a checked provider rotation
   and immutable transition proof. The operation can read/delete the old
   guest's rows and read/write the new principal's rows, under its declared
   table grants, in one transaction with its record and outbox. Quantity
   merging is Pleris business logic. Browser arguments grant no transition
   authority; a mutable principal-cache observation is no transition proof.
3. Clock-origin owned-order actions reuse the existing persisted run owner
   and host-created on-behalf binding. The run's generation/completion fence
   remains in the same database transaction. Tests must execute real owned
   reads/writes after logout and reject wrong owners, stale/replayed runs
   and direct browser invocation.
4. Generic upload operations must preserve claim ownership and lifetime:
   failed commands return leases, successful rows and blob publication
   commit together, and deleting the last reference stops serving a blob.
   Feed upload compatibility is proved before removing its startup refusal.
5. Generated bounded reads must preserve the existing databases' bounded
   result work. Applying List.take after an unrestricted SQL read is not
   sufficient. Verify substantial cardinalities, rows/bytes and fuel as
   well as SQL-call counts. Per-owner quotas and broader cursor/H4 work
   retain their own scope; no H4 completion is inferred here.

## Application modeling

Feed data includes users/profiles, posts/thread roots/stable IDs, likes,
follows, notifications, messages and per-reader read marks. Public posts
and private activity have distinct declared visibility. Owner-only writes
continue to hold; recipient read marks are recipient-owned rows. Compiled
OwnsPost and MayMessage use the invocation's staged/snapshot reads.

Store data includes catalogue/menu/hours/fees/promotions, carts, addresses,
checkout choices, promotion uses, orders/charges/payment references and
scheduled times, and ratings. Compiled eligibility predicates preserve
authoritative pricing, stock/reach checks, quantity-overflow refusal,
latest-order eligibility and rating uniqueness. Public reference data is
declared alongside protected rows so the canonical program uses one adapter.

SignedIn remains the verified deployment identity predicate. Cross-user
activity is modeled explicitly using public rows or declared sharers and
separate owner-only marks; private records are never made public merely
to make an operation expressible.

## Evidence required before local completion

Start with failing regressions and valid neighbors at each new platform
seam. Re-anchor negative controls to the new owner of each rule. Tests must
assert which adapter is active and inspect real persisted rows/restarts;
old PW_FEED/PW_STORE settings must not silently exercise memory tables.
Existing browser/API assertions remain in force. Historical legacy fixture
tests are labeled separately and cannot prove the migrated canonical path.

Run the affected identity/session, upload, payment, scheduling, cross-host,
feed/store and table campaigns, plus preflight, test-compile, compiler
controls and full CI at a frozen source. Record exact generated CI inputs,
independent review, restoration and original-worktree preservation.
No prior-SHA, cancelled, invalid or skipped result receives passing credit.

Both original dirty row/durable worktrees remain untouched. No production
database conversion, remote publication, deployment, paid study or H3/H4
completion is authorized or claimed by this local implementation task.

## Outbox protocol references and first witnesses

PostgreSQL 18's [NOTIFY](https://www.postgresql.org/docs/18/sql-notify.html)
delivers transaction notifications after commit; notifications carry an empty
payload here and the retained log carries data. Its
[LISTEN guidance](https://www.postgresql.org/docs/18/sql-listen.html) requires
listening before inspecting the log. The generic publisher keeps its existing
transaction advisory lock, as described by PostgreSQL's
[locking documentation](https://www.postgresql.org/docs/18/explicit-locking.html).
No dependency upgrade was introduced.

The first real-database regressions failed at absent generic host instances
and absent remote live-document updates. The repaired witness checks keyed
insert markup as well as replacement markup; a text-only test helper omits
keyed-list insert operations. Further witnesses check the publisher's actual
lock under a database-observed wait, wake after commit, retained rows,
missed-trim recovery, fresh-host cursors and bounded maintenance batches.
These are working-tree diagnostics until their frozen-source gates finish.

### Protocol upgrade boundary

Stop all older generic-table workers before opening a deployment with platform
0003/shared tails. Their immediate outbox deletion does not advance the new
trim watermark, so mixed old/new publishers are unsupported. This local task
does not convert production data or authorize a deployment. Relation names
introduced by 0003 are reserved at manifest admission; an older app that used
one needs an explicit application-table rename before the platform upgrade.
Tail startup/poll SQL uses its dedicated connection and is not counted by the
adapter's statement counter. Bounded-read claims must say which connection
and row/byte work they measured.

### Identity lifecycle authority

`invoked_by Identity` names one parameter-free, idempotent command. Browser
routes and page handlers cannot invoke it. `context.previous_user()` returns
an optional guest handle only during that command; `current_user()` is the
verified target account. Outside the lifecycle it returns None. A host-private
one-use transition scope binds the original admitted request identity before
provider exchange, source `guest_user(old_session)`, exact newly opened target
principal/epoch, and lifecycle component. It retires on every exit.

The engine accepts transition callers only for Identity command exports. A
previous guest handle can read/delete owner-keyed rows, never insert/update
old rows or read through a sharer index. The current handle has ordinary owner
authority. Both remain restricted by the command's generated table grants.
The lifecycle uses a real new-session actor and existing local/same-transaction
PostgreSQL fences, rather than the general platform actor exemption. Its row
merge, old-row deletion, receipt and outbox share one transaction. A definite
rollback preserves the old guest rows. An unconfirmed result can follow a
successful commit (authority changed before delivery) or an indeterminate
acknowledgement; it establishes neither rollback nor retention. Stored rows
and receipts remain authoritative. Sign-in and row merge are separate outcomes.

Anonymous means no current account, not no authentication history. The proof
never grants a source account id or legacy session-keyed rows; account-owned
rows from an expired sign-in are not guest rows. Old application adapters do
not become alternative data paths for a declared-table program.


The current host refuses an Identity lifecycle program without the generic
declared-table layer at startup, by name; it never routes transition handles
through legacy operations. Legacy user-keyed cart compatibility uses only the
guest-user source. Legacy session-keyed carts are not transition sources:
anonymous admission cannot establish that their rows have no account history.
The target actor wrapper checks current authority for recalled lifecycle
receipts as well as fresh execution. Private source/session fields, not public
presentation fields, drive the proof binding.

### Counts over declared selectors

The next prerequisite adds `count_by_<fields>` and its `_many` form over the
table's existing key prefixes (including its whole key) and indexes. This is
an aggregate of an already declared read, with the same table grant, selector
validation, owner/sharer handle and transaction/snapshot; it is not a query
language, arbitrary filter, or expanded row authority. No unqualified whole-
table count is generated. Owned set forms still omit a form with nothing
after the single reader handle, as existing row reads do.

A single count returns a nonnegative Int. A set count returns one generated
`{ key, count }` record for each distinct requested selector, including zero
for absent selectors, in the selectors' canonical order. The key is the plain
selector value or tuple, never a returned authority handle. The gate checks
the entire result shape, key membership/order/uniqueness, and nonnegative
counts before the component sees it. A SQL adapter executes one grouped
aggregate for the set, not a loop or a fetch of every row followed by length.
Memory is the answer oracle; SQL preserves the same zero/duplicate semantics.

Acceptance includes plain/owned/shared selectors, staged and snapshot reads,
forged owner/key/answer refusals, and large-cardinality SQL returned rows/bytes
and compiled fuel bounds in addition to call counts. Database aggregate scan
work is not a constant-work claim; per-owner quotas, ranges/windows, relation
fusion and broader H4 remain separate. Limits elsewhere must still execute
before unrestricted row materialization; adding counts does not complete the
canonical apps' bounded-read prerequisite or authorize relaxing startup guards.

PostgreSQL uses multi-array `unnest` with ordinality and a left join, counting
a non-null primary-key column; this retains one zero per absent requested key.
The client pairs validated one-based slots with its canonical typed input keys.
These semantics are documented by PostgreSQL's
[array functions](https://www.postgresql.org/docs/18/functions-array.html),
[table functions](https://www.postgresql.org/docs/18/queries-table-expressions.html#QUERIES-TABLEFUNCTIONS)
and [aggregates](https://www.postgresql.org/docs/18/functions-aggregate.html).
No dependency or stored-schema migration is introduced by the new read forms.

Adapter counters now measure actual prepared-statement result rows and binary
field payload bytes before decoding, without retaining data. They exclude NULL
markers, protocol/row framing, simple-query results and the dedicated Tail
connection. Prepared startup catalog/schema reads are included; measure deltas
around application operations. A count may scan many database rows; those
internal scans are not these result counters. A compiled count is tested under
the same 1,000-fuel ceiling at 5 and 1,500 rows, with a one-fuel failing neighbor;
this is a ceiling test, not an exact fuel-used or constant database-work claim.
The set-count witness uses a 3,000-fuel ceiling for two distinct requested keys.

Generated full selector/count records are checked by emitted type identity,
including Count-suffix and owned full-key collisions. Host manifest admission
also refuses an owned set count with no selector after its reader. Per-element
count loops retain PW0405 and the generated set-form repair. The existing
table, owned-row and PostgreSQL recipes include the new witnesses and controls.
Frozen-source verification and review are recorded separately; the migration
and its final browser/whole-campaign/CI acceptance remain open.

Independent review of the first counts checkpoint found a function-name alias:
scalar selector `a_many` and set selector `a` both spell `count_by_a_many`.
Planning now coalesces only the same name/primitive/selector; distinct meanings
remain visible to the existing named duplicate-import refusal. Both key/index
orders and renamed-field neighbors have a native regression. Review also found
that the new result counters included startup prepared reads; their contract
now says so, with open/reopen witnesses and unchanged per-operation delta tests.
The interrupted broad check at the first checkpoint receives no complete-suite
credit. Successor frozen checks and review remain required.

The first successor server run exposed the generic conformance suite's old
row-only assumption (166 passed, two PostgreSQL conformance failures). The
oracle now computes scalar counts and canonical distinct key/count results
from its own generated rows at both sizes, outside and inside a render. It
does not call an adapter or the production result codec. A small independent
oracle witness includes scalar, absent, duplicate, empty and tuple selectors;
the whole generated guestbook/lists suite runs on memory and PostgreSQL.
Those two failed results are retained as diagnostics, not passing credit.

Counts code/test source `15f3c9df` passed its frozen affected native checks,
test-compile/preflight and focused strict controls, with independent review
clear. The [counts record](../diagnostics/protected-feed-store-counts-2026-11-04.md)
links the exact-source receipts. This closes counts only, not the apps or their
whole-campaign/browser/CI acceptance.

### Next bounded read: ordered selector windows

The next slice generates `first_by_<fields>` and `last_by_<fields>` over the
same existing nonempty key prefixes/full keys and indexes. Each takes its
usual selectors and an Int limit, returning a list of full/seen rows under
the same grant and owner/sharer authority. The `_many` form gives the existing
typed selector list and one shared limit, returning at most that many rows
**per distinct selector**. It preserves scalar-to-set meaning for PW0405;
it is not a global window across all selectors. Owned forms with nothing
after the reader still have no `_many`. Generated bound-parameter names must
remain distinct from app selector names, including their emitted identities.

First orders by the table's declared order then full primary key ascending;
last reverses that complete within-selector order, including key tie-breaks.
Set results group by canonical selector ascending, then the within-group
direction. Validate every selector before an empty/nonpositive bound answers
empty. Negative limits clamp to zero as the feed does today. Do not reserve
memory directly from an untrusted maximum Int. Gate answers for exact rows,
membership, uniqueness, order/direction and per-selector cardinality; refuse
invalid results whole rather than silently truncating them.

PostgreSQL applies ORDER BY/LIMIT before returning rows: one prepared statement
for a scalar window or one unnest/ordinality/LATERAL statement with a bounded
subquery per distinct key. Memory may scan matching row references while
keeping bounded top-N metadata, cloning only selected rows. No new query
language, arbitrary filters, optimizer, implicit index/schema migration,
constant database-scan work or H4 closure is introduced. Ordered prefix reads
may need a matching declared index for efficient internal database work.

Acceptance includes compiled owned/shared callers, no/forged-caller refusals,
zero/negative/maximum limits, missing/duplicate/tuple/typed keys, order ties,
staged/snapshot/reopen behavior, malformed excessive/foreign/reversed results,
generated-name collisions, PW0405 repairs, generic conformance and strict
controls. Measure SQL result rows/payload bytes and compiled fuel at substantial
cardinality with growing unbounded neighbors. Following-feed selection still
needs fixed relation/global-window support without first materializing every
followed principal or per-author window. This slice cannot claim that behavior
proved; unread/range and upload lifetime remain separate pending work.

The local window implementation uses those fixed forms. At 12 and 1,500 rows,
the compiled scalar window returns two SQL result rows and the shared set
window returns four, using the same 10,000-fuel ceiling; payloads are bounded
for the fixture's fixed-size text fields. An unrestricted row-read neighbor
grows with cardinality, and a one-fuel neighbor fails. A strict fault fetches
all scalar-window rows and clips the correct answer afterward; only the real
result-size oracle detects that hidden materialization. These are result-work
measurements, not a claim about internal database scans or unbounded row sizes.
Raw adapter calls for invalid/empty windows run no SQL. A compiled snapshot
can still open/end its transaction; those boundaries remain separate from
operation-statement and returned-payload counters.

A compiled latest-row predicate sees its command's staged insert, rejects an
existing non-newest row before the body, and shares a held query snapshot
with the body after another writer commits a newer row. The same fixture
runs on memory and PostgreSQL. Generic conformance covers five limits, missing
and duplicate selectors, direction and key ties, for every table of the
guestbook/lists builds in both render modes and sizes. Original row/count
oracles and statement-cost assertions remain in force.

Parameter-name tests exposed pre-existing generated-formal collisions under
legal owner fields: `values` collided with a set's list, and `row` with a
write's record. Only colliding generated formals are renamed, preserving
argument positions, field names and permissions. Reader/list/bound names are
also checked by emitted identity and by real compiled components for owner
fields values/keys/row/limit. A fixture that mutates a table key now adjusts
new window metadata to keep its old type-refusal oracle otherwise whole.
The initial implementation was frozen for the checks recorded below; later paragraphs track review fixes and final evidence.


Review of immutable window source `79de0dd0` found two diagnostic defects,
reproduced by native regressions (five existing tests passed, two new tests
failed). PW0405 described an owned set's first parameter, User<UserId>, as
the list's record and included the reader in every selector. Guidance now
locates the top-level List parameter by constructor, preserves its complete
element type, and describes the reader separately. Owned/shared scalar and
composite cases and the existing unowned neighbors are checked. A loop whose
window bound varies previously offered the single-shared-bound set as a
replacement. Direct/wrapped varying-bound cases now get common-bound and
pure-selection guidance, while an invariant-bound selector loop still names
its matching set form. The refusal and generated ABI are unchanged.

The new window snapshot fixture now changes the pinned row's body as well as
inserting a newer predicate row. A held query must retain both its eligibility
and original body; a fresh query is denied. Both adapters execute the compiled
fixture. The initial follow-up preflight exposed one moved bounded-checker
mutation anchor; it is re-anchored to the same scalar/composite decision and
must still be killed. These local review fixes require their own frozen checks
and independent delta review; earlier source results are not transferred.


Combined selector-and-bound dependencies now have direct/wrapped native
neighbors: the suggested set read is explicitly one common bound followed by
pure per-request selection. The shared helper's older count output guidance
also now describes key/count entries and a zero for a missing key, rather than
rows and absence. Its new regression failed at the former wording before the
repair (seven passed, one failed). Existing unowned row-output guidance remains
unchanged. The moved flat-batch mutation anchor retains the same wrong repair;
a new count-guidance fault must be caught at a native assertion. No admission,
ABI, runtime or stored-schema behavior changes in this follow-up.


The [window execution record](../diagnostics/protected-feed-store-windows-2026-11-04.md)
records the frozen 79de implementation checks, d417 snapshot/diagnostic proofs
and final 5b7 guidance checks, with raw logs, exact source guards and hashes.
Independent review is clear. This closes ordered selector windows locally;
the canonical apps and their final whole-campaign/browser/CI acceptance remain open.


### Next bounded read: counts after an order mark

Notification rows disappear with deleted posts. Subtracting an historical read
count from the current total becomes wrong when old read rows disappear. A
recipient-owned last-read sequence plus a count of current rows strictly after
that mark preserves the intended unread total. Message totals may instead use
their append-only history and separately owned read totals; that app model is
not established by this primitive. Stable sequence allocation across deletion
is an independent application/platform obligation: MAX(current rows) must not
permit a new notification to reuse a mark already read.

Generate `count_after_by_<fields>(selectors..., after)` only where the table
declares `order`, over the same nonempty key prefixes/full keys and indexes.
The trailing argument has the order field's complete declared type, including
nominal identity and refinements. It is a strict comparison of that one field:
order > mark. Equal-order rows are excluded regardless of primary-key ties.
This is not an order-plus-key cursor, arbitrary filter or query language. No
global count, new row authority, dependency or stored-schema migration is added.
Use ordinary count for an absent initial mark where the order's refinement has
no representable before-all sentinel. Never bypass the refinement for a mark.

ADR-0338 section1.2 requires a set form for every keyed read. Generate a
heterogeneous `_many` form now, not a shared-mark substitute. Each input is a
generated record of the ordinary selector fields plus a mark field. For an
owned/shared read, pass the reader handle separately and omit its field from
each input record. Even a reader-only selector has a meaningful set form: its
input record holds the mark alone. The old empty-after-reader set prohibition
does not apply to this new form because the mark varies. Every key/cutoff is
validated before adapter access; any forged reader or malformed member refuses
the whole request, including when no range matches.

Choose the generated mark field/formal as `after`, adding `_mark` until its WIT
identity differs from every stored row field. Use the same chosen mark name for
all the table's range records. Input range records are named from their held
selector fields with an After suffix; full returned key records include the
plain reader id where relevant and the mark. A set returns `{ key, count }`
entries for distinct full selector-and-mark pairs, including zero for missing
ranges, in canonical selector-then-mark order. Same selector/different mark
remains two requests; exact duplicates coalesce. Generated type/import collisions
must retain the existing named refusal instead of silently aliasing meanings.
The ordinary equality `by` metadata remains separate from the mark. Range
aggregates return no stored rows, so their stored-row sorted_by is empty;
canonical request/result ordering is defined by this primitive's full key.

Use the same table read grant, owner/sharer handle, command staging and query
snapshot. Existing private-order restrictions for shared tables remain in force.
The host independently admits the declared order, typed input and exact scalar/
set answer ABI, request membership/order/uniqueness and nonnegative counts.
Returned keys are plain values, never authority handles. PW0405 still refuses
per-element selector, mark or combined dependencies; its repair names this
heterogeneous set and includes the mark in each record, with the reader passed
separately. A window's one-common-limit guidance does not apply to range counts.

Memory counts matching row references without copying row payloads. PostgreSQL
uses one COUNT with equality selectors and order > typed mark, or typed-array
unnest/ordinality plus a left-join aggregate per distinct selector/mark. Counting
a non-null primary-key column preserves zero for missing ranges. The mark is
bound using the declared order column's native kind, including bytea for strings,
so no text-collation or JSON comparison replaces the existing value order.
The [comparison documentation](https://www.postgresql.org/docs/18/functions-comparison.html)
and [binary-string documentation](https://www.postgresql.org/docs/18/functions-binarystring.html)
were checked. A local PostgreSQL 18.6 synthetic probe confirms Bool and UTF-8-byte
comparisons, including NUL/prefix strings, and distinguishes strict1 from
inclusive3 over [1,2,2,3] at mark2. No dependency upgrade is introduced.

Acceptance: native failing regressions and valid neighbors; owned/shared/reader-
only heterogeneous sets; missing/exact-duplicate/different-mark requests; equality
ties and deletion before/after a mark; Bool, Unicode/NUL String, refined/nominal
Int types; generated field/formal/type collisions; no/forged callers and malformed
whole sets before callback/SQL; actual compiled staged and held-snapshot reads;
independent conformance at both sizes/render modes; substantial-cardinality SQL
result rows/bytes and compiled fuel with failing/growing neighbors. Strict faults
must catch ignored/inclusive/wrong-field marks, lost zeros/marks, per-key SQL and
unrestricted rows fetched then counted. Freeze exact checks and independent review.
Database internal scan work, sequences, relation/global windows, upload lifetime,
initial fixtures, private-child deletion and canonical app ports remain separate.


The [range execution record](../diagnostics/protected-feed-store-ranges-2026-11-04.md)
records frozen implementation f4fc668f and its script-only wrong-field-control
successor 06a6ad18. Independent review is clear. This closes typed order-range
counts locally, not global relation windows, upload lifetime or the canonical
apps and their final whole-campaign/browser/CI acceptance.


### Next fixed read: relation windows

Decision made before implementation, 2026-11-04. Independent read-only design
review accepted the fixed one-hop rewrite with the limits below. Implementation
and all native/strict/frozen acceptance remain pending. No app port or startup
guard relaxation follows merely from this decision.

Problem: following timeline is newest root posts globally among own/followed
authors. Fetching all membership rows or every author's N posts before clipping
does not meet the result-work bound. ADR0338 permits a fixed declared relation
rewrite, not an optimizer/query language. Root-only public posts can be modeled
as a separate declared table maintained in Pleris, avoiding a filter expression.
Pleris maintains a separate private subscription edge for self; it is distinct
from visible following counts and follows, and must be tested on first post,
following before/after another author's posts, unfollow, restart and delete.

Declare a fixed membership relation:

    table Subscriptions
        row SubscriptionRow
        key reader, author
        owned by reader
        window posts from RootPosts row RootPosts.RootRow on author to author

The explicit row annotation uses ordinary imported type resolution. Generation
runs in single-file lowering, before a Workspace exists; discovering a target's
chosen row name would otherwise require a new cross-unit generation/reindex
phase across checking and executable preparation. Keep the narrower typed
declaration. Later semantic checking requires the annotation to be the target
table's exact resolved row type, including nominal identity/arguments, never
merely matching fields or WIT shape. Runtime row metadata comes from the validated
target table, not a second annotation authority. Cover structurally identical
different row types, missing/wrong target, stale/private/nonexported annotation,
qualified/imported aliases of the same type and reversed file order.

This declares the equality Subscriptions.author=RootPosts.author. RootPosts is
an ordinary PUBLIC target table with a declared order and an index/key starting
with the target author field. No private target, projection/filter expression,
multi-hop relation, arbitrary SQL, aggregate or implicit cascade is introduced.
Both tables must be in one declared source/adapter/snapshot/transaction. The
compiler proves the match fields have the same resolved declared type (including
nominal identity and refinement), and a comparable scalar stored kind. WIT erases
nominal identities; the host's independent check is complete Stored kind and
refinement, as at the existing row boundary, not nominal equality inferred from
unqualified type spellings. Duplicated membership rows must not duplicate
target rows: EXISTS/semi-join semantics.

Generate fixed first_posts_by_<selector> and last_posts_by_<selector> reads.
Eligible selectors are nonempty membership key/index prefixes with the
source match field immediately after them in that key/index. This ensures an
existing declared lookup for (selector, target-match-value); do not invent an
index. Ordinary index reads currently expose only complete index field lists;
this clause explicitly authorizes these relation-only index-prefix selectors,
without broadening ordinary row reads. Owner/sharer restrictions follow the source selector; private source
columns never reach the app. The target row is public, exactly its declared row.
Each generated function requires BOTH table read grants. The host independently
checks both declarations/source identities and target-public restriction, stored
match types/refinements/indexes, function metadata and complete ABI before execution.
Host table admission must retain source identities (it currently drops source),
require the same nonempty explicit source, and refuse cross-source co-location
even on memory. Extend the existing compiler ambiguous bare-source-name guard
to the target. Duplicate relation names are named-refused by emitted identity
before expansion; plan coalescing includes relation/target meaning, not merely
name/primitive/by. Generated record/import aliases retain named refusal.

Both read effects must survive generation into signatures/contracts, and the
engine independently requires BOTH grants at admission/link, including predicate
reuse and unused imports, before callback. A sharer relation cannot match a
private source field: existence of the target result would reveal that field.
Use a named refusal analogous to the existing private-order restriction whenever
an eligible sharer selector's source match field is not Seen. Owner-only matching
may use its own private fields; private source values are never returned.
The selection itself remains private when its source membership is private,
even though each target row is public. Preserve source-derived privacy/cache
labels through the verified reader handle; pin PW5001 for a shared-cache attempt,
its private-cache neighbor and two-reader same-target/key isolation. No H3/H4
closure is inferred from this narrow prerequisite.

Scalar takes source selectors (including verified reader handle where relevant)
and Int limit; returns List<TargetRow>. Limit clamps nonpositive to empty AFTER
validating every selector. First orders by full target order+PK ascending; last
reverses ALL of it including ties. One global limit per source selector, not per
matching author. No maxInt reserve. Target-only row ordering metadata must not
pretend that source fields exist on the target row.

Every eligible keyed read gets its ordinary heterogeneous-selector _many form
with ONE common limit. Reuse the existing owned reader-only empty-after-reader
exception (there is no varying remaining selector); it has no meaningful set
parameter. For owned composite selectors, reader is separate and list contains
only remaining typed source selectors. Public source selectors get ordinary set
forms. Unlike range counts, no per-request varying mark/limit is introduced.

A relation set cannot return flat target rows whose fields don't identify their
source selector. Return a generated List<{key: full plain source selector,
rows: List<TargetRow>}> instead, exactly one group per distinct canonical input
key including zero-row groups. Keys contain plain ids, never handles. The gate
checks exact request coverage/order/membership, no duplicates, each group's bound,
target row ABI/PK uniqueness/order. It checks the scalar target rows the same way.
Target PK uniqueness is per group, not global: overlapping topics may each return
the same post. A duplicate source edge within one topic still yields it once.
This is an adapter contract: the two generic adapters execute the admitted match.
As for ordinary adapters, the gate checks shape/authority fields, not independent
proof that a row exists in the physical database. Target rows are public; this
first version must not generalize to protected target rows without a new design.
Public targets do not make foreign private membership public. Distinct Alice/Bob
membership sets and sentinel posts must catch wrong-source-reader selection and
authority injection faults, alongside forged group-key refusals.

Memory scans borrowed target row references and tests existence through the
declared membership key/index for each source selector and target match value,
retaining only bounded top-N refs; clones only final selected rows. No all-follows
list or per-author post windows. PostgreSQL scalar is one SELECT of target columns
WHERE EXISTS (source equality selector AND source-match=target-match) ORDER/LIMIT.
Set is typed arrays+ordinality and LATERAL, with global target ORDER/LIMIT INSIDE
each selector subquery. Adapter reconstructs empty output groups from validated
keys; zero SQL rows need no fake target payload. Result work <=N*distinctKeys,
not N*author-count. No constant internal database-scan-work claim.

Primary documentation checked: PostgreSQL 18
[subquery expressions](https://www.postgresql.org/docs/18/functions-subquery.html)
(EXISTS/correlation) and
[SELECT](https://www.postgresql.org/docs/18/sql-select.html)
(ORDER/LIMIT/LATERAL). Require a local synthetic
probe with duplicate edges, interleaved authors/ties/missing selectors and a
per-author-window+globalclip fault, plus real compiler/adapters before acceptance.

The global home timeline can use an ordinary constant-partition index in Pleris
(for example, every root row has lane0) and last_by_lane(0,N). No unqualified
unbounded target read is added. Public profiles use last_by_author(id,N); a full
thread's result is inherently all rows of its declared root index, assembled in
Pleris. These are app modeling proposals requiring their own unchanged assertions.

Native acceptance: failing regression+valid neighbor first; compiler distinct
nominal types with equal representations AND same local spelling across modules,
valid aliases of one imported type, complete refined types, host forged Stored
kind/refinement negatives, name/record/formal collisions, no/forged reader, malformed whole
sets/limits before callback/SQL, both grants and target/source schema tampering,
public-target admission, shared source seen/private fields never returned, staged
membership and target writes, held snapshot changes BOTH membership and body,
first/last PK ties, zero/missing/duplicates/maxInt, independent conformance.
At12/1500 authors and substantial posts, fixed SQL result rows/payload bytes and
compiled fuel; unbounded/per-author neighbors must grow. Strict faults: match
ignored/wrong field, source reader ignored, join duplicates, global set limit,
per-author limit, fetchall membership or target then clip, per-key SQL, missing
dual grant/admission, group result coverage/order/bounds, counters bypassed.
The per-author-window work fault must return those extra rows across the SQL
adapter boundary and clip in host while returning the correct final N. Use one
SQL statement so real result-size counters, not just N+1 call counts, detect it.
An internal SQL expansion ending in global LIMIT before returning N is internal
database work and outside the claimed result-work bound.

Additional review witnesses: duplicate relation names with different targets;
equivalent/different/absent/ambiguous sources and forged memory co-location;
missing target grant in bodies/predicates/unused imports; private match field
through sharer rejected and Seen neighbor valid; overlapping groups duplicate
target PK across groups valid, duplicate edge in one group yields one target;
reversed/foreign/missing/over-limit nested replies are whole refusals.
Positive relation-only index-prefix case without a separate prefix index, and
negative with no declared (selector,match) lookup path.

The [local synthetic probe](../evidence/E14/protected-relations-design-2026-11-04/primary-probe.log) ran against PostgreSQL 18.6: duplicate
Alice->A edges plus A/B interleaved/tied posts still yield global b3,a3 at N2;
Bob->C yields c9, absent selector yields no SQL row, overlapping topics each get
the same A posts independently. All fixture tables were TEMP and rolled back.
This verifies selected SQL semantics only, not the not-yet-built generic path.

### Local relation prerequisite record

Frozen source `d8c634c9` passes affected compiler 350, CLI 5, host 118 (9 existing
ignored), memory 132, PostgreSQL 95, selected server 180, test-compile/preflight 5,253
and strict 36/36 focused controls. The test-only `9451277a` adds actual
authorization-only reuse proof and passes memory 8/preflight/dual-grant 2/2.
Separate immutable review is clear, including source-key Identity authority,
independent contract/actual grants, named schema refusals, private-match paths,
warm two-reader caches and predicate reuse. Correct-answer all-target and
per-author postclip controls are caught by real SQL returned-row accounting.
See the [relation record](../diagnostics/protected-feed-store-relations-2026-11-04.md).
No canonical application has been ported by this prerequisite.

Remaining after this: generic upload claim/publication/reference/GC lifetime,
initial public fixtures, stable IDs/private child deletion as required, actual
canonical feed/store ports, final whole campaigns/browser/CI and frozen review.

### Next seam: declared public upload references

Decision recorded before upload implementation, after relation record `ff57edc7`.
This is a local generic capability prerequisite. Existing mixed-layer/upload
startup refusals stay until the complete replacement passes native checks on
both adapters and the actual server path. No legacy data conversion is implied.

**Declaration and authority.** A generic upload is declared in the module of
its own name, following tables' existing module convention. Legacy inline
uploads remain legacy fixtures until ported. Generate ordinary module types
`Claim`, `Ref`, `Kind`, `Attached`, `Error` and resource-only `attached`, `claim`,
`publish`, `discard` functions, plus a pure source-URL projection. Names/formals/
WIT aliases retain named collision refusal. The operations never write a Post
or another application domain row. A claimed upload is the existing linear
resource; `publish` releases it to `Ref`, after which Pleris inserts/updates its
ordinary row. Alt text stays typed application data, outside the descriptor.

A repeatable table clause `asset image from PostImage` associates one top-level
`PostImage.Ref` or `Option<PostImage.Ref>` field with that resolved declaration.
Initial scope is public asset-bearing tables only; no private serving policy is
introduced. The source explicitly holds the upload and its associated tables;
all uses of one upload must resolve to the same explicit source. The compiler
checks actual nominal identity and complete shape/refinements. The host checks
declaration/source identities, permitted kinds/limits, exact stored descriptor
shape and emitted operation ABI independently. Generic startup still refuses
unassociated uploads, malformed/stale metadata and mixed legacy imports.

The descriptor contains content key, unique storage generation, canonical kind
and dimensions, from sanitized actual bytes. Ref is data, not a stored authority
handle. A write may copy an exact currently committed live Ref of the SAME
upload declaration under normal table write authority; no new claim is needed
for that public data. Unknown/orphan/tombstoned/stale-generation adoption is
refused. A new verified claim may enroll its actual declaration for matching
live bytes, subject to that declaration's own limits. Lease/claim identity must
include the actual declaration, never the first declaration in a vector.
Session arguments and both resource/database grants are checked before callback,
including unused imports. Caller-owned table authority is unchanged. Generated
effects use database.read/write<Upload> plus resource.acquire/release<Claim>;
source and deployment admission retain both effects. A command acquiring or
releasing a generic claim requires ordinary idempotent_by and its real commit
record; runtime commit independently refuses a missing record before publication.

**Reference transactions.** Insert/put/delete/prefix-delete maintain internal
descriptor/reference bookkeeping in the same transaction as declared rows,
records and outbox. Rejected/conflicting writes do not drift references. Count
unchanged, replacement, duplicate fields/rows/declarations and final net state;
a delete-then-put/move cannot make a transient zero irreversible. No full-table
payload fetch or app-specific Rust adapter. Publication chooses a live generation
or a fresh generation inside this adapter transaction and returns that Ref;
actual bytes are kept in generic commit preparation before COMMIT. A release
without a final row reference is not an attachment. Explicitly settle its
disposition after applied commit, without making unreferenced bytes servable.

Maintain at most one live generation per content hash in a physical namespace.
Same-byte claims reuse it while live references exist. Descriptor metadata is
immutable per generation. Global last-reference removal, tombstone and a cleanup
job capturing that generation commit atomically. A tombstoned generation never
becomes live or reuses its path again; a new claim after it uses a fresh unique
generation. Physical collection and completion/retry target the captured old
generation only, never a mutable hash-to-current-generation pointer. Count all
declarations sharing physical bytes; declaration-specific serving still requires
its own live reference. Failed cleanup retains its job and cannot undo an applied
row commit. Failed publication can leave unserved orphan bytes, the existing
legacy limitation; no orphan sweep is claimed without a separate bounded proof.

**Exclusion and storage fencing.** A schema/deployment registry and its lock
must own the SAME physical namespace. Enroll a durable namespace identity and
use isolated subdirectories, so sharing PW_BLOB_DIR across different databases/
schemas does not merge reference universes. Hosts of one durable deployment
must share read-after-write storage. Staged ephemeral leases have separate
host-local namespaces; their local hold counts cannot delete another host's
staged bytes. Cross-host ephemeral lease failover is not added by this seam.

For upload-enabled generic programs, reserve the existing pool lease, acquire
the caller session fence if any, then lifetime SESSION advisory lock, then host
turn, then BEGIN/first snapshot. Unfenced guest/Clock/maintenance/ref mutations
follow the same relative order. Hold this SAME connection through keep, COMMIT,
fresh latest-reference check and generation-specific collection; unlock/end
afterward. Do not wait for lifetime after host turn or use a second pool lease
while holding it. Session open/close need no lifetime lock; outbox lock remains
later in commit. This serializes upload-program commands across hosts, with no
throughput claim. A lock acquired after begin_fenced returns is too late: that
API already begins and reads its session row. Existing publish_with ends the
connection before returning; postcommit hooks must execute before that end.

An advisory lock alone does not fence physical storage after its DB connection
dies. A stale process may resume cleanup after another host republishes identical
bytes. Unique generation paths, not SHA/content ETags, make that stale cleanup
harmless. Preserve external /images/<SHA>.<kind> URLs; serving resolves current
authoritative live descriptor/generation BEFORE 304/ETag handling. No reference
means 404; live descriptor with missing/unreadable storage means 503. Never serve
an orphan/tombstone merely because flat hash bytes exist.

**Uncertain outcomes.** Known refusal/rollback/trap returns held claims; known
applied commit settles them. Unknown COMMIT gives claims an explicit unresolved
disposition, not the existing rollback Drop path. Retain their bytes and pending
identity. Reconciliation reacquires lifetime exclusion, then reads the actual
durable interaction record: matching record proves applied; absence can prove
rollback only after the old transaction has ended and while retention can still
prove absence. Expired/indeterminate evidence stays unresolved, never guessed
rollback. Session/lease lifecycle and retry must not hand another invocation the
same unresolved claim. Do not collect on an uncertain outcome. Native witnesses
must cover receipt reconciliation, principal rotation and lock/drop cleanup.

**Proof before guard replacement.** Require compiler failing regressions with
valid neighbors; host forged identity/ABI/refinement and independent grant axes;
actual compiled resource/table flow on memory and PostgreSQL; session/rotated/
foreign-declaration claims and malformed whole refs; rejected writes, replace,
duplicates and final net moves; trap/refusal/keep error/late fence/unknown COMMIT;
reopen, metadata upgrades/reserved names, simultaneous hosts, same-byte last
delete vs publish in both orders; authoritative HTTP 404/503 and 304 ordering.
Terminate host A's actual PostgreSQL backend after committed no-ref inspection
but before physical unlink, let B republish identical bytes/new generation and
commit, then resume A and verify B's actual bytes and serving. Add strict faults
at real assertion sites, complete inventories and exact-source/restoration
receipts. Existing browser/API assertions remain unchanged final port gates.

Primary PostgreSQL 18 docs checked: [explicit locking](https://www.postgresql.org/docs/18/explicit-locking.html)
and [transaction isolation](https://www.postgresql.org/docs/18/transaction-iso.html).
The [synthetic protocol probe](../evidence/E14/protected-upload-design-2026-11-04/primary-probe.log)
on 18.6 reproduces advisory-only stale unlink after actual backend termination,
and preserves new identical bytes with distinct captured paths. It also shows
late-lock pinned snapshot vs pre-BEGIN fresh snapshot. Separate review found no
false-pass for that limited claim. Its refs are key-only joined to a mutable
pointer, paths fixedold/new, no contended snapshot wait, no tombstones, adapters,
namespace enrollment or HTTP. It is not implementation acceptance.


### Upload storage wire prerequisite: closed, payload-free cases

Accepted 2026-11-04 before implementation. The compiler emits Upload.Kind as a
WIT variant even when every case has no payload; the generic host and both row
adapters currently refuse Stored::Cases. Close that encoding gap before asset
integration. This is not a second upload kind parser or a new query language.

Serve a nonempty Stored::Cases only when every case is payload-free and every
normalized WIT case identifier is nonempty and distinct. Its internal row JSON
is the canonical WIT identifier string (for example Png becomes "png"), and its
component value is Val::Variant(identifier, None). Enum/string values, unknown
tags, payloads, empty/ambiguous schemas and all payload-bearing variants remain
refused. Recursive, Map and Set encodings remain outside this slice. Nested
records/lists/options use the same closed-tag encoding recursively.

Scalar case columns use PostgreSQL bytea UTF-8 tags, like stored strings. Keys,
indexes, order marks and canonical set results compare UTF-8 WIT identifiers,
not constructor ordinal or source spelling. The host checks the actual WIT
variant's complete ordered case list and absent payloads against tables.json.
Metadata, ABI and whole values are validated before adapter callbacks or SQL.
Compiler nominal identity and upload asset bindings remain independent checks;
a structural case ABI grants no reference or resource authority.

Native witnesses must reverse declaration order and use A_Z versus AB (wire
a-z sorts before ab, unlike source spelling), cover scalar keys/indexes/window
and range/set canonicalization plus nested values, malformed whole inputs and
unsupported schemas, and exercise compiled calls on actual PostgreSQL. Extend
the independent generic conformance oracle rather than teaching it adapter
implementation details. Keep asset storage guards until lifecycle integration
passes. Existing unbound-asset tests must become independently sensitive to
their guard once the otherwise-supported Ref.kind encoding is installed.

The pinned Wasmtime variant API was inspected in the installed crate source:
Variant::cases yields ordered name and optional Type, and payload-free cases
still use Type::Variant/Val::Variant. No dependency/version change is needed.


### Runtime upload binding and registry ownership

Accepted 2026-11-04 before implementation. Keep the compiler's uploads.json and
tables.json as separate projections. A shared pw-host assets module reads the
upload metadata and defines immutable reference/descriptor and transition types.
Bind that validated upload manifest explicitly to the existing host table
Manifest, which row-aware engine calls already carry. Every pw:uploads import,
including an unused one, must pass its declaration identity, independent
database/resource contract and actual-grant checks, and actual Wasmtime ABI.
An unbound import fails closed. No implicit binding from a host closure's key.
The compiler's nominal types remain the source authority; WIT structure and a
plain Ref provide no lease or publication authority.

At binding, validate public top-level Ref/Option<Ref> asset fields, exact stored
descriptor/closed-kind shape, unique associations, actual declaration identity,
unambiguous route/serving identities and the single explicit source supported
by these adapters. Persist immutable base declaration and asset association
metadata. Refuse changed/removed bindings, including opening an old asset
database with a new manifest that has no assets. Base declaration changes are
conservatively refused in this first slice; deployment-lowered admission limits
remain separate and do not retroactively change descriptors. No automatic
conversion of the legacy feed/store database is introduced.

Use indexed memory state and PostgreSQL platform rows, with shared pure
per-generation transition rules, not a whole-registry JSON snapshot. Retain
existing platform migration bytes/checksums and append a migration for namespace,
declarations/bindings, immutable generations, current positive per-declaration
reference counts and captured-generation cleanup jobs. Choose current LIVE or
fresh generation and validate every final touched reference under lifetime
exclusion; compute final net counts across ordinary mutations. Apply registry
transitions and cleanup jobs in the same command transaction. Resource and
registry work must remain visible in the adapter's cost accounting.

The shared storage boundary uses enrolled namespace, captured generation and
content hash. Server local/memory implementations supply physical storage; no
application operation appears in this boundary. Keep the original upload and
mixed-layer startup guards until the accepted lifecycle, uncertain-outcome,
namespace, HTTP and real PostgreSQL backend-death witnesses pass.


### Root admission on authorization-only reuse

Accepted 2026-11-04 before the API repair. Review of the first runtime binding
slice found that call execution runs the new gate, while stub instantiation and
authorization-only root reuse did not. Keep every entry point fail closed.
Add a root admission method on Prepared that validates generic table/upload
declarations, both contract and actual grants, and actual imported WIT types,
without asking for body callbacks or executing an export. Authorization-only
reuse receives the root's actual Granted explicitly and performs this admission
before evaluating predicates. Predicates keep their own grants and invocation
snapshot; they cannot borrow the root's authority.

The server must run root admission even for a reused query with no authorization
predicate, before its existing early return. That check needs no new table
snapshot or data work. Guarded reuse continues to evaluate predicates against
the same per-invocation read state as fresh execution. A refused root admission
escapes cache/fallback just as a predicate refusal does. This closes declaration/
grant/ABI admission parity; actual upload session ownership, lease lifetime and
durable commit safety remain separate obligations before enabling uploads.


### Generation transition implementation

Accepted 2026-11-05 before registry code. Share a pure per-generation transition
planner between indexed memory state and PostgreSQL persistence. Its input is
an immutable descriptor, prior committed per-declaration positive counts,
transaction-owned publication plans after claim validation, and original/final
reference uses of touched rows. Validate every final reference even when its net
count is zero. The planner does not authenticate a lease or choose a physical
namespace; those remain the actual transaction/lease bridge's responsibility.

Only a prior LIVE same-declaration reference or that declaration's fresh
publication plan permits adoption. Descriptor equality includes all Ref fields
and byte count. Compute counts from final net changes with checked arithmetic;
reject missing generations, tombstones, contradictory old counts and cross-
declaration enrollment. A committed last removal tombstones the captured
generation and queues its cleanup. A new publication with no final row use is
explicitly retired, never LIVE with zero references. Retain retired generation
identities; physical keep_new also reserves a generation identity so an orphaned
or collected path is not reused. No whole-registry JSON load or orphan collector.

Publication usage is based on final touched row uses, not whether some unrelated
old row already references the same bytes. Multiple declarations can use one
LIVE physical generation after independent fresh-claim validation, but current
counts remain declaration-specific. The planner's native tests prove these
arithmetic/identity rules only; actual claims, locks, storage and uncertain
commit reconciliation remain required before the existing guards change.


### Memory row reference integration and lease cardinality

Recorded 2026-11-05 before row integration. Preserve the existing attachment
cardinality: one host-local pending lease per (requested session, declaration).
An invocation has one verified requested session. Claiming freezes that lease
through known rollback/applied settlement or explicit uncertain reconciliation;
publish/discard do not return it early, and replacement while claimed refuses.
Consequently an invocation can propose at most one publication per declaration.
The registry's duplicate-declaration refusal relies on this bridge invariant;
the actual bridge must prove it before enabling generic uploads. This does not
introduce multiple attached files per declaration.

Add an explicit upload-bound memory constructor for trusted host integration;
the ordinary constructor and server startup guards still refuse unbound assets.
The adapter captures original/final references only from each touched key and
declared asset field, including unchanged final references. It prepares the
shared registry patch alongside row constraints and applies it under the same
publication lock as rows. Proposals stay transaction-local; dropped commands
and refused writes do not publish registry changes. Expose captured plans to
the later storage/lease lifecycle without performing storage effects here.
Registry work has its own operation counter; row primitive counts keep their
existing meaning. Native row tests using trusted descriptor proposals prove
atomic bookkeeping, not authentication, storage or durable receipt safety.


### In-memory registry instance affinity

Recorded 2026-11-05 before repairing a review-reproduced foreign-draft refusal.
A revision is not the identity of the registry that produced it. Retain an
opaque process-local instance witness in each memory registry draft and patch;
propose/prepare/apply require that exact witness as well as the revision before
reading or changing state. Two isolated instances at equal revisions must
reject each other's drafts and patches. This is neither a persisted namespace
nor a public generation value and reveals no activity counter. The later
durable adapter still binds its actual namespace and connection independently.


### Namespace and generation storage provider

Recorded 2026-11-05 before provider implementation. Add a shared typed storage
boundary in pw-host::assets: an opaque validated 128-bit Namespace, explicit
fresh/existing enrollment, keep_new, exact descriptor read and captured-generation
collection. The concrete development LocalBlobs and MemoryBlobs providers
implement it alongside their legacy flat-key API; routing/claims remain closed.
The durable adapter must choose/enroll its namespace within its migration and
verify existing enrollment before serving or running a command. A missing or
different marker refuses; an unrelated storage root must not silently enroll
an already recorded namespace. Random namespace/generation candidates come from
the operating system, not a public activity counter.

Local storage uses <blob-root>/.generic/<namespace>/generations/<generation>/
with an immutable descriptor marker and a payload named by its validated hash.
Reserve each generation directory exclusively BEFORE writing its marker/bytes;
failure or rollback leaves that identity reserved. Collection removes only the
captured payload, never its marker/directory. keep_new refuses any previously
reserved identity, including an orphan or collected generation. Namespace and
descriptor markers are checked on every operation; full Ref, canonical byte
count and content hash are checked before successful reads. Writes request file
and directory sync before returning; native Linux tests do not claim power-loss
or other-OS/filesystem acceptance. No orphan collector is introduced. Legacy
flat-key counting must ignore the separate generic storage directory.

Native provider tests use actual arbitrary bytes (not image sanitization) and
prove cross-handle enrollment, isolated namespaces, exclusive generation
reservation, corruption/missing reads, same-hash fresh-generation safety and
old captured cleanup. They do not establish authoritative HTTP liveness,
database enrollment, claim authority, backend-death fencing or unknown outcome
reconciliation; those remain actual integration gates.

Primary API checks: Rust [OpenOptions::create_new](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new)
and [File::sync_all](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all),
and PostgreSQL 18 [session advisory lock lifetime](https://www.postgresql.org/docs/18/explicit-locking.html#ADVISORY-LOCKS).
The database session ending releases exclusion; only distinct physical generation
paths can fence a process that resumes stale cleanup after that event.


### Durable namespace/metadata enrollment before row lifecycle

Recorded 2026-11-05 before PostgreSQL enrollment implementation. Append platform
migration 0005 (written as 0004 before L3's `0004_outbox_removals_marked`
landed first); retain 0001-0004 bytes verbatim. Persist namespace, immutable base
declarations/bindings, immutable generations, positive per-declaration counts
and captured cleanup jobs as indexed platform rows. The explicit trusted upload
constructor validates both manifests before connecting; ordinary unbound open
still refuses assets. While enrollment is implemented, upload-bound command
begin remains explicitly closed until actual row/storage/lifetime integration.

Under the existing migration transaction/lock, validate persisted upload metadata
BEFORE any application schema DDL. Missing existing storage marker refuses;
fresh storage enrollment completes before its namespace row can commit. Failed
DDL may leave an unserved namespace reservation; a retry needs a new random
candidate, never silent marker reuse. Reopen selects the persisted namespace,
independent of the new host's proposed candidate. Removing/changing declarations
or bindings refuses, including opening a prior asset database with an unbound
manifest containing no asset clauses. Base policy and association metadata are
immutable; lowered deployment admission remains a separate later bridge setting.

In this first slice a NEW asset association is allowed only on a table absent
from the recorded schema, or on the initial fresh schema. Refuse adding an asset
clause to ANY previously recorded table, including a new optional Ref field.
That conservative restriction prevents an already-running older unbound host
from deleting rows with references in a newly introduced field it cannot track;
empty/populated checks would not fence that old writer. Additive non-asset
optional fields and wholly new asset tables remain supported. There is no legacy
Ref enrollment/conversion. Later relaxing this rule requires a persisted version
fence held by every old writer or a comparably complete protocol/native proof.

Reserve the pw_asset_ relation prefix for all new platform tables and indexes,
including automatically named primary-key indexes. Actual command lifetime locks
and transactional registry transitions remain the next step after enrollment;
enrollment-only tests do not claim command/HTTP/session/uncertain-outcome safety.


Enrollment review reproduced successful open after BEFORE INSERT hooks rewrote
the namespace, declaration or binding despite RETURNING 1. AFTER INSERT and
application-schema-record hooks can likewise rewrite earlier metadata after its
insert returned. Require a fresh exact read of persisted namespace and complete
immutable declaration/binding maps AFTER all application DDL/schema recording,
inside the same migration transaction immediately before COMMIT. Mismatches
refuse and roll back; the storage reservation remains an unserved orphan. Row
count acknowledgments still independently detect suppressed inserts.

The recorded application schema is also an enrollment-history backstop. If it
has asset associations, an unbound open refuses even when all current namespace,
declaration and binding rows were lost. Missing authority metadata is drift,
not evidence that the schema was never enrolled; do not overwrite that history.

The history check applies to every constructor: a bound wrapper cannot omit a
recorded association after full or partial persisted-binding loss. Compare each
recorded association/field to its current persisted binding before enrollment.
Flush pending deferrable constraints/triggers after application schema recording
and BEFORE final metadata verification, using SET CONSTRAINTS ALL IMMEDIATE.
Otherwise a deferred schema-record hook could rewrite namespace at COMMIT after
the verification read. Native immediate/deferred trigger regressions and valid
neighbors pin this order. Primary PostgreSQL 18 documentation:
[SET CONSTRAINTS](https://www.postgresql.org/docs/18/sql-set-constraints.html).


### PostgreSQL row/storage command integration

Recorded 2026-11-05 before the next implementation. Replace the enrollment-only
command refusal only with the accepted actual lifetime: same reserved pool lease,
session fence where present, namespace SESSION exclusion, host turn, then command
BEGIN/first snapshot. Retain the lease through canonical keep, COMMIT, fresh
captured-generation no-reference checks and collection/completion. Schema
migration takes the same namespace exclusion before application DDL and releases
it only after transaction completion. Commands cannot acquire it after BEGIN.

Trusted lease-bridge proposals supply canonical descriptors/bytes; the adapter
chooses current LIVE or fresh generation and shares the pure transition planner.
Read only touched generations/current declarations, derive original/final uses
from the existing DML preimages/touched log, and persist descriptor/count/state/
captured jobs in the same transaction as application rows and records. Every
published proposal, or a bridge-marked acquired/discarded claim, requires an
actual interaction record before any physical keep. Pleris idempotency metadata
alone is insufficient. Row-only copies of a current same-declaration Ref need
ordinary table authority but no new upload receipt.

Expose prepared disposition plans to the later actual claim bridge. Keep only
fresh generations with final row uses, after record/outbox preparation and before
the final session clock fence/COMMIT. Known rollback performs no GC; unknown
COMMIT performs no GC and preserves OutcomeUnknown for explicit claim handling.
Known committed collection/completion errors leave durable jobs pending and
preserve the applied acknowledgement. Cleanup always captures old G; no mutable
hash pointer chooses a physical deletion. Full claim reconciliation and actual
server HTTP/backend-death witnesses remain required before startup guards change.

Native adapter tests use trusted proposals and a byte-store test double with
real PostgreSQL records, not a substitute authorization ledger. Actual compiled
resource/session callbacks and concrete physical-provider/HTTP composition remain
separate following tests. There is no implicit legacy database conversion.


Receipt and completion acknowledgements require persisted evidence. For every
claim-bearing command verify the exact requested interaction key, sent value,
answer and stored instant after receipt/outbox hooks and before keep; verify
again after deferred hooks finish and before the final clock fence/COMMIT.
An INSERT RETURNING row alone does not establish that proof. After captured-job
DELETE, inspect that exact job's absence on the still-held namespace connection
before counting completion. A hook that reinserts it leaves a retryable job and
cannot turn the already-applied command into a refusal.


### Unknown claim outcome requires attempt provenance

Recorded 2026-11-05 before the actual server claim bridge. An interaction key,
canonical sent body, deterministic answer and host-clock millisecond identify a
logical interaction, not THIS attempt. A's uncertain transaction can actually
roll back; another host B can then commit the same interaction using a different
host-local lease before A reacquires namespace exclusion. Equal timestamps and
answers do not prove A's lease was consumed. Namespace reacquisition proves the
old transaction ended, not that a later receipt originated in it.

Generate an opaque random attempt nonce for each actual claim-bearing invocation,
freeze it with that invocation's claims, and persist it as a platform field in
the existing JSON answer envelope. Keep the interaction key and canonical sent
body unchanged. Existing answer decoding tolerates platform metadata, preserving
browser retries; claim reconciliation must inspect the RAW persisted envelope,
since decoding and re-encoding an Answered drops unknown fields. Only this
attempt's nonce can establish that its claims committed. Missing or different
nonce cannot settle this invocation's lease as applied. Absence proves rollback
only under the accepted reacquired lifetime/ended-old-transaction/retention
conditions; indeterminate outcomes stay held. Recorded-before-COMMIT is a known
noncommit of this transaction even if the host answers another attempt's receipt.

Pin with actual unknown-but-rolled-back A, equal-clock deterministic B retry and
different local leases, plus matching-nonce applied/uncertain neighbors. No new
registry table/column or client-visible interaction argument is needed.

### Concrete public serving composition

The generic upload handler binds a same-declaration current-generation reader
from its actual data adapter once before serving. Freeze and verify returned
bytes before consulting If-None-Match, including HEAD requests. No current ref
is 404 even while its old file remains; LIVE missing/corrupt/unreadable is 503
before 304. Preserve the existing external SHA.kind URL, JPEG spelling and image
headers. Test actual sanitizer, PostgreSQL, LocalBlobs, backend death and HTTP
request/response bytes together; isolated handler tests do not establish the
unfinished full Server/compiled claim startup path. Keep that guard closed until
the actual resource/session/row bridge and its gates complete.


Staging uses one actual (session, declaration) lease key. Private previews bind
the declaration whose route was requested. Explicit resource lookups use their
admitted declaration and captured immutable reader without a new network read
under the command's database lifetime. Legacy wrappers select only their first
declaration while the canonical port is pending. Claimed values retain exact
declaration/reader/lease identity and redact private cookies/tokens/hashes in
diagnostics. Unclaimed lease expiry applies at metadata/preview/claim lookup,
even before periodic sweep. Held claims survive elapsed time and sweeping until
an explicit outcome returns/consumes them. A returned expired lease is hidden
immediately and later swept; no expiry can silently release an uncertain claim.


### Receipt absence currently has no retention proof

Recorded before actual claim outcome implementation, 2026-11-05. Different hosts
keep different clocks (ADR-0304/0369); no bounded-skew assumption is established.
A writer's forget_before_ms can prune another attempt's committed receipt while
that attempt's host still considers it within its local retention day. Reacquired
namespace exclusion and a missing receipt do not distinguish this from rollback.
The current recall API supplies no durable retention provenance. Therefore the
initial actual claim bridge retains Unresolved on absent/nonmatching evidence;
only a matching raw attempt receipt proves Applied. Explicit known rollback
still returns claims immediately. Do not infer rollback from a local age check.
A later negative-proof protocol needs its own complete evidence, including
expired same-key ON CONFLICT UPDATE replacement as well as DELETE pruning.

Pending invocation metadata is host-local and bounded independently of the staged
byte cap: at most 1024 unresolved entries and 16 MiB of owned receipt proof text.
Capacity refusal precedes COMMIT and returns that invocation's claims. Elapsed
time never clears a pending invocation. Native ledger tests alone do not replace
the required actual row/receipt/lifetime, compiled resource and startup gates.

### The server's claim bridge (L5b), decided 2026-11-16

The integrator's, under the owner's delegation of 2026-10-02, before the
code. L5a lands the claim ledger (`uploads::generic`) and every adapter
check below it; nothing binds a compiled program's upload imports yet, and
`tables::servable` refuses a program of tables that declares an upload.

**Decided.**

1. **One ledger per command run.** The host makes an `Invocation` in
   `command_held` after the command's authority is taken, for a command
   whose contract imports an upload's claim, publish or discard, with the
   reader it admitted, frozen. Its declarations' imports
   (`pw:uploads/<declaration>#claim`, `#publish`, `#discard`) are bound to
   it for that run alone, by the bound manifest's interfaces, never the
   first declaration; a handle the program passes is the request's
   session's, or the call is refused (a trap, the command run no further).
   A query reads `#attached`, the
   session's own leases. A command no reader runs (a Clock's) claims
   nothing: its upload imports refuse.
2. **The layer publishes in the command's own transaction**
   (`Staged::upload_publisher`): PostgreSQL's adapter publisher and receipt
   marker, taken after the command's authority; the memory layer's
   publisher, the canonical bytes kept beside it for the keep.
3. **The commit carries the attempt.** The answer the record keeps is
   sealed with the attempt's nonce before it is recorded, the ledger is
   frozen just before `COMMIT` (its capacity refused before `COMMIT`, which
   is answered refused: no `COMMIT` was sent), and a command bound to the
   ledger commits its record even where it writes no row (a discard), so
   that a retried interaction is answered from it.
4. **Each outcome settles the claims once.** Committed: applied. Kept
   nothing (a refusal, a definite rollback, or `Recorded`: another
   attempt's record answers this interaction, and this attempt committed
   nothing): returned. Otherwise pending: reconciled from the raw receipt
   once the command has let go of its turn, its lease and the namespace
   lock (it still holds its session's one-at-a-time lock, so that session's
   other requests on its host wait for the inspection), and by a periodic
   pass that retries only an inspection that failed, in turn, never the
   first few alone. Absence stays held.
5. **The memory layer keeps uploads too**: bytes in memory storage, kept
   before the publication and collected after it; an unknown memory
   outcome (a commit failing after the rows were published) holds its
   claims, as no inspection exists and the tables close.
6. **Startup opens for generic declarations only.** Still refused: a legacy
   or malformed declaration, one no table's asset names, a binding the
   manifest refuses, a hand-written layer's imports beside the tables, and
   a layer whose manifest binds no uploads. The feed's compatibility stays
   L7's gate (decision 4 of this ADR).

**Accepted, and named in KNOWN_LIMITATIONS.** An unknown outcome that in
fact rolled back holds its session's lease for that declaration until the
host restarts, and a replacement upload is refused meanwhile (409); it is
logged. Proving the rollback is the negative-proof protocol named above,
work of its own. Leases are host-local: an upload and the command that
claims it reach one host (affinity, L6's deploy). On PostgreSQL every
upload-enabled command and image read takes the namespace lock (five
seconds at most), to be measured before L7.

**Not chosen.** Releasing a held claim by its age, or inferring a rollback
from a missing receipt (both above); a fresh interaction per attempt (the
record that answers a retry is keyed by the interaction).

**Built** (2026-11-16, `wip/l5b-claims`). `uploads::bridge` binds each
declaration's imports to the run's `Invocation`; both layers open bound to
`uploads.json` (PostgreSQL's generations in `LocalBlobs` under the
deployment's blob root, memory's in `MemoryBlobs`); `main.rs` seals the
answer, freezes the ledger just before the commit (a claiming command
commits its record even where it writes no row), settles each outcome, and
inspects again, in turn, an attempt whose inspection failed; `servable`
admits an upload a table keeps. The ledger gained `due_attempts` (never an
attempt its owner may still be committing) and `Invocation::reconcile`.

**Held by** `tests::upload_bridge`, a compiled program through the server,
on memory and on PostgreSQL: a claimed image committed with its row and
served (`304` on its `ETag`, nothing pending, the interaction answered again
as it was, the lease taken); a discard ending its lease and keeping its
record; a removed row's image answered `404`; a refused commit giving its
claim back; on PostgreSQL, a commit whose answer was lost settled from its
own receipt (applied, served), and one that never reached the database held
for good (absent, inspected once).

The seven tests the decision owed are written too. On both layers:
- a lease staged before its reader rotated (the session signed in) is shown
  to no one and claimed by no one, while the account's own lease is its;
- a claim through one declaration finds no other declaration's lease
  (PostImage's `Keep` beside a staged Avatar, which `SetAvatar` then
  claims);
- a keep of a new generation's bytes that fails keeps nothing and gives the
  claim back.

On PostgreSQL, across two hosts of one schema where two are named:
- a commit another attempt's record answered first (`Recorded`, B's commit
  made before A's authority) keeps nothing and gives A's claim back, and A is
  answered from B's record;
- an equal-clock retry by host B (both hosts read one frozen instant), after
  A's `COMMIT` was dropped before the database saw it, leaves a receipt
  that is A's own proof in its key, sent body, answer and millisecond, and
  differs only in its nonce. A holds its claim for good, neither applied
  nor given back;
- a reconcile the namespace lock delayed (another session holds it through
  A's own inspection, five seconds) stays due and is made at the next
  periodic pass;
- a live generation whose stored bytes were corrupted is answered `503`, and
  so is a request naming the `ETag` it was served with, before any `304`.

Their seams are test-only:
- `before_upload_inspection`, run before a failed command inspects its own
  receipt;
- `frozen_now`, a host's fixed instant;
- `Ticket::proof`;
- memory's `FAIL_NEXT_KEEP`.

Thirty mutants in `generic_upload_claims_mutations.py` (`just
e14-generic-upload-claims`) hold it, each killed, as
`docs/evidence/E14/generic-upload-claims.txt` records. Eight came with the
seven tests:
- `Recorded` left pending;
- a lease keyed by another declaration than its route's;
- a lease claimed, or shown, whoever its reader;
- another attempt's receipt, alike but for its nonce, settling this one;
- an inspection that could not be made held for good;
- the periodic pass inspecting nothing;
- PostgreSQL's keep failure not refusing its commit.

Six came with the review below, and four with L5a's after it. d3's "a
connection keeps the operating system's keepalives" and data-path's "an
upload's binding is not checked where it is served" are each killed too.

The serving handler's check of a live generation's bytes and the storage's
own are each the other's backstop, so neither alone is a mutant.

**Reviewed** independently (2026-11-16, `014439c5..a7c3377b`). No high
finding in the bridge; three medium findings and eleven low, each repaired
or recorded:
- **The binding check where a build is served could not be failed** (M1):
  each layer binds again, so the data-path mutant on it survived.
  `an_upload_whose_binding_the_build_broke_is_refused_before_the_database`
  now serves a build whose upload names another source than its tables'
  on a database no one serves. `servable` refuses it, saying why, before
  anything connects.
- **The kills were claimed with no record** (M2): `docs/evidence/E14/
  generic-upload-claims.txt` is now the whole script's run, and the full
  verify records it again at the landing.
- **A half-open connection could hold the namespace lock for hours** (M3):
  each connection now sets the server's keepalives, about a minute
  (`each_connection_sets_the_servers_keepalives`, and d3's mutant "a
  connection keeps the operating system's keepalives"). KNOWN_LIMITATIONS
  says what remains.
- **The tests' records were the host's cache's** (L1): the served test
  reads the durable record, and the discard test holds the lease ended
  (nothing pending, another image staged). **A helper caught a refused
  commit first** (L2): the served test asserts its commit itself.
- **Rules with no mutant** (L3), now held:
  - the periodic pass holding an absent receipt's claim for good, by a new
    test, `the_periodic_pass_holds_an_absent_receipts_claim_for_good_on_postgresql`;
  - an attempt dropped uninspected offered to the pass, and the pass's
    turn, by their unit tests;
  - PostgreSQL's collection, at the commit and by housekeeping, by two new
    tests.
  A readerless command's refusing imports at the server, and memory's
  collection, have no mutant: the script says why.
- **A commit its ledger refused before its `COMMIT` was answered unknown**
  (L5), and the host kept that for its interaction: it is answered refused
  now (`a_commit_its_ledger_refuses_is_answered_refused`, on both layers).
  The ledger's other refusal, a claim left with no disposition, is the
  compiler's (PW2005).
- **Robustness** (L6, L7, L9): each needs a panic or a race no test makes,
  and none has a mutant.
  - A poisoned counter after a landed `COMMIT` answered it refused and
    ended nothing; the lock is now taken as it is.
  - Memory's collection was set before its publication succeeded; now it is
    set after.
  - A ledger that failed while it reconciled stranded the attempt;
    reconciled is now set only once the ledger took the outcome.
- **A removal racing memory's read answered 503** (L8): a second look
  answers 404, or serves a generation published again. The duplicate
  collection the review also named does not happen: both storages' collect
  and the registry's `collected` are idempotent.
- **Docs** (L4, L10, L11): KNOWN_LIMITATIONS now says what an inspection
  holds, the host-wide bounds, the shared blob root and the pages a
  settlement does not tell. Decision 4 above no longer says the inspecting
  command holds nothing: it holds its session's lock.
- **Nits:** two doc comments are back on their functions; the stale
  `dead_code` allowances are gone (the ledger's test-only parts are
  `cfg(test)`); the forced record's comment and decision 1's foreign handle
  now say what the code does. The PostgreSQL keep test fails, not skips,
  when run as root, as its own doc says.

**L5a reviewed** independently (2026-11-16, `6407ba55..014439c5`). No high
finding is reachable in L5a, whose server refuses uploads beside tables;
each becomes live with L5b. So **L5 lands as one train**, from
`wip/l5b-claims`, with L5a's findings repaired there:
- **A read held to the lifetime lock** (M1): each image request waited for
  every command, holding a pool connection through its file and its hash.
  A read now takes no lock and holds its connection for one statement; a
  second look tells a collection that raced it.
  `an_image_is_served_while_the_namespace_lock_is_held_on_postgresql` holds
  it, and the mutant "an image read waits for the namespace's lock".
- **A database copy sharing its original's blob root** (M2) would collect
  what the original still serves. A namespace's storage now names its
  database's identity and refuses another
  (`a_database_copy_sharing_its_originals_blob_root_is_refused_on_postgresql`,
  two mutants). A physical copy is not told apart (KNOWN_LIMITATIONS).
- **Memory and PostgreSQL not alike** (M3): L5b gave memory its keep, its
  collection and its verified read. A shared contract suite for assets is
  NEXT's.
- **An upload no table keeps** (L1): `servable` refuses it (L5b), before
  any layer enrolls it.
- **One failing collection stopped its pass** (L2), the same one first each
  time: each job is now made on its own, in turn
  (`a_collection_that_keeps_failing_keeps_no_other_from_its_turn_on_postgresql`,
  a mutant).
- **The namespace lock's five seconds governed the migration's DDL** (L3):
  the default is restored after the lock. Repaired by reading: no test makes
  the DDL wait.
- **A delete touching more keys than 65,535 parameters hold** could never
  commit (L8): the check is batched. Repaired by reading: a test would write
  65,536 rows.
- **Upload paths the host refuses compiled** (L6): PW5602 now holds the
  host's rule (equal or nested paths, an upload's own two included). A type
  named twice is PW5601 at the upload, not PW0028 at a generated
  declaration. Two compiler tests hold these.
- **An unknown outcome that rolled back pins its lease** (L5): recorded in
  KNOWN_LIMITATIONS (L5b's).
- **Left to NEXT, each low:**
  - three PostgreSQL rules with no mutant (L4): the descriptor's
    immutability in `propose_asset`, `persist`'s `ON CONFLICT` guard and
    `lock_lifetime`'s marker check, each needing a fixture of its own;
  - upload refusals filed under PW5601's invariant, and no corpus cases
    (L7);
  - bare `is_err()` refusals (L9).
- **Nits:**
  - `Declaration::source()` no longer panics on an unvalidated value;
  - a manifest that declares no upload binds none, so it enrolls no
    namespace;
  - the descriptor marker stays byte for byte, and the code now says so;
  - `PLATFORM_RELATIONS`' comment names 0005's prefix rule;
  - the immutable cache header, which predates L5, stays.

## Landing (L3, 2026-11-13)

L3 merges the integration branch's six commits after `a9a8d79e`
(`b7db5877` to `2e2ef45b`, 33 files, +2,901/−148) onto train-34
(`0dcff90a`, which holds L1, L2 and train-33) in `067cfbdd`, branch
`data/l3-platform-20261113`, pushed as `wip/l3-platform`. They are the
first three prerequisites above and the witnesses each needs:

- generic PostgreSQL hosts retain and tail their shared outbox
  (`b7db5877`, the first);
- admitted anonymous callers keep their guest-owned rows (`c70d774a`), and
  the original guest partition is inspected after authority changes
  (`53ae2a64`);
- owned Clock actions keep the authority of the run that scheduled them
  (`4f80e543`, the third);
- protected row joins run on checked identity transitions (`892e9c0d`, the
  second), and an unconfirmed lifecycle outcome is reported without
  claiming rollback (`2e2ef45b`).

What the merge decided:

- **A host retains the outbox rows it delivers.** L1 (`f7741cf1`) had a
  host delete the rows it had delivered, at once; L3 keeps every row until
  the shared retention age, ten minutes, and trims by age with a durable
  watermark, so that a host sharing the database (ADR-0369) that has not
  yet read a row still reads it, and a reader that missed trimmed rows
  sees the gap. Git took L3's code without a conflict (the deletion is
  gone); the test helper's comment conflicted and takes L3's account, with
  L2's `pub(super)` (the owned Clock tests call it). The durable tables'
  mutant "a delivered outbox is deleted before another host can read it"
  puts L1's deletion back. `PgTables::forget_outbox` is left with no
  caller but its own test (NEXT).
- **Six files conflicted, none in behaviour**: `main.rs` (two functions
  added at one place, `is_command` and `identity_lifecycle`, both kept,
  the second given a doc comment), the test helper above, two mutation
  scripts (both sides' test filters, sources and commands), STATUS and
  NEXT (L3's notes of 2026-11-04 replaced by this landing).
- **What git did not show**: five commands of the protected migration's
  test programs (`cart.pw`'s `keep`, `forge` and `clear`, `join.pw`'s
  `keep`, `clock.pw`'s `later`) said nothing of who may run them, which
  ADR-0406's PW0367, landed after `a9a8d79e`, refuses: 21 of the module's
  tests failed at "the program builds". Each says `requires Anyone`
  (`41a03c5d`), as the guests and accounts that ran them before; what the
  tests hold is the platform's answer to any reader. `find_unstated.py`
  (the requires evidence's scan) finds no command of the merge left
  unstated, the tree's other 25 the same as at `0dcff90a`.
- **The protocol upgrade boundary** above (no older generic-table worker
  beside one of platform migration 0003) meets no deployment: no deployed
  host keeps declared tables in a database (L1, which first does, lands in
  #108), and ADR-0396's deploy (L6) starts from this protocol. Since
  `dbbd8e65` an older worker beside a newer one is no longer unsafe, only
  slower to be heard (below).
- **An independent review** of the six commits and the merge, read from
  git objects, found no way past the lifecycle's, the guest rows' or the
  Clock actions' checks. Three layers refuse the lifecycle command to a
  browser (the route, the server's proof, the engine's caller). A previous
  guest's handle reads and deletes its owner-keyed rows and nothing else.
  The proof is one-use, bound to the session, the component and the opened
  epoch, and retired on every exit. The outbox's commit order, tail,
  watermark and fresh hosts' cursors hold. It found four things:
  1. **Nothing refused a host built before 0003 a 0003 database**, and
     such a host deletes what it delivers and trims at 60 s without moving
     the watermark, so a host reading the log behind it missed those rows
     unseen (an operator's DELETE or TRUNCATE likewise). Platform migration
     `0004_outbox_removals_marked` (`dbbd8e65`) puts the rule where the
     rows are: statement triggers after DELETE and TRUNCATE on `pw_outbox`
     raise the watermark to the highest id taken, whoever takes it. Test
     first (`any_deletion_from_the_outbox_moves_the_trim_watermark`, red
     without the triggers, then at the truncation with the deletion's
     alone), held by two controls in `d3_postgres_mutations.py`, both
     killed.
  2. **A guest's rows are keyed by the session cookie**, whose value the
     client chooses (`session_of` takes any `pw-session`) and which has no
     `__Host-` prefix: a page that can set cookies for the origin, a
     sibling subdomain's, can fix a victim's anonymous session, then read
     its guest rows, take them into its own account by signing in first
     with that cookie, or seed rows that join the victim's account. Legacy
     user-keyed carts already were exposed so; L3 extends it to every
     owned table. Open: NEXT, before a deployment keeps guest rows (L6,
     L7).
  3. **A lifecycle that does not commit strands the guest's rows**: kept,
     as "sign-in and row merge are separate outcomes" says, but
     unreachable. The proof is spent, the session is the account's, and
     nothing retries the merge or sweeps them; a serialization failure
     against the account's own write on another device loses the guest's
     cart. Open: NEXT.
  4. **The merge doubled a durable tables' mutant** (L3's and L2's port of
     the same witness), so the count of those killed was one too high:
     written once (`22e77419`). The witness tests themselves stand twice
     (`tests::protected_migration` and `tests::owned_clock`): NEXT.

- **The merge's seven recipes, each whole before the record, found three
  more**, each fixed test first, each control seen to fail where it should:
  1. **Three of the durable tables' controls came out INVALID, not
     killed**: the producer takes one panic a failing test, at an assertion
     of the script's own sources. Under "a fence admits a signed-in reader
     as no one" and "sessions are kept in the host's memory",
     `generic_hosts_use_the_publishers_ordering_lock_and_wake_after_commit`
     asserted its writer's answer on the writer's thread as well as on its
     own (two panics); under "a fenced command's session lock is never
     given back", L3's identity tests failed at the flow's helper's
     assertion (`tests/sign_in.rs`, outside the script's sources), and the
     exchange race at a test sign-in's `expect` on the host's own thread.
     The writer's answer is asserted once it is joined, the helper is a
     source, and the race keeps its callback's outcome for the test's own
     assertion (`try_signed_in_for_test`, `try_signed_out_for_test`).
  2. **"A signed-in session that signs in again is joined to the next
     user" survived** the store's tests: `892e9c0d` moved who may join to
     the reader the callback's request was admitted as, and what is joined
     to the session's guest partition, but the store's record L3 carried
     was of 2026-10-25, before; and no store test held a session admitted
     signed in whose guest partition still holds lines, the one state the
     mutant changes. `a_signed_in_sessions_guest_lines_join_no_one_at_its_next_sign_in`
     does, and fails under the mutant on both layers.
  3. **Second host's five failures** at 32 test threads were the
     machine's: the local database allows 100 connections, PostgreSQL's
     default, and the tests asked for more (`53300: sorry, too many
     clients already`); at four threads, as CI's runners have, they pass
     and its 50 controls are killed (NEXT).
- **`just ci` refused the trusted platform contract's hash**: `892e9c0d`
  declared `context.previous_user()` (`pw:host/principal#previous`), the
  lifecycle's previous guest, `None` outside that one invocation, and
  moved no hash, which no run of the integration branch's own checked.
  The declaration is intended: the hash is the merge's, with its dated
  note (`f641ba0c`).

Measured locally on PostgreSQL 18.6, each recipe whole with its mutation
script, on L3's own databases: at `41a03c5d`, invoked-by (14 of 14
controls killed), row-ownership (105 of 105), d3-postgres (63 of 63) and
scheduled (38 of 38); at `dbbd8e65`, platform 0004's two controls
(killed, the 78 tests of the baseline green); at `71c31395`,
durable-tables (57 of 57), store-accounts (37 of 37) and second-host at
four test threads (50 of 50). The full verify at the branch's head is the
record.

## Amended 2026-11-16: a lifecycle the database could not serialize runs again

The integrator's, under the owner's delegation of 2026-10-02: NEXT after L3,
item 3 (the L3 review's third finding).

**Found.** A lifecycle that did not commit left the guest's rows where
nothing reads them: its proof ends with the sign-in's request, the session
is the account's, and no client sends a lifecycle again. The review named
a serialization failure against the account's own write on another device.
It reproduces: with the account's other device served by a second host on
the same database (ADR-0369), the account keeps its quantity after the
lifecycle's transaction has taken its snapshot (at the session's fence) and
before it reads; the lifecycle's `put` then fails with PostgreSQL's 40001
("could not serialize access due to concurrent update"), the sign-in
completes, and the guest's 3 stay in their partition beside the account's
10, unread.

Running the command again was not enough by itself: an idempotent command's
outcome is reserved by its interaction (ADR-0121), and a second attempt
under `identity-joined` was handed the first's refusal, and nothing ran.

**Decided.**

1. **A layer says when a command failed as the database could not
   serialize it** (`data::Staged::conflicted`): the PostgreSQL layer, where a
   statement met SQLSTATE 40001 or 40P01 (the adapter's
   `Command::conflicted`, from what the failed statement left) or the commit
   did, each of the commit's statements (the run it takes, the runs it
   schedules, its prepare, its `COMMIT`) answering through one place
   (`refused_by`). Such a command's transaction kept nothing. The memory
   layer runs one command at a time and never conflicts. The host notes it,
   for the command's component, where the run or the commit failed
   (`data::note_conflicted`), as its error is a `String` from there on, read
   by the caller that runs it again, as the refusal's predicate is
   (`identity::take_refusal`); another command run on the thread meanwhile
   is never taken for the lifecycle's.
2. **The platform runs an identity's lifecycle again where it conflicted**
   (`Server::identity_answer`): in a transaction of its own, while its proof
   holds, under the same interaction, at most four times
   (`pw_table_postgres::ATTEMPTS`, as the layer runs its own transactions).
   Before each rerun it lets go of the outcome it reserved for the
   interaction (`Server::forget_reserved`, its shared result; its place in
   the session's queue admits the same interaction sent again as before):
   the attempt kept nothing, so it has no outcome to share. A refusal, a
   trap, an unconfirmed commit and any other end are the answer, as before;
   "sign-in and row merge are separate outcomes" stands for them. Each rerun
   is logged, and so are four conflicts in a row, after which the guest's
   rows stay where they were (sustained contention stays open).

The proof's one use is the lifecycle's outcome: an attempt rolled back by a
conflict is no outcome, and the reruns are inside the same use, the same
component, the same transition. A browser's command that conflicts is
answered refused as before; its client presses again (L1's open item, a
command run again after a serialization failure for every layer and every
command, stays open).

**Not chosen.** *Keeping the guest's rows reachable to a later merge* (a
pending merge recorded beside the rows, completed by a later request or a
sweep): it would cover a lifecycle that never ran at all (a host that died
between the account's opening and its lifecycle), but it needs the
transition proof kept durable, which ADR-0440's authority model keeps
host-private and one-use on purpose; the conflict the review named is
closed without it. *A fresh interaction per attempt*: the reservation would
let it run, but the record that answers a replayed lifecycle is keyed by
`identity-joined`.

**Held by** tests of `tests::protected_migration`, on PostgreSQL, each
reading the host's record of every attempt (`Server::lifecycle_attempts`,
a test's): `a_lifecycle_that_met_the_accounts_own_write_runs_again_on_
postgresql` (the two-host race: the first attempt conflicted and the second
committed, the account holds 10 + 3, the guest nothing, one record; before,
10 and the guest's 3 stranded), `a_lifecycle_that_conflicts_each_time_is_
run_four_times_on_postgresql` and `a_lifecycle_that_deadlocks_each_time_
is_run_four_times_on_postgresql` (a deferred trigger answering 40001, or
40P01, at every commit: four attempts, each refused by the database, the
guest's rows kept), `a_lifecycle_whose_scheduling_conflicts_is_run_four_
times_on_postgresql` (a lifecycle that schedules a run, its scheduling
answered 40001: four attempts), `a_lifecycle_whose_statement_fails_
otherwise_runs_once_on_postgresql` (a write a trigger refuses with an
exception of its own: one attempt), the refusal test (one attempt: a
refusal is no conflict), and `a_conflict_noted_for_one_command_is_not_
anothers`. Twelve mutants in `durable_tables_mutations.py`, one for each
rule above.

**Reviewed** (2026-11-16, a reviewer given the diff, not this account): no
attempt runs again after one that committed, no conflict mark reaches a
lifecycle from another request, the outcome let go is only the
lifecycle's own, and each attempt takes its authority again. It found the
commit's scheduling statements refused unmarked (a lifecycle that
schedules a run met a conflict there and ran once), the race test unable
to show that it retried, no control for a statement failing otherwise, for
a deadlock or for the outcome let go, a queue step nothing observed, the
mark kept for the thread rather than for the command, and the row-ownership
script left without the Clock's witness. Each is repaired above, the
witness's test added back to `row_ownership_mutations.py`, with a test or
a mutant for each; the commit's taking of a run marks a conflict the same
way, but no lifecycle completes a run, so no test reaches it.

**And the owned Clock witness is one** (the landing's fourth finding, NEXT
after L3, item 4): `tests::owned_clock` is kept and
`tests::protected_migration`'s copy of it, with its `clock.pw`, removed; the
two were the same scenario under two programs' names.

## Landing (L4, 2026-11-16)

L4 merges the integration branch's seventeen commits after `2e2ef45b` (to
`ff57edc7`: 67 files of code and 1,370 receipts) onto L3 (`f375b92d`) in
`7b8fb7cc`, branch `wip/l4-bounded`: the fifth prerequisite above, counts
over declared selectors, ordered selector windows, counts after an order
mark and fixed relation windows, through the compiler, the host and both
adapters. Six files conflicted: `tables.rs` four times (L4's count, window
and relation readers beside L2's `public` tables and the PW0370 numbering,
which master kept; the table manifest's `public` and `windows` both kept),
`head-only-allow.txt` (PW0370 again, and L4's own entry), the row-ownership
script's test list (both sides', nineteen commands, none twice), and the
docs (the integration branch's 454 lines added to the unnumbered decision
moved here, before L3's landing). Nothing else needed a hand: at the merge
the workspace's 4,274 tests pass on PostgreSQL 18.6 (26 ignored), and every
mutation anchor matches once (6,104). The full verify 43809526375 at
`7b8fb7cc` passed every job; its summary found two of 357 recipes failing,
each repaired at the branch's head and run again there:
`e14-row-ownership` (141 of 142, one kill INVALID: its failing test
panicked at an `unwrap` of `table_ranges.rs`, which the strict validator
does not read as an assertion; the test now fails only through assertions)
and `e14-check-is-build` (the program census covered no assembly of
`examples/relations/`, L4's relation fixture; it is one now). The other 355
recipes' evidence (646 files, `bddbcf54`) is the merge's record.

**Reviewed** (2026-11-16, a reviewer given the diff and this ADR's
sections, not the account of the merge): no high or medium finding in
authority, privacy, adapter parity or bounds. Each new read goes through
the existing gate, owner check, answer check and shared-row projection;
strings order byte by byte on both adapters (PostgreSQL's `bytea`); every
limit applies inside the database. It found two low ones, each repaired
with a test that failed before and a mutant:

1. **An owned `public table`'s relation was refused by the host** through
   an index its owner does not lead, where the compiler reads a public
   table by any key or index (L2) and made the read: the build failed at
   startup instead of answering. The host admits a public source's every
   path (`window_target`), as it admits an unowned one's; a public table
   keeps no private field (the compiler refuses one), so no match field is
   revealed. `a_public_owned_membership_is_read_through_any_index_by_anyone`,
   on both adapters.
2. **PostgreSQL refused malformed direct range and relation requests in
   other words than the memory adapter's**, which `shape.rs` promises word
   for word: its range parsing had messages of its own, and a relation's
   refusal named "relation window" where the memory adapter names the
   import. It reads a range request as the memory adapter does
   (`named_selections`, then each value held to its field's type and bound
   in the canonical order, so that a request failing two ways is refused
   for the same one) and names the import. The parity workload now reads
   twelve malformed range requests after every command, and
   `a_malformed_relation_request_is_refused_alike_on_both_adapters`
   compares five. Compiled calls were never affected: the host checks
   their arguments before either adapter sees them.

**Left for the apps' port (L7)**, by design: the memory adapter's relation
read scans the target for each selector it is given, which this decision
allows, and no server-side cap bounds a window's or a relation's limit. The
primitive is bounded by its argument; an app that passes a browser's limit
through (`examples/relations/app.pw`'s `Following`) clamps it when ported.

## Amended 2026-11-16: an outbox row leaves by age alone

The integrator's, under the owner's delegation of 2026-10-02: NEXT after L3,
item 9.

**Found.** Since L3 a host retains what it delivers until the shared
retention age, and `PgTables::forget_outbox`, L1's removal of a commit's own
rows at once, was left with no caller but its own test; the durable tables'
mutant "a delivered outbox is deleted before another host can read it"
called it to put L1's deletion back.

**Decided.** It goes, with its prepared statement: no caller needs a row
gone before its age, and platform 0004's triggers move the watermark for
any deletion, an operator's statement among them
(`any_deletion_from_the_outbox_moves_the_trim_watermark`). Its test keeps
the trimming it also held, now with the limit's batches, first ids first
(`the_outboxs_rows_are_trimmed_once_older_than_an_age`). The mutant deletes
through `trim_outbox` at age zero instead, every row at once, and is killed:
7 of the script's 91 tests fail in the run of record
(`docs/evidence/E14/durable-tables.txt`, verify 43893340880 at `148405a7`,
master's L4 merged in, 71 of 71 mutants killed). The local run while it was
written saw 8 of 85, before the review's test and L4 were in the count.

**Reviewed** (independently, 2026-11-16): sound. One low finding is repaired
here. The test could not tell the trim's `ORDER BY id` from the order its rows
lie in, or from their age, since all three agreed. It now ages its rows apart,
one statement each, so that neither agrees with their ids. Two new mutants
take the rows as they lie and the oldest by their time; each is killed at the
test's assertion. ADR-0397 points here, as its decisions 3 and 6 still
described the outbox L1 forgot.

## Amended 2026-11-17: each upload and asset refusal told by its own rule (L5's follow-up)

The integrator's, under the owner's delegation, from L5a's independent
review (its findings L7 and L9) and the follow-up's own review. The compiler
filed refusals under rules whose invariants do not describe them, and the
asset tests held most refusals to being errors, whatever their reason.

- **PW5604 `upload_identity_taken`**: an upload's emitted interface and
  generated declarations are its own, no other upload's or declaration's.
  It tells both collisions (two uploads whose names emit one interface; a
  declaration emitting a generated declaration's identity), which PW5601
  (an upload's clauses and limits) told until now.
- **PW5605 `upload_claim_not_idempotent`**: a command that claims or releases
  an upload declares `idempotent_by`, the key the claim bridge settles a
  claim by (L5b above); PW5601's until now.
- **PW0371 `table_asset_malformed`**: a table keeps an upload only as an asset
  of a public table, one top-level field of its row holding the exact
  generated reference of a public upload its source holds, associated once,
  and never a claim. `tables::assets`' twelve refusals were PW0361's, whose
  invariant is the row, its key and its indexes. An `asset` clause written
  wrong stays PW0361's, with every clause's form.
- **PW5601** now names the ceiling it has always held (ADR-0260): no limit
  past what a host reads whole.
- **Each asset test's refusal names its reason**: the 85 bare `is_err()` in
  the six asset suites and nine checks that matched only a prefix every
  refusal of their kind carries, each reason observed before it was
  written; a loop's or a helper's cases name themselves when they fail, and
  three enum variants are matched exactly.

Held by `tests/table_assets.rs` (each refusal by its code and its words;
`a_declaration_emitting_a_generated_upload_identity_is_pw5604` and
`a_command_claiming_an_upload_without_idempotent_by_is_pw5605`, new), and four
mutants in `row_ownership_mutations.py`, each telling one of the three new
codes' refusals as its old code again.

## Amended 2026-11-17: the corpus's upload and asset cases, and each refusal's boundary (L5's follow-up)

The integrator's, under the owner's delegation, from L5a's review (a low
finding: no corpus case held an upload or asset rule) and the architect's
ruling of 2026-08-06 (an invariant the compiler enforces has its canonical
accepted and rejected specification, and survives a program its fixture did
not anticipate). Corpus C34 (`docs/CORPUS.md`):

- **Six rejected fixtures**, each its own defect alone: R-119 (an upload that
  states no limit on its bytes, PW5601), R-120 (an upload's route that is a
  page's, PW5602), R-121 (a form that sends a file where no upload takes it,
  PW5603), R-122 (a declaration that emits a name an upload generates,
  PW5604), R-123 (a command that claims an upload with no idempotency key,
  PW5605) and R-124 (an asset held in a field that is not its upload's
  reference, PW0371).
- **Two accepted controls**: A-061, an upload in the module of its name and
  a form that posts a file to it; A-062, a public table that keeps that
  upload's reference as its asset, written by a command that claims it and
  declares `idempotent_by`, and read by any reader.
- **Six generality pairs** (`examples/generality/<symbol>/`), each a shape
  its fixture does not have, and a valid neighbour: a limit past a host's;
  a serving path under another module's upload's; a file form posting to an
  upload's route without multipart; a type taking a generated operation's
  emitted name; a claim in a helper, and one inside an `if`, beside a
  command that only reads what its reader attached; an asset table whose
  source does not hold its upload, and a claim kept in a row, beside a table
  with no asset. An owned table's asset is held by `tests/table_assets.rs`
  alone: a witness's table would need the upload's source, and A-062's is the
  one that may hold it. Generality is 82/82.

Writing them found **eight refusals, of four rules, that named no
boundary**, which charter §16.3 asks of every rejection and the corpus asks
of each refusal a fixture reaches (`every_diagnostic_carries_what_charter_16_3_requires`):
PW5601's clause left out, bytes past what a host reads whole, and width or
height past any image's; PW5602's path a page or another upload holds, and
path under another's; PW5604's two collisions; and PW5605's. Each now points
at what the rule holds it to, in the file that declares it: a clause left
out, and bytes past what a host reads whole, at the route where the upload
takes a reader's file (its name, where that route is itself refused); a size
past any image's at the kinds it takes; a path at the page or upload holding
it; an interface at the other upload emitting it (only one in the module of
its name, which alone emits one); a generated name at the program's
declaration taking it; and a claim at the upload whose claim the command
takes. Held by `each_upload_refusal_names_the_boundary_it_is_held_to`
(`tests/uploads.rs`: each note's label, file and span, a holder in another
file where the rule crosses files), four tests of `tests/table_assets.rs`
(`bounded`, which holds a note's span too, and
`an_interface_collision_is_noted_only_at_an_upload_that_emits_it`), the
corpus's §16.3 test, and ten mutants, seven in `uploads_mutations.py` and
three in `row_ownership_mutations.py`, each dropping a note, pointing it
elsewhere, or filing it in the refusal's own file, each killed at an
assertion in a local run; their recipes' evidence is this pull request's
verify's.

**Reviewed** independently (2026-11-17, `e10a59c6`): the compiler change is
sound, and no refusal changed whether it fires. Its findings, each repaired:
three workspace tests the new controls broke and neither preflight nor the
suites it was run with reach (`from_manifest.rs`' count of the corpus's
resources, 28 with A-062's three; `ownership.rs`' affine types, which list
the upload's claim, `PostImage.Claim`; and `lowering.rs`' policy spans, which
exempt an upload's generated operations as a table's); the owned-table
witness, whose source check caught it as well, so it witnessed no ownership
(replaced, above); notes held by their labels alone (M2: now by file and
span, with three mutants); PW5604's note at an upload of the other's name
that emits nothing; three notes whose labels said what was not so, or that
spanned the declaration they were in; and two words of PROFILES' and
R-124's.
