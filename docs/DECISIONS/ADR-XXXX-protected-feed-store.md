# ADR-XXXX: protected feed and store data use the declared-table gate

Status: proposed local migration, 2026-11-04, starting at a9a8d79e.
Milestone: E14. Builds on ADR-0338, ADR-0350, ADR-0385, ADR-0386,
ADR-0389, ADR-0393, the owned-table and durable-table integration ADRs,
and ADR-0369. The decision was recorded before implementation; full migration gates remain pending.

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
means404; live descriptor with missing/unreadable storage means503. Never serve
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
delete vs publish in both orders; authoritative HTTP404/503 and304 ordering.
Terminate host A's actual PostgreSQL backend after committed no-ref inspection
but before physical unlink, let B republish identical bytes/new generation and
commit, then resume A and verify B's actual bytes and serving. Add strict faults
at real assertion sites, complete inventories and exact-source/restoration
receipts. Existing browser/API assertions remain unchanged final port gates.

Primary PostgreSQL18 docs checked: [explicit locking](https://www.postgresql.org/docs/18/explicit-locking.html)
and [transaction isolation](https://www.postgresql.org/docs/18/transaction-iso.html).
The [synthetic protocol probe](../evidence/E14/protected-upload-design-2026-11-04/primary-probe.log)
on18.6 reproduces advisory-only stale unlink after actual backend termination,
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
and PostgreSQL18 [session advisory lock lifetime](https://www.postgresql.org/docs/18/explicit-locking.html#ADVISORY-LOCKS).
The database session ending releases exclusion; only distinct physical generation
paths can fence a process that resumes stale cleanup after that event.


### Durable namespace/metadata enrollment before row lifecycle

Recorded 2026-11-05 before PostgreSQL enrollment implementation. Append platform
migration0004; retain0001-0003 bytes verbatim. Persist namespace, immutable base
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
the namespace, declaration or binding despite RETURNING1. AFTER INSERT and
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
neighbors pin this order. Primary PostgreSQL18 documentation:
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
