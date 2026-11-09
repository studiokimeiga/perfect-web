# The device runtime's recipes (a page that talks to no server starts a runtime
# with no server's code in it, ADR unnumbered until it lands). The root justfile
# imports this file, and its settings and variables (`set shell`, `PATH`) apply
# here as there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# A page no reader decides and with no layout starts a runtime of its own: its
# decision held to `pw-resume`'s own build over changed manifests; what the
# packaging writes; what the host starts; TodoMVC's whole suite and the page's
# requests on a production build, in three engines; and the mutation controls.
e14-device-runtime:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @PW_PRODUCTION_ASSETS=1 bash spikes/own-renderer/todomvc.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @cargo build --quiet --locked -p pw-resume-wasm --target wasm32-unknown-unknown --profile browser
    @mkdir -p docs/evidence/E14
    @{ echo "a page that talks to no server starts a runtime with no server's code in it"; echo; \
       echo "produced by: just e14-device-runtime"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the page's decision, asked as pw-resume is asked (decision-agreement.test.mjs)"; echo; \
       node --test --test-reporter=spec spikes/own-renderer/decision-agreement.test.mjs; \
       echo; echo "== what the packaging writes (optimize-assets.test.mjs)"; echo; \
       node --test --test-reporter=spec spikes/own-renderer/optimize-assets.test.mjs; \
       echo; echo "== what the host starts (tests::page_bundle::)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server tests::page_bundle:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a production build, in three engines (e2e/todomvc.spec.mjs, e2e/device-runtime.spec.mjs)"; echo; \
       PW_PRODUCTION_ASSETS=1 bash scripts/run_suite.sh -- pnpm exec playwright test e2e/todomvc.spec.mjs e2e/device-runtime.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/device_runtime_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/device_runtime_mutations.py; \
     } > docs/evidence/E14/device-runtime.txt
    @grep -E "^ℹ (pass|fail)|^test result|passed|mutants killed|panicked at" docs/evidence/E14/device-runtime.txt
