#!/usr/bin/env python3
"""Mutation controls for track `todomvc`: TodoMVC in Pleris, held to its own
spec (ADR-0366), its todos on the device since track `device-store`
(ADR-0366).

Each mutant undoes one thing the spec states:
- the program (`examples/todomvc/app.pw`): the todos kept on the device, under
  the spec's name; the hidden main and footer, the trim, the counter's words
  and what it counts, the toggle-all's rule and its checked state, each
  filter and the fragment's forms, the editing mode (which row, its class,
  its field and what it holds, Enter, Escape and blur), the clear button, a
  todo's toggle, an edit trimmed to nothing, an id one more than any before
  it, where a todo is added, and the field's focus and clearing;
- the browser's runtime (`spikes/own-renderer/public/pw-runtime.mjs`), where
  a patch sets a checkbox's state as well as its attribute, and the edit
  field a patch inserts takes focus (the runtime's own `autofocus` is
  controlled in `scripts/autofocus_mutations.py`, its device store and its
  morph in `scripts/device_store_mutations.py`). A mutant of the runtime
  rebuilds the pages too (`run.sh`), as every control that changes it does:
  the Playwright config refuses to run against a `dist` built with another
  runtime, and a suite that did not run kills nothing.

**A program mutant must still be a well-typed program, or name the rule it
takes away** (`scripts/mutant_programs.py`, the integrator's audit of train-7):
each is checked with `pw check` before anything runs, and one that does not
check is no mutant and fails this script (a rule that refused it would be
killing it for the wrong reason), unless the checker's refusal IS what it
undoes and `CHECKED` says so, by the rule's code; such a mutant is killed by the
checker, says so, and runs no test. A mutant is run against the server's tests
(`tests::todomvc::`, which build the program as `pw build` does) and, where
they hold, against the browser's (`e2e/todomvc.spec.mjs`, in Chromium); one
the browser alone kills is one only a browser sees.

Run from the repository root; `just e14-todomvc` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import json
import pathlib
import re
import secrets
import signal
import subprocess
import sys
import tempfile

import browser_test_results
import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP = ROOT / "examples/todomvc/app.pw"
RUNTIME_FILE = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

PROGRAM = "program"
HOST = "host"
RUNTIME = "runtime"

# The program mutants the checker refuses, by the rule that is the protection
# each takes away (`scripts/mutant_programs.py`): every other program mutant
# here leaves a program that checks. Each is killed by the checker, and no test
# is run for it.
CHECKED = {
    # The todos kept on the device say not whose they are.
    "the todos say not whose they are": {"PW6805"},
}

# (what is undone, which tests, file, anchor, replacement)
MUTANTS = [
    (
        "the todos are kept under another name than the spec's",
        PROGRAM,
        APP,
        '    storage  device "todos-pleris"\n',
        '    storage  device "pleris-todos"\n',
    ),
    (
        "the todos say not whose they are",
        PROGRAM,
        APP,
        "    privacy  Device\n    conflict last_write_wins\n",
        "    conflict last_write_wins\n",
    ),
    (
        "main is not hidden with no todos",
        PROGRAM,
        APP,
        '            <section class="main" hidden={List.length(todos) == 0}>\n',
        '            <section class="main">\n',
    ),
    (
        "the footer is not hidden with no todos",
        PROGRAM,
        APP,
        '            <footer class="footer" hidden={List.length(todos) == 0}>\n',
        '            <footer class="footer">\n',
    ),
    (
        "a title is not trimmed",
        PROGRAM,
        APP,
        "    let trimmed = String.trim(draft)\n",
        "    let trimmed = draft\n",
    ),
    (
        "the counter says `items left` of one",
        PROGRAM,
        APP,
        '    if n == 1 {\n        "item left"\n',
        '    if n == 0 {\n        "item left"\n',
    ),
    (
        "the counter counts the completed",
        PROGRAM,
        APP,
        "fn left(todos: List<Todo>) -> Int !{} {\n    List.length(List.filter(todos, (t) => !t.completed))\n",
        "fn left(todos: List<Todo>) -> Int !{} {\n    List.length(List.filter(todos, (t) => t.completed))\n",
    ),
    (
        "the toggle-all completes only when every todo is active",
        PROGRAM,
        APP,
        "    let any_active = List.any(todos, (t) => !t.completed)\n",
        "    let any_active = List.all(todos, (t) => !t.completed)\n",
    ),
    (
        "the toggle-all is checked with no todos",
        PROGRAM,
        APP,
        "    List.length(todos) > 0 & List.all(todos, (t) => t.completed)\n",
        "    List.all(todos, (t) => t.completed)\n",
    ),
    (
        "the toggle-all does what clearing the completed does",
        PROGRAM,
        APP,
        "                    on:press={() => todos = toggled_all(todos)} />\n",
        "                    on:press={() => todos = cleared(todos)} />\n",
    ),
    (
        "the active filter shows the completed",
        PROGRAM,
        APP,
        '    if filter == "active" {\n        !t.completed\n',
        '    if filter == "active" {\n        t.completed\n',
    ),
    (
        "the completed filter shows the active",
        PROGRAM,
        APP,
        '    } else if filter == "completed" {\n        t.completed\n',
        '    } else if filter == "completed" {\n        !t.completed\n',
    ),
    (
        "the default filter shows nothing",
        PROGRAM,
        APP,
        "    } else {\n        true\n    }\n",
        "    } else {\n        false\n    }\n",
    ),
    (
        "the model does not filter",
        PROGRAM,
        APP,
        # Re-anchored by the row's computed holes' ADR: the list iterates the
        # todos the filter shows.
        "    List.filter(todos, (t) => shown(t, filter))\n",
        "    todos\n",
    ),
    (
        "the clear button counts what is left",
        PROGRAM,
        APP,
        "fn done(todos: List<Todo>) -> Int !{} {\n    List.length(List.filter(todos, (t) => t.completed))\n",
        "fn done(todos: List<Todo>) -> Int !{} {\n    List.length(List.filter(todos, (t) => !t.completed))\n",
    ),
    (
        "no row is the one being edited",
        PROGRAM,
        APP,
        # Re-anchored by the row's computed holes' ADR: each row computes
        # whether it is edited, its field and its class apart.
        "                            {#if row.id == editing}\n",
        "                            {#if false}\n",
    ),
    (
        "no row's class says it is edited",
        PROGRAM,
        APP,
        "<li class={row_class(row.completed, row.id == editing)}>",
        "<li class={row_class(row.completed, false)}>",
    ),
    (
        "an edited todo that is completed is not shown completed",
        PROGRAM,
        APP,
        '        "completed editing"\n',
        '        "editing"\n',
    ),
    (
        "an edited todo has no editing class",
        PROGRAM,
        APP,
        '    } else if editing {\n        "editing"\n',
        '    } else if editing {\n        ""\n',
    ),
    (
        "a completed todo has no completed class",
        PROGRAM,
        APP,
        '        "completed"\n    } else if editing {\n',
        '        ""\n    } else if editing {\n',
    ),
    (
        "a toggle sets what the todo already is",
        PROGRAM,
        APP,
        "        Todo { id: t.id, title: t.title, completed: !t.completed }\n",
        "        Todo { id: t.id, title: t.title, completed: t.completed }\n",
    ),
    (
        "an edit trimmed to nothing is kept, not destroyed",
        PROGRAM,
        APP,
        "        None => removed(todos, id),\n",
        "        None => todos,\n",
    ),
    (
        "destroy keeps the todo",
        PROGRAM,
        APP,
        "    List.filter(todos, (t) => t.id != id)\n",
        "    List.filter(todos, (t) => t.id == t.id)\n",
    ),
    (
        "clear completed clears none",
        PROGRAM,
        APP,
        "fn cleared(todos: List<Todo>) -> List<Todo> !{} {\n    List.filter(todos, (t) => !t.completed)\n",
        "fn cleared(todos: List<Todo>) -> List<Todo> !{} {\n    todos\n",
    ),
    (
        "a todo is added at the top of the list",
        PROGRAM,
        APP,
        "    List.concat(todos, [Todo { id: last + 1, title: title, completed: false }])\n",
        "    List.concat([Todo { id: last + 1, title: title, completed: false }], todos)\n",
    ),
    (
        "an id is one before it again",
        PROGRAM,
        APP,
        "    let last = List.fold(todos, 0, (m, t) => if t.id > m {\n",
        "    let last = List.fold(todos, 0, (m, t) => if t.id < m {\n",
    ),
    (
        "the active link goes to the completed",
        PROGRAM,
        APP,
        '<li><a class={selected(filter, "active")} href="#/active">Active</a></li>',
        '<li><a class={selected(filter, "active")} href="#/completed">Active</a></li>',
    ),
    (
        "an active address shows every todo",
        PROGRAM,
        APP,
        '"/active" => "active", "/completed"',
        '"/active" => "all", "/completed"',
    ),
    (
        "a completed address shows the active",
        PROGRAM,
        APP,
        '"/completed" => "completed"\n',
        '"/completed" => "active"\n',
    ),
    (
        "the selected link is every other",
        PROGRAM,
        APP,
        '    if filter == mine {\n        "selected"\n',
        '    if filter != mine {\n        "selected"\n',
    ),
    (
        "a double press does not begin an edit",
        PROGRAM,
        APP,
        '                                    editing = row.id\n',
        '                                    editing = 0\n',
    ),
    (
        "beginning an edit forgets the title",
        PROGRAM,
        APP,
        '                                    edit_text = row.title\n',
        '                                    edit_text = ""\n',
    ),
    (
        "the field starts empty",
        PROGRAM,
        APP,
        '<input class="edit" aria-label="Edit" autofocus value={row.title}\n',
        '<input class="edit" aria-label="Edit" autofocus value=""\n',
    ),
    (
        "what is typed in the field is not kept",
        PROGRAM,
        APP,
        "on:input={(e: InputEvent) => edit_text = e.value}",
        'on:input={(e: InputEvent) => edit_text = ""}',
    ),
    (
        "Escape does not leave editing",
        PROGRAM,
        APP,
        '                                    } else if e.key == "Escape" {\n                                        editing = 0\n',
        '                                    } else if e.key == "Escape" {\n                                        editing = editing\n',
    ),
    (
        "blur does not save",
        PROGRAM,
        APP,
        '                                    on:blur={(e: BlurEvent) => if editing != 0 {\n',
        '                                    on:blur={(e: BlurEvent) => if editing == -1 {\n',
    ),
    (
        "Enter does not save",
        PROGRAM,
        APP,
        '                                    on:keydown={(e: KeyEvent) => if e.key == "Enter" {\n                                        todos = retitled(todos, editing, edit_text)\n',
        '                                    on:keydown={(e: KeyEvent) => if e.key == "Enter" {\n                                        todos = todos\n',
    ),
    (
        "the new-todo field is not cleared",
        PROGRAM,
        APP,
        '                                todos = added(todos, title)\n                                draft = ""\n',
        "                                todos = added(todos, title)\n                                draft = draft\n",
    ),
    (
        "a title of blanks clears the field",
        PROGRAM,
        APP,
        "                            None => draft = draft,\n                        }\n                    }} />\n",
        '                            None => draft = "",\n                        }\n                    }} />\n',
    ),
    (
        "the new-todo field is not focused when the page loads",
        PROGRAM,
        APP,
        '<input class="new-todo" placeholder="What needs to be done?" aria-label="New todo" autofocus\n',
        '<input class="new-todo" placeholder="What needs to be done?" aria-label="New todo"\n',
    ),
    (
        "a patch that removes `checked` leaves the box as a person left it",
        RUNTIME,
        RUNTIME_FILE,
        # Device-backed TodoMVC changes now reach live parts. Preserve every
        # other boolean attribute and the opposite checked transition.
        '      if (typeof el[live.attribute] === "boolean") el[live.attribute] = on;\n',
        '      if (typeof el[live.attribute] === "boolean" && (live.attribute !== "checked" || on)) el[live.attribute] = on;\n',
    ),
    (
        "a patch that sets `checked` leaves the box as a person left it",
        RUNTIME,
        RUNTIME_FILE,
        '      if (typeof el[live.attribute] === "boolean") el[live.attribute] = on;\n',
        '      if (typeof el[live.attribute] === "boolean" && (live.attribute !== "checked" || !on)) el[live.attribute] = on;\n',
    ),
    (
        "an edit field inside the row a patch inserts is not given focus",
        RUNTIME,
        RUNTIME_FILE,
        # The morph inserts the edit input itself, so removing only descendant
        # search no longer reaches its focus. Omit the insertion-time lookup;
        # initial document autofocus remains the browser's.
        '    const el = autofocusIn(records.flatMap((r) => [...r.addedNodes]));\n',
        '    const el = null;\n',
    ),
]

SERVER_TESTS = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
    "--", "tests::todomvc::", "--format=pretty",
]
SERVER_SOURCE = ROOT / "spikes/own-renderer/server/src/tests/todomvc.rs"
SERVER_NAMES = {
    "tests::todomvc::the_host_renders_the_default_and_hides_what_the_device_decides",
    "tests::todomvc::the_document_says_what_the_device_keeps",
    "tests::todomvc::no_host_holds_a_todo",
    "tests::todomvc::the_page_has_the_templates_markup",
}
SPEC = ["e2e/todomvc.spec.mjs", "--project=chromium"]
BROWSER_COUNT = 54
INVENTORIES = {}

# A stopped, incomplete or erroneous producer is invalid, never a test failure.
BOUND = 1800


def bounded(cmd, **kw):
    """The shared owner reaps its native process before source restoration."""
    try:
        result = producer_process.run(cmd, timeout=BOUND, **kw)
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired as expired:
        return expired.output, None


def program_files():
    return [str(path) for directory in ("packages/pw-std", "packages/pw-platform-web", "examples/todomvc")
            for path in sorted((ROOT / directory).glob("*.pw"))]


def checker_result(code, output, files):
    """Native complete pw-check output: clean, exact diagnosed refusal, or invalid.

    This is the same plain diagnostic grammar as the guide producer. A Cargo
    error, warning, panic, truncated summary or arbitrary line cannot mint a
    named CHECKED kill merely by mentioning PW6805.
    """
    if type(code) is not int or code not in (0, 1) or not isinstance(output, str):
        return False, False, set()
    lines = output.splitlines()
    if code == 0:
        clean = lines == [f"pw check: {files} file(s), no diagnostics"]
        return clean, clean, set()
    if not lines:
        return False, False, set()
    summary = re.fullmatch(r"pw check: (\d+) file\(s\), (\d+) error\(s\), 0 warning\(s\)", lines[-1])
    errors = [line for line in lines if line.startswith("error:")]
    codes = [re.fullmatch(r"error: \[(PW\d{4})\] .+", line) for line in errors]
    started = False
    for line in lines[:-1]:
        if line.startswith("error:"):
            started = True
        elif line and (not started or re.fullmatch(
                r"(?:\s*(?:\d+|\.\.\.)?\s*\|.*|  --> .+:\d+:\d+|   = (?:note|help):.*)", line) is None):
            return False, False, set()
    if (summary is None or int(summary[1]) != files or int(summary[2]) != len(errors)
            or not errors or not all(codes)):
        return False, False, set()
    return True, False, {match[1] for match in codes}


def checks():
    """The program must check, except the one explicitly named rule witness."""
    files = program_files()
    out, code = bounded(
        ["cargo", "run", "--quiet", "--locked", "-p", "pw-cli", "--", "check", "--plain",
         *files],
        cwd=ROOT,
    )
    result = checker_result(code, out, len(files))
    if not result[0]:
        print(out, flush=True)
    return result


def server_tests(_what=None):
    """Four complete named Cargo tests, with native assertion-site authority."""
    out, code = bounded(SERVER_TESTS, cwd=ROOT)
    result = cargo_assertion_results.validate(code, out, root=ROOT,
        sources=[SERVER_SOURCE], expected=SERVER_NAMES)
    if not result.valid or result.failed:
        print(out, flush=True)
    return (True, result.passed, result.failed) if result.valid else (False, 0, 0)


def build(pages=False):
    """Build actual source; no failed build is a mutation witness."""
    commands = []
    if pages:
        commands.append((["bash", "spikes/own-renderer/run.sh"], {"BUILD_ONLY": "1"}))
    commands.extend([
        (["bash", "spikes/own-renderer/todomvc.sh"], {}),
        (["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"], {}),
    ])
    for command, extra in commands:
        out, code = bounded(command, cwd=ROOT, env={**os.environ, **extra})
        if code != 0:
            print(out, flush=True)
            return False
    return True


def browser_tests(_what=None, pages=False):
    """Fresh complete assertion receipt, with the baseline's frozen IDs, `BROWSER_COUNT` of them."""
    if not build(pages):
        return False, 0, 0
    with tempfile.TemporaryDirectory(prefix="pw-todomvc-browser-") as directory:
        receipt = pathlib.Path(directory) / "result.json"
        nonce = secrets.token_hex(32)
        out, code = bounded(
            ["bash", str(ROOT / "scripts/run_suite.sh"), "-C", ".", "--",
             "pnpm", "exec", "playwright", "test", *SPEC, "--retries=0",
             "--reporter=line,./assertion-result-reporter.mjs"],
            cwd=ROOT / "spikes/own-renderer",
            env={**os.environ, "PW_ASSERTION_REPORT": str(receipt), "PW_ASSERTION_NONCE": nonce},
        )
        try:
            report = json.loads(receipt.read_text())
        except (OSError, ValueError):
            report = None
        result = browser_test_results.parse(code, report, nonce, INVENTORIES.get("browser"))
        if not result.valid or result.failed:
            print(out, flush=True)
        if not result.valid or result.passed + result.failed != BROWSER_COUNT:
            print(f"REFUSED browser producer: {result.reason or f'expected {BROWSER_COUNT} tests'}", flush=True)
            return False, 0, 0
        INVENTORIES.setdefault("browser", result.inventory)
        return True, result.passed, result.failed


def runtime_browser_tests(_what=None):
    """The browser's tests, over pages built with the runtime as it is."""
    return browser_tests(pages=True)


# What each kind of mutant is run against, in order.
RUNS = {
    PROGRAM: [("server", server_tests), ("browser", browser_tests)],
    HOST: [("browser", browser_tests)],
    RUNTIME: [("browser", runtime_browser_tests)],
}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    watch = mutation_baseline.mutation_bound.start()
    INVENTORIES.clear()
    for name, run in (("server", server_tests), ("browser", browser_tests)):
        valid, passed, failed = run()
        print(f"baseline ({name}): {passed} passed, {failed} failed", flush=True)
        if not valid or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    killed = refused = survivors = 0
    for what, kind, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})")
            refused += 1
            break
        verdict = None
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            if kind == PROGRAM:
                valid, clean, codes = checks()
                named = CHECKED.get(what, set())
                if not valid or watch.stopped:
                    verdict = "REFUSED (invalid or stopped checker; no kill)"
                elif not clean:
                    if named and codes == named:
                        verdict = f"KILLED by the checker ({', '.join(sorted(codes))}, the rule this mutant takes away)"
                    else:
                        verdict = f"REFUSED (unexpected checker diagnostics: {sorted(codes)})"
                elif named:
                    verdict = "SURVIVED (the checker accepts the named rule's mutant)"
            if verdict is None:
                for name, run in RUNS[kind]:
                    valid, passed, failed = run()
                    if not valid or not passed + failed or watch.stopped:
                        verdict = f"REFUSED (invalid or stopped {name} producer; no kill)"
                    elif failed:
                        verdict = f"KILLED by the {name}'s tests ({failed} of {passed + failed} tests fail)"
                    if verdict:
                        break
                else:
                    verdict = "SURVIVED"
        if verdict.startswith("KILLED"):
            killed += 1
        elif verdict.startswith("REFUSED"):
            refused += 1
        else:
            survivors += 1
        print(f"{what} [{kind}]: {verdict}", flush=True)
        if refused or watch.stopped:
            break
    # Rebuild and execute the full frozen suites on restored source. A failed
    # restoration check refuses the campaign even if every mutant was killed.
    if watch.stopped:
        print("REFUSED: memory guard stopped the campaign")
        return 1
    for name, run in (("server", server_tests), ("browser", runtime_browser_tests)):
        valid, passed, failed = run()
        print(f"restored ({name}): {passed} passed, {failed} failed", flush=True)
        if not valid or failed or not passed or watch.stopped:
            print(f"REFUSED: restored {name} producer is not green", flush=True)
            refused += 1
            break

    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors or killed != len(MUTANTS)))


if __name__ == "__main__":
    sys.exit(main())
