# The page bundle's recipes (a page's code is one module, ADR unnumbered until
# it lands). The root justfile imports this file, and its settings and
# variables (`set shell`, `PATH`) apply here as there. An evidence recipe is
# `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned by the
# verification run like any other (scripts/ci_plan.py reads this file too).

# A production build puts each page's code in one module, and a page asks for
# that rather than for each piece of it. What the packaging writes; what the
# host names and serves, over real connections; a page through it in three
# engines; and the mutation controls.
e14-page-bundle:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "a page's code is one module"; echo; \
       echo "produced by: just e14-page-bundle"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== what the packaging writes (optimize-assets.test.mjs)"; echo; \
       node --test --test-reporter=spec spikes/own-renderer/optimize-assets.test.mjs; \
       echo; echo "== what the host names and serves (tests::page_bundle::)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server tests::page_bundle:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a page through it, in three engines (e2e/page-bundle.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/page-bundle.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/page_bundle_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/page_bundle_mutations.py; \
     } > docs/evidence/E14/page-bundle.txt
    @grep -E "^ℹ (pass|fail)|^test result|passed|mutants killed|panicked at" docs/evidence/E14/page-bundle.txt
