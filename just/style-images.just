# The stylesheets' images' recipes (a build's stylesheets draw with their own
# images, ADR unnumbered until it lands). The root justfile imports this file,
# and its settings and variables (`set shell`, `PATH`) apply here as there. An
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is
# planned by the verification run like any other (scripts/ci_plan.py reads this
# file too).

# A build whose own stylesheets draw with `data:` images is allowed them, and
# TodoMVC uses the official sheet as it ships: what the policy allows, the
# drawing in three engines, and the mutation controls.
e14-style-images:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/todomvc.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "a build's stylesheets draw with their own images"; echo; \
       echo "produced by: just e14-style-images"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== what a page's policy allows (tests::page_policy::)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server tests::page_policy:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== TodoMVC, in three engines (e2e/todomvc.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/todomvc.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/style_images_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/style_images_mutations.py; \
     } > docs/evidence/E14/style-images.txt
    @grep -E "^test result|passed|mutants killed|panicked at" docs/evidence/E14/style-images.txt
