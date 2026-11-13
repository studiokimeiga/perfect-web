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

Measured locally on PostgreSQL 18.6, each recipe whole with its mutation
script, on L3's own databases: at `41a03c5d`, invoked-by (14 of 14
controls killed), row-ownership (105 of 105), d3-postgres (63 of 63) and
scheduled (38 of 38); at `dbbd8e65`, platform 0004's two controls
(killed, the 78 tests of the baseline green); at `71c31395`,
durable-tables (57 of 57), store-accounts (37 of 37) and second-host at
four test threads (50 of 50). The full verify at the branch's head is the
record.
