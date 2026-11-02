#!/usr/bin/env python3
"""Mutation controls for ADR-0190: every page that binds a query is served at
its route, and kept current by its own plan.

Each mutant undoes one piece of the development server's:
- a page that binds a query served as a page of signals, which refuses it;
- every document recorded as the store's;
- a document's bindings read by the store's plan;
- a document patched against the store's template;
- a change sent to the store's documents alone;
- the store's menu fragment rendered for every page;
- the store's menu told to every document of its store.

Every mutant must fail the development server's tests of a second page.

Run from the repository root; `just e14-pages` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a page that binds a query is served as a page of signals",
        SERVER,
        "            .and_then(|plan| plan[\"bindings\"].as_array())\n"
        "            .is_some_and(|b| !b.is_empty())\n",
        "            .and_then(|plan| plan[\"bindings\"].as_array())\n"
        "            .is_some_and(|_| false)\n",
    ),
    (
        "every document is recorded as the store's",
        SERVER,
        "            .insert(doc.clone(), page.to_string());\n",
        "            .insert(doc.clone(), self.store_page().to_string());\n",
    ),
    (
        # Re-anchored by ADR-0304: what a document's values hold until is
        # noted as they are read.
        "a document's bindings are read by the store's plan",
        SERVER,
        "        let plan = self.plan_of(&page);\n"
        "        // What the document's values hold until (ADR-0304): the earliest\n",
        "        let plan = &self.plan;\n"
        "        // What the document's values hold until (ADR-0304): the earliest\n",
    ),
    (
        "a document is patched against the store's template",
        SERVER,
        "    ) -> Result<Vec<Targeted>, String> {\n"
        "        let template = self.template_of(page);\n",
        "    ) -> Result<Vec<Targeted>, String> {\n"
        "        let template = self.store_template();\n",
    ),
    (
        "a change is sent to the store's documents alone",
        SERVER,
        # Re-anchored by ADR-0231: a document's parameters, not its store;
        # by ADR-0277, whose documents are those a telling takes; by
        # ADR-0297, which takes them before the loop; and by NEXT 4c's render
        # merge (track `render-merge`), which reads those whose pages ask.
        '        for (doc, reader) in live {\n',
        '        for (doc, reader) in live.into_iter().filter(|(doc,_)| self.page_of(doc) == self.store_page()) {\n',
    ),
    (
        "every page renders the store's menu",
        SERVER,
        "        let menu = (page == self.store_page()).then(|| {\n",
        "        let menu = true.then(|| {\n",
    ),
    (
        "the store's menu is told to every document of its store",
        SERVER,
        "                .is_none_or(|page| page.as_str() == self.store_page())\n",
        "                .is_none_or(|_| true)\n",
    ),
    # ADR-0191 lets every page speculate from its own module: the two
    # mutants that sent the store's speculation to every page are
    # `page_speculation_mutations.py`'s now, turned around.
]

TESTS = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
    "a_page_that_binds_a_query_is_served_at_its_route",
    "a_change_reaches_each_page_that_reads_it_by_its_own_plan",
    "a_page_without_the_menu_is_told_nothing_of_it",
]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests():
    """(built, passed, failed) over the tests."""
    p = subprocess.Popen(TESTS, cwd=ROOT, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(n) for n, _ in found), sum(int(f) for _, f in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
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
            built, passed, failed = run_tests()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
