#!/usr/bin/env python3
"""Mutation controls for ADR-0187: nothing the store contains is on screen
when its page is first laid out.

Each mutant undoes one piece of the host's style for the store:
- the first items of a list contained, or the first lists;
- the lists' threshold dropped, so a menu of many short categories is never
  contained;
- no item contained at all;
- the placeholder back at 42 px;
- the style back in the body.

Every mutant must fail `e2e/stable-layout.spec.mjs`, in some engine, against
a build of the mutated server.

Run from the repository root; `just e14-stable-layout` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "the first items of a list are contained",
        SERVER,
        "#menu > ul > li:nth-child(n+29)",
        "#menu > ul > li:nth-child(n+1)",
    ),
    (
        "the first lists are contained",
        SERVER,
        "#menu > ul:nth-of-type(n+17) > li",
        "#menu > ul:nth-of-type(n+1) > li",
    ),
    (
        "a list far down the menu is not contained",
        SERVER,
        ", #menu > ul:nth-of-type(n+17) > li",
        "",
    ),
    (
        "no item is contained",
        SERVER,
        "{ content-visibility: auto;",
        "{ content-visibility: visible;",
    ),
    (
        "the placeholder is 42 px",
        SERVER,
        "contain-intrinsic-size: auto 7.75em;",
        "contain-intrinsic-size: auto 42px;",
    ),
    (
        "the style is in the body",
        SERVER,
        # Re-anchored by ADR-0220: the style is the data layer's; and by track
        # `device-store`, whose style follows it; and by ADR-0399,
        # whose boot links end the head.
        "{metadata}{style}{device}{BOOT}</head>\\n<body>\\n{body}\\n\\\n",
        "{metadata}{device}{BOOT}</head>\\n<body>\\n{body}\\n\\\n         {style}\\\n",
    ),
]

SPEC = ["e2e/stable-layout.spec.mjs"]

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
    """The server the spec runs, as the source says."""
    out, code = bounded(["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"], cwd=ROOT)
    return code == 0


def run_spec():
    """(built, passed, failed) for the spec in three engines, after a build."""
    if not build():
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
    built, passed, failed = run_spec()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
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
            if not built:
                verdict = "KILLED (does not build)"
            elif failed:
                verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
            else:
                verdict = "SURVIVED"
                survivors += 1
            print(f"{what}: {verdict}", flush=True)
    finally:
        # The server as the source says, whatever a mutant left built.
        build()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
