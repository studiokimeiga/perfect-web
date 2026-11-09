#!/usr/bin/env python3
"""Mutation controls for ADR-0155: the page keeps its subscription, and
recovers a refused handler.

Each mutant undoes one piece of the browser runtime:
- a failed subscription request ends the subscription, as it did;
- a press on a refused handler does nothing, as it did;
- a region of private state is told to refetch, as it was.

The browser tests that drop a connection and press a stale handler must
then fail, in Chromium.

Run from the repository root; `just e14-runtime-recovery` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a failed subscription request ends the subscription",
        RUNTIME,
        "    } catch {\n      if (leaving || life.signal.aborted) return;\n      failures += 1;\n",
        "    } catch {\n      return;\n      failures += 1;\n",
    ),
    (
        "a press on a refused handler does nothing",
        RUNTIME,
        "    recoverOnPress(part, owners, verdict);\n",
        "    void recoverOnPress;\n",
    ),
    (
        "a region of private state is told to refetch",
        RUNTIME,
        "          : scope === \"public\"\n            ? \"refetch-region\"\n            : \"rerender-private-slot\";\n",
        "          : \"refetch-region\";\n",
    ),
]

TESTS = ["e2e/transport.spec.mjs", "e2e/recovery.spec.mjs"]
GREP = "drops|another build|still stale"


def run_tests():
    """(built, passed, failed) for the recovery tests, after a build."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    r = subprocess.run(
        ["pnpm", "exec", "playwright", "test", *TESTS, "--project=chromium", "-g", GREP,
         "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
        capture_output=True,
        text=True,
    )
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


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
    # The page is built again from the restored source.
    run_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
