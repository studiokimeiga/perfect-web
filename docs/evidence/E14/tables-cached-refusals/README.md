# Local closed-table cache verification

Source `b496bdc34467269c2b436350ba1cfb2a52e268ed`, branch
`fix/tables-cached-refusals`. See `manifest.json` for commands, statuses and
raw/compressed SHA-256 hashes. Gzip preserves diagnostic bytes without turning
failed transcripts into canonical successful recipe summaries.

- `regression-069ffc6f.log.gz` and `render-regression-4b314278.log.gz` are
  expected failing-first assertions at those exact local commits.
- `interrupted-03149f73.log.gz` is an incomplete campaign, deliberately stopped
  for a review correction after three controls. It receives no campaign credit.
- `interim-*` receipts retain earlier verification and receive no automatic
  final-source credit.
- `data-path-b496bdc3.log.gz` is the complete final local gate: 60/60 controls,
  62/0 mutation baseline, 15 browser cases in Chromium/Firefox/WebKit.
- `ci-b496bdc3.log.gz` is successful local CI on that code/test source, with
  only the generated data-path receipt dirty at start (retained separately).
  Server: 907 passed, nine ignored. Ignored tests receive no passing credit.
- The 160-recipe plan and full hosted browser/publication checks remain
  separate landing obligations. These files were not fetched from hosted CI.

Final independent committed-source review has no outstanding actionable
findings; see `independent-review.md`. This establishes neither production
readiness nor row-level authorization. Publication-to-COMMIT visibility,
already returned values, later HTTP/frame delivery and retained cache bytes
remain outside the repair's guarantee.
