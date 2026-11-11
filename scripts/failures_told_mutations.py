#!/usr/bin/env python3
"""Mutation controls for ADR-0431: a failure the runtime recovers from is told
the developer as the platform tells an error nothing caught (`reportError`),
and a condition it names, which the reader is told, is not.

Each mutant undoes one piece: a failure of the program, its build or the
runtime told no developer (a press that failed, the page's module, a
speculation's module, a rollback, a navigation in place, the next runtime, a
boot); a condition the reader is told by name told the developer too (a
refusal, no answer); or a failure the browser already tells told twice (the
page's computed values). Each must fail the spec that drives it, in Chromium:
on the demos and the feed as developed, and on TodoMVC as served for a boot.

Run from the repository root; `just e14-failures-told` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# The specs each suite runs, and the tests of them that drive a failure.
DEVELOPED = [
    "e2e/lazy-handler.spec.mjs",
    "e2e/argument-invariants.spec.mjs",
    "e2e/identity.spec.mjs",
    "e2e/computed-modules.spec.mjs",
    "e2e/page-bundle.spec.mjs",
    "e2e/press-modules.spec.mjs",
    "e2e/soft-navigation.spec.mjs",
    "e2e/feed.spec.mjs",
]
FAILURES = (
    "fails to load is visible|altered on the way|signed out in another tab is refused where"
    "|no answer comes for is told|computed values module that failed|bundle that does not load"
    "|sent unspeculated|next runtime that does not load|cannot be shown in place"
    "|a rollback that cannot render"
)
SERVED = ["e2e/device-runtime.spec.mjs"]
BOOT = "a boot that fails"

PRESS_FAILED = (
    "      reportError(error);\n"
    "    }\n"
    "    REPORT: log.push(`handler ${part.value} failed: ${error.message ?? error}`);\n"
)

# (what, suite, file, anchor, replacement)
MUTANTS = [
    ("a press that failed is told no developer", "developed", RUNTIME,
     PRESS_FAILED, PRESS_FAILED.replace("      reportError(error);\n", "", 1)),
    ("a refusal is told the developer", "developed", RUNTIME,
     "      tell(el, error.says || PLATFORM_SAYS.failed, `refused:${error.predicate}`);\n",
     "      tell(el, error.says || PLATFORM_SAYS.failed, `refused:${error.predicate}`);\n      reportError(error);\n"),
    ("no answer is told the developer", "developed", RUNTIME,
     '      tell(el, PLATFORM_SAYS.unreachable, "unreachable");\n',
     '      tell(el, PLATFORM_SAYS.unreachable, "unreachable");\n      reportError(error);\n'),
    ("the page's module that did not load is told no developer", "developed", RUNTIME,
     "    reportError(error);\n    throw error;\n", "    throw error;\n"),
    ("a press sent unspeculated is told no developer", "developed", RUNTIME,
     "    reportError(error);\n    return [];\n", "    return [];\n"),
    ("a rollback that failed is told no developer", "developed", RUNTIME,
     "    REPORT: log.push(`speculation rollback failed: ${error.message ?? error}`);\n    reportError(error);\n",
     "    REPORT: log.push(`speculation rollback failed: ${error.message ?? error}`);\n"),
    ("a navigation that failed in place is told no developer", "developed", RUNTIME,
     "    REPORT: log.push(`navigation to ${at} failed in place: ${e}`);\n    reportError(e);\n",
     "    REPORT: log.push(`navigation to ${at} failed in place: ${e}`);\n"),
    ("a next runtime that did not load is told no developer", "developed", RUNTIME,
     "    REPORT: log.push(`the next runtime did not load: ${e}`);\n    reportError(e);\n",
     "    REPORT: log.push(`the next runtime did not load: ${e}`);\n"),
    ("computed values that did not load are told twice", "developed", RUNTIME,
     "      REPORT: log.push(`computed values not loaded: ${error.message ?? error}`);\n      throw error;\n",
     "      REPORT: log.push(`computed values not loaded: ${error.message ?? error}`);\n      reportError(error);\n      throw error;\n"),
    ("a boot that failed is told no developer", "served", RUNTIME,
     "  REPORT: log.push(`boot failed: ${e}`);\n  reportError(e);\n",
     "  REPORT: log.push(`boot failed: ${e}`);\n"),
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


def suite(builds, specs, grep, env):
    """(built, passed, failed): each build made from the runtime as it is
    now, which it copies, then the specs' tests that drive a failure."""
    for script in builds:
        _, code = bounded(["bash", f"spikes/own-renderer/{script}"], cwd=ROOT, env=env)
        if code != 0:
            return False, 0, 0
    out, code = bounded(["pnpm", "exec", "playwright", "test", *specs, "--grep", grep,
                         "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer", env=env)
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


def developed():
    """The demos and the feed as developed, which report."""
    env = {**os.environ, "BUILD_ONLY": "1"}
    return suite(["run.sh", "feed.sh"], DEVELOPED, FAILURES, env)


def served():
    """TodoMVC as it is served: a production build, which reports nothing."""
    env = {**os.environ, "PW_PRODUCTION_ASSETS": "1"}
    return suite(["todomvc.sh"], SERVED, BOOT, env)


SUITES = {"developed": developed, "served": served}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built = subprocess.run(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"],
                           cwd=ROOT, capture_output=True, text=True)
    if built.returncode != 0:
        print("FAIL: the servers the suites run do not build")
        return 1
    for name, run in SUITES.items():
        ok, passed, failed = run()
        print(f"baseline ({name}): {passed} passed, {failed} failed", flush=True)
        if not ok or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, name, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            ok, passed, failed = SUITES[name]()
        finally:
            path.write_text(original)
        if not ok:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{name}]: {verdict}", flush=True)
    # What was built from a mutant is built again from the restored source.
    for run in SUITES.values():
        run()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
