# Closed tables and cached reads (2026-11-02)

Local continuation on `fix/tables-cached-refusals`, using the existing
`/home/haki/code/pw-wt/data-path` checkout at PR #72's source
`687dd9225de06cb6fd245ba54489fbd7dcb31761`. The row-ownership worktree and
its uncommitted work are preserved. No remote changes are authorized here.

## Reproduced invariant and repair boundary

ADR-0394 (currently `ADR-XXXX-an-app-with-no-rust-data-layer.md`), decision
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
regressions and five additional strict mutation controls. The reviewer runs
no campaign and edits no source; final source re-review is still required.

## Execution record

Pending: retain exact regression and repair SHAs, commands, exit statuses,
mutation controls, independent review and affected gate plan here. No final
acceptance or production-readiness claim is made by the design record.
