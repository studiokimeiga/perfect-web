# Row browsers and PostgreSQL, local verification 2026-11-03

Verified code/test source `6b31f4ba3468fdd50d0fb4ce357a86b0e571f9b6` on `fix/row-data-integration`, building
on the previous local record fe5ce0c6. Nothing was published or deployed.

## Behavior

Two browser account contexts exercise same-key owned rows, forged authors,
private projections and a guarded query told its authored predicate words.
The same spec runs in Chromium, Firefox and WebKit on memory and PostgreSQL.
Typed query denials survive fresh loads and failed shared flights without
using command refusal state; deployment-only predicates use the typed engine
path as well. Words are escaped at the HTTP page boundary.

The PostgreSQL bridge supplies `CallTables` for Command and Snapshot, with
staged command predicate reads supplied by the generic server's existing
fenced transaction. Real database witnesses cover staged visibility, refusal
before body execution, deletion between predicate and query body, a saved
snapshot versus a new one, serialization refusal after a conflicting commit,
ended/wrong-kind callbacks and a restart with stored sessions. No business
rule was implemented in Rust.

## Failure-first records and review repairs

- Chromium's isolation neighbor passed while the guarded page expected 403
  and received 503 (`browser-red.log.gz`). Typed error identity had been lost.
- Independent review found the legacy no-table path still flattening its
  deployment denial. A real HTTP regression reproduced 503, then the empty
  manifest typed path repaired it (`deployment-query-red.log.gz`).
- A denied shared flight now has a direct seam witness that proves a second
  invocation actually joined it. Its removal control fails an assertion.
  Each worker returns its own command-TLS state, so the assertion checks the
  thread that performed the query.
- The first re-anchored TLS operator targeted execution rather than cached
  authorization and survived: no credit. Restored to the original cached
  authorization fault, it is killed. Both diagnostic and final logs remain.
- PostgreSQL's owned edit was refused because staging supplied no predicate
  reads (`durable-owned-red.log.gz`). Callbacks now use that command's state.
- The combined durable baseline was 47 passed / 3 failed: snapshot transaction
  overhead was charged against a primitive's one-operation bound. The total
  SQL counter is unchanged. An independent counter excludes only exact
  transaction-boundary SQL strings; every other query/batch remains counted.
  Total and operation counts are both pinned on real PostgreSQL. Counter
  suppression, boundary misclassification and a repeated-read control fail.
- Independent review corrected a label: successful read-only snapshots end
  with COMMIT. The bfcd campaign was intentionally stopped cleanly after two
  controls to correct it; source restoration passed. No whole-run credit is
  transferred. All accepted gates completed at 6b31f4ba.
- At f81, a sequential D3 attempt stopped after 16 controls before a
  complete ADR0357 parallel restart. The partial run receives no credit.
- All f81 runtime campaigns completed, but CI failed three tooling checks:
  the fake build inventory omitted owned.sh, the suite-status gate rejected
  the browser helper's OR-list, and failed browser evidence was removed.
  The repair registers the fixture builder, builds the shared runtime
  explicitly, calls the browser helper directly under errexit, and retains
  failed evidence while returning failure. Focused build-order, suite-status
  and failure-injection tests pass (160 injected suite failures across 106
  recipes). The f81 receipts and failed CI are diagnostic history.
- The b890 CI then passed native tests (server 951 passed, 9 ignored), but
  failed canonical Pleris formatting of examples/owned/pages.pw. The formatter
  changes only spacing on seven lines; git diff -w is empty. test-compile
  already covers the fixture, while preflight does not run that check. The
  compile gate and remaining compiler controls passed before the successor
  was frozen; those working-tree diagnostics receive no acceptance credit.
- Every final gate reran at 6b31f4ba with four isolated native-mutation copies,
  four Cargo jobs and two native test threads per copy. The refusal parts
  run concurrently in separate copies with their standard producers; CI
  uses eight native threads. Exact runner bytes and settings are retained.
  No acceptance transfers from f81 or b890 to the corrected source.

## Complete local gates

Run `work/run_owned_corrected_gates.sh` from the continuation workspace through
WSL, sourcing `/home/haki/pw-setup/env.sh`. It requires a throwaway PostgreSQL
URL and records its server version. At this source, every step completed:

| Command | Result |
|---|---|
| `just e14-row-ownership` | 71/71; baseline 100/0; six memory browser checks |
| `just e14-data-path` | 60/60; 15 guestbook browser checks |
| `just e14-d3-postgres` | 60/60 |
| `just e14-durable-tables` | 51/51; owned-row and guestbook/restart browsers |
| `just e14-refusal` | 23/23 |
| `just e14-refusal-2` | 7/7 |
| `RUST_TEST_THREADS=8 just ci` | passed; server 951 passed, 9 ignored |
| `python3 scripts/mutation_anchors.py` | 5163 anchors, 327 scripts, each once |

Ignored/skipped tests receive no passing credit. CI began with only generated
receipts dirty; exact input bytes and hashes are retained. Every completed
campaign restored its source. Independent final review has no outstanding
findings. These local receipts are not fresh hosted landing results.

## Preservation and evidence

The original row checkout remains ff9ff2c6 on track/row-ownership, with its
real index SHA256 16c5e851629966e104c105e2baf02cc7f859d7f7f26b66d92ecea181b09b0d33
and all 20 edited files unchanged. Recovery snapshot b407ede4 remains retained.
The original durable checkout remains d6442c4e on wip/durable-tables, with its
three edited files unchanged. Preservation manifests and final audits are
retained alongside command logs and exact recipe receipts in
`docs/evidence/E14/row-browser-postgres/`; manifest.json gives raw/gzip hashes.

Durable code was merged from d6442c4e without Rust conflicts. Existing
integration receipts were retained at the merge; recipe imports were united
and existing timing estimates combined by maximum, not presented as new
measurements. New final-source receipts supersede those historical inputs.

## Limits

Authorization is for an invocation's selected read snapshot, not later HTTP
or frame delivery. Reused values receive the new invocation's authorization;
their data still follows the query's declared freshness. PostgreSQL's snapshot begins at its first table read.
The operation metric counts non-boundary SQL calls, not total database round
trips. The operation-cost seam excludes transaction boundaries: the total counter
and explicit overhead witness own that separate claim. The memory adapter retains its publication-before-COMMIT window and does not
promise cache-byte eviction. The durable adapter retains its single-host scope; no outbox tail, feed/store migration, H3/H4,
private write-flow or default-deny phase was added. ADR numbering and every
fresh published-head landing gate remain. Paid model studies stay deferred.
