#!/usr/bin/env python3
"""Mutation controls for track `store-search` (ADR-0308, docs/PARALLEL.md,
W9): the stores and their dishes that match a reader's text, matched in the
store's data layer to one meaning in memory and on PostgreSQL.

Each mutant undoes one piece:
- the terms (`search.rs`): the bound counted inclusively, or in bytes; a
  blank text searched; a term not lowered; Unicode 17.0's new capitals
  lowered where PostgreSQL 18 does not;
- the meaning in memory: one term enough where every term must match; a
  name not its own tier; a store's cuisine not read; a store listed only
  where it matched itself;
- the order and the cut: a store matched only by its dishes ranked with the
  stores that matched; stores by id before name; dishes left in their
  menu's order; every dish listed; the stores past twenty not counted;
- the operation: a declared case misnamed; the operation not registered
  where the layers' catalogue is (the seam in `store.rs`); a menu change's
  route telling no session whose open page reads a search (the seam in
  `main.rs`);
- the program (`examples/store/search.pw`): the search box holding nothing
  of what was asked; the title naming no search; a long text told as a blank
  one; a store's dishes not shown; a search kept past a menu's change;
- PostgreSQL: `%`, `_` or `\\` not escaped in a term's pattern; the layer's
  own statements not run, each store's menu read and matched on the host
  (the seam in `store_pg.rs`); a dish's name lowered under the `C`
  collation; a store's cuisine, or a dish's category, not
  read for its tier; a store holding a matched dish not listed; the
  migration's cuisine not the in-memory layer's.

A mutant must fail the suite it is listed with: the development server's
search tests in memory (`server`), or on PostgreSQL (`postgres`, which needs
PW_STORE_DATABASE_URL), each narrowed to the tests that can see its rule
(measured by scripts/mutant_killers.py, below).

Run from the repository root; `just e14-store-search` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SEARCH = ROOT / "spikes/own-renderer/server/src/search.rs"
STORE_DATA = ROOT / "spikes/own-renderer/server/src/store.rs"
STORE_PG = ROOT / "spikes/own-renderer/server/src/store_pg.rs"
SERVER_RS = ROOT / "spikes/own-renderer/server/src/main.rs"
MIGRATION = ROOT / "spikes/own-renderer/server/migrations/store/0010_search.sql"
PROGRAM = ROOT / "examples/store/search.pw"
APP = ROOT / "examples/store/app.pw"
CUISINE_MIGRATION = ROOT / "spikes/own-renderer/server/migrations/store/0011_store_cuisine.sql"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    # --- the terms ------------------------------------------------------------
    (
        "a text of 100 characters is too long",
        "server",
        SEARCH,
        "    if text.chars().count() > MOST_CHARACTERS {\n",
        "    if text.chars().count() >= MOST_CHARACTERS {\n",
    ),
    (
        "the bound counts bytes",
        "server",
        SEARCH,
        "    if text.chars().count() > MOST_CHARACTERS {\n",
        "    if text.len() > MOST_CHARACTERS {\n",
    ),
    (
        "a blank text is searched",
        "server",
        SEARCH,
        "    if terms.is_empty() {\n",
        "    if terms.is_empty() && false {\n",
    ),
    (
        "a character is not lowered",
        "server",
        SEARCH,
        "            c.to_lowercase().next().unwrap_or(c)\n        })\n",
        "            c\n        })\n",
    ),
    (
        "Unicode 17.0's new capitals are lowered",
        "postgres",
        SEARCH,
        "                .any(|r| r.contains(&u32::from(c)))\n",
        "                .any(|r| r.contains(&u32::from(c)) && false)\n",
    ),
    # --- the meaning in memory --------------------------------------------------
    (
        "one term is enough",
        "server",
        SEARCH,
        "        .find(|&n| terms.iter().all(|t| in_first(t, n)))\n",
        "        .find(|&n| terms.iter().any(|t| in_first(t, n)))\n",
    ),
    (
        "a name is not a tier of its own",
        "server",
        SEARCH,
        "    (1..=3)\n",
        "    (2..=3)\n",
    ),
    (
        "a store's cuisine is not read in memory",
        "server",
        SEARCH,
        # Re-anchored by ADR-0308's amendment: the cuisine is the store's.
        "                cuisine: s.cuisine,\n",
        "                cuisine: String::new(),\n",
    ),
    (
        "a store is listed only where it matched itself",
        "server",
        SEARCH,
        "        if tier_of_store.is_some() || !dishes.is_empty() {\n",
        "        if tier_of_store.is_some() {\n",
    ),
    (
        "a dish's description before its category",
        "server",
        SEARCH,
        "                        &lower(&i.category_name),\n                        &lower(&i.description),\n",
        "                        &lower(&i.description),\n                        &lower(&i.category_name),\n",
    ),
    # --- the order and the cut ----------------------------------------------------
    (
        "a store matched by its dishes alone ranks with the rest",
        "server",
        SEARCH,
        "        None => 3 + m.dishes.iter().map(|d| d.tier).min().unwrap_or(2),\n",
        "        None => m.dishes.iter().map(|d| d.tier).min().unwrap_or(2),\n",
    ),
    (
        "stores by id before name",
        "server",
        SEARCH,
        "    matched.sort_by(|a, b| (rank(a), &a.name, &a.id).cmp(&(rank(b), &b.name, &b.id)));\n",
        "    matched.sort_by(|a, b| (rank(a), &a.id, &a.name).cmp(&(rank(b), &b.id, &b.name)));\n",
    ),
    (
        "dishes stay in their menu's order",
        "server",
        SEARCH,
        "            .sort_by(|a, b| (a.tier, &a.name, &a.id).cmp(&(b.tier, &b.name, &b.id)));\n",
        "            .sort_by(|a, b| a.tier.cmp(&a.tier).then(b.tier.cmp(&b.tier)));\n",
    ),
    (
        "every dish is listed",
        "server",
        SEARCH,
        "                .take(MOST_DISHES)\n",
        "                .take(usize::MAX)\n",
    ),
    (
        "the stores past twenty are not counted",
        "server",
        SEARCH,
        "    let more_stores = ordered.len().saturating_sub(MOST_STORES);\n",
        "    let more_stores = 0;\n",
    ),
    # --- the operation ------------------------------------------------------------
    (
        "a blank text's case is misnamed",
        "server",
        SEARCH,
        '        Asked::Blank => Ok(case("blank")),\n',
        '        Asked::Blank => Ok(case("empty")),\n',
    ),
    # --- the store's cuisine, and a reader's search (ADR-0308, amended) -------
    (
        "a store's cuisine is not given to the program",
        "server",
        STORE_DATA,
        '        ("cuisine".into(), Val::String(row.cuisine.clone())),\n',
        "",
    ),
    (
        "a reader's search lists a store that does not reach them",
        "server",
        SEARCH,
        "                if reach.delivers {\n",
        "                if true {\n",
    ),
    (
        "a store left out is not counted",
        "server",
        SEARCH,
        "                    near.beyond += 1;\n",
        "",
    ),
    (
        "a reader's search does not say an address is chosen",
        "server",
        SEARCH,
        "                near.chosen = true;\n",
        "",
    ),
    (
        "the page searches every store, whatever the reader's address",
        "server",
        PROGRAM,
        "    let answer = find_for(text, reader)\n",
        "    let answer = find(text)\n",
    ),
    (
        "the home page has no search box",
        "server",
        APP,
        '            <form id="home-search" method="get" action="/search" role="search">'
        '<label>Stores and dishes <input type="search" name="q" /></label>'
        '<button>Search</button></form>\n',
        "",
    ),
    (
        "the migration does not copy the cuisine into the store's row",
        "postgres",
        CUISINE_MIGRATION,
        "UPDATE stores SET cuisine = c.cuisine FROM store_cuisines c WHERE c.store = stores.id;\n",
        "",
    ),
    (
        "the operation is not registered with the catalogue",
        "server",
        STORE_DATA,
        "    ops.extend(crate::search::ops(with.clone())); // TRACK SEAM (store-search)\n",
        "\n",
    ),
    (
        "a menu change's route tells no page reading a search",
        "server",
        SERVER_RS,
        "                    String::new(),\n                    reached\n",
        "                    String::new(),\n                    Vec::<Reached>::new()\n",
    ),
    # --- the program ----------------------------------------------------------------
    (
        "a long text is told as a blank one",
        "server",
        PROGRAM,
        'said: "That search is too long: 100 characters at most."',
        'said: "Type a store, a dish or a cuisine to search for."',
    ),
    (
        "a store's dishes are not shown",
        "server",
        PROGRAM,
        "        dishes: s.dishes,\n",
        "        dishes: [],\n",
    ),
    (
        "a search is kept past a menu's change",
        "server",
        PROGRAM,
        # Re-anchored by ADR-0308's amendment: a reader's search.
        # Re-anchored by track store-hours: a store's hours change it too.
        # Re-anchored by track reviews: and a store's ratings.
        "    invalidates_on MenuChanged(_), InventoryChanged(_, _), AddressesChanged(reader), StoreChanged(_), StoreRated(_)\n",
        "    invalidates_on AddressesChanged(reader), StoreChanged(_), StoreRated(_)\n",
    ),
    (
        "the search box holds nothing of what was asked",
        "server",
        PROGRAM,
        ' name="q" value={search.asked} />',
        ' name="q" />',
    ),
    (
        "the page's title names no search",
        "server",
        PROGRAM,
        '{ "Search" } else { "Search for “{asked}”" }',
        '{ "Search" } else { "Search" }',
    ),
    # --- PostgreSQL -----------------------------------------------------------------
    (
        "a `%` is not escaped",
        "postgres",
        SEARCH,
        "        if matches!(c, '\\\\' | '%' | '_') {\n",
        "        if matches!(c, '\\\\' | '_') {\n",
    ),
    (
        "an `_` is not escaped",
        "postgres",
        SEARCH,
        "        if matches!(c, '\\\\' | '%' | '_') {\n",
        "        if matches!(c, '\\\\' | '%') {\n",
    ),
    (
        "a backslash is not escaped",
        "postgres",
        SEARCH,
        "        if matches!(c, '\\\\' | '%' | '_') {\n",
        "        if matches!(c, '%' | '_') {\n",
    ),
    (
        "PostgreSQL's layer searches as memory does",
        "postgres",
        STORE_PG,
        "        crate::search::on_postgres(&mut self.0, terms)\n",
        "        crate::search::scan(self, terms)\n",
    ),
    (
        "a dish's name is lowered under the C collation",
        "postgres",
        SEARCH,
        "          WHEN (SELECT bool_and(lower(i.name COLLATE pg_c_utf8) LIKE p ESCAPE '\\\\') \\\n",
        "          WHEN (SELECT bool_and(lower(i.name COLLATE \\\"C\\\") LIKE p ESCAPE '\\\\') \\\n",
    ),
    (
        "a store's cuisine is not its second field on PostgreSQL",
        "postgres",
        SEARCH,
        # Re-anchored by ADR-0308's amendment: the cuisine is the store's.
        "                             OR lower(s.cuisine COLLATE pg_c_utf8) LIKE p ESCAPE '\\\\') \\\n",
        "                             OR lower(s.description COLLATE pg_c_utf8) LIKE p ESCAPE '\\\\') \\\n",
    ),
    (
        "a dish's category is not its second field on PostgreSQL",
        "postgres",
        SEARCH,
        "                             OR lower(c.name COLLATE pg_c_utf8) LIKE p ESCAPE '\\\\') \\\n",
        "                             OR lower(i.description COLLATE pg_c_utf8) LIKE p ESCAPE '\\\\') \\\n",
    ),
    (
        "a store holding a matched dish is not listed on PostgreSQL",
        "postgres",
        SEARCH,
        "    ) AS m WHERE tier IS NOT NULL OR id = ANY($2::text[])\";\n",
        "    ) AS m WHERE tier IS NOT NULL OR id = ANY($2::text[]) AND false\";\n",
    ),
    (
        "the migration's cuisine is not memory's",
        "postgres",
        MIGRATION,
        "    ('48', 'Café, Bakery');\n",
        "    ('48', 'Cafe, Bakery');\n",
    ),
]

# The tests that can see the mutants' rules (W6's ruling of 2026-10-09: a
# mutant is given the tests that can see its rule), the development
# server's search tests and its search page's, by name. Each suite is the
# server's one test target, narrowed by test name.
SERVER = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
    "search::tests::", "tests::store_search::",
]
CARGO = {
    "server": (SERVER, {"PW_STORE_TEST_LAYER": "memory", "PW_STORE_DATABASE_URL": ""}),
    "postgres": (SERVER, {"PW_STORE_TEST_LAYER": "postgres"}),
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, env):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, cwd=ROOT, env={**os.environ, **env}, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    cmd, env = CARGO[suite]
    out, code = bounded(cmd, env)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    if not os.environ.get("PW_STORE_DATABASE_URL"):
        print("FAIL: the postgres suite needs PW_STORE_DATABASE_URL, a throwaway database")
        return 1
    for suite in CARGO:
        built, passed, failed = cargo_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = cargo_tests(suite)
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
