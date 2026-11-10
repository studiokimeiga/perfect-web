#!/usr/bin/env python3
"""Mutation controls for ADR-0058: handlers that compute.

Each mutant undoes one piece of how a handler's body is lowered, how its
captures are read, how its commands are sent and awaited, or what it refuses,
and the handler tests, which run each module under Node, must then fail.

Run from the repository root; `just e10-handlers-compute` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
JS_PURE = ROOT / "compiler/pw-core/src/backend/js_pure.rs"
JS = ROOT / "compiler/pw-core/src/backend/js.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a captured path is not its value",
        LOWER,
        "            && let Some(v) = self.captured.get(&path).copied()",
        "            && let Some(v) = None::<ValueId>",
    ),
    (
        "a command is sent without its arguments",
        LOWER,
        # Re-anchored by ADR-0157: the command's answer is typed.
        "                command,\n                args: lowered,\n                ty,",
        "                command,\n                args: Vec::new(),\n                ty,",
    ),
    (
        "a command inside a function value is let through",
        LOWER,
        "            if self.in_lambda > 0 || !self.handler {",
        "            if self.in_lambda > 0 {",
    ),
    (
        "a query called from a handler is let through",
        LOWER,
        "            && decl.kind == DeclKind::Query\n        {",
        "            && decl.kind == DeclKind::Query\n            && false\n        {",
    ),
    (
        "a captured value read in a function value is not named",
        LOWER,
        "                None if self.internal.borrow().captured_roots.contains(n) => {",
        "                None if false => {",
    ),
    (
        "a captured Int stays a JavaScript number",
        JS_PURE,
        '            Type::Int => format!("BigInt({expr})"),',
        "            Type::Int => expr.to_string(),",
    ),
    (
        "an Int past 2^53 is sent rounded",
        JS_PURE,
        "if (v < -9007199254740992n || v > 9007199254740992n) trap(",
        "if (false) trap(",
    ),
    # ADR-0427: a list of literals is a value of the handler's module.
    (
        "a handler's list of literals is made at each call",
        JS_PURE,
        "                    Some(xs) if !xs.is_empty() => {\n",
        "                    Some(xs) if false => {\n",
    ),
    (
        "the literals a handler's list of its module read are written",
        JS_PURE,
        "        self.line(&format!(\"return {};\", val(*result)));\n        self.unread_dropped();\n        Ok(())\n    }\n\n    /// An instance's shape",
        "        self.line(&format!(\"return {};\", val(*result)));\n        Ok(())\n    }\n\n    /// An instance's shape",
    ),
    # The merge of a callee's lists into the handler's module has no control:
    # the inliner copies each callee's first call into the handler, list and
    # all (ADR-0427).
    (
        "an opaque argument is sent as it is held",
        JS_PURE,
        "                Some(Shape::Alias(of)) => self.wire(v, &of),",
        "                Some(Shape::Alias(_)) => Ok(v.to_string()),",
    ),
    (
        "a command is not awaited",
        JS_PURE,
        # Re-anchored by ADR-0157: the answer is bound, then decoded; and by
        # ADR-0173: how it is sent again follows its arguments.
        '"const {answered} = await context.command({}, [{}]{how});"',
        '"const {answered} = context.command({}, [{}]{how});"',
    ),
    (
        "a handler that calls no command is compiled",
        JS,
        # Re-anchored by ADR-0133: a handler may change a signal instead.
        "    if commands.is_empty() && !changes_a_signal {",
        "    if false && commands.is_empty() && !changes_a_signal {",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "handlers"],
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
