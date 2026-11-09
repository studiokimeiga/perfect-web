#!/usr/bin/env python3
"""Mutation controls for the longer-holds ADR's browser half: which pages ask
to be held long.

Each mutant undoes one piece of the runtime: every tab asking on HTTP/1.1, no
tab asking, HTTP/2 not recognised, the long hold not handed on when a tab
closes, and either transport not saying so. Each must fail
`e2e/longer-holds.spec.mjs` in Chromium.

Run from the repository root; `just e14-longer-holds` records the output. The
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

# How long one command may run; past it, the run counts as failing.
BOUND = 900

ASK = "  askToHoldLong();\n"
PROTOCOL = "  if (/^h[23]/.test(protocol)) {\n"
REQUEST = '    ?.request("pw-subscription", { signal: life.signal }, () => {\n'

# (what, file, anchor, replacement)
MUTANTS = [
    ("every tab of an HTTP/1.1 origin is held long", RUNTIME,
     "function askToHoldLong() {\n", 'function askToHoldLong() {\n  holdLong = "&hold=long";\n'),
    ("no tab is held long", RUNTIME, ASK, ""),
    ("HTTP/2 is not recognised", RUNTIME, PROTOCOL, "  if (false) {\n"),
    ("the long hold is not handed on", RUNTIME, REQUEST,
     '    ?.request("pw-subscription", { ifAvailable: true }, (lock) => {\n      if (!lock) return;\n'),
    ("a stream does not say so", RUNTIME,
     "&since=${cursor}&mode=stream${holdLong}`", "&since=${cursor}&mode=stream`"),
    ("a long poll does not say so", RUNTIME,
     "/stream?doc=${documentCursor}&since=${cursor}${holdLong}`", "/stream?doc=${documentCursor}&since=${cursor}`"),
]


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


def browser():
    _, code = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT, env={**os.environ, "BUILD_ONLY": "1"})
    if code != 0:
        return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", "e2e/longer-holds.spec.mjs", "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer")
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
    built, passed, failed = browser()
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
            built, passed, failed = browser()
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
    # What was built from a mutant is built again from the restored source.
    browser()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
