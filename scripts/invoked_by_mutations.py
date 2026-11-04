#!/usr/bin/env python3
"""Mutation controls for NEXT 6c's step six: a command the platform invokes,
which no reader presses (`invoked_by Payments`, PW64xx).

Each mutant undoes one piece: a command the platform invokes may hold a
reader to a predicate (PW6401); it may take what the host does not give,
or the program's own `Settled` (PW6402), and is told so at no boundary
span (charter §16.3, corpus C24), as one keyed by nothing is (PW6405); two may answer a payment's outcome,
or the first is refused rather than the second (PW6403); a page may press
it (PW6404); its contract does not say who invokes it, or its interface
hash does not; a word the platform does not invoke by is taken, or the
clause on a query; and the host takes a reader's press of it.

Each must fail `compiler/pw-core/tests/invoked_by.rs` or the server's
`payments::invoked_tests`. The press route's own guard is held by the
e2e of step seven, where the store's program settles its orders.

Run from the repository root; `just e14-invoked-by` records the output. The
source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
INVOKED = ROOT / "compiler/pw-core/src/invoked.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
POLICY = ROOT / "compiler/pw-core/src/policy.rs"
PAYMENTS = ROOT / "spikes/own-renderer/server/src/payments.rs"

# (what, file, anchor, replacement)
MUTANTS = [
    (
        "a payment's command given what is no `Settled` names no boundary",
        INVOKED,
        '                vec![Related::new(\n                    by.span.clone(),\n                    format!("`invoked_by {invoker}`: the host gives it a payment\'s outcome"),\n                )],\n',
        "                Vec::new(),\n",
    ),
    (
        "a command the platform invokes keyed by nothing names no boundary",
        INVOKED,
        '                vec![Related::new(\n                    by.span.clone(),\n                    format!("`invoked_by {invoker}`: its word may come more than once"),\n                )],\n',
        "                Vec::new(),\n",
    ),
    (
        "a command the platform invokes may hold a reader to a predicate",
        INVOKED,
        '        if let Some(requires) = decl.policy("requires") {\n',
        '        if let Some(requires) = decl.policy("requires").filter(|_| false) {\n',
    ),
    (
        "a command the platform invokes may take any parameters",
        INVOKED,
        "    let [param] = decl.params.as_slice() else {\n        return false;\n    };\n",
        "    let [param] = decl.params.as_slice() else {\n        return true;\n    };\n",
    ),
    (
        "the program's own `Settled` is taken for the platform's",
        INVOKED,
        '                m.name == "payments"\n                    && ',
        "                ",
    ),
    (
        "two commands may answer a payment's outcome",
        INVOKED,
        "            && (first_unit, first) != (unit, id)\n",
        "            && false\n",
    ),
    (
        "the first command is refused rather than the second",
        INVOKED,
        "            && (first_unit, first) != (unit, id)\n",
        "            && (first_unit, first) == (unit, id)\n",
    ),
    (
        "a page may press a command the platform invokes",
        INVOKED,
        "        if let Some(calls) = sent.get(&def) {\n",
        "        if let Some(calls) = sent.get(&def).filter(|_| false) {\n",
    ),
    (
        "the contract does not say who invokes the command",
        CONTRACT,
        '                .policy("invoked_by")\n',
        '                .policy("invoked_by_")\n',
    ),
    (
        "who invokes a command is not part of its interface",
        CONTRACT,
        '                text.push_str("|invoked_by:");\n                text.push_str(invoker);\n',
        "                let _ = invoker;\n",
    ),
    (
        "a word the platform does not invoke by is taken",
        # Re-anchored by the scheduled-commands track: the clause's words
        # are `invoked.rs`'s, which `policy.rs` reads, and the clock is one.
        INVOKED,
        'pub const INVOKERS: &[&str] = &["Payments", "Clock", "Identity"];\n',
        'pub const INVOKERS: &[&str] = &["Payments", "Clock", "Identity", "Stripe"];\n',
    ),
    (
        "the clause is taken on a query",
        POLICY,
        '        "idempotent_by" | "transaction" | "optimistic" | "invoked_by" => {\n'
        '            (&[K::Command], "a command")\n'
        "        }\n",
        '        "idempotent_by" | "transaction" | "optimistic" => (&[K::Command], "a command"),\n'
        '        "invoked_by" => (&[K::Command, K::Query], "a command"),\n',
    ),
    (
        "a command the platform invokes need not be idempotent",
        INVOKED,
        '        if decl.policy("idempotent_by").is_none() {\n',
        '        if decl.policy("idempotent_by").is_none() && false {\n',
    ),
    (
        "the host takes a reader's press of a command the platform invokes",
        PAYMENTS,
        "        .is_some_and(|e| e.invoked_by.is_some())\n",
        "        .is_some_and(|e| e.invoked_by.is_some() && false)\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "invoked_by"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "invoked_tests"],
]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 900


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


def cargo_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        out, code = bounded(cmd, cwd=ROOT)
        if code is None:
            failed += 1
            continue
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            built = False
            continue
        passed += sum(int(p) for p, _ in found)
        failed += sum(int(f) for _, f in found)
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    built, passed, failed = cargo_tests()
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
            built, passed, failed = cargo_tests()
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
