# Declared data and row authority: local integration (2026-11-03)

The user asked to continue after the cache-fence repair. This work reuses the
clean `/home/haki/code/pw-wt/data-path` checkout on `fix/row-data-integration`.
Its first parent is `3ae9c3c5`, which records the verified cache-fence source
`b496bdc3`. No remote publication or deployment is part of this continuation.

## Preservation and baseline

The original row-ownership checkout remains on `track/row-ownership` at
`ff9ff2c6`, with its 19 default porcelain entries (20 actual files). A temporary
Git index captured those edits in local snapshot
`b407ede454126709e7bf217ac82664a742d89c48`, branch
`wip/row-ownership-recovery`. The original branch, real index, status and every
edited file's SHA-256 were checked unchanged. The preservation manifest is in
this Codex chat's `work/row-ownership-snapshot/`, outside the source repository.
No recovered conversation transcripts are copied into this repository.

Before integration, the saved source passed the focused compiler tests (11
predicate-body and 20 table-owner cases) and three real server guarded-query
tests. Those observations do not establish generic server wiring for owned
tables or a shared query/predicate snapshot.

## Integration rulings

- Both tracks reserved PW69xx. Keep all public diagnostic numbers unchanged,
  with one registry owner, `Dating`, for the integrated H1/H2 privacy rules,
  including `ROW_HOLDS_HANDLE` (PW6901). The one-owner-per-block gate remains
  intact. This changes registration ownership, not authored language behavior.
- Preserve both distinct host APIs: the caller-less authorized-table path and
  the row-aware `call_with_tables_as` path. The former explicitly runs as
  `Caller::Nobody`; it must not acquire row authority merely because histories
  were merged. The first combined build caught the missing caller argument.
- The platform retains both `account_of` and `id_of`. Recompute the trusted
  platform source pin only after verifying their merged declarations; do not
  waive its guard.
- Regenerate the diagnostics guide from the merged registry. Preserve both
  recipe imports. For conflicting planner timings, use the larger previously
  recorded value; these are conservative planning estimates, not a new measured
  performance result.

## Next proof boundary

ADR-0115's amendment and the owned-tables ADR require a program predicate and
its guarded query to read one state. The row branch's `CallTables` currently
reads published tables separately; the data-path branch supplies snapshots.
Their integration must preserve the engine's caller, gate and shared fuel
budget while giving both reads one snapshot. Generic server wiring and real
two-user witnesses remain required; PostgreSQL's corresponding transaction
proof is separate. No row-authority completion is claimed by merging code.

## Verification

Pending combined compiler/adapter/server tests, strict whole campaigns,
independent review and exact-source receipts. The earlier cache repair's
receipts remain at its own branch and source; they are not transferred to the
combined tree.

The integration baseline now passes `cargo check --locked -p pw-dev-server`,
the three compiler targets `platform_contracts`, `table_owners`,
`predicate_bodies`, the four adapter targets `compiled_predicates`,
`compiled_owners`, `compiled_guarded`, `compiled_snapshot`, seven combined
guarded-query/cache-fence server cases, and all twelve code-registry tests.
The measured trusted-platform union pin is `0x0c6761b484fa478d`, retaining
`context.account_of` and `capability.id_of`. All 5,091 mutation anchors match.
These are merge-baseline checks, not a whole-campaign or server-wiring claim.
