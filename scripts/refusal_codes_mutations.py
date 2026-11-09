#!/usr/bin/env python3
"""Mutation controls for ADR-0400: the browser's decision
carries none of its trace's words, and a refused handler is told in the
console by its code and the page that explains it.

Each mutant undoes one piece:
- the decision's WebAssembly built with its trace's words, as `pw-resume`
  builds by default;
- a refusal not told in the console;
- a refusal told without its page;
- a refusal told by a code its page does not have.

The WebAssembly's mutant must fail `e2e/runtime-size.spec.mjs`; a runtime
mutant, the refused handler's test in `e2e/lazy-handler.spec.mjs`. Each runs
in Chromium, against a build of the mutated source.

Run from the repository root; `just e14-refusal-codes` records the output.
The source is restored after every mutant, whatever happens, and the build
is made again from it.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DECISION = ROOT / "runtime/pw-resume-wasm/Cargo.toml"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "the decision's WebAssembly keeps its trace's words",
        "size",
        DECISION,
        'pw-resume = { path = "../pw-resume", default-features = false }\n',
        'pw-resume = { path = "../pw-resume" }\n',
    ),
    (
        "a decision keeps what it allocated",
        "decision",
        ROOT / "runtime/pw-resume-wasm/src/browser.rs",
        "            HEAP.next.set(self.0);\n",
        "            let _ = self.0;\n",
    ),
    (
        "the decision's WebAssembly carries a sort for its dependencies",
        "decision",
        ROOT / "runtime/pw-resume/src/lib.rs",
        '''        DependencySet(Hash::of(&canonical.join("\\u{2}")))\n''',
        '''        canonical.sort();\n        DependencySet(Hash::of(&canonical.join("\\u{2}")))\n''',
    ),
    (
        "a refusal is not told in the console",
        "refused",
        RUNTIME,
        "    console.warn(`Pleris: handler ${named} not attached: ${explained(verdict.code)}, recovery ${verdict.recovery}`);\n",
        "",
    ),
    (
        "a refusal is told without its page",
        "refused",
        RUNTIME,
        '  return code ? `${name}, https://pleris.dev/e/${name}` : "no decision";\n',
        '  return code ? name : "no decision";\n',
    ),
    (
        "a refusal is told by a code its page does not have",
        "refused",
        RUNTIME,
        '  const name = `PWR${String(code).padStart(2, "0")}`;\n',
        "  const name = `PWR${code}`;\n",
    ),
]

SUITES = {
    "size": ["e2e/runtime-size.spec.mjs", "-g", "none of its trace", "--project=chromium", "--workers=1"],
    "decision": ["e2e/runtime-size.spec.mjs", "-g", "keeps nothing", "--project=chromium", "--workers=1"],
    "refused": ["e2e/lazy-handler.spec.mjs", "-g", "did not compile is refused", "--project=chromium"],
}

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


def build():
    """The page, its runtime and its WebAssembly, and the server, as the source says."""
    _, page = bounded(["bash", "spikes/own-renderer/run.sh"], cwd=ROOT,
                      env={**os.environ, "BUILD_ONLY": "1"})
    _, server = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    return page == 0 and server == 0


def run_suite(suite):
    """(passed, failed) for one suite in Chromium, against what is built."""
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SUITES[suite], "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    # A source that does not build is a baseline that is not green.
    built = build()
    for suite in SUITES:
        passed, failed = run_suite(suite) if built else (0, 0)
        print(f"baseline {suite}: {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no control can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    try:
        for what, suite, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1:
                print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
                survivors += 1
                continue
            try:
                path.write_text(original.replace(anchor, replacement, 1))
                built = build()
                passed, failed = run_suite(suite) if built else (0, 0)
            finally:
                path.write_text(original)
            # A mutant that does not build tests nothing: it counts against
            # the run, not for it.
            if not built:
                verdict = "DID NOT BUILD, so it shows nothing"
                survivors += 1
            elif failed:
                verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            else:
                verdict = "SURVIVED"
                survivors += 1
            print(f"{what}: {verdict}", flush=True)
    finally:
        # The page, its runtime and the server as the source says, whatever a
        # mutant left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
