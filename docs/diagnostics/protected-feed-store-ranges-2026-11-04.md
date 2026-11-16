# Protected feed/store prerequisite: counts after declared order marks

Implementation/test source: `f4fc668f9d6783993203fe27e02180a3e377fc0e`.
Wrong-field-control-only successor: `06a6ad18a190cf2b69df3809e20213e5b3e6baa3`.
The accepted design preceded code at `99e1f5dc`. Independent immutable review
found no production authority/privacy/lifetime/race or range-semantics issue.
Its missing wrong-field proof is now a strict control at the successor; no
compiler/runtime/test bytes changed. **Canonical feed/store still use legacy
Rust data layers.** This prerequisite does not close their migration.

## Behavior

Scalar count_after reads use existing equality selectors and the actual order
field's type, including refinements and compiler nominal identity. Comparison
is strictly order > mark; equal values are excluded even with different PKs.
It is not an order-plus-PK cursor. No declared order means no range read. An
absent initial mark uses ordinary count rather than bypassing a refinement.
Stable IDs/order allocation after deletion remains a separate obligation.

Every set request is a selector-and-mark record, with the verified reader
passed separately. Reader-only mark records are meaningful and supported;
the ordinary CountMany empty-after-reader ban stays. Full pairs are canonical
and distinct, including zero for missing ranges; same selector/different mark
stays two requests, exact duplicates coalesce. The mark name appends _mark
until distinct from every stored field's emitted identity. Generated imports,
formals and record types retain named collision refusal. PW0405 describes the
actual heterogeneous input record including mark and separate reader.

The host checks scalar mark ABI before execution, including an unused branch,
and validates every member before callbacks. Counts have exact complete pair
coverage/order/membership, no duplicates/extras and nonnegative Int values.
Original table grants, owner/sharer authority, staged commands and pinned query
snapshots remain. Full result keys contain plain ids, never authority handles.

Memory counts borrowed matching row references and borrowed order values.
PostgreSQL binds native declared kinds (bytea UTF-8 strings), running one scalar
COUNT or one typed-array/ordinality LEFT JOIN aggregate for the whole set,
counting a non-null PK to retain missing zeros. No dependency or stored-schema
migration was introduced. Native Bool, refined Int, Unicode/NUL/prefix String
and values beyond JavaScript's exact integer range cover comparison/ABI.

At 12 and 1,500 fixture rows, compiled scalar counts return 1 SQL row/8 payload
bytes under 1,000 fuel; the three-distinct-pair set returns 3 rows/48 bytes under
6,000 fuel. A one-fuel neighbor fails and unrestricted row reads grow. These
measure actual prepared-statement result rows/binary field payload before
decode, using operation deltas; startup prepared reads remain included. They
do not measure constant database internal scans or exact fuel used. The strict
single-query fault fetching all rows then returning the correct count is caught.

Compiled predicates see staged rows. A held query retains eligibility and its
original body after another transaction changes that body and removes the only
newer row; a fresh query is denied. Real Pleris List.map constructs heterogeneous
records before one set call. Forged/no callers, bad types/refinements and malformed
whole sets are refused before callback/SQL. Generic conformance computes ranges
independently from generated data at both sizes and render modes, preserving
ordinary row, count, window and statement-cost assertions. PG random parity and
primitive inventories include the new forms without dropping existing ones.

## Frozen verification

[Raw receipts, runners and hashes](../evidence/E14/protected-ranges-2026-11-04/reproduce.sh)
retain exact source guards. At f4fc: affected core336, CLI5, host117 (9 existing
ignored fixtures), memory124, PostgreSQL89 and selected server175 passed;
test-compile and preflight passed (5,229 unique anchors). Focused strict native
campaigns: D3 baseline89/0,10/10 killed; row209/0,11/11; data74/0,4/4. Each binary's
complete named inventory is held; build/infrastructure failures, survivors or
stopped producers receive no kill credit. Source guard is clean before/after.

At 06a6ad18a190cf2b69df3809e20213e5b3e6baa3, the sole change is one registered SQL wrong-field mutation. It keeps
String/bytea types compatible but compares a PK instead of declared order,
allowing valid wrong answers to reach native equality assertions. Preflight
passes (5,230 anchors), strict D3 baseline89/0 and1/1 killed, clean source guard.
The named native String-order test's full raw baseline1/0 and valid mutant0/1
assertion receipts are retained with another clean exact-source guard.
Production/compiler/test bytes are unchanged from f4fc (verified Git diff).
Independent delta review is clear. These are focused prerequisite records;
whole app campaigns, fresh browsers and full CI remain final-port acceptance.

Earlier native red/working logs, fixture/build errors and obsolete API/oracle
failures are retained as diagnostics only. The first broader run exposed
handwritten generated-API expectations and a row-only conformance oracle; these
were extended while retaining existing assertions. Preflight found two older
count anchors newly matching both count paths; full old-count context now makes
each original fault unique. No earlier source result is relabeled as final.

Remaining: fixed global relation windows, generic upload claim/publication/
reference/GC lifetime, initial public data and stable-ID/private-child behavior
where needed, then actual canonical ports and their final acceptance. Existing
startup/mixed-layer/upload guards stay. Prior output bundles and original dirty
row/durable worktrees are preserved. No remote or production work is claimed.
