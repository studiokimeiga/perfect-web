# ADR0389: temporary upload leases retain their admitted reader, and each
# preview/publication inspects current authority. This bounded native gate is
# separate from the whole legacy uploads/compiler/browser track.
e14-upload-authority:
    @git diff --quiet HEAD -- compiler runtime packages examples scripts spikes/own-renderer/server just justfile Cargo.toml Cargo.lock rust-toolchain.toml .cargo .github || { echo "upload authority gate requires committed source" >&2; exit 1; }
    @test -z "$(git ls-files --others --exclude-standard -- compiler runtime packages examples scripts spikes/own-renderer/server just justfile Cargo.toml Cargo.lock rust-toolchain.toml .cargo .github)" || { echo "upload authority gate refuses untracked source" >&2; exit 1; }
    @mkdir -p docs/evidence/E14
    @bundle="$(mktemp -d docs/evidence/E14/upload-authority.XXXXXX)"; \
     report="$bundle/console.txt"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; exit "$status"' EXIT; \
     { echo "ADR0389 - private upload lease and preview authority"; \
       echo "produced by: just e14-upload-authority"; \
       echo "commit: $(git rev-parse HEAD)"; \
       echo "rust: $(rustc --version)"; \
       echo "campaign: $bundle/campaign/report.json"; \
       python3 -m unittest -v scripts/tests/test_upload_authority_mutations.py; \
       python3 scripts/upload_authority_mutations.py --expected-head "$(git rev-parse HEAD)" --output "$bundle/campaign"; \
     } > "$report" 2>&1; \
     cp "$report" docs/evidence/E14/upload-authority.txt
    @grep -E 'KILLED|scope=|^Ran |^OK|^commit:' docs/evidence/E14/upload-authority.txt
