# Recipes for pleris.dev's measured claims (ADR-0408): the site shows the
# head-to-head's record as the instrument wrote and judged it. The root
# justfile imports this file, and its settings and variables (`set shell`,
# `PATH`) apply here as there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# The site's tests, the whole site built from the committed record among
# them, and the mutation controls on what it shows from the record.
e14-site:
    @mkdir -p docs/evidence/E14
    @{ echo "pleris.dev - its measured rows are the head-to-head's record"; echo; \
       echo "produced by: just e14-site"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "node: $(node --version)"; echo; \
       echo "== the site's tests (site/build.test.mjs)"; echo; \
       node --test site/build.test.mjs > "${TMPDIR:-/tmp}/e14-site-tests.txt" 2>&1 \
         || echo "FAIL: the site's tests"; \
       grep -E '^(# (tests|pass|fail) |not ok )' "${TMPDIR:-/tmp}/e14-site-tests.txt"; \
       echo; echo "== mutation controls (scripts/site_mutations.py)"; echo; \
       python3 scripts/site_mutations.py || echo "FAIL: the mutation controls"; \
     } > docs/evidence/E14/site.txt
    @grep -E "^# (pass|fail) |mutants killed|^FAIL|^not ok" docs/evidence/E14/site.txt
    @! grep -qE "^FAIL|^not ok" docs/evidence/E14/site.txt
