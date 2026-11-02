#!/usr/bin/env python3
"""Mutation controls for ADR-0151: a page's values are read outside the
subscriber table.

Each mutant undoes one piece:
- a document read inside the table again, so pages read at once wait for
  each other and never share a query's flight;
- a document installed although a change reached its session while it was
  read, or a frame pushed to a session not counted;
- the cart's count read from every binding, so a command asks every query
  its page reads.

A mutant must fail the server's tests that see it: ADR-0148's controls
found tests a name filter left out.

Run from the repository root; `just e14-document-reads` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a document is read inside the subscriber table",
        SERVER,
        # Re-anchored by ADR-0161: each attempt reads one document's.
        # Re-anchored by ADR-0190: a document of any page is read so.
        "                    let read = self.render_document(&doc, settled)?;\n"
        "                    let mut queue = self.pending.lock().expect(\"pending\");\n",
        "                    let mut queue = self.pending.lock().expect(\"pending\");\n"
        "                    let read = self.render_document(&doc, settled)?;\n",
    ),
    (
        "a document is installed after a change reached its session",
        SERVER,
        "        if pushed.is_some_and(|pushed| waiting.pushed != pushed)\n",
        "        if (pushed.is_some_and(|pushed| waiting.pushed != pushed) && false)\n",
    ),
    (
        "a frame pushed to a session is not counted",
        SERVER,
        "        self.pushed += 1;\n",
        "        self.pushed += 0;\n",
    ),
    (
        "the cart's count is read from every binding",
        SERVER,
        "        let bindings = self.binding(session, part[\"binding\"].as_str().unwrap_or_default())?;\n",
        "        let bindings = self.bindings(session)?;\n",
    ),
]

TESTS = [
    # The tests that fail under its mutants, measured against the whole suite
    # by `scripts/mutant_killers.py` on 2026-10-12 and 2026-10-13 (W6's
    # ruling of 2026-10-09), two per mutant where two fail.
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_change_that_reaches_a_session_while_its_page_is_read_is_not_lost",
        "tests::a_longer_read_is_not_applied_over_a_commit_it_did_not_see",
        "tests::pages_read_at_once_each_ask_when_their_query_says_parallel",
        "tests::pages_read_at_once_share_an_ask_their_query_says_is_one_per_key",
    ],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", r.stdout + r.stderr)
        if not found:
            built = False
            continue
        for p, f in found:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run_tests()
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
        print(f"{what}: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
