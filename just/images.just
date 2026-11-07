# Recipes for what stands for an image (ADR-0404) and an input button's name
# (ADR-0405). The root justfile imports this file, and its settings and
# variables (`set shell`, `PATH`) apply here as there. An evidence recipe is
# `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned by the
# verification run like any other (scripts/ci_plan.py reads this file too).
# Until train-14 no recipe ran `images_mutations.py`, so a change that undid
# its controls' faults was checked by nothing but the anchors.

# ADR-0404 and ADR-0405: an `<img>` has an `alt`, an image button an `alt`
# with text, each read whatever its case. Its tests, the generality witnesses,
# and the mutation controls.
e14-images:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0404 - an image says what it shows; ADR-0405 - an image button's alt"; echo; \
       echo "produced by: just e14-images"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== an image and an image button (compiler/pw-core/tests/images.rs, generality.rs)"; echo; \
       cargo test --locked -p pw-core --test images --test generality 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/images_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/images_mutations.py; \
     } > docs/evidence/E14/images.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/images.txt
