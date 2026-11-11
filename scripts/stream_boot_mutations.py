#!/usr/bin/env python3
"""Mutation controls for ADR-0272: a region the browser fills while the
runtime boots is bound.

Each mutant undoes one piece of the browser runtime: the regions pending
when the page is first indexed not recorded; one that settled while the
runtime booted not read again and bound; and not reported. Each must fail
`e2e/stream.spec.mjs`'s boot test in Chrome, which holds the runtime's boot
at the network until Chrome has filled the region.

Run from the repository root; `just e14-stream-boot` records the output. The
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
        "the regions pending when the page is indexed are not recorded",
        RUNTIME,
        "    if (p.kind === \"stream\" && streamPending(String(p.id))) pendingAtIndex.add(String(p.id));\n",
        "",
    ),
    (
        "a region that settled while the runtime booted is not bound",
        RUNTIME,
        "    buildIndex();\n    bindEvents();\n  }\n  if (pendingStreams.size === 0) return;\n",
        "  }\n  if (pendingStreams.size === 0) return;\n",
    ),
    (
        "a region that settled while the runtime booted is not reported",
        RUNTIME,
        "    REPORT: for (const id of whileBooting) window.__pw.settled.push({ part: Number(id), by: \"browser\" });\n",
        "",
    ),
]

SPECS = [
    [
        "e2e/stream.spec.mjs",
        "-g",
        "while the runtime boots|Chrome 150 and later",
        "--project=chromium",
    ],
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
    """(built, passed, failed), after a build: the page, which serves the
    runtime it was built with, and the server the suite runs, which
    `run.sh` does not build."""
    for script in ["run.sh"]:
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
