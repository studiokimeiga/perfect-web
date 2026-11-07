#!/usr/bin/env python3
"""Mutation controls for ADR-0406: a command says who may run it.

Each mutant undoes one part of PW0367's rule, or of what `Anyone` means:
- a command a reader can run, and one only a request names, says who may
  run it, at its name, and a press that sends it is named beside it;
- one the platform runs (`invoked_by`) is not asked;
- `Anyone` stands alone, takes no argument, and is the platform's;
- the contract asks nothing for `Anyone`, and a page needs no guard for it.

The tests in `who_may_run.rs` and `component_contract.rs`, and the
generality witnesses, must then fail.

Run from the repository root. The source is restored after every mutant,
whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
HOLDS = ROOT / "compiler/pw-core/src/holds.rs"

COMMAND_ARM = '            hir::DeclKind::Command if decl.policy("invoked_by").is_none() => {\n'

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a command that says nothing of who may run it is not refused",
        CHECK,
        COMMAND_ARM,
        "            hir::DeclKind::Command if false => {\n",
    ),
    (
        "a command the platform runs is asked for `requires` too",
        CHECK,
        COMMAND_ARM,
        "            hir::DeclKind::Command => {\n",
    ),
    (
        "`Anyone` beside another predicate is taken",
        CHECK,
        "            } else if predicates.len() > 1 {\n",
        "            } else if false {\n",
    ),
    (
        "`Anyone` with arguments is taken",
        CHECK,
        "            let (reason, message) = if !anyone.arguments.is_empty() {\n",
        "            let (reason, message) = if false {\n",
    ),
    (
        "a program's own `Anyone` is taken",
        CHECK,
        "            hir::DeclKind::Predicate if decl.name == crate::policy::ANYONE => {\n",
        "            hir::DeclKind::Predicate if false => {\n",
    ),
    (
        "the press that sends it is not named",
        CHECK,
        "    let related = if presses.is_empty() {\n",
        "    let related = if true {\n",
    ),
    (
        "it is reported at its declaration, not its name",
        CHECK,
        "            decl.name_span.clone(),\n            vec![\n                \"declare `requires`",
        "            hir.decl_span(id),\n            vec![\n                \"declare `requires`",
    ),
    (
        "the contract asks `Anyone` of a request",
        CONTRACT,
        "            .filter(|predicate| {\n"
        "                !(predicate.name == crate::policy::ANYONE && predicate.arguments.is_empty())\n"
        "            })\n",
        "",
    ),
    (
        "an unchecked `Anyone(n)` asks nothing (train-14's review)",
        CONTRACT,
        "            .filter(|predicate| {\n"
        "                !(predicate.name == crate::policy::ANYONE && predicate.arguments.is_empty())\n"
        "            })\n",
        "            .filter(|predicate| predicate.name != crate::policy::ANYONE)\n",
    ),
    (
        "a page is asked to guard a control anyone may press",
        HOLDS,
        "            .filter(|p| p.arguments.is_empty() && p.name != crate::policy::ANYONE)\n",
        "            .filter(|p| p.arguments.is_empty())\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "who_may_run"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "component_contract"],
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
