#!/usr/bin/env python3
"""Mutation controls for ADR-0082: a `derived` value performs no effect.

Each mutant undoes one piece of how a `derived` value is held pure: that it
is checked at all, that only what the value itself performs counts, and that
what it names and reads counts as well as what it calls. The derived-purity
tests must then fail.

Run from the repository root; `just e10-derived` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
EFFECTS = ROOT / "compiler/pw-core/src/effects.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a derived value is not checked",
        CHECK,
        '        derived_values_are_pure(inference, sigs, i, &u.hir, per_unit);\n',
        "",
    ),
    (
        "an effect beside a derived value is counted as its",
        CHECK,
        "                .find(|s| s.span.start >= region.start && s.span.end <= region.end)",
        "                .find(|s| s.span.start >= region.start || true)",
    ),
    (
        # Until 2026-10-25 this replaced the check's `infer_in_at` with
        # `infer_at`, which then counted calls alone. ADR-0351 made
        # `infer_at` read the members and values of every body `run` typed,
        # so it found what `infer_in_at` finds, and the mutant survived as an
        # equivalent (the nightly of 2026-10-25, at cc47a64b). The same
        # fault, where the inference the check uses adds them: calls alone.
        "only what a derived value calls counts",
        EFFECTS,
        "        self.member_effects(body, body.root, types, &mut out, reads.as_deref_mut());\n"
        "        self.value_effects(unit, body, body.root, types, &mut out, reads);\n",
        "        let _ = &mut out;\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "derived_purity"],
]

INVENTORY = {}
SOURCES = [ROOT / "compiler/pw-core/tests/derived_purity.rs"]


def run_tests():
    """Complete named native assertion results; broken producers are invalid."""
    passed, failed = 0, 0
    for cmd in TESTS:
        try:
            child = producer_process.run([*cmd, "--", "--format=pretty"], cwd=ROOT, timeout=900)
        except subprocess.TimeoutExpired:
            return False, passed, failed
        key = tuple(cmd)
        result = cargo_assertion_results.validate(child.returncode, child.stdout,
            root=ROOT, sources=SOURCES, expected=INVENTORY.get(key))
        if not result.valid:
            print(child.stdout[-4000:], flush=True)
            return False, passed, failed
        INVENTORY.setdefault(key, cargo_assertion_results.names(child.stdout))
        passed += result.passed
        failed += result.failed
    return True, passed, failed


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
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed = run_tests()
        if not built:
            verdict = "INVALID (build, infrastructure or native assertion producer)"
            survivors += 1
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
