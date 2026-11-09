#!/usr/bin/env python3
"""Mutation controls for the soft navigation's ADR: a navigation keeps the
layout and shows the next page in it.

Each mutant undoes one piece of the runtime's navigation:
- which navigations are kept: a link's, a handler's after its commit, back
  and forward's;
- what is held to the page: the same build, a layout at all;
- the swap: the page's markup in the slot, the layout's regions taken from
  the next document, focus where the page begins;
- the hand-over: the old runtime ended;
- what a leaving page does: the frames it is told applied to nothing, a
  reader's next choice taken over the one in flight;
- the back-forward cache: a page shown again listening again.

Each must fail `e2e/soft-navigation.spec.mjs` in Chromium. Run from the
repository root; `just e14-soft-navigation` records the output. The source
is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "every link loads its page whole",
        RUNTIME,
        '    softNavigate(url.href, "push", undefined, true);\n',
        "    location.assign(url.href);\n",
    ),
    (
        "a handler's navigation after its commit loads its page whole",
        RUNTIME,
        '  if (PW_SERVER && parts.layout) return softNavigate(url, "push", mine);\n',
        "",
    ),
    (
        "back and forward read nothing",
        RUNTIME,
        '    softNavigate(location.href, "pop", undefined, true);\n',
        "",
    ),
    (
        "a page of another build is kept",
        RUNTIME,
        '  if (!response.ok || !servedBuild || response.headers.get("pw-build") !== servedBuild) {\n',
        "  if (!response.ok) {\n",
    ),
    (
        "the page's markup is not shown in the slot",
        RUNTIME,
        "  here.end.before(page);\n",
        "",
    ),
    (
        "the layout's regions are not taken from the next document",
        RUNTIME,
        "    if (markupBetween(own.start, own.end) === markupBetween(theirs.start, theirs.end)) continue;\n",
        "    continue;\n",
    ),
    (
        "focus is left where it was",
        RUNTIME,
        "    target.focus({ preventScroll: true });\n",
        "",
    ),
    (
        "the old runtime goes on after handing the page over",
        RUNTIME,
        # Re-anchored by ADR-0399 and its review: what the old runtime's
        # renderer holds is let go after it.
        "  life.abort();\n  // What its renderer holds let go: this module stays in the module map\n",
        "  // What its renderer holds let go: this module stays in the module map\n",
    ),
    (
        "what a leaving page is told is applied",
        RUNTIME,
        "  if (life.signal.aborted || navigating) return;\n",
        "  if (life.signal.aborted) return;\n",
    ),
    (
        "a reader's next choice waits for the navigation in flight",
        RUNTIME,
        "  if (navigating && !(chosen && pending)) {\n",
        "  if (navigating) {\n",
    ),
    (
        "a page shown again from the back-forward cache does not listen",
        RUNTIME,
        "      leaving = false;\n      subscribe();\n",
        "      leaving = false;\n",
    ),
    (
        "a page with no layout is shown in this one",
        RUNTIME,
        "    !theirs ||\n",
        "    false ||\n",
    ),
]

SPEC = ["e2e/soft-navigation.spec.mjs", "--project=chromium"]

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


def browser_tests():
    """(built, passed, failed), after a build: the pages, which carry the
    runtime they were built with, and the server the suite runs."""
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
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"],
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


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = browser_tests()
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
            built, passed, failed = browser_tests()
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
    # The pages are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
