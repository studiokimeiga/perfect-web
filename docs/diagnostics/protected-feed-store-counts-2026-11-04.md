# Protected feed/store prerequisite: declared-selector counts

Code/test source: `15f3c9df5542fea913f84896aabc841c94ce9369`, local branch `fix/row-data-integration`.
Independent immutable review is clear, carrying forward the authority review
after two reproduced repairs and the test-only conformance extension.
This completes a local prerequisite. **Canonical feed/store data still use
their legacy layers.** No full migration, browser/whole-campaign/CI, H3/H4,
production conversion, remote publication or deployment completion is claimed.

## Behavior and implementation

Generated scalar/set counts use only existing declared key prefixes/full keys
and indexes, with the same owner/sharer handles and grants. Set counts include
zero for every distinct missing selector, ordered canonically, with plain
selector keys. The gate validates exact result shape, requested keys, ordering,
uniqueness and nonnegative counts. No unqualified table count is generated.
Owned set forms with no selector after the reader are refused by compiler and
host. Per-element loops retain PW0405 and its generated set repair.

Memory counts index/key entries without copying stored rows. PostgreSQL uses
one scalar aggregate or one multi-array unnest/ordinality left-join aggregate,
counting a non-null key. Missing selectors retain zero; no per-key SQL loop or
full-row fetch/length is used. Snapshots/staging and persisted reopen are tested.
No dependency or stored-schema migration is added.

At 5 and 1,500 stored rows the compiled scalar query uses one SQL result row,
8 binary field-payload bytes and a 1,000-fuel ceiling. Two distinct set keys use
two result rows, 32 bytes and a 3,000-fuel ceiling. A one-fuel failing neighbor
proves metering, and an unrestricted row-read neighbor proves counters grow.
Counters include prepared startup reads; these measurements use deltas. NULL
markers, framing, simple-query results and the dedicated Tail are excluded.
Database scan work is not a constant-work claim.

## Exact-source verification

Commands and unmodified raw output are in
[the evidence directory](../evidence/E14/protected-counts-2026-11-04/reproduce.sh),
with `SHA256SUMS`. The runner required an actual throwaway PostgreSQL URL, exact
HEAD and a clean worktree before/after; no skipped database result was credited.

- core: 317 passed.
- host: 113 passed.
- memory: 115 passed.
- pg: 77 passed.
- server: 173 passed.

Test-compile and preflight pass; preflight holds all 5,194 mutation anchors.
Focused strict campaigns: D3 baseline 77/0, 6/6 killed; row baseline 180/0,
7/7 killed; generated data-path baseline 63/0, 2/2 result faults
killed. All have a valid complete native assertion inventory;
no invalid result, survivor or stopped process is acceptance evidence.
These focused controls do not replace the final whole campaigns.

The generated adapter conformance suite now checks counts for every table of
the guestbook/lists programs at both data sizes, outside and inside a render,
using independently generated rows. The ordinary row oracle and all statement-
cost assertions are retained.
Compiled owner/shared queries and forged/no-caller refusals run on both adapters.

## Reproductions, repairs and limits

Review found scalar selector `a_many` silently aliasing set selector `a` at
`count_by_a_many`. Planning now coalesces only the same name/primitive/selector;
different meanings reach the existing named duplicate-import refusal. Native
tests cover both declaration orders and renamed neighbors retaining all forms.
Review also found startup accounting documentation contradicted its counters;
the contract now includes those prepared reads, with real open/reopen witnesses.

The first broad run at `37edc0db` was interrupted cleanly before mutations after
the alias finding; its partial all-core output has no complete-suite credit.
The first `d4b6af86` server run passed 166 and failed two conformance tests because
its oracle still expected rows for counts. That failure led to the independent
count oracle; it receives no passing credit. Earlier invalid fixture/compile
attempts and native red regressions remain under `/tmp/pw-protected-migration`.

Full all-core/full CI, whole affected campaigns and browser acceptance remain
the final canonical-port gates. Next: bounded ordered windows/ranges/fixed
relation fusion, generic upload claim/publication/reference/GC lifetime, then
actual canonical app rows, projections, transitions and predicates in Pleris.
The mixed-layer and upload startup guards remain in force. Original dirty
worktrees and prior row/browser/PostgreSQL output bundles remain untouched.
