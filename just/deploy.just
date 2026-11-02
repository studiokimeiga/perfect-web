# Track deploy's recipes (ADR-XXXX, a production host): a Pleris program
# built, packaged and run as production runs it. The root justfile imports
# this file; an evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other.
#
# Nothing here reaches another machine without the owner's step
# (DEPLOY-OWNER-STEP.md in the handoff folder): `deploy-vm` reads
# ~/.config/pleris/deploy.env, which only that step makes.

# A program's production build (store, feed or kiokun): `pw build`, the
# browser's WebAssembly and the runtime minified, in target/deploy/<program>/dist.
deploy-dist program="store":
    deploy/build-dist.sh {{program}} target/deploy/{{program}}/dist

# A program's OCI image (deploy/Containerfile), with Podman or Docker,
# whichever is installed; the development PC has neither.
deploy-image program="store":
    #!/usr/bin/env bash
    set -euo pipefail
    engine="$(command -v podman || command -v docker || true)"
    if [ -z "$engine" ]; then
      echo "deploy-image: neither podman nor docker is installed here"
      exit 1
    fi
    commit="$(git rev-parse HEAD)"
    "$engine" build -f deploy/Containerfile --build-arg PROGRAM={{program}} \
      --build-arg COMMIT="$commit" -t "pleris-{{program}}:${commit:0:12}" .

# The local production smoke test alone: the release host, the store's
# production build, a database of its own, a TLS front and three browser
# engines, before and after a restart. PORT (14241 here) is the front's.
deploy-smoke:
    bash deploy/smoke.sh

# One release to the owner's VM: `--setup` the first time, `--data` to copy
# kiokun's dictionary. Refused without the owner's ~/.config/pleris/deploy.env.
deploy-vm program="kiokun" *flags:
    deploy/vm/release.sh {{program}} {{flags}}

# ADR-XXXX: a production host. The server's production tests (its refusals at
# start, its keys, its own answers, its assets, its secrets, its log, its
# stop, no sign-in), the binary as production runs it (tests/production_process.rs,
# the store on a database of its own), the image's minifier locked from
# pnpm-lock.yaml, the local production smoke test (deploy/smoke.sh: playwright
# in Chromium, Firefox and WebKit through a TLS front, before and after a
# SIGTERM restart), and the mutation controls (scripts/deploy_mutations.py).
e14-deploy:
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-XXXX - a production host"; echo; \
       echo "produced by: just e14-deploy"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "node: $(node --version); esbuild: $(spikes/own-renderer/node_modules/.bin/esbuild --version)"; \
       echo "postgresql: $(psql "$PW_STORE_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null)"; echo; \
       echo "== the production host's tests (production::, tests::production_host::, kiokun's cache)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- production 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host as a process (tests/production_process.rs)"; echo; \
       cargo test --locked -p pw-dev-server --test production_process 2>&1 | grep -E '^(test |test result)|panicked at|skipped'; \
       echo; echo "== the image's minifier, locked from pnpm-lock.yaml"; echo; \
       python3 scripts/deploy_minify_lock.py --check; \
       echo; echo "== the local production smoke (deploy/smoke.sh: playwright, three engines)"; echo; \
       PORT="${PORT:-14241}" bash deploy/smoke.sh; \
       echo; echo "== mutation controls (scripts/deploy_mutations.py)"; echo; \
       python3 scripts/deploy_mutations.py; \
     } > docs/evidence/E14/deploy.txt
    @grep -E "^test result|mutants killed|panicked at|deploy smoke:|FAIL" docs/evidence/E14/deploy.txt
