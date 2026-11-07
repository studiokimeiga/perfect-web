#!/usr/bin/env python3
"""Mutation controls for ADR-0090: `pw build` checks what `pw check` checks.

Each mutant undoes one piece: running the declaration rules inside the one
checker, and holding their diagnostics to the checker's standard (the
registry's sentence for an invariant, a boundary span, an explanation, the
policy as written). The tests in `one_checker.rs` and `checking_source.rs`
must then fail.

Run from the repository root; `just e10-one-checker` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
RULES = ROOT / "compiler/pw-core/src/rules.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "the declaration rules run only in the command",
        CHECK,
        "        per_unit.extend(crate::rules::check(&u.hir));\n",
        "",
    ),
    (
        "a rule restates its invariant in its own words",
        RULES,
        "    crate::codes::lookup(code).map_or(written, |c| c.invariant)",
        "    crate::codes::lookup(code).map_or(written, |_| written)",
    ),
    (
        "an unbounded retry has no boundary span",
        RULES,
        "                .related(name_span.clone(), format!(\"`{name}` would retry without end\"))\n",
        "",
    ),
    (
        "a retry without idempotency does not name its policy",
        RULES,
        "                    \"`{name}` declares `retry {}` but is not idempotent\",",
        "                    \"`{name}` declares a retry policy{} but is not idempotent\",",
    ),
    # Until the stale-key ADR this mutant removed PW0325's explanation; that
    # rule is PW5309's now, outside `rules.rs`. R-014's rule is explained here.
    (
        "a retry without idempotency is not explained",
        RULES,
        "            .explain(\n                \"retrying a non-idempotent mutation can apply it more than once: a request \\\n                 that failed in transit may have arrived, and its sender cannot tell\",\n            )\n",
        "",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "one_checker"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "checking_source"],
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
