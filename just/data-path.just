# Track `data-path` (ADR-0338's D2: an app with no Rust data layer): the
# development server serves a program's declared tables through the table
# gate alone, a command's rows published with its records or not at all; a
# guarded command answered before its tables are linked; one render reading
# one state of the tables; the guide's guestbook on the tables it declares,
# the tutorial byte for byte the app; the conformance suite over the
# `DataLayer` seam, generated for every declared table of the guestbook and
# the lists app; then every mutation control, whole. (The guestbook in three
# engines is `just e14-guide`'s.)
e14-data-path:
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.data-path.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D2 - an app with no Rust data layer: declared tables served through the gate, the guestbook on them, the conformance suite"; echo; \
       echo "produced by: just e14-data-path"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== the server: routing, the guestbook, the conformance suite, the reader's account"; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::tables:: tests::guestbook:: tests::conformance:: notifications::tests::the_account_operation --nocapture; \
       echo; echo "== the engine: a guarded command through the gate; the adapter's snapshot"; \
       cargo test --locked -p pw-table-memory --test compiled_guarded --test compiled_snapshot; \
       echo; echo "== the platform's reader: their account, labelled the reader's"; \
       cargo test --locked -p pw-core --test handles --test platform_contracts; \
       echo; echo "== the tutorial is the app (compiler/pw-cli/tests/guide.rs)"; \
       cargo test --locked -p pw-cli --test guide; \
       echo; echo "== mutation controls (scripts/data_path_mutations.py)"; \
       python3 scripts/data_path_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/data-path.txt
    @grep -E 'test result|mutants killed|^baseline:|^conformance:' docs/evidence/E14/data-path.txt
