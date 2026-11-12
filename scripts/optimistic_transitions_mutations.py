#!/usr/bin/env python3
"""Mutation controls for ADR-0122: optimistic transitions, compiled and held.

Each mutant undoes one piece: the backend reading a member function as a
member, the refusal of a speculated read inside a block, the decoder reading
an `Int` into a `BigInt`, the store's transition itself, the server sending
the value a page speculates on, and the version a commit reports. The tests
must then fail.

Run from the repository root; `just e14-optimistic` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
JS_PURE = ROOT / "compiler/pw-core/src/backend/js_pure.rs"
# The store's cart is a user's since track store-accounts: its transitions
# are UserCarts'.
USER_CARTS = ROOT / "examples/lib/UserCarts.pw"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a member function is not read as a member",
        LOWER,
        "        if field.is_none()\n",
        "        if field.is_none() && false\n",
    ),
    (
        "a speculated read inside a block is compiled as if it were outside",
        SPECULATION,
        # Re-anchored by ADR-0228, whose computed part asks the same, deeper.
        "        if hole.nested {\n            // Rendered with its region (ADR-0172).\n",
        "        if hole.nested && false {\n            // Rendered with its region (ADR-0172).\n",
    ),
    # "A decoded Int stays a JSON number" was a mutant here until ADR-0433,
    # which made it the design: an Int is the number JSON carries. That a
    # value the page holds is read as it is, not copied, is held by
    # handler_mutations.py, whose tests read the module's text; these tests
    # cannot tell a copy from the value.
    (
        "the transition drops a new item",
        USER_CARTS,
        # Re-anchored by ADR-0172: a new line takes the item's name and price.
        # And by track store-accounts: the store's cart is a user's, its
        # transitions UserCarts'.
        "            lines: List.concat(cart.lines, [CartLine { item_id: item.id, name: item.name, quantity: quantity, unit_price: item.price }]),\n",
        "            lines: cart.lines,\n",
    ),
    (
        "the server sends no value to a page that speculates",
        SERVER,
        # Re-anchored by ADR-0161: a change reaches each document in turn.
        # And by the stream records' branch: each document in a loop of its own.
        "                if let Some(value) = value {\n                    waiting.push(StreamFrame::EntryValue {\n",
        "                if let Some(value) = value.filter(|_| false) {\n                    waiting.push(StreamFrame::EntryValue {\n",
    ),
    (
        "a commit reports no newer version",
        SERVER,
        # Re-anchored by ADR-0176: the version is the entry's, or the one a
        # failed regeneration was tried at.
        '        serde_json::json!([{ "entry": self.cart_entry(session), "version": version }])\n',
        '        serde_json::json!([{ "entry": self.cart_entry(session), "version": 0 }])\n',
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "speculation"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "speculat"],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            built = False
            continue
        for p, f in found:
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
