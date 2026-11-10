#!/usr/bin/env python3
"""Mutation controls for ADR-0416: a build's stylesheets
draw with their own images.

Each mutant undoes one piece: the build's stylesheets not read, every build
allowed, a mention of the words taken for an image, a quote or a space read as
part of one, an upper-case one missed, and the policy not told. Each must fail
`tests::page_policy::`.

Run from the repository root; `just e14-style-images` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
CSP = ROOT / "spikes/own-renderer/server/src/csp.rs"

# How long one command may run; past it, the run counts as failing.
BOUND = 900

# (what, file, anchor, replacement)
MUTANTS = [
    ("any file counts as a stylesheet", SERVER,
     '        .filter(|path| path.extension().is_some_and(|x| x == "css"))\n', ""),
    ("a mention of the words is an image", SERVER,
     '    css.match_indices("url(").any(|(at, _)| {\n',
     '    css.contains("data:image/") || css.match_indices("url(").any(|(at, _)| {\n'),
    ("an image in upper case is missed", SERVER,
     "fn draws_with_data(css: &str) -> bool {\n    let css = css.to_ascii_lowercase();\n",
     "fn draws_with_data(css: &str) -> bool {\n"),
    ("a space before an image is its first character", SERVER,
     '        css[at + "url(".len()..]\n            .trim_start()\n',
     '        css[at + "url(".len()..]\n'),
    ("a quote is part of an image", SERVER,
     """            .trim_start_matches(['"', '\\''])\n            .starts_with("data:image/")\n""",
     """            .starts_with("data:image/")\n"""),
    ("the policy allows them for every build", CSP,
     """        if needs.data_images {\n            "img-src 'self' data:"\n""",
     """        if true {\n            "img-src 'self' data:"\n"""),
    ("the policy never allows them", CSP,
     """        if needs.data_images {\n            "img-src 'self' data:"\n""",
     """        if false {\n            "img-src 'self' data:"\n"""),
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


def host():
    out, code = bounded(["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server", "tests::page_policy::"], cwd=ROOT)
    if code is None:
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(p) for p, _ in found), sum(int(f) for _, f in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = host()
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
            built, passed, failed = host()
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
