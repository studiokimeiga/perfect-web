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
