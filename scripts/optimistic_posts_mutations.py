#!/usr/bin/env python3
"""Mutation controls for ADR-0222: a post is shown before the server
answers.

Each mutant undoes one piece: a target's key left unnamed, in the check and
in the speculation's match; the value a page speculates on, read with what
it shows, carried into the page, sent when it changes and only then, sent
after a longer read, and named by a commit's answer; and the store's cart,
kept apart. The tests of each must then fail.

The browser suite's `e2e/feed.spec.mjs` runs the post in three engines.

Run from the repository root; `just e14-optimistic-posts` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECK = ROOT / "compiler/pw-core/src/check.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a key left unnamed is a name that does not resolve",
        CHECK,
        # Re-anchored by ADR-0226, which skips a module a path starts with
        # there too.
        "                if wildcards.contains(&nid) || heads.contains(&nid) {\n",
        "                if false && wildcards.contains(&nid) || heads.contains(&nid) {\n",
    ),
    (
        "`_` is unnamed wherever a policy writes it",
        CHECK,
        "    if root.context != crate::hir::ExecutionContext::TargetSelection {\n",
        "    if false && root.context != crate::hir::ExecutionContext::TargetSelection {\n",
    ),
    (
        "the speculation reads `_` as no key",
        SPECULATION,
        # Re-anchored by ADR-0236, whose key places are named.
        '                    Expr::Name(n) if n == "_" => Some(KeyArg::Any),\n',
        '                    Expr::Name(n) if false && n == "_" => Some(KeyArg::Any),\n',
    ),
    (
        "a key left unnamed matches no key",
        SPECULATION,
        "                        Some(KeyArg::Any) => true,\n",
        "                        Some(KeyArg::Any) => false,\n",
    ),
    (
        "what a page shows holds no value it speculates on",
        SERVER,
        # Re-anchored by ADR-0233: written by its query's type.
        "                shown.speculated.insert(binding, written);\n",
        "                let _ = (binding, written);\n",
    ),
    (
        "a page is served no value it speculates on",
        SERVER,
        "        for (binding, value) in &shown.speculated {\n",
        "        for (binding, value) in shown.speculated.iter().take(0) {\n",
    ),
    (
        "a change sends no value a page speculates on",
        SERVER,
        "            for frame in speculated {\n",
        "            for frame in speculated.into_iter().take(0) {\n",
    ),
    (
        "a longer read sends no value a page speculates on",
        SERVER,
        "        for frame in speculated {\n            waiting.push(frame);\n        }\n        Ok(KeyOutcome::Applied)\n",
        "        let _ = speculated;\n        Ok(KeyOutcome::Applied)\n",
    ),
    (
        "a value that did not change is sent again",
        SERVER,
        "            if was.speculated.get(binding) == Some(value) {\n",
        "            if false && was.speculated.get(binding) == Some(value) {\n",
    ),
    (
        "a commit's answer names no value it changed",
        SERVER,
        '                .filter(|((s, actor, ..), _)| s == session && *actor == reader)\n',
        '                .filter(|_| false)\n',
    ),
    (
        "the store's cart is speculated twice",
        SERVER,
        "            .filter(|b| cart.as_deref() != Some(b.as_str()))\n",
        "            .filter(|_| true)\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "optimistic_keys"],
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "a_post_is_shown_before_the_server_answers",
        "a_speculated_value_that_did_not_change_is_not_sent_again",
        "a_page_that_reads_more_holds_what_it_shows",
        "a_speculating_page_is_sent_its_carts_value",
    ],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not results:
            built = False
            continue
        for p, f in results:
            passed += int(p)
            failed += int(f)
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
