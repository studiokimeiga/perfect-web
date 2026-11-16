# Protected feed/store prerequisite: ordered selector windows

Window implementation source: `79de0dd0970cf0477d5d67b814eb1038b7ceacfd`.
Snapshot-proof/diagnostic repair: `d417983f91c8eae565d139ccce37dea85feda4ab`.
Final diagnostic/test-only successor: `5b7f5be8e26f84852ebffce6a194acdd3f651943`.
Independent immutable review is clear at each final delta. These records close
a local prerequisite only. **Canonical feed/store still use legacy Rust data
layers.** Final application whole campaigns, browsers and CI remain pending.

## Behavior and result work

Generated first/last windows use the table's existing nonempty key prefixes,
full keys and indexes, ordinary read grants and owner/sharer handles. A scalar
limit applies to that selector; a set limit applies separately to each distinct
selector. Groups are canonical ascending; last reverses the complete within-
selector order, including primary-key ties. The gate rejects excessive,
duplicate, foreign or incorrectly ordered answers. Every selector is validated
before a nonpositive limit returns empty. No capacity is reserved from an
untrusted maximum Int. Owned reader-only selectors still have no empty set form.

Memory keeps bounded ordering metadata/row references and clones only selected
rows. PostgreSQL uses a scalar ORDER BY/LIMIT or unnest/ordinality plus LATERAL
with LIMIT inside each selector's subquery. At 12 and 1,500 fixture rows, compiled
scalar windows return 2 SQL rows and shared set windows 4, with bounded fixed-size
fixture payloads under the same 10,000-fuel ceiling. A one-fuel neighbor fails;
an unrestricted row-read neighbor grows. A strict fault fetches unrestricted
SQL rows and clips the correct result afterward; the real row/byte oracle
catches it. These are returned-result measurements, not constant database scan
work or bounds on arbitrary row sizes. Prepared startup reads are included in
counters; operation tests use deltas. Raw malformed/empty calls issue no SQL;
compiled snapshots may still open/end a transaction, counted separately.

Compiled predicates see staged rows. A held query preserves both its newest-
row predicate and original body after another transaction changes the body and
inserts a new newest row; a fresh query is denied. Generic conformance computes
windows independently from generated rows for all guestbook/lists tables at
both sizes, outside/inside renders, across five bounds, ties and duplicate/
missing keys. Existing ordinary-row/count/cost assertions remain intact.

## Exact-source verification

Unmodified logs, source guards, commands and hashes are in
[the evidence directory](../evidence/E14/protected-windows-2026-11-04/reproduce.sh).
These are three exact-source records; prior-SHA results are not relabeled.

At 79de: affected core 322, CLI 5, host 115 (9 pre-existing ignored fixtures),
memory 117, PostgreSQL 81, selected server 174 passed; test-compile and preflight
passed (5,207 anchors). Focused strict controls: D3 baseline 81/0, 10/10 killed;
row 189/0, 15/15; data 66/0, 4/4; table-host 153/0, 3/3. No invalid result, survivor
or stopped process receives credit. These are focused, not whole app campaigns.

At d417: affected core 252, compiled memory windows 2 and real PostgreSQL windows 4
passed; test-compile/preflight 5,209 passed. Strict review controls: row baseline
191/0, 2/2 killed. The bounded-checker script ignores its CLI argument and ran
its entire existing campaign: baseline 40/0, 47/47 killed. This historical script
uses its existing assertion-result parser; final moved controls are additionally
run with strict complete native inventories at the successor below.

At 5b7: affected core 253, compiled memory windows 2 and real PostgreSQL windows 4
passed; test-compile/preflight 5,210 passed. Strict diagnostic controls: row 192/0,
3/3 killed; moved bounded-guidance controls 40/0, 2/2 killed with complete native
inventories. All three runners required exact HEAD and clean source before/after.
Runtime/adapter implementation bytes are unchanged after 79de; d417 strengthens
the snapshot fixture, and the final successor changes diagnostic guidance/tests.
No all-core/full-server/full-CI or fresh browser claim is made here.

## Reproductions and repairs

Legal owner names exposed generated formal aliases: values aliased a list and
row aliased a written record. Only colliding generated formal names change;
positions, app field identities and permissions stay fixed. Real compiled WIT
components cover owner fields values/keys/row/limit. A changed-key type-refusal
fixture was updated only to keep new window metadata whole.

Review found PW0405 treating an owned set's User<UserId> parameter as its list
record and including the reader in every batched selector. It now locates the
actual List, preserves its complete element type and describes the reader
separately. Varying window bounds no longer suggest a single-shared-bound set
as a direct replacement; combined key/bound direct and wrapped neighbors pin
common-bound/pure-selection guidance. Count hints describe key/count entries,
including missing-key zeros. Native red receipts retain 5 pass/2 fail and 7 pass/1 fail
before the respective repairs. One moved mutation anchor failed preflight;
re-anchoring preserves its original fault. Admission and ABI remain unchanged.

The remaining migration includes range/unread and fixed global relation reads,
generic upload claim/publication/reference/GC lifetime, initial public fixture
data and any needed private-child/ID primitives, then actual canonical ports.
Startup guards stay. Whole campaigns and three-engine/two-adapter browsers plus
CI must run at the final app source. Original dirty row/durable worktrees and
previous output bundles remain untouched; no remote or production work is claimed.
