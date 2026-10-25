#!/usr/bin/env python3
"""**What TodoMVC's own Cypress suite says of TodoMVC's own React and Svelte
apps** (the head-to-head, ADR-XXXX).

    python3 scripts/head_to_head_cypress.py <react|svelte> <cypress output> [exit status]

`benchmarks/headtohead/todomvc/cypress.sh` runs the same compatibility copy
of the suite that TodoMVC in Pleris is held to (`todomvc-cypress-compat-v1`,
`spikes/todomvc-cypress/UPSTREAM.md`) against the vendored app, under the
app's own name, and gives Cypress's output to this script. The suite's own
tables then decide what it asks of the app: both names are in its
`noLocalStorageCheck`, so it asks nothing of `localStorage`, and its
persistence test skips itself.

**The expectation**, per app, is written here and held: every one of the 29
tests runs; each passes, except the tests the suite's own tables skip for
the app's name, which must be reported pending, and each of which is the
app's **gap** against TodoMVC's specification, printed as such. Anything
else (a failure, a test that did not run, a table that disagrees with its
lines, no table at all) fails, exit status 1: an app that stops passing what
it passed is a change to investigate, never a gap to note.

The tests and their states are read by `todomvc_cypress_result.py`'s own
reader, the one TodoMVC in Pleris's acceptance is judged by.
"""

import pathlib
import re
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import todomvc_cypress_result as cy  # noqa: E402

# The tests the suite's own tables skip for each name, and what each means.
SKIPPED_BY_THE_SUITE = {
    cy.PERSISTENCE: "the app keeps no todos: TodoMVC's specification asks every app to persist them in localStorage",
}
EXPECT = {
    "react": {"pending": [cy.PERSISTENCE]},
    "svelte": {"pending": [cy.PERSISTENCE]},
}


def in_table(spec_text, table, name):
    """Whether the spec's table `table` names `name` (its `name: true` line)."""
    m = re.search(r"const " + table + r" = \{(.*?)\n\}", spec_text, re.S)
    if not m:
        raise ValueError(f"the spec has no table {table}")
    return re.search(r"(^|\s)'?" + re.escape(name) + r"'?: true", m.group(1), re.M) is not None


def judge(framework, tests, run, exit_status, spec_text):
    problems = []
    expect = EXPECT[framework]
    if not in_table(spec_text, "noLocalStorageCheck", framework):
        problems.append(f"the suite's noLocalStorageCheck no longer names {framework}: the expectation needs review")
    if len(tests) != cy.EXPECTED_TESTS:
        problems.append(f"the spec has {len(tests)} tests, not {cy.EXPECTED_TESTS}")
    counts = run["counts"]
    if "Tests" not in counts or run["cypress"] is None:
        problems.append("Cypress did not run the suite: its output has no results table")
        return None, problems
    st = cy.states(tests, run, problems)
    got = {
        "Tests": len(tests),
        "Passing": len(run["passed"]),
        "Failing": len(run["failed"]),
        "Pending": len(run["pending"]),
        "Skipped": sum(1 for t in tests if st[t.n].state == "skip"),
    }
    for key, lines in got.items():
        if counts.get(key) != lines:
            problems.append(f"Cypress's table says {key}: {counts.get(key)}, its lines and the spec say {lines}")
    if exit_status is not None and exit_status != counts.get("Failing"):
        problems.append(f"Cypress exited with status {exit_status}, and its table counts {counts.get('Failing')} failing")
    for t in tests:
        state, error = st[t.n]
        if t.title in expect["pending"]:
            if state != "pending":
                problems.append(f"test {t.n} ({t.title!r}) is {state}; the suite's own table skips it for {framework}")
        elif state != "pass":
            problems.append(f"test {t.n} ({t.title!r}) is {state}: {error}")
    return st, problems


def report(framework, tests, run, st, problems, exit_status):
    lines = [
        f"TodoMVC's own {framework} app, under its own name: the suite's tables decide what it asks",
        "(noLocalStorageCheck names it: nothing of localStorage is asked, and the persistence test skips itself).",
        "",
    ]
    if st is None:
        lines.append(f"{framework}: the suite did not run: Cypress printed no results table")
    else:
        n = {s: sum(1 for t in tests if st[t.n].state == s) for s in ("pass", "fail", "hook", "pending", "skip")}
        lines.append(
            f"{framework}: {n['pass']} of {len(tests)} passed; {n['fail'] + n['hook']} failed; "
            f"{n['pending']} pending; {n['skip']} not run"
        )
        lines.append(f"cypress {run['cypress']} on {run['browser']}; exit status {exit_status}")
        for t in tests:
            state, error = st[t.n]
            if state != "pass":
                gap = SKIPPED_BY_THE_SUITE.get(t.title)
                lines.append(f"{t.n:3d}  {state:7s} [{t.context}] {t.title}")
                if gap and state == "pending":
                    lines.append(f"{'':13s}GAP: {gap}")
                elif error:
                    lines.append(f"{'':13s}cypress: {error}")
    lines.append("")
    if problems:
        lines += [f"PROBLEM: {p}" for p in problems]
        lines.append(f"{framework}: FAILED: {len(problems)} problem(s)")
    else:
        lines.append(f"{framework}: as expected: every test passed but those the suite skips for it, each a gap above")
    return "\n".join(lines)


def main(argv):
    if len(argv) < 3 or argv[1] not in EXPECT:
        print("usage: head_to_head_cypress.py <react|svelte> <cypress output> [exit status]")
        return 2
    framework, path = argv[1], pathlib.Path(argv[2])
    exit_status = int(argv[3]) if len(argv) > 3 else None
    spec_text = cy.SPEC.read_text(encoding="utf-8")
    tests = cy.spec_tests(spec_text)
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as why_not:
        print(f"{framework}: Cypress's output is not there: {why_not}")
        return 1
    run = cy.parse_run(text)
    st, problems = judge(framework, tests, run, exit_status, spec_text)
    print(report(framework, tests, run, st, problems, exit_status))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
