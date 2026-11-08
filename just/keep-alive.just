# Track `keep-alive`'s recipes (ADR-0401). The root justfile imports
# this file, and its settings and variables (`set shell`, `PATH`) apply here
# as there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# ADR-0401: a host keeps a connection for its next request where
# HTTP/1.1 keeps it. It frames each request's body strictly, so that no
# request is read out of another's body; closes a connection whose request
# asks to close, whose body it has not read whole, past its cap, or after an
# answer not written whole, or none; waits at most its idle time for a next
# request; and tells each commit to its readers after its answer. Its tests
# over real connections (`tests::keep_alive::`, and the upload's), and the
# mutation controls.
e14-keep-alive:
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0401 - a host keeps a connection for its next request"; echo; \
       echo "produced by: just e14-keep-alive"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== a connection kept, its requests framed, and closed (tests::keep_alive::, and the upload's)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::keep_alive:: tests::uploads::an_uploads_connection_is_closed_after_its_answer 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/keep_alive_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/keep_alive_mutations.py; \
     } > docs/evidence/E14/keep-alive.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/keep-alive.txt
