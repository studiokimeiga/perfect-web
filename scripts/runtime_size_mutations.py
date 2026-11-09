#!/usr/bin/env python3
"""Negative controls for ADR-0188: a page's runtime is bounded as it is sent.

The control grows what a page downloads by bytes that do not compress: the
runtime's script, by a comment of 48 KB of random text, which gate item 7b must
refuse. And a page that fetches a renderer again must fail gate item 7d.

A bound that a grown file passes bounds nothing. Every control must fail
`e2e/runtime-size.spec.mjs`, against a build of the grown source.

Run from the repository root; `just e14-runtime-size` records the output.
The source is restored after every control, whatever happens, and the build
is made again from it.
"""

import base64
import os
import pathlib
import random
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
# Seeded, so a run is the same run.
NOISE = random.Random(188)
SCRIPT_NOISE = base64.b64encode(NOISE.randbytes(36 * 1024)).decode()

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "the runtime's script grows by 48 KB that do not compress",
        RUNTIME,
        "// E7-R — the browser runtime.\n",
        f"// E7-R — the browser runtime.\n// {SCRIPT_NOISE}\n",
    ),
    (
        "a block fetches a file to be rendered with",
        RUNTIME,
        "async function renderBlock(id) {\n",
        "async function renderBlock(id) {\n"
        '  await fetch("/pw-render.wasm");\n',
    ),
]

SPEC = ["e2e/runtime-size.spec.mjs", "--project=chromium", "--workers=1"]

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


def run_spec():
    """(built, passed, failed) for the spec in Chromium, after a build."""
    if not build():
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
        env={**os.environ, "PW_PERFORMANCE": "1"},
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
    built, passed, failed = run_spec()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no control can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    try:
        for what, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1:
                print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
                survivors += 1
                continue
            try:
                path.write_text(original.replace(anchor, replacement, 1))
                built, passed, failed = run_spec()
            finally:
                path.write_text(original)
            # A control that does not build tests no bound: it counts
            # against the run, not for it.
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
        # control left built.
        build()
        PAD.unlink(missing_ok=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
