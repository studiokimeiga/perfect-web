# The prerender's recipes (a page no reader decides is one document, ADR
# unnumbered until it lands). The root justfile imports this file, and its
# settings and variables (`set shell`, `PATH`) apply here as there. An evidence
# recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned
# by the verification run like any other (scripts/ci_plan.py reads this file
# too).

# A page no reader decides is rendered once and sent to everyone, compressed
# and revalidated, naming no session. Which pages the build marks (the
# compiler's tests); what the host sends for one, over real connections, and
# that everything else is as it was (the host's); TodoMVC through it in three
# engines, a page the back-forward cache brings back among them; and the
# mutation controls.
e14-prerender:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/todomvc.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "a page no reader decides is one document"; echo; \
       echo "produced by: just e14-prerender"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== which pages the build marks (compiler/pw-core/tests/prerender.rs)"; echo; \
       cargo test --locked -p pw-core --test prerender 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== what the host sends for one (tests::prerender::, TodoMVC's)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::prerender:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== TodoMVC through it in three engines (e2e/todomvc.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/todomvc.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/prerender_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/prerender_mutations.py; \
     } > docs/evidence/E14/prerender.txt
    @grep -E "^test result|passed|mutants killed|panicked at" docs/evidence/E14/prerender.txt
