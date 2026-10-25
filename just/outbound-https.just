# Track outbound-https's recipes (ADR-0387): the host's one outbound HTTPS
# client. The root justfile imports this file; an evidence recipe is
# `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is planned by
# the verification run like any other.

# ADR-0387: the host's one outbound client, reaching only the origins a
# service's grant declares, and the payments reaching their provider through
# it. The client's tests (runtime/pw-outbound: the grant, the addresses, the
# certificates against an authority made for the run, redirects, the
# deadline, the bounds, the framing, the secret), the payments' (payments::,
# the store's among them, in memory; an https provider behind a TLS front),
# and the mutation controls (scripts/outbound_https_mutations.py).
e14-outbound-https:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0387 - the host's one outbound HTTPS client"; echo; \
       echo "produced by: just e14-outbound-https"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "tls: $(cargo tree --locked -p pw-outbound -e normal --prefix none --no-dedupe 2>/dev/null | grep -E '^(rustls|ring|rustls-webpki|rustls-native-certs) v' | sort -u | tr '\n' ' ')"; echo; \
       echo "== the client (runtime/pw-outbound)"; echo; \
       cargo test --locked -p pw-outbound --lib 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the payments through it (spikes/own-renderer/server/src/payments.rs, tests/store_payments.rs, in memory)"; echo; \
       PW_STORE_TEST_LAYER=memory cargo test --locked -p pw-dev-server --bin pw-dev-server -- payments:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/outbound_https_mutations.py)"; echo; \
       python3 scripts/outbound_https_mutations.py; \
     } > docs/evidence/E14/outbound-https.txt
    @grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/outbound-https.txt
