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
import re
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


CYPRESS_LINE = re.compile(r"^(?:acceptance|react|svelte): (\d+) of (\d+) passed", re.M)


def cypress_counts(text):
    """(passed, total) from a Cypress judge's report: `todomvc_cypress_result.py`'s
    acceptance line for Pleris, `head_to_head_cypress.py`'s for the others."""
    m = CYPRESS_LINE.search(text)
    return (int(m.group(1)), int(m.group(2))) if m else None


def summary(report, cypress_texts):
    """The data the head-to-head's summary reads: each stack's tests passed in
    the three engines together, and its Cypress count."""
    found = results(report)
    spec = {}
    for stack in STACKS:
        cells = [found.get((stack, e), {}) for e in ENGINES]
        spec[stack] = {
            "passed": sum(1 for c in cells for s in c.values() if s == "passed"),
            "total": sum(len(c) for c in cells),
            "gaps": sorted(GAPS[stack]),
        }
    cypress = {}
    for stack, text in cypress_texts.items():
        counts = cypress_counts(text)
        if counts:
            cypress[stack] = {"passed": counts[0], "total": counts[1]}
    return {"spec": {"engines": list(ENGINES), "stacks": spec}, "cypress": cypress}


def main(argv):
    args = argv[1:]
    out = None
    cypress_texts = {}
    if "--json" in args:
        i = args.index("--json")
        out = pathlib.Path(args[i + 1])
        del args[i : i + 2]
    while "--cypress" in args:
        i = args.index("--cypress")
        stack, _, file = args[i + 1].partition("=")
        cypress_texts[stack] = pathlib.Path(file).read_text(encoding="utf-8", errors="replace")
        del args[i : i + 2]
    if len(args) != 1:
        print("usage: head_to_head_behaviour.py <playwright json report> [--json out] [--cypress stack=judge-output ...]")
        return 2
    try:
        report = json.loads(pathlib.Path(args[0]).read_text(encoding="utf-8"))
    except (OSError, ValueError) as why:
        print(f"behaviour: no report to read: {why}")
        return 1
    if out:
        out.write_text(json.dumps(summary(report, cypress_texts), indent=1) + "\n")
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
