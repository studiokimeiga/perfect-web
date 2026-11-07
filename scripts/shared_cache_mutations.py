#!/usr/bin/env python3
"""Mutation controls for ADR-0128: a shared cache holds no one reader's
value, whatever its declaration says.

Each mutant undoes one piece: a declaration observing its own parameters, a
reader's value being unshareable whatever its key, PW5001 reading the whole
label, a key item naming a tenant parameter, and PW5004 leaving a reader's
value to PW5001. The tests in `shared_cache_holds_no_readers_value.rs` and
the generality witnesses must then fail.

Run from the repository root; `just e14-shared-cache` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
PRIVACY = ROOT / "compiler/pw-core/src/privacy.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a declaration's own parameters are not read",
        CHECK,
        "                    if let Some(given) = given_label(sigs, sig) {\n",
        "                    if let Some(given) = None::<Given> {\n",
    ),
    (
        "a reader's value keyed by the reader is shareable",
        PRIVACY,
        "            .all(|r| matches!(r, Restriction::Organization(_)))\n",
        "            .all(|r| !matches!(r, Restriction::Secret(_)))\n",
    ),
    (
        "PW5001 reads the declared label only",
        CHECK,
        "    let held = label.join(&reads.observed(def));\n",
        "    let held = label.clone();\n",
    ),
    (
        "a key item naming a tenant parameter does not count",
        CHECK,
        "                covered.extend(sigs.label(ty).required_cache_partitions());\n",
        "                let _ = ty;\n",
    ),
    (
        "PW5004 does not read what the declaration observes",
        CHECK,
        "    let label = label_of(decl).join(&read).join(&reads.observed(def));\n",
        "    let label = label_of(decl).join(&read);\n",
    ),
]

TESTS = [
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core",
        "--test", "shared_cache_holds_no_readers_value",
    ],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "generality"],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        m = re.search(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if m is None:
            built = False
            continue
        passed += int(m.group(1))
        failed += int(m.group(2))
    return built, passed, failed


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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
