#!/usr/bin/env python3
"""Mutation controls for ADR-0245's amendment of 2026-11-18: a campaign's
bundle is kept while a record names it (`evidence_bundles.py`), and a fetch
prunes the bundles its evidence superseded (`evidence_fetch.py`).

Each mutant undoes one piece:
- an unnamed bundle is kept, or the check passes it;
- a bundle's own files keep what they list, or any line of a text record
  names a bundle;
- a document outside the evidence (an ADR citing a bundle) names nothing;
- a bundle git does not track (a run here) can go;
- a bundle is any directory its template begins, or of more letters than
  its X's, or a file; a recipe the justfile imports, or a quoted template,
  names none;
- a fetch prunes nothing, prunes before it copies, prunes having copied
  nothing, prunes what it refused, or prunes fetching times alone.

Each runs `scripts/tests/test_evidence_bundles.py`'s `Bundles` and `Fetch`;
at least one test must fail.

Run from the repository root; `just e14-evidence-bundles` records the
output. The sources are restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUNDLES = ROOT / "scripts/evidence_bundles.py"
FETCH = ROOT / "scripts/evidence_fetch.py"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "an unnamed bundle is kept",
        BUNDLES,
        "    gone = unnamed(root)\n",
        "    gone = []\n",
    ),
    (
        "the check passes an unnamed bundle",
        BUNDLES,
        "    if left:\n",
        "    if None:\n",
    ),
    (
        "a bundle's own files keep what they list",
        BUNDLES,
        "        if any(parent in inside for parent in path.resolve().parents):\n"
        "            continue\n",
        "",
    ),
    (
        "any line of a text record names a bundle",
        BUNDLES,
        '        if path.suffix == ".txt":\n'
        '            text = "\\n".join(CAMPAIGN.findall(text))\n',
        "",
    ),
    (
        "a document outside the evidence names no bundle",
        BUNDLES,
        '    for path in sorted((root / "docs").rglob("*")):\n',
        '    for path in sorted((root / "docs" / "evidence").rglob("*")):\n',
    ),
    (
        "an untracked bundle can go",
        BUNDLES,
        "    return sorted(b for b in tracked(root) if b not in kept)\n",
        "    return sorted(b for b in bundles(root) if b not in kept)\n",
    ),
    (
        "a bundle is any directory its template begins",
        BUNDLES,
        '        made = re.compile(rf"^{name}\\.[A-Za-z0-9]{{{width}}}$")\n',
        '        made = re.compile(rf"^{name}")\n',
    ),
    (
        "a bundle of more letters than its template's X's is a bundle",
        BUNDLES,
        '        made = re.compile(rf"^{name}\\.[A-Za-z0-9]{{{width}}}$")\n',
        '        made = re.compile(rf"^{name}\\.[A-Za-z0-9]{{{width},}}$")\n',
    ),
    (
        "a file is a bundle",
        BUNDLES,
        "        out.extend(p for p in parent.iterdir() if p.is_dir() and made.match(p.name))\n",
        "        out.extend(p for p in parent.iterdir() if made.match(p.name))\n",
    ),
    (
        "a recipe the justfile imports names no template",
        BUNDLES,
        '    for recipe in [root / "justfile", *sorted((root / "just").glob("*.just"))]:\n',
        '    for recipe in [root / "justfile"]:\n',
    ),
    (
        "a quoted template is not read",
        BUNDLES,
        'MKTEMP = re.compile(r"""mktemp -d ["\']?(docs/evidence/[\\w./-]+?)\\.(X{6,})""")\n',
        'MKTEMP = re.compile(r"""mktemp -d (docs/evidence/[\\w./-]+?)\\.(X{6,})""")\n',
    ),
    (
        "a fetch prunes nothing",
        FETCH,
        "        if staged:\n"
        "            pruned()\n",
        "",
    ),
    (
        "a fetch that copied nothing prunes",
        FETCH,
        "        if staged:\n"
        "            pruned()\n",
        "        if True:\n"
        "            pruned()\n",
    ),
    (
        "a fetch prunes before it copies",
        FETCH,
        "        for path, data in sorted(staged.items()):\n"
        "            path.parent.mkdir(parents=True, exist_ok=True)\n"
        "            path.write_bytes(data)\n"
        '            print(f"  {path.relative_to(ROOT)}")\n'
        "        if staged:\n"
        "            pruned()\n",
        "        if staged:\n"
        "            pruned()\n"
        "        for path, data in sorted(staged.items()):\n"
        "            path.parent.mkdir(parents=True, exist_ok=True)\n"
        "            path.write_bytes(data)\n"
        '            print(f"  {path.relative_to(ROOT)}")\n',
    ),
    (
        "a fetch it refused prunes",
        FETCH,
        "        if refused:\n"
        '            print("evidence-fetch: refused, nothing copied:',
        "        pruned()\n"
        "        if refused:\n"
        '            print("evidence-fetch: refused, nothing copied:',
    ),
    (
        "a fetch of times alone prunes",
        FETCH,
        "            n = remember(timed(results_of(shard_dirs(pathlib.Path(tmp)))), SECONDS)\n",
        "            n = remember(timed(results_of(shard_dirs(pathlib.Path(tmp)))), SECONDS)\n"
        "            pruned()\n",
    ),
]

# The tests of trees of their own; the repository's own test is
# `evidence-gates`' alone: a campaign run in this checkout beside the
# controls would hold their baseline to its untracked bundle.
SUITE = [sys.executable, "-m", "unittest",
         "scripts.tests.test_evidence_bundles.Bundles", "scripts.tests.test_evidence_bundles.Fetch"]

# How long one run may take. Past the bound it is killed with everything it
# started, and the run counts as failing.
BOUND = 600


def counted(out):
    """(ran, passed, failed) as `unittest`'s `Ran` and `OK`/`FAILED` lines say
    (a skipped test neither passed nor failed)."""
    ran = [int(n) for n in re.findall(r"^Ran (\d+) tests? in", out, re.M)]
    if not ran:
        return False, 0, 0
    ended = re.findall(r"^(?:OK|FAILED)(?: \((.*)\))?$", out, re.M)
    failed = sum(int(n) for line in ended for n in re.findall(r"(?:failures|errors)=(\d+)", line))
    skipped = sum(int(n) for line in ended for n in re.findall(r"skipped=(\d+)", line))
    return True, sum(ran) - failed - skipped, failed


def run_tests():
    """(ran, passed, failed) over the suite."""
    env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
    p = subprocess.Popen(SUITE, cwd=ROOT, env=env, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    return counted(out)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the sources are still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    ran, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not ran or failed or not passed:
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
            ran, passed, failed = run_tests()
        finally:
            path.write_text(original)
        if not ran:
            verdict = "INVALID OR SURVIVED"
            survivors += 1
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.path.insert(0, str(ROOT / "scripts"))
    sys.exit(main())
