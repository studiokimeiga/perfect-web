# Independent review of the cache fence

Reviewer: separate agent `/root/review_cached_refusals`, read-only committed
objects in the existing `/home/haki/code/pw-wt/reviews` worktree. The reviewer
edited no source and ran no builds/tests/campaigns.

1. Review at `c9fa13c1` found a P1: `Render::once` could return a table
   operation already answered inside the render without executing the adapter's
   health-checked callback. `answer_within` and `read_value`'s no-page-policy
   path needed the same fence. A completed failing assertion at `4b314278`
   reproduced it. `03149f73` adds the direct-query before/after fence and a
   strict mutation control removing it.
2. Re-review at `03149f73` found a test-scope gap: the operation was warmed as
   reader `b`, while `read_value` uses the empty reader. `b496bdc3` consistently
   uses the empty reader, explicitly reuses `read_value` before closure and
   verifies an unchanged adapter statement count.
3. Final read-only review of `b496bdc3`: no outstanding actionable findings.
   The reviewer confirms the render-local bypass and test-scope gap are closed.

The parent executed the retained native/whole-campaign/CI receipts. Review
does not extend the guarantee past the checked read boundary: the existing
publication-to-COMMIT window, retained cache bytes and later HTTP/frame delivery
remain explicit limits.
