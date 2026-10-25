#!/usr/bin/env python3
"""**What TodoMVC's specification says of each stack's TodoMVC, in three
engines** (the head-to-head, ADR-XXXX).

    python3 scripts/head_to_head_behaviour.py <playwright json report>

`benchmarks/headtohead/todomvc/behaviour.sh` runs
`benchmarks/headtohead/todomvc/behaviour.spec.mjs` (one test per behaviour
`app-spec.md` states) on TodoMVC in Pleris and on TodoMVC's own React and
Svelte apps, in Chromium, Firefox and WebKit, and gives Playwright's JSON
report to this script.

**The expectation** is written here, per stack: every test runs in every
engine and passes, except the stack's known gaps, each of which must fail in
every engine, and each of which names the line of the app's source that
makes it a gap (read there, not inferred from the failure). A gap that
starts passing, a new failure, a test that did not run, an engine missing:
each fails the run (exit status 1). The gaps are printed as findings about
those apps, never as Pleris's wins by themselves: TodoMVC's own Cypress suite
does not ask for most of them.
"""

import json
import pathlib
import sys

sys.dont_write_bytecode = True

ENGINES = ("chromium", "firefox", "webkit")
STACKS = ("pleris", "react", "svelte")
TESTS = 20

GAPS = {
    "pleris": {},
    "react": {
        "editing: Escape discards the edit": (
            "Escape does not leave the edit: the edit field handles Enter alone "
            "(react/src/todo/components/input.jsx, handleKeyDown: `if (e.key !== \"Enter\") return;`)"
        ),
        "counter: the number left in a <strong>, then 'item left' or 'items left'": (
            "the counter reads \"1 item left!\" with no <strong> "
            "(react/src/todo/components/footer.jsx: `${activeTodos.length} ${...} left!`)"
        ),
        "persistence: the todos and their states survive a reload": (
            "the todos are kept in memory only: a reload empties the list "
            "(react/src/todo/app.jsx: `useReducer(todoReducer, [])`, no storage)"
        ),
    },
    "svelte": {
        "editing: Enter saves the edit, trimmed": (
            "an edit is saved untrimmed (svelte/src/Item.svelte, updateItem: `item.description = value`)"
        ),
        "editing: an edit left empty destroys the todo": (
            "an edit of spaces is saved, not destroyed: the emptiness check does not trim "
            "(svelte/src/Item.svelte, updateItem: `if (value.length)`)"
        ),
        "persistence: the todos and their states survive a reload": (
            "the todos are kept in memory only: a reload empties the list "
            "(svelte/src/App.svelte: `let items = $state([])`, no storage)"
        ),
        "routing: a page opened at #/completed shows that filter": (
            "the filter is read only when the fragment changes, not when the page opens "
            "(svelte/src/router.js: `init` adds a `hashchange` listener and reads nothing)"
        ),
    },
}


def results(report):
    """{(stack, engine): {title: status}} from Playwright's JSON report."""
    out = {}

    def walk(suite):
        for spec in suite.get("specs", []):
            for t in spec.get("tests", []):
                project = t.get("projectName", "")
                stack, _, engine = project.partition("-")
                last = (t.get("results") or [{}])[-1]
                status = last.get("status", "skipped")
                out.setdefault((stack, engine), {})[spec["title"]] = status
        for inner in suite.get("suites", []):
            walk(inner)

    for suite in report.get("suites", []):
        walk(suite)
    return out


def judge(report):
    found = results(report)
    problems = []
    lines = []
    for stack in STACKS:
        gaps = GAPS[stack]
        per_engine = []
        for engine in ENGINES:
            got = found.get((stack, engine))
            if got is None:
                problems.append(f"{stack} in {engine}: no test ran")
                per_engine.append(f"{engine} not run")
                continue
            if len(got) != TESTS:
                problems.append(f"{stack} in {engine}: {len(got)} tests, not {TESTS}")
            passed = sum(1 for s in got.values() if s == "passed")
            per_engine.append(f"{engine} {passed}/{len(got)}")
            for title, status in sorted(got.items()):
                if title in gaps:
                    if status != "failed":
                        problems.append(f"{stack} in {engine}: {title!r} is {status}; it is a known gap, which must fail")
                elif status != "passed":
                    problems.append(f"{stack} in {engine}: {title!r} is {status}")
            for title in gaps:
                if title not in got:
                    problems.append(f"{stack} in {engine}: the gap {title!r} names no test that ran")
        lines.append(f"{stack}: {'; '.join(per_engine)} passed; {len(gaps)} gap(s) against the specification")
        for title, why in gaps.items():
            lines.append(f"  GAP {stack}: {title}: {why}")
    return lines, problems


def main(argv):
    if len(argv) != 2:
        print("usage: head_to_head_behaviour.py <playwright json report>")
        return 2
    try:
        report = json.loads(pathlib.Path(argv[1]).read_text(encoding="utf-8"))
    except (OSError, ValueError) as why:
        print(f"behaviour: no report to read: {why}")
        return 1
    lines, problems = judge(report)
    print("TodoMVC's specification (app-spec.md), one test per behaviour, three engines (behaviour.spec.mjs):")
    for line in lines:
        print(line)
    print()
    if problems:
        for p in problems:
            print(f"PROBLEM: {p}")
        print(f"behaviour: FAILED: {len(problems)} problem(s)")
        return 1
    print("behaviour: as expected: every test passed in every engine but each stack's gaps, each failing in all three")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
