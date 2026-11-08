# ADR-0400: the browser's decision carries none of its trace's
# words, and a refused handler is told in the console by its code and the
# page that explains it (`pleris.dev/e/PWR04`). The decision's tests with its
# trace and without, the two browser tests in Chromium, and the mutation
# controls. A failed suite stops the recipe and stays in its evidence.
e14-refusal-codes:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0400 - a refusal has a code and a page, and the browser's decision carries no words"; echo; \
       echo "produced by: just e14-refusal-codes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the decision with its trace (runtime/pw-resume)"; echo; \
       cargo test --locked -p pw-resume 2>&1 | grep -E '^(test result)|panicked at'; \
       echo; echo "== the decision without it, as the browser's build has it (runtime/pw-resume --no-default-features, runtime/pw-resume-wasm)"; echo; \
       cargo test --locked -p pw-resume --no-default-features 2>&1 | grep -E '^(test result)|panicked at'; \
       cargo test --locked -p pw-resume-wasm 2>&1 | grep -E '^(test result)|panicked at'; \
       echo; echo "== the decision as a page downloads it, and a refusal as a developer reads it (Chromium)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/runtime-size.spec.mjs e2e/lazy-handler.spec.mjs --project=chromium --reporter=line; \
       echo; echo "== mutation controls (scripts/refusal_codes_mutations.py)"; echo; \
       python3 scripts/refusal_codes_mutations.py; \
     } > docs/evidence/E14/refusal-codes.txt
    @grep -E "test result|passed|failed|mutants killed" docs/evidence/E14/refusal-codes.txt
