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
Frozen window checks and independent review are still pending.


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
