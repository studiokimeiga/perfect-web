# E14-E's footprint: a run's changed lines count the files an agent makes,
# and nothing the sandbox ignores; changes git cannot list are refused. The
# harness's own tests and the mutation controls. A failed test stops the
# recipe and stays in its evidence.
e14-bench-footprint:
    @mkdir -p docs/evidence/E14
    @{ echo "E14-E - a run's changed lines count the files an agent makes"; echo; \
       echo "produced by: just e14-bench-footprint"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)"; echo; \
       echo "== the harness's tests (benchmarks/harness/test)"; echo; \
       (cd benchmarks/harness && node --test test/changed-lines.test.mjs test/isolation.test.mjs 2>&1 \
         | grep -E '^(ok|not ok) |^# (tests|pass|fail) '); \
       echo; echo "== mutation controls (scripts/bench_footprint_mutations.py)"; echo; \
       python3 scripts/bench_footprint_mutations.py; \
     } > docs/evidence/E14/bench-footprint.txt
    @grep -E "^(ok|not ok) |mutants killed" docs/evidence/E14/bench-footprint.txt

# A real no-network agent adapter reports usage without affecting grading;
# infrastructure exits do not become model failure scores (ADR-0391).
e14-agent-telemetry:
    @node --test benchmarks/harness/test/agent-telemetry.test.mjs
