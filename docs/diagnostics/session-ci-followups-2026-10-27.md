# Session authority: CI integration follow-ups, 2026-10-27

Candidate `2f5798b1` passed its 17 local gates, both ordinary CI events, full
three-engine browsers and aggregate CodeQL. It is **not merge accepted**:
actual recipe artifacts expose failed recipes even when the shard runner's
shell job succeeds. The final workflow summary checks those recorded exits.
Failed, cancelled, partial and earlier-head evidence supplies no new-head credit.

## Four reproduced causes and repairs

- The verification Recipes shell exported only `PW_FEED_DATABASE_URL`; the
  strict command-authority gate requires both adapter URLs. Export the chosen
  database as both, retaining the strict refusal. A regression runs the actual
  shell block across a child-process boundary, clears inherited URLs, checks
  the populated and no-database neighbors, and exercises the real gate refusal.
- The full PostgreSQL server partition found two obsolete bounded-statement
  fixtures. Measured seed and larger-store vectors remain equal; only the two
  Clock transactions add two SQL statements for exact-generation locked read
  and conditional consumption. Pin expire_order at 29 and send_to_store at 12;
  browser and Payments budgets are unchanged. Replace the synthetic browser
  Clock call and split test-only cleanup with a real adapter-bound Candidate
  and Clock driver. Prove physical consumption, then hold a real payment commit
  while a conditional retry of that consumed revision waits on Writes and
  returns false without restoring the row. Keep the exact [29,15] transactions,
  completed payment marker and canceled-order assertions.
- The producer-ownership controller still froze ten methods after the PID
  publication regression increased the actual suite to eleven. Correct that
  explicit count, preserve all nine controls and strict terminal validation,
  and calibrate real discovery including the new PID witness. A stale ten-test
  freeze must continue to refuse.
- Repeated browser verification found a distinct other-tab race. HERE's held
  document was correct, but THERE's own live stream returned recovery:reload
  after successful sign-out, aborting its fetch redirect before the next stale
  press. Give each page its own held routes and await THERE's actual next
  subscription before its unchanged sign-out fetch. Keep HERE's cursor/header,
  403, refusal, cart rollback and legitimate 202 predicates. No retry, sleep or
  timeout increase is introduced.

## Component evidence and review scope

Records are in `docs/evidence/E14/session-ci-followups-2026-10-27/`.
The `.gz` files retain exact original bytes; readable copies normalize LF
and trailing whitespace for repository checks. `transcript-index.json`
records both hashes. Original log hashes in the proposal decision refer
to those exact originals, recoverable by decompressing their `.gz` files.
The original two PostgreSQL failures reproduce at the old immutable head.
The reviewed fixture yields complete named 2-pass baseline and restoration
runs; removing only production retry_current's Writes guard yields one native
assertion failure after the retry and provider workers drain, with retry_waited
false, retried false, completed marker one and transactions [29,15]. The first
terse diagnostic run was refused by the collector for missing named inventory
and supplies no gate credit; its original record is retained outside the repo.

The actual producer-ownership recipe has an eleven-method green baseline and
nine valid assertion controls. The CI environment regression has three green
cases after a reproduced missing-URL failure. The Firefox controlled-server
probe extracts the actual helper/sign-out phase: old source gets NetworkError,
the repair completes the redirect with HERE's document retained, and removing
only THERE's hold restores NetworkError. This is helper-mechanism evidence,
not a Pleris or three-engine acceptance claim.

Separate reviewers inspected the source, unchanged older controls, named native
failure sites, worker draining and restoration. These are component/precommit
proofs. The composed commit must still pass fresh preflight, real full-PostgreSQL
and memory partitions, bounded-statement and ownership controls, command and
scheduler controls, actual browsers, CodeQL and every planned hosted recipe.
Runtime authority fences and their privacy checks are not weakened.

## Reproduction

With both throwaway PostgreSQL URLs configured:

```sh
PW_STORE_TEST_LAYER=postgres cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::bounded_statements:: --test-threads=1
just e14-bounded-statements
just e14-producer-ownership
python3 -m unittest -v scripts/tests/test_verify_recipe_database_env.py scripts/tests/test_producer_ownership_metadata.py
```

The final fresh-head gate also runs the full server partition with
`PW_STORE_TEST_LAYER=postgres`, not just the curated command tests. Source and
recipe results must be attributed to that exact composed commit before merge.
