#!/usr/bin/env python3
"""Mutation controls for ADR-0164: a menu change drops what declares it, for
its store.

Each mutant puts back one piece of the drop by name:
- the server drops every store's kept menu, by the query's name, or nothing;
- the event names no store, or another store;
- the store's `Menu` declares no `invalidates_on MenuChanged(id)`.

Each must fail the development server's tests that see it (measured,
below). The tests build the store from `examples`, so a mutant of the
store's source is read when they run.

Run from the repository root; `just e14-menu-changed` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
STORE = ROOT / "examples/store/app.pw"

# Re-anchored by ADR-0178, whose stock change is an event of its own.
# Re-anchored by track store-pg: the change is written through the layer
# with its event, and what committed is delivered.
# Re-anchored by track store-search: what the change dropped is told to
# each session reading it, so the drop is the closure's first line.
EVENT = (
    "            |delivered| {\n"
    "                let reached = self.invalidate_queries(\"\", &[], delivered);\n"
)

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a menu change drops every store's menu, by the query's name",
        SERVER,
        EVENT,
        "            |_| {\n"
        "                self.queries.invalidate(\"store.page.Menu\");\n"
        "                let reached: Vec<Reached> = Vec::new();\n",
    ),
    (
        "a menu change drops nothing",
        SERVER,
        EVENT,
        "            |_| {\n"
        "                let reached: Vec<Reached> = Vec::new();\n",
    ),
    (
        "the event names no store",
        SERVER,
        # Re-anchored by ADR-0178, and by track store-pg.
        "                \"Events.MenuChanged\".to_string(),\n"
        "                vec![Val::String(STORE_ID.into())],\n",
        "                \"Events.MenuChanged\".to_string(),\n"
        "                vec![],\n",
    ),
    (
        "the event is another store's",
        SERVER,
        # Re-anchored by ADR-0178, and by track store-pg.
        "                \"Events.MenuChanged\".to_string(),\n"
        "                vec![Val::String(STORE_ID.into())],\n",
        "                \"Events.MenuChanged\".to_string(),\n"
        "                vec![Val::String(SECOND_STORE.0.into())],\n",
    ),
    (
        "the store's menu declares no invalidation",
        STORE,
        # Re-anchored by ADR-0165, whose recommendations declare it too, and
        # by ADR-0178, whose menu listens for a stock change as well.
        "    invalidates_on MenuChanged(id), InventoryChanged(id, _)\n"
        "    concurrency    one_per_key\n"
        "    on_key_change  cancel\n"
        "    timeout        2.seconds\n",
        "    invalidates_on InventoryChanged(id, _)\n"
        "    concurrency    one_per_key\n"
        "    on_key_change  cancel\n"
        "    timeout        2.seconds\n",
    ),
]

# The development server's tests that see its mutants' rules (W6's ruling of
# 2026-10-09: a mutant that takes long is given the tests that can see its
# rule), measured by `scripts/mutant_killers.py` on 2026-10-12 against its
# whole suite: each of the five is failed by two of these, of the 3 to 16 that
# fail under it. The whole suite ran for every mutant, and the recipe took
# 3,671 s of a verify.
CARGO = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
    "tests::a_menu_change_drops_that_stores_kept_menu_only",
    "tests::materializations::a_document_that_reads_another_key_is_told_nothing",
]

# How long one run may take. Past the bound it is killed with everything it
# started, and the run counts as failing.
# 1800 from 2026-10-12 (track store-accounts) while these mutants ran the
# server's whole suite, 941 to 961 s on CI's runner at 420 tests; they run
# the tests that see them since (above).
BOUND = 900


def run():
    """(built, passed, failed) over the server's tests."""
    p = subprocess.Popen(CARGO, cwd=ROOT, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
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
    built, passed, failed = run()
    print(f"baseline: {passed} passed, {failed} failed")
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
