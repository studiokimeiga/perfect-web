# ADR-0385: a compiled component reads its tables through the table gate, typed and exact

Status: accepted under the owner's delegation of 2026-10-02, by the
integrator's twelve rulings of 2026-10-25 on PRs #29 to #40; landed on
master by PR #53 (merge `4345726a`, head `fd8e2402`, 2026-10-25). Date:
2026-10-25. Milestone: E14, the app layer. Track: `d2-integrate`. The read
half of ADR-0338's D2 (issue #27's items 1 to 4); ADR-0386 takes the writes
and the other reads. Builds on ADR-0338 (decision 1, D2), ADR-0350 (D1: the
table declaration and `tables.json`), ADR-0018 (the host mirrors the
compiler's artifacts by field name), ADR-0166 (a host's record is read
through the declared type), ADR-0179 (an answer from outside holds its
invariants) and ADR-0277 (a read is linked with no grant).

## Context

- **D1 wrote half of a table** (ADR-0350): a program declares a table, the
  compiler generates its functions, each bound to an import of its own
  (`pw:tables/lists#get`, capability `database.read<Lists>`), and `pw build`
  writes `tables.json`. Nothing linked such an import, so no declared table
  could be read by a running component.
- **Issue #27** asked for one vertical slice: the generated `get` and
  `get-many`, linked through the existing `pw-host` execution path to the
  memory adapter (`pw-table-memory`, PR #12), from a real compiled component,
  with no app-specific Rust, refusing what the manifest or the contract does
  not name, and rejecting unsupported values rather than defaulting them.
- **Twelve stacked PRs** (#29 to #40) each built a piece: the gate (#29),
  typed selectors in the adapter (#33), the WIT selector decoder (#30), the
  row encoder (#31), their composition into one host callback (#32), and the
  compiled readers' imports, admission and calls (#34 to #40). Their author
  had no Rust: only #29, #30 and #35 had run the full checks.
- **The coordinator's review** (2026-10-25) wired the three stacks and ran
  every test. It found no authority leak in any one PR, and several between
  them: `linkable` trusts an import's own claim of its capability, so a
  contract claiming `database.read<Other>` on `pw:tables/lists#get`, on a
  node granting `Other`, linked whatever the host map held there;
  `instantiate` linked a stub for any granted import, table reads included;
  the engine projected every answer onto the component's type, so a row
  with a field more (`secret`) reached no component and was not refused
  either; #31 encoded a row's fields in the order of a JSON object's keys.

## Decision

The integrator's twelve rulings, as PR #53 built them:

1. **One end-to-end test**: the compiler's own `tables.reader` components
   (`examples/tables/reader.pw`), admitted by the imports Wasmtime reads from
   their bytes, read `MemoryTables` through `MemoryTables::read_host_fns`
   and `pw_host::engine::call_with_tables`
   (`runtime/pw-table-memory/tests/compiled_reader.rs`).
2. **The gate is the only way a table import links.** `pw_host::tables`
   mirrors `tables.json` (ADR-0018's way) and holds #29's gate
   (`Manifest::admit_reads`): a `pw:tables/…` import, in any spelling of the
   namespace, links only as a `get` or `get-many` the manifest declares, the
   host's own import (not a component's), carrying exactly its table's read
   capability, and granted; any other table import refuses the whole
   component, named. The engine asks the gate for every table import a grant
   would link (`gate_links`) and links none without a manifest:
   `call_within`, `call_measured`, `call_authorized_within` and
   `instantiate*` link no table import; `call_with_tables` links them
   through the gate.
3. **A manifest is whole before anything reads it** (`Manifest::from_json`):
   each table's interface is its name's, each function's import is at its
   own table's interface under its own name, a `get` at `#get` and a
   `get-many` at `#get-many`, one function an import, each effect its
   primitive's, every field a function names its table's, and a `get` and a
   `get-many` read by the table's key. The memory adapter reads its manifest
   back as the host does, so the two are one.
4. **Types where it links**: `engine::table_link::check_import` holds each
   table import's type, read from the component, to the read tables.json
   declares (a `get`'s key fields and `option<row>`, a `get-many`'s list of
   keys and `list<row>`, a row's fields counted, named, typed, in order),
   and refuses before anything is instantiated.
5. **A key is held before an adapter reads it**
   (`engine::table_selectors`): its arity, each field's declared type and
   bounds, a composite key's fields by their own WIT names, never by place.
6. **A row is encoded as its declared record** (`engine::table_results`):
   its fields in their declared order, by their WIT names, each by its
   stored type and within its bounds; nothing depends on a JSON object's
   key order.
7. **Every answer is its read's, always** (`table_link::check_answer`),
   whoever supplies it: a `get` answers none or the row of the key it was
   given; a `get-many` answers rows of keys it was asked, each once, in
   tables.json's order (`sorted_by`, then the table's order, then its key);
   a key not found is left out.
8. **A row is exactly its table's**: no field more, none less, none out of
   place, each of its type and within its bounds. Table answers are not
   projected; ADR-0166's projection stays for other host records.
9. **#40's case is a type**: an `added` given as an `s32` (which no
   invariant covers) is refused for its type, and a valid read after it
   still works.
10. **`pw-core` links no Wasmtime**: the engine tests moved to
    `runtime/pw-table-memory/tests/compiled_reader_abi.rs`.
11. **One test per component**: no `if component_id == …` branch that a
    rename would skip silently.
12. **#36's formatting failure** was fixed, and replays as the case
    `pr36-rustfmt` in `docs/quality/pr-regressions.json`.

## Alternatives

- **Link a table import by `linkable`, as every other host import.**
  Rejected (ruling 2): `linkable` trusts an import's own claim of its
  capability, which is right for a capability and wrong for a table, whose
  capability is fixed by tables.json, not by the import.
- **Project a table's answer onto the component's type** (ADR-0166).
  Rejected (ruling 8): a projection drops what the type does not name, so
  an adapter leaking a column would go unseen; the declared row is the
  contract, both ways.
- **Check an import's type when it is called.** Rejected (ruling 4): a
  stale or forged build is refused before it runs, not at its first read.
- **JSON as the WIT ABI of a table read.** Rejected by #30 to #32's design:
  JSON is the memory adapter's internal value, confined to its boundary.

## Evidence

- `docs/evidence/E14/table-host.txt`, by `just e14-table-host` at
  `34348f56`: `pw-table-memory` 35 tests (compiled reader 10, its ABI 12,
  memory 11, selector types 2), `pw-host --features engine --lib` with its
  17 table tests, `pw-core --test table_host_reader_fixture` 1; and
  `scripts/table_host_mutations.py`, **49 of 49 mutants killed**, each by a
  failing test (the manifest held whole, the gate, the only way, the
  link-time types, the keys, the rows' encoding, the answers, the exact
  row).
- `just ci` passed at `fd8e2402` on the PC (368 test binaries); every
  workflow at that head passed (CI 40219469308, verify 40219469162, whose
  plan held `e14-table-host` current).

## Not claimed

- **Writes**, and reads other than `get` and `get-many` (prefix, index,
  `all`): ADR-0386.
- **The development server serving a program's tables** (routing in
  `from_build_layers`), staged commits with the platform's records, the
  generated conformance suite, and an app served with no Rust data layer:
  later D2 tracks.
- **Keys of other than `Int`, `String` or `Bool`** (D1 also allows nullary
  cases), and row fields of `Unit`, `Set`, `Map`, cases or a type that
  contains itself: refused where linked, not served.
- **The manifest's provenance**: the gate trusts the deployment's
  `tables.json` as the build's own, and an answer is held to the manifest,
  not to the database.
- **ADR-0338's TodoMVC acceptance**: TodoMVC has had no host data layer
  since ADR-0366 moved its todos to the device, so "TodoMVC with its layer's
  Rust deleted" cannot measure D2; the coordinator ruled its replacement
  (ADR-0386, "What remains").
