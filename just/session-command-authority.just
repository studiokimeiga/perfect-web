# ADR0389: immutable browser command/receipt authority. No database skips,
# frozen native inventories, assertion attribution and owned-schema cleanup.
e14-session-command-authority:
    @test -n "${PW_FEED_DATABASE_URL:-}" && test -n "${PW_STORE_DATABASE_URL:-}" || { echo "session command gate requires both PostgreSQL URLs" >&2; exit 1; }
    @git diff --quiet HEAD -- compiler runtime packages examples scripts spikes/own-renderer/server just justfile Cargo.toml Cargo.lock rust-toolchain.toml || { echo "session command gate requires committed source" >&2; exit 1; }
    @test -z "$(git ls-files --others --exclude-standard -- compiler runtime packages examples scripts spikes/own-renderer/server just justfile Cargo.toml Cargo.lock rust-toolchain.toml)" || { echo "session command gate refuses untracked source" >&2; exit 1; }
    @mkdir -p docs/evidence/E14
    @bundle="$(mktemp -d docs/evidence/E14/session-commands.XXXXXX)"; \
     report="$bundle/console.txt"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; exit "$status"' EXIT; \
     { echo "ADR0389 - browser command, receipt and acknowledgement authority"; \
       echo "produced by: just e14-session-command-authority"; \
       echo "commit: $(git rev-parse HEAD)"; \
       echo "rust: $(rustc --version)"; \
       echo "campaign: $bundle/campaign/report.json"; \
       python3 -m unittest -v scripts/tests/test_session_command_mutations.py; \
       python3 scripts/session_command_mutations.py --suite all --expected-head "$(git rev-parse HEAD)" --output "$bundle/campaign"; \
     } > "$report" 2>&1; \
     cp "$report" docs/evidence/E14/session-command-authority.txt
    @grep -E 'KILLED|scope=|^Ran |^OK|^commit:' docs/evidence/E14/session-command-authority.txt
