# Closed tables and cached reads (2026-11-02)

Local continuation on `fix/tables-cached-refusals`, using the existing
`/home/haki/code/pw-wt/data-path` checkout at PR #72's source
`687dd9225de06cb6fd245ba54489fbd7dcb31761`. The row-ownership worktree and
its uncommitted work are preserved. No remote changes are authorized here.

## Reproduced invariant and repair boundary

ADR-0394 (currently `ADR-0394-an-app-with-no-rust-data-layer.md`), decision
4, requires tables to fail closed after publication whose platform COMMIT
fails. A direct adapter read refuses `StoragePoisoned`, but a warm query
outside that command's invalidations still answers from the resource cache.
The failing regression uses a real deferred SQLite constraint at COMMIT;
the healthy cache neighbor verifies the adapter's statement count does not
increase. The failure is at the final cached-answer assertion.

The first probe, using the guestbook's `Latest`, passed: the host's uncertain
command recovery already invalidates that query. That does not cover other
queries of the now-closed adapter. The final regression holds that distinction.

The repair extends the existing refusal to the host's cache boundaries.
`DataLayer::check_read` defaults to success; the declared memory tables check
their adapter's health. Query answers and materialized values check before
reading and again before returning a successful answer, including a cache hit,
single-flight answer and last-known-good fallback. An adapter failure during a
read must not turn into a successful cached answer. Ordinary source outages
retain their declared fallback behavior. This changes no language policy,
authority, storage representation or dependency version.

This is a read admission/delivery fence, not a rollback of already delivered
responses or an eviction guarantee for retained cache bytes. The existing
publication-to-COMMIT window remains a limitation; durable atomic tables are
separate work on `wip/durable-tables`.

## Independent review correction

The separate read-only reviewer of `c9fa13c1` found that `Render::once` can
return a remembered table operation without executing its adapter callback.
Direct `answer_within` calls, including a materialization's query with no page
policy, therefore needed the same fence. The regression at `4b314278` retains
one live render across closure and fails at its remembered-answer assertion;
its healthy neighbor verifies the first operation was actually shared.
`answer_within` now uses the same before/after check. There are four focused
regressions and five additional strict mutation controls. Final read-only
re-review at `b496bdc3` reports no outstanding actionable findings; the reviewer
ran no tests and edited no source.

Re-review found a test-scope gap: `read_value` uses the empty session, while
the test warmed the operation as `b`. The final test warms and reuses the
operation as the empty session, including a healthy `read_value` reuse with
an unchanged statement count. The preliminary `03149f73` recipe was stopped
after three completed controls so the corrected witness could be frozen; it is incomplete,
receives no campaign credit, and source restoration passed all 5,035 anchors.

## Execution record

Final code/test source: `b496bdc34467269c2b436350ba1cfb2a52e268ed`.
The [manifest](../evidence/E14/tables-cached-refusals/manifest.json) records exact
commands, native exits and SHA-256 hashes of the retained raw/gzip receipts.

| Execution | Result |
|---|---|
| Cached-query regression at `069ffc6f7e51ede213aae02070a97da10c84bc04` | exit 1, completed assertion failure |
| Render-cache regression at `4b314278ce785b2747eed47373acf4b1cbecb307` | exit 1, completed assertion failure |
| `just e14-data-path` at `b496bdc3` | exit 0; 60/60 strict controls killed, mutation baseline 62 passed/0 failed; 15 browser cases in three engines; no memory-bound stop |
| `RUST_TEST_THREADS=8 just ci` on code/test source `b496bdc3` | exit 0, `ci: OK`; server 907 passed/9 ignored, evidence machinery 1,177 tests; 351 source mutants judged by 567 checks, zero failing |
| Mutation restoration | 5,035 anchors in 325 scripts, each matching once |
| Immutable-SHA `ci_plan.py --changed 5d4d4d55… b496bdc3… --current` | exit 0; 160 recipes in 16 shards |
| `just doctor` | exit 1: standalone Koka/Wasmtime executables missing; no installation attempted |

CI began with only the completed generated `docs/evidence/E14/data-path.txt`
receipt dirty; code and tests matched `b496bdc3` exactly. Its input bytes and
start status are retained. This is local source verification, not a hosted
exact-head landing receipt. Ignored cases receive no passing credit. The complete
campaign has 60 controls: 55 already on the live branch and five added here;
the older PR body's 50-control count is historical.

No push, PR modification, merge, deployment or paid model study occurred.
The broader affected recipe plan, full hosted browser jobs, final numbering and
published-head gates remain before landing. The publication-to-COMMIT window,
already returned values, later HTTP/frame delivery and retained cache bytes
remain outside this repair's guarantee. Row authority and durable server
integration remain separate work.
