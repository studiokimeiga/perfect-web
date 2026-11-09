# The decision in JavaScript's recipes (every page decides in its runtime, ADR
# unnumbered until it lands). The root justfile imports this file, and its
# settings and variables (`set shell`, `PATH`) apply here as there. An evidence
# recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned
# by the verification run like any other (scripts/ci_plan.py reads this file
# too).

# A page decides in a few lines of its runtime, held to `pw-resume`: the two
# asked the same over changed manifests; a document's head and policy asking
# for no WebAssembly; the pages that decide, in three engines; and the mutation
# controls.
e14-decision-in-js:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @cargo build --quiet --locked -p pw-resume-wasm --target wasm32-unknown-unknown --profile browser
    @mkdir -p docs/evidence/E14
    @{ echo "every page decides in its runtime, held to pw-resume"; echo; \
       echo "produced by: just e14-decision-in-js"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the two, asked the same (decision-agreement.test.mjs)"; echo; \
       node --test --test-reporter=spec spikes/own-renderer/decision-agreement.test.mjs; \
       echo; echo "== the crate that specifies it (runtime/pw-resume-wasm)"; echo; \
       cargo test --locked -p pw-resume-wasm 2>&1 | grep -E '^(test result)|panicked at'; \
       echo; echo "== what a document asks for and allows (the host's tests)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server a_document_preloads_what_its_runtime_boots_with 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server tests::page_policy:: 2>&1 | grep -E '^(test result)|panicked at'; \
       echo; echo "== the pages that decide, in three engines"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/lazy-handler.spec.mjs e2e/recovery.spec.mjs e2e/page-policy.spec.mjs e2e/runtime-size.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/decision_in_js_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/decision_in_js_mutations.py; \
     } > docs/evidence/E14/decision-in-js.txt
    @grep -E "^ℹ (pass|fail)|^test result|passed|mutants killed|panicked at" docs/evidence/E14/decision-in-js.txt
