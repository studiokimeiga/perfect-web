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

# ADR-0391: the usage an agent adapter reports is kept or refused and never
# changes a grade; a timeout is the agent's failure, told apart; only
# infrastructure is ungraded. The harness's tests and the mutation controls.
# A failed test stops the recipe and stays in its evidence.
e14-agent-telemetry:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0391 - an agent's usage is measured apart from its grade"; echo; \
       echo "produced by: just e14-agent-telemetry"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)"; echo; \
       echo "== the harness's tests (benchmarks/harness/test/agent-telemetry.test.mjs)"; echo; \
       (cd benchmarks/harness && node --test test/agent-telemetry.test.mjs 2>&1 \
         | grep -E '^(ok|not ok) |^# (tests|pass|fail) '); \
       echo; echo "== mutation controls (scripts/agent_telemetry_mutations.py)"; echo; \
       python3 scripts/agent_telemetry_mutations.py; \
     } > docs/evidence/E14/agent-telemetry.txt
    @grep -E "^(ok|not ok) |mutants killed" docs/evidence/E14/agent-telemetry.txt
