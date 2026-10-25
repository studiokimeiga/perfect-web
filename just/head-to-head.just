# The head-to-head track's recipes (ADR-XXXX, "the head-to-head"): the same
# apps in Pleris and in the mainstream stacks, measured by one instrument,
# Pleris's losses first. The root justfile imports this file; an evidence
# recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is
# planned by the verification run like any other.

# ADR-XXXX: the head-to-head. TodoMVC in Pleris against TodoMVC's own React
# and Svelte apps (vendored at tastejs/todomvc@ff43b02e,
# benchmarks/headtohead/todomvc/UPSTREAM.md), and ADR-0120's store in
# Pleris, Next.js and SvelteKit. In order:
#
# - each stack built as production: the Pleris pages and its host's release
#   build; the frameworks' stores by `next build` and `vite build`, after the
#   feature changes' checker runs, which patch their baselines in place;
# - the same feature change in each stack: the twelve tasks' reference and
#   unsafe patches, each stack's own checker on each (feature-change.mjs);
# - TodoMVC's behaviour on every stack: its official Cypress suite (the
#   compatibility copy Pleris is held to; spikes/todomvc-cypress/run.sh for
#   Pleris, todomvc/cypress.sh for React and Svelte) and its specification in
#   three engines (todomvc/behaviour.sh), each judged against written
#   expectations, the gaps reported;
# - the source each stack's developer writes (scripts/head_to_head_loc.py)
#   and the museum's classes each lets ship (museum-classes.mjs, from
#   e14-museum's record);
# - the measurement (head-to-head.mjs, with Playwright in Chromium, Firefox
#   and WebKit): bytes, first paint, interactive, each interaction, cold and
#   warm, Chromium also shaped, seven runs a cell; each server under a fixed
#   load; twenty idle pages; every rule of validate.mjs, a broken one
#   failing the recipe; the report, where Pleris loses first;
# - the mutation and integrity controls (scripts/head_to_head_mutations.py).
#
# Ports: PORT (13441 for this track) + 220..249. Run alone: a loaded machine
# measures the load (the machine and its load average are printed).
#
# The same apps in Pleris and the mainstream stacks, measured, Pleris's losses first.
e14-head-to-head:
    @cargo build --quiet --locked -p pw-cli -p pw-dev-server
    @cargo build --quiet --locked --release -p pw-dev-server
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/todomvc.sh > /dev/null
    @bash spikes/own-renderer/baseline-store.sh > /dev/null
    @cd spikes/todomvc-cypress && pnpm exec cypress install > /dev/null
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-XXXX - the head-to-head: the same apps in Pleris and in the mainstream stacks, Pleris's losses first"; echo; \
       echo "produced by: just e14-head-to-head"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)  pnpm: $(pnpm --version)"; \
       echo "playwright: $(cd benchmarks/headtohead && pnpm exec playwright --version)"; \
       echo "cypress: $(cd spikes/todomvc-cypress && pnpm exec cypress --version | tr '\n' ' ')"; \
       for p in next react svelte @sveltejs/kit vite typescript esbuild; do \
         for d in benchmarks/baselines/next-react benchmarks/baselines/sveltekit benchmarks/headtohead; do \
           f="$d/node_modules/$p/package.json"; [ -f "$f" ] && printf '  %-18s %s  (%s)\n' "$p" "$(node -p "require('./$f').version")" "$d"; \
         done; done | sort -u; \
       S="$(mktemp -d /tmp/h2h-sections.XXXXXX)"; echo "sections and the raw runs kept in $S"; echo; \
       node benchmarks/headtohead/feature-change.mjs --json "$S/feature.json" > "$S/feature.txt"; \
       (cd benchmarks/baselines/next-react && pnpm --silent build > /dev/null); \
       (cd benchmarks/baselines/sveltekit && pnpm --silent build > /dev/null 2>&1); \
       { bash spikes/todomvc-cypress/run.sh > "$S/cypress-pleris.txt" 2>&1 && \
         bash benchmarks/headtohead/todomvc/cypress.sh react > "$S/cypress-react.txt" 2>&1 && \
         bash benchmarks/headtohead/todomvc/cypress.sh svelte > "$S/cypress-svelte.txt" 2>&1; } \
         || { cat "$S"/cypress-*.txt; echo "FAIL: a Cypress run did not come out as expected"; exit 1; }; \
       SUMMARY="$S/behaviour.json" CYPRESS="pleris=$S/cypress-pleris.txt react=$S/cypress-react.txt svelte=$S/cypress-svelte.txt" \
         bash benchmarks/headtohead/todomvc/behaviour.sh > "$S/behaviour-spec.txt" 2>&1 \
         || { cat "$S/behaviour-spec.txt"; echo "FAIL: the specification's checks did not come out as expected"; exit 1; }; \
       { for s in pleris react svelte; do echo "-- TodoMVC's Cypress suite on $s"; grep -vE '^\s*$' "$S/cypress-$s.txt" | grep -E 'passed|FAIL|GAP|pending|PROBLEM|acceptance|as expected|cypress:' | head -12; echo; done; \
         echo "-- TodoMVC's specification in three engines (behaviour.spec.mjs)"; cat "$S/behaviour-spec.txt"; } > "$S/behaviour.txt"; \
       python3 scripts/head_to_head_loc.py > "$S/loc.txt"; \
       python3 scripts/head_to_head_loc.py --json > "$S/loc.json"; \
       node benchmarks/headtohead/museum-classes.mjs --json "$S/museum.json" > "$S/museum.txt"; \
       PORT="${PORT:-3141}" node benchmarks/headtohead/head-to-head.mjs --sections "$S" --json "$S/runs.json"; \
       echo; echo "== mutation and integrity controls (scripts/head_to_head_mutations.py)"; echo; \
       PORT="${PORT:-3141}" python3 scripts/head_to_head_mutations.py; \
     } > docs/evidence/E14/head-to-head.txt
    @grep -E "^== where|LOSS|REFUSED|every rule held|mutants killed|^FAIL" docs/evidence/E14/head-to-head.txt
