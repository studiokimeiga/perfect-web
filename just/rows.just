# The large list's recipes (a press changes what it changes, ADR-0417). The root justfile imports this file, and its settings and variables
# (`set shell`, `PATH`) apply here as there. An evidence recipe is
# `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned by the
# verification run like any other (scripts/ci_plan.py reads this file too).

# A template's layout is not the page: what the compiler writes for it, and the
# mutation controls. The large list's own suite and controls are the device
# runtime's recipe's.
e14-rows:
    @mkdir -p docs/evidence/E14
    @{ echo "a template's layout is not the page"; echo; \
       echo "produced by: just e14-rows"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== what the compiler writes (compiler/pw-core/tests/template_layout.rs)"; echo; \
       cargo test --locked -p pw-core --test template_layout 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/template_layout_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/template_layout_mutations.py; \
     } > docs/evidence/E14/rows.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/rows.txt
