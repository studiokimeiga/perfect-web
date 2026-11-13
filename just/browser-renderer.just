# The browser renderer's recipes (the browser renders with a page of
# JavaScript, ADR-0411). The root justfile imports this
# file, and its settings and variables (`set shell`, `PATH`) apply here as
# there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# The browser's renderer and the server's give one answer: the same requests
# to each, over the compiler's templates, generated ones of every part and
# context, and every code point; a planned row as a device build makes it,
# over the large list's and its fixture's templates too (`rows.sh`,
# `row-events.sh`); the browser suites that render through it in three
# engines; and the mutation controls.
e14-browser-renderer:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @bash spikes/own-renderer/rows.sh > /dev/null
    @bash spikes/own-renderer/row-events.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server -p pw-render-oracle
    @mkdir -p docs/evidence/E14
    @{ echo "the browser renders with a page of JavaScript, held to the server's renderer"; echo; \
       echo "produced by: just e14-browser-renderer"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the two renderers, asked the same (renderer-agreement.test.mjs)"; echo; \
       node --test --test-reporter=spec spikes/own-renderer/renderer-agreement.test.mjs; \
       echo; echo "== the server's renderer on the address that panicked it (escape.rs)"; echo; \
       cargo test --locked -p pw-render a_wide_character_where 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what a page renders through it, in three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/feed.spec.mjs e2e/press-modules.spec.mjs e2e/warm.spec.mjs e2e/runtime-size.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/browser_renderer_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/browser_renderer_mutations.py; \
     } > docs/evidence/E14/browser-renderer.txt
    @grep -E "^ℹ (pass|fail)|^test result|passed|mutants killed|panicked at" docs/evidence/E14/browser-renderer.txt
