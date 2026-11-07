#!/usr/bin/env python3
"""Mutation controls for the command route: only a command is a request's to
run by its id (train-14's review, 2026-11-07).

`pw build` compiles a query, a page, a derived value and a function to
components as it does a command, and the development server hosts them all.
`POST /command/<id>` ran any of them until train-14: a query no page reads
answered a request with any arguments, and a query declares no `requires`.
Each mutant below undoes one part of the guard, and
`tests::a_query_posted_as_a_command_is_not_found` must then fail:

- the route stops asking an export's kind;
- the kind is read as anything but `command`.

Run from the repository root. The source is restored after every mutant,
whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

GUARD = (
    "                server.components.contains_key(id)\n"
    "                    && server.is_command(id)\n"
    "                    && !server.invoked_by_platform(id)\n"
)
KIND = '            .is_some_and(|e| e.kind == "command")\n'

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "the route runs any hosted export, not only a command",
        SERVER,
        GUARD,
        "                server.components.contains_key(id)\n"
        "                    && !server.invoked_by_platform(id)\n",
    ),
    (
        "a query is taken for a command",
        SERVER,
        KIND,
        '            .is_some_and(|e| e.kind == "command" || e.kind == "query")\n',
    ),
]

TESTS = [
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "--exact", "tests::a_query_posted_as_a_command_is_not_found",
    ],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if m is None:
            built = False
            continue
        passed += int(m.group(1))
        failed += int(m.group(2))
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
