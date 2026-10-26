# ADR0388: no passing no-DB skip; full native inventories and strict controls.
e14-scheduled-replay:
    @test -n "${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}" || { echo "scheduler gate requires PostgreSQL" >&2; exit 1; }
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.scheduled-replay.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "ADR0388 - scheduled generation consumed with its authoritative commit"; \
       echo "produced by: just e14-scheduled-replay"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       python3 -m unittest -v scripts/tests/test_scheduled_replay_results.py; \
       python3 scripts/scheduled_replay_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/scheduled-replay.txt
    @grep -E 'baseline:|campaign scope:|mutants killed|^Ran |^OK' docs/evidence/E14/scheduled-replay.txt
