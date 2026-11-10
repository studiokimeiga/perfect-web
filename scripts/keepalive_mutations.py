#!/usr/bin/env python3
"""Mutation controls for ADR-0268: a command outlives the page that sent it.

Each mutant undoes one piece of the browser runtime's `command`: the request
not kept alive; the budget not read; the bytes in flight not counted, or not
uncounted when a request is answered or fails; a body counted in characters
rather than bytes; and `?keepalive=` raising the budget past the Fetch
standard's 64 KiB, or an unreadable one keeping it whole. Each must fail
`e2e/keepalive.spec.mjs`, or the feed's post counted by its bytes, in
Chromium.

Run from the repository root; `just e14-keepalive` records the output. The
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

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a command's request is not kept alive",
        RUNTIME,
        "        keepalive,\n",
        "        keepalive: false,\n",
    ),
    (
        "the budget is not read",
        RUNTIME,
        "    const keepalive = keptAlive + bytes <= keepaliveBudget;\n",
        "    const keepalive = true;\n",
    ),
    (
        "the bytes in flight are not counted",
        RUNTIME,
        "    if (keepalive) keptAlive += bytes;\n",
        "",
    ),
    (
        # Re-anchored by ADR-0305: an answer that came early is read first.
        "an answered request stays counted",
        RUNTIME,
        "    if (keepalive) keptAlive -= bytes;\n    if (response.status === 409) {\n",
        "    if (response.status === 409) {\n",
    ),
    (
        # Re-anchored by ADR-0302: a request with no answer is thrown as
        # unreachable. And by ADR-0381: a send held for the one before it is
        # told apart first.
        "a failed request stays counted",
        RUNTIME,
        "      if (keepalive) keptAlive -= bytes;\n"
        "      // **Held for the one before it, not lost** (ADR-0381): the host runs a\n",
        "      // **Held for the one before it, not lost** (ADR-0381): the host runs a\n",
    ),
    (
        "a body is counted in characters",
        RUNTIME,
        "  const bytes = new Blob([sent]).size;\n",
        "  const bytes = sent.length;\n",
    ),
    (
        "`?keepalive=` raises the budget past 64 KiB",
        RUNTIME,
        "  : Math.min(64 * 1024, Number(",
        "  : Math.min(Infinity, Number(",
    ),
    (
        "an unreadable `?keepalive=` keeps the whole budget",
        RUNTIME,
        '.get("keepalive") ?? 64 * 1024) || 0);\n',
        '.get("keepalive") ?? 64 * 1024) || 64 * 1024);\n',
    ),
]

SPECS = [
    ["e2e/keepalive.spec.mjs", "--project=chromium"],
    ["e2e/feed.spec.mjs", "-g", "kept alive, counted by its bytes", "--project=chromium"],
]

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
    """(built, passed, failed), after a build: the store's page and the
    feed's, each of which serves the runtime it was built with, and the
    server the suite runs, which neither script builds."""
    for script in ["run.sh", "feed.sh"]:
        built = subprocess.run(
            ["bash", f"spikes/own-renderer/{script}"],
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
    passed = failed = 0
    for spec in SPECS:
        out, code = bounded(
            ["pnpm", "exec", "playwright", "test", *spec, "--reporter=line"],
            cwd=ROOT / "spikes/own-renderer",
        )
        if code is None:
            failed += 1
            continue
        out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
        passed += sum(int(n) for n in re.findall(r"(\d+) passed", out))
        failed += sum(int(n) for n in re.findall(r"(\d+) failed", out))
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
    # The pages and the server are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
