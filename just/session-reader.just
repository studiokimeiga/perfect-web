# Recipes for ADR-0389's immutable readers: a render and a stream worker keep
# the reader they began with. The root justfile imports this file, and its
# settings and variables (`set shell`, `PATH`) apply here as there. An
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and
# is planned by the verification run like any other (scripts/ci_plan.py reads
# this file too). Its mutation script landed on 2026-10-26 with no recipe to
# run it, which train-14 found.

# ADR-0389: a synchronous render and a stream worker are bound to the reader
# they began with, on the memory data layer. The script runs its four tests,
# the baseline and each control, with the shared strict assertion validator.
e14-session-reader:
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0389 - immutable readers for renders and stream workers"; echo; \
       echo "produced by: just e14-session-reader"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== mutation controls (scripts/session_reader_mutations.py)"; echo; \
       PW_STORE_TEST_LAYER=memory CARGO_INCREMENTAL=0 python3 scripts/session_reader_mutations.py; \
     } > docs/evidence/E14/session-reader.txt
    @grep -E "^controls:|KILLED|INVALID|FAIL" docs/evidence/E14/session-reader.txt
