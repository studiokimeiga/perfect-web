#!/usr/bin/env python3
"""Mutation controls for ADR-0280: a handler navigates after its command
commits.

Each mutant undoes one piece:
- the compiler's: a navigation naming a page; its arguments related to the
  page's parameters, and the page looked for among the views; written last
  in the `Ok` arm of the command's answer nearest it, an answer bound by a
  `let` among them, and in no handler's body bare; and each value given by
  its parameter's name;
- the runtime's: an answer awaited, not assumed; an `Ok` that did not
  commit refused; a press made while the page leaves not taken; a second
  navigation not taken; the presses made before a navigation answered
  first, each counted while it runs and finished when it ends; a stopped
  navigation, and a page shown again, taking presses again; and an address
  that carries each value as one segment, or refuses it.

The tests of each must then fail: `compiler/pw-core/tests/navigate.rs` for
the compiler's, and `e2e/navigate.spec.mjs` in Chromium for the runtime's.

Run from the repository root; `just e14-navigate` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

COMPILER = "compiler"
BROWSER = "browser"

# (what is undone, which tests, file, anchor, replacement)
MUTANTS = [
    (
        "a navigation names whatever it names",
        COMPILER,
        CHECK,
        "            if kind == Some(DeclKind::Page) {\n",
        "            if true {\n",
    ),
    (
        "its arguments are related to nothing",
        COMPILER,
        VALUES,
        # Re-anchored by the scheduled-commands track: a schedule's arguments
        # are related there too.
        '                "query" | "subscription" | "navigate" | "schedule" | "unschedule"\n',
        '                "query" | "subscription" | "schedule" | "unschedule"\n',
    ),
    (
        "its page is looked for among the terms",
        COMPILER,
        VALUES,
        '                        "navigate" => Namespace::Ui,\n',
        '                        "navigate" => Namespace::Term,\n',
    ),
    (
        "a refusal's arm navigates",
        COMPILER,
        CHECK,
        "                    true => is_ok(body, arm.pat),\n",
        "                    true => true,\n",
    ),
    (
        "what follows a navigation runs",
        COMPILER,
        CHECK,
        "                after_a_commit(body, is_answer, *s, last && i + 1 == stmts.len(), allowed);\n",
        "                after_a_commit(body, is_answer, *s, last, allowed);\n",
    ),
    (
        "a handler navigates with no answer",
        COMPILER,
        CHECK,
        "            after_a_commit(body, &is_answer, start, false, &mut allowed);\n",
        "            after_a_commit(body, &is_answer, start, true, &mut allowed);\n",
    ),
    (
        "an answer a `let` binds is no answer",
        COMPILER,
        CHECK,
        "                || matches!(lexical.binder(e), Some(Binder::Pattern(p)) if answers.contains(&p))\n",
        "                || false\n",
    ),
    (
        "a value is given by no name",
        COMPILER,
        LOWER,
        "                Lowering::Lowered(v) => given.push((param.name.clone(), v)),\n",
        "                Lowering::Lowered(v) => given.push((String::new(), v)),\n",
    ),
    (
        "an answer is assumed before it comes",
        BROWSER,
        RUNTIME,
        "          answer = await command(component, args, interaction, how.retry);\n",
        "          command(component, args, interaction, how.retry);\n"
        "          answer = { committed: true, result: { $case: \"ok\" } };\n",
    ),
    (
        "an `Ok` that did not commit is a commit",
        BROWSER,
        RUNTIME,
        '        if (answer.committed !== true && answer.result?.$case !== "err") {\n',
        "        if (false) {\n",
    ),
    (
        "a press made while the page leaves is taken",
        BROWSER,
        RUNTIME,
        "  if (navigating) {\n"
        "    REPORT: log.push(`press on ${part.value} not taken: the page is leaving`);\n",
        "  if (false) {\n"
        "    REPORT: log.push(`press on ${part.value} not taken: the page is leaving`);\n",
    ),
    (
        "a second navigation is taken",
        BROWSER,
        RUNTIME,
        # Re-anchored by ADR-0306: one rule for both ways a
        # page leaves, which the store's pages, shown in a layout, reach softly.
        "  if (navigating && !(chosen && pending)) {\n"
        "    REPORT: log.push(`navigation to ${url} not taken: the page is already leaving`);\n",
        "  if (false) {\n"
        "    REPORT: log.push(`navigation to ${url} not taken: the page is already leaving`);\n",
    ),
    (
        "a navigation goes before the presses made before it",
        BROWSER,
        RUNTIME,
        # And one wait for the presses before it, for both.
        "  return Promise.allSettled([...pressing].filter((p) => p !== mine));\n",
        "  return Promise.allSettled([]);\n",
    ),
    (
        "a press is not counted while it runs",
        BROWSER,
        RUNTIME,
        "  pressing.add(done);\n",
        "",
    ),
    (
        "a press never finishes",
        BROWSER,
        RUNTIME,
        "    pressing.delete(done);\n    finished();\n",
        "    pressing.delete(done);\n",
    ),
    (
        "a stopped navigation leaves the page leaving",
        BROWSER,
        RUNTIME,
        '      (e) => e.signal.addEventListener("abort", () => (navigating = false), { once: true }),\n',
        "      () => {},\n",
    ),
    (
        "a page shown again stays leaving",
        BROWSER,
        RUNTIME,
        "    if (!e.persisted) return;\n    navigating = false;\n",
        "    if (!e.persisted) return;\n",
    ),
    (
        "an address carries a value as written",
        BROWSER,
        RUNTIME,
        '    out += unreserved ? String.fromCharCode(b) : "%" + b.toString(16).toUpperCase().padStart(2, "0");\n',
        "    out += String.fromCharCode(b);\n",
    ),
    (
        "an address carries what no segment can",
        BROWSER,
        RUNTIME,
        '    if (value === "" || value === "." || value === "..") {\n',
        "    if (false) {\n",
    ),
]

# How long one command may run. Past it, it and what it started are stopped.
BOUND = 1800


def bounded(cmd, cwd=ROOT, env=None):
    """(exit status or None past the bound, output) of one command."""
    p = subprocess.Popen(
        cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        start_new_session=True,
    )
    try:
        out, _ = p.communicate(timeout=BOUND)
        return p.returncode, out
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return None, out


def compiler_tests():
    """(built, passed, failed) of the compiler's tests."""
    _, out = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "navigate"])
    results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not results:
        return False, 0, 0
    return True, sum(int(p) for p, _ in results), sum(int(f) for _, f in results)


def browser_tests():
    """(built, passed, failed), after a build: the store's pages, which serve
    the runtime they were built with, and the server the suite runs."""
    code, _ = bounded(
        ["bash", "spikes/own-renderer/run.sh"], env={**os.environ, "BUILD_ONLY": "1"}
    )
    if code != 0:
        return False, 0, 0
    code, _ = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"])
    if code != 0:
        return False, 0, 0
    code, out = bounded(
        [
            "pnpm", "exec", "playwright", "test", "e2e/navigate.spec.mjs",
            "--project=chromium", "--reporter=line",
        ],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


RUN = {COMPILER: compiler_tests, BROWSER: browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: a stop is an
    # exception here, which the `finally` below meets.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    for group, run in RUN.items():
        built, passed, failed = run()
        print(f"baseline ({group}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, group, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = RUN[group]()
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{group}]: {verdict}", flush=True)
    # The pages are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
