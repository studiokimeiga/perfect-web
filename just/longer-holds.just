# Track `longer-holds`'s recipes (the longer-holds ADR). The root justfile
# imports this file, and its settings and variables (`set shell`, `PATH`)
# apply here as there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# The longer-holds ADR: a subscription is held until something happens, up to
# 25 s, a stream's and a long poll's alike; held, its page is live, each pass
# renewing its document's `seen` (ADR-0297's `LIVE`); and a page gone ends its
# hold at the next pass. Its tests (`tests::longer_holds::`, and the idle
# subscription's) and the mutation controls.
e14-longer-holds:
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "The longer-holds ADR - a subscription held until something happens"; echo; \
       echo "produced by: just e14-longer-holds"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== held, live and ended by its page (tests::longer_holds::, and the idle subscription's)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::longer_holds:: tests::an_idle_subscription_waits_on_the_table_rather_than_asking_it 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/longer_holds_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/longer_holds_mutations.py; \
     } > docs/evidence/E14/longer-holds.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/longer-holds.txt
