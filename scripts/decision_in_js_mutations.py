#!/usr/bin/env python3
"""Mutation controls for ADR-0414: every page decides in its runtime.

The decision's own lines are held to `pw-resume` by
`decision-agreement.test.mjs`, and `device_runtime_mutations.py` undoes each of
them. Each mutant here undoes what this ADR adds around them:
- a page's use of the decision: none made, the table not heard, the page's own
  word not used;
- the head: asking for a module no build holds;
- a page's address: the module served after all.

A mutant of the runtime must fail, in Chromium, the browser specs of a page that
decides; of the host, its tests.

Run from the repository root; `just e14-decision-in-js` records the output. The
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
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

SPECS = ["e2e/lazy-handler.spec.mjs", "e2e/recovery.spec.mjs", "e2e/runtime-size.spec.mjs", "e2e/warm.spec.mjs"]

# (what, suite, file, anchor, replacement)
MUTANTS = [
    ("a page decides nothing, and everything attaches", "browser", RUNTIME,
     "  decide = decider(lines, PAGE);\n",
     '  decide = () => ({ attach: true, recovery: "reload", code: 0 });\n'),
    ("no decision is made, and nothing attaches", "browser", RUNTIME,
     "  decide = decider(lines, PAGE);\n", ""),
    ("a page's decision is another page's", "browser", RUNTIME,
     "  decide = decider(lines, PAGE);\n", '  decide = decider(lines, "another");\n'),
    ("the table is not heard", "browser", RUNTIME,
     '  const lines = heardTable(\n',
     '  const lines = ((t) => t.split("\\n"))(\n'),
    ("a table that cannot be read is believed", "browser", RUNTIME,
     '    PW_ALONE ? tableAlone() : await fetch(delivered("/pw-handlers")).then((r) => (r.ok ? r.text() : "")),\n',
     '    PW_ALONE ? tableAlone() : await fetch(delivered("/pw-handlers")).then((r) => r.text()),\n'),
    ("the head asks for a module no build holds", "host", SERVER,
     'const BOOT: &str = "<link rel=\\"modulepreload\\" href=\\"/pw-runtime.mjs\\">\\n\\\n',
     'const BOOT: &str = "<link rel=\\"modulepreload\\" href=\\"/pw-runtime.mjs\\">\\n\\\n'
     '     <link rel=\\"preload\\" href=\\"/pw-resume.wasm\\" as=\\"fetch\\" crossorigin>\\n\\\n'),
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
    out, code = bounded(["pnpm", "exec", "playwright", "test", *SPECS, "--project=chromium", "--reporter=line"],
                        cwd=ROOT / "spikes/own-renderer")
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


def host():
    out, code = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
                         "a_document_preloads_what_its_runtime_boots_with"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


SUITES = {"browser": browser, "host": host}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite, run in SUITES.items():
        built, passed, failed = run()
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
            built, passed, failed = SUITES[suite]()
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
    # What was built from a mutant is built again from the restored source.
    browser()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
