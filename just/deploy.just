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

# ADR-XXXX: a production host. Everything its ADR claims, each by the command
# that holds it: the server's production tests (its refusals at start, its
# keys, its own answers and what it serves, its connections' bounds, its
# assets, its secrets, its log, its stop, no sign-in) and an upload under a
# path the platform keeps; the compiler's refusal of a program route the
# platform keeps (PW7001, its code block); a domain that never says its key;
# the binary as production runs it (tests/production_process.rs, the store on
# a database of its own); the unit as systemd reads it and the scripts as
# shellcheck does, where each is installed; what release.sh hands a machine
# (deploy/vm/release-checks.sh); the image's minifier locked from
# pnpm-lock.yaml; the local production smoke test (deploy/smoke.sh:
# playwright in Chromium, Firefox and WebKit through a TLS front, before and
# after a SIGTERM restart); kiokun's release host's peak memory against
# pleris.service's share of a 2 GiB VM (deploy/host-memory.sh); and the
# mutation controls
# (scripts/deploy_mutations.py). The process tests and the campaign start
# their hosts under a directory of this run's own, four tests at a time (a
# host takes up to a gigabyte); after each, and whenever the run ends early,
# deploy/hosts-left.sh says and kills any host still running, and one found
# fails the recipe.
e14-deploy:
    @mkdir -p docs/evidence/E14
    @hosts="$(mktemp -d "${TMPDIR:-/tmp}/pw-deploy-hosts.XXXXXX")"; \
     trap 'status=$?; if [ "$status" != 0 ]; then bash deploy/hosts-left.sh "$hosts" >> docs/evidence/E14/deploy.txt || true; fi; rm -rf "$hosts"; exit "$status"' EXIT; \
     { echo "ADR-XXXX - a production host"; echo; \
       echo "produced by: just e14-deploy"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "node: $(node --version); esbuild: $(spikes/own-renderer/node_modules/.bin/esbuild --version)"; \
       echo "postgresql: $(psql "$PW_STORE_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null)"; echo; \
       echo "== the production host's tests (production::, tests::production_host::, kiokun's cache)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- production 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== an upload under a path the platform keeps, its build made by hand (tests::session_upload_reserved_routes::)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::session_upload_reserved_routes:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the compiler refuses a program route the platform keeps (PW7001, the deploy block)"; echo; \
       cargo test --locked -p pw-core --test reserved_routes 2>&1 | grep -E '^(test |test result)|panicked at'; \
       cargo test --locked -p pw-core --lib -- reserved:: codes::tests:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a domain never says its key (pw-document)"; echo; \
       cargo test --locked -p pw-document -- domain::tests:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host as a process (tests/production_process.rs, four at a time)"; echo; \
       TMPDIR="$hosts" cargo test --locked -p pw-dev-server --test production_process -- --test-threads=4 2>&1 | grep -E '^(test |test result)|panicked at|skipped'; \
       echo; echo "== no host the process tests started outlives their run (deploy/hosts-left.sh, which holds itself first)"; echo; \
       bash deploy/hosts-left.sh --self-check; \
       bash deploy/hosts-left.sh "$hosts"; \
       echo; echo "== the unit, as systemd reads it (systemd-analyze verify)"; echo; \
       if command -v systemd-analyze >/dev/null; then \
         unit_dir="$(mktemp -d)"; cp deploy/vm/pleris.service "$unit_dir/"; \
         echo "systemd-analyze: $(systemd-analyze --version | head -1)"; \
         systemd-analyze verify "$unit_dir/pleris.service" 2>&1 | grep 'pleris.service' || echo "pleris.service: no finding"; \
         rm -rf "$unit_dir"; \
       else echo "systemd-analyze: not installed here"; fi; \
       echo; echo "== the deploy scripts, as shellcheck reads them"; echo; \
       if command -v shellcheck >/dev/null; then \
         echo "shellcheck: $(shellcheck --version | sed -n 's/^version: //p')"; \
         if shellcheck -S warning deploy/smoke.sh deploy/build-dist.sh deploy/hosts-left.sh deploy/host-memory.sh deploy/vm/release.sh deploy/vm/setup.sh deploy/vm/release-checks.sh; \
         then echo "shellcheck: no warning"; else echo "shellcheck: the warnings above"; fi; \
       else echo "shellcheck: not installed here"; fi; \
       echo; echo "== what release.sh hands a machine, and what it refuses to (deploy/vm/release-checks.sh)"; echo; \
       bash deploy/vm/release-checks.sh; \
       echo; echo "== the image's minifier, locked from pnpm-lock.yaml"; echo; \
       python3 scripts/deploy_minify_lock.py --check; \
       echo; echo "== the local production smoke (deploy/smoke.sh: playwright, three engines)"; echo; \
       PORT="${PORT:-14241}" bash deploy/smoke.sh; \
       echo; echo "== what a production host's memory comes to, against pleris.service's share of a 2 GiB VM (deploy/host-memory.sh: kiokun, the release build)"; echo; \
       bash deploy/host-memory.sh; \
       echo; echo "== mutation controls (scripts/deploy_mutations.py)"; echo; \
       TMPDIR="$hosts" python3 scripts/deploy_mutations.py; \
       echo; echo "== no host the campaign's runs started outlives them (deploy/hosts-left.sh)"; echo; \
       bash deploy/hosts-left.sh "$hosts"; \
     } > docs/evidence/E14/deploy.txt
    @grep -E "^test result|mutants killed|panicked at|deploy smoke:|release checks:|shellcheck:|systemd-analyze:|hosts left:|host memory:|FAIL" docs/evidence/E14/deploy.txt
