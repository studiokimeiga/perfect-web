#!/usr/bin/env python3
"""Mutation controls for track `todomvc`'s `autofocus` (ADR-0367): an element
a block shows with `autofocus`, after the page has loaded, takes focus as it
appears (the runtime's `followAutofocus`), as TodoMVC's edit field does.

Each mutant undoes one piece of the runtime:
- the focusing itself (an element that asks for focus is not given it);
- the watching (what is put into the page is not looked at);
- the asking (an element that does not ask for focus is given it).

Each must fail `e2e/focus.spec.mjs` over `examples/demo/focus.pw`, in Chromium.
What only TodoMVC's own page shows (an `autofocus` element inside the row a
patch inserts) is controlled in `scripts/todomvc_mutations.py`.

Run from the repository root; `just e14-autofocus` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "an element that asks for focus is not given it",
        "browser",
        RUNTIME,
        "    if (el && document.activeElement !== el) el.focus();\n",
        "    if (el && document.activeElement !== el) void 0;\n",
    ),
    (
        "what is put into the page is not looked at",
        "browser",
        RUNTIME,
        "  // What a block shows from here on, with `autofocus`, takes focus.\n  if (PW_AUTOFOCUS) followAutofocus();\n",
        "",
    ),
    (
        "an input that does not ask for focus is given it",
        "browser",
        RUNTIME,
        'node.matches("[autofocus]") ? [node]',
        'node.matches("input") ? [node]',
    ),
]

SPEC = ["e2e/focus.spec.mjs", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, **kw):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def browser_tests(_suite="browser"):
    """(built, passed, failed), after a build: the pages the demo serves
    beside the store, and the servers the suite starts."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    server = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if server.returncode != 0:
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


SUITES = {
    "browser": browser_tests,
}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = SUITES[suite](suite)
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
    # The pages and the server are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
