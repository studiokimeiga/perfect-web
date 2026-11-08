#!/usr/bin/env python3
"""Mutation controls for pleris.dev's measured claims (ADR-0408): the site
shows the head-to-head's record as the instrument wrote and judged it, every
row of it on `/measured`, and no record the instrument refused, measured on a
changed tree, or holding a loss that names no stack.

Each mutant undoes one rule in `site/build.mjs`. `site/build.test.mjs` must
fail each, by an assertion of its witness test in a complete run: a run past
its bound, a test that fails by an error, or a test missing from the
baseline's inventory kills nothing. The witnesses read the record and the
pages' HTML and judge each row's marks themselves, so no mutant changes what
its test expects; none of them builds the whole site, whose count of
mutation anchors a mutant applied would stop.

Run from the repository root; `just e14-site` records the output. The source
is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

import mutation_source

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUILD = ROOT / "site/build.mjs"
TEST = "site/build.test.mjs"

STANDING = "the standing the home page states is the whole record's: rows ahead, and rows behind"
HOME = "the home page's tables are the record's rows, under the page's names, each marked as the instrument judged it"
EVERY = "/measured shows every row of the record, in the instrument's words, each marked as it judged it"
CHECKED = "a record the instrument refused, one measured on a changed tree, or a loss that names no stack, is shown on no page"
NAMED = {STANDING, HOME, EVERY, CHECKED}

# (what is undone, file, anchor, replacement); each one's witness test is in
# WITNESSES.
MUTANTS = [
    ("a run the instrument refused is shown", BUILD, "  if (rec.problems.length) throw", "  if (false) throw"),
    ("a run on a changed tree is shown", BUILD, "  if (rec.dirty) throw", "  if (false) throw"),
    ("a loss that names no stack is shown", BUILD, "  if (!r.better.length) throw", "  if (false) throw"),
    (
        "a cell shows another stack's value",
        BUILD,
        "    const v = r.values.find((x) => x.label === s);\n",
        "    const v = r.values[0];\n",
    ),
    ("a stack ahead of Pleris is not marked", BUILD, "  return r.better;\n", "  return [];\n"),
    (
        "the standing counts the ties as losses",
        BUILD,
        'it is ahead in ${count("win")} and behind in ${count("loss")}:',
        'it is ahead in ${count("win")} and behind in ${count("neither")}:',
    ),
    (
        "/measured leaves out the ties",
        BUILD,
        "rec.rows.filter((r) => r.app === t.app)",
        'rec.rows.filter((r) => r.app === t.app && r.verdict !== "neither")',
    ),
]

# The test each mutant must fail, by an assertion.
WITNESSES = {
    "a run the instrument refused is shown": CHECKED,
    "a run on a changed tree is shown": CHECKED,
    "a loss that names no stack is shown": CHECKED,
    "a cell shows another stack's value": HOME,
    "a stack ahead of Pleris is not marked": HOME,
    "the standing counts the ties as losses": STANDING,
    "/measured leaves out the ties": EVERY,
}

# How long one run may take before it is a hang: killed, and no kill.
BOUND = 300

TOP = re.compile(r"^(not ok|ok) \d+ - (.*)$")


def run_suite():
    """(exit status, {test: None where it passed, else its failure's code}),
    or (None, {}) past the bound."""
    child = subprocess.Popen(
        ["node", "--test", "--test-reporter=tap", TEST],
        cwd=ROOT, start_new_session=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        out, _ = child.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.communicate()
        return None, {}
    except BaseException:
        os.killpg(child.pid, signal.SIGKILL)
        child.communicate()
        raise
    results, current = {}, None
    for line in out.splitlines():
        top = TOP.match(line)
        if top:
            current = top.group(2).replace("\\#", "#")
            results[current] = None if top.group(1) == "ok" else "failed"
            continue
        code = re.match(r"^  code: '([A-Z_]+)'$", line)
        if code and current is not None and results.get(current) == "failed":
            results[current] = code.group(1)
    return child.returncode, results


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    status, baseline = run_suite()
    passed = sum(1 for r in baseline.values() if r is None)
    print(f"baseline: {passed} passed, {len(baseline) - passed} failed", flush=True)
    if status != 0 or not baseline or any(r is not None for r in baseline.values()):
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    missing = sorted(NAMED - set(baseline))
    if missing:
        print("FAIL: a mutant names a test the suite does not have: " + "; ".join(missing))
        return 1
    if set(WITNESSES) != {m[0] for m in MUTANTS}:
        print("FAIL: a mutant has no witness, or a witness no mutant")
        return 1

    killed = refused = survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        witness = WITNESSES[what]
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: REFUSED (anchor not unique in {path.name})", flush=True)
            refused += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            status, results = run_suite()
        if status is None or set(results) != set(baseline):
            verdict = "REFUSED (incomplete run; no kill)"
            refused += 1
        elif results.get(witness) == "ERR_ASSERTION":
            failing = sum(1 for r in results.values() if r is not None)
            verdict = f"KILLED (by an assertion of its test; {failing} of {len(results)} tests fail)"
            killed += 1
        else:
            print(f"  (its test {witness!r}: {results.get(witness) or 'passed'})", flush=True)
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what}: {verdict}", flush=True)
    print(f"{killed} of {len(MUTANTS)} mutants killed; {refused} refused; {survivors} survived")
    return int(bool(refused or survivors))


if __name__ == "__main__":
    sys.exit(main())
