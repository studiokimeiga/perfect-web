# Recipes for who may run a command (ADR-0406). The root justfile imports this
# file, and its settings and variables (`set shell`, `PATH`) apply here as
# there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too). Until train-14 no recipe
# ran `who_may_run_mutations.py`.

# ADR-0406: a command a reader can run declares `requires`; `requires Anyone`
# asks nothing, and an unchecked `Anyone(n)` fails closed. Its tests, the
# contract's, and the mutation controls.
e14-who-may-run:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0406 - a command says who may run it"; echo; \
       echo "produced by: just e14-who-may-run"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== PW0367 and the contract (compiler/pw-core/tests/who_may_run.rs, component_contract.rs)"; echo; \
       cargo test --locked -p pw-core --test who_may_run --test component_contract 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/who_may_run_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/who_may_run_mutations.py; \
     } > docs/evidence/E14/who-may-run.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/who-may-run.txt
