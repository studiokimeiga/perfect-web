#!/usr/bin/env python3
"""Mutation controls for ADR-0104 and ADR-0208: the dev server commits the
events a command computes, and no others.

Each mutant undoes one piece: committing the events the command handed the
outbox rather than `CartChanged` whatever it computed, keeping what it
handed, and each value's text as a key. The dev server's tests must then
fail.

Run from the repository root; `just e10-committed-events` records the
output. The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    ("`CartChanged` is committed whatever is computed", SERVER, '                    schedules::write(tx, scheduled)?;\n                    Ok::<_, String>(events)\n', '                    schedules::write(tx, scheduled)?;\n                    let _ = events;\n                    let session = rows.first()\n                        .map(|(k, _)| k.trim_start_matches("cart:").to_string())\n                        .unwrap_or_default();\n                    Ok::<_, String>(vec![pw_materialize::Event::new(\n                        &["Events", ".CartChanged"].concat(), &[session.as_str()],\n                    )])\n'),
    # ADR-0208: the command computes its events and hands them to the
    # outbox. ADR-0104's four controls of the server's key evaluation are
    # retired with it.
    (
        "the outbox keeps nothing a command hands it",
        SERVER,
        "                        .push((event.clone(), values.to_vec()));\n",
        "                        .clear();\n",
    ),
    (
        "an `Int` is not its text",
        SERVER,
        "            Val::S64(n) => Ok(n.to_string()),\n",
        "            Val::S64(n) => Ok(format!(\"{n}.0\")),\n",
    ),
    (
        "a `Bool` is not its text",
        SERVER,
        "            Val::Bool(b) => Ok(b.to_string()),\n",
        "            Val::Bool(b) => Ok(u8::from(*b).to_string()),\n",
    ),
]

TESTS = [
    # The tests that fail under its mutants, measured against the whole suite
    # by `scripts/mutant_killers.py` on 2026-10-13 (W6's ruling of
    # 2026-10-09), two per mutant where two fail.
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact",
        "tests::a_change_reaches_each_page_that_reads_it_by_its_own_plan",
        "tests::a_command_commits_the_events_it_computes",
    ],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not results:
            built = False
            continue
        for p, f in results:
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
