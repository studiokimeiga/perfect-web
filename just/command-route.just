# Recipes for the command route: only a command is a request's to run by its
# id (train-14's review, 2026-11-07). The root justfile imports this file, and
# its settings and variables (`set shell`, `PATH`) apply here as there. An
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and
# is planned by the verification run like any other (scripts/ci_plan.py reads
# this file too).

# `POST /command/<id>` runs an export of kind `command` alone: a query, a
# page, a derived value or a function the build compiled answers 404, as an
# `invoked_by` command does. Its test, and the mutation controls.
e14-command-route:
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "The command route - only a command is a request's to run"; echo; \
       echo "produced by: just e14-command-route"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== every hosted export that is no command (tests::a_query_posted_as_a_command_is_not_found)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- --exact tests::a_query_posted_as_a_command_is_not_found 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/command_route_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/command_route_mutations.py; \
     } > docs/evidence/E14/command-route.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/command-route.txt
