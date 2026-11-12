#!/usr/bin/env python3
"""Mutation controls for ADR-0044: pure computation compiled to JavaScript.

The differential test agreed on its first run, which is exactly what a test
that cannot fail also does. Each mutant here puts back one place where
JavaScript's own semantics differ from Pleris's, and the test must then find
the component and the module disagreeing.

Run from the repository root; `just e10-javascript` records the output. The
source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
JS = ROOT / "compiler/pw-core/src/backend/js_pure.rs"
IR = ROOT / "compiler/pw-core/src/backend/ir.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a String is ordered by UTF-16 unit, as JavaScript's `<` is",
        JS,
        'format!("{}({a}, {b}) {} 0", self.uses("compare"), symbol(*op))',
        'format!("{a} {} {b}", symbol(*op))',
    ),
    (
        "String.trim is ECMAScript's trim",
        JS,
        'Intrinsic::StrTrim => format!("{}({})", self.uses("trim"), arg(0)),',
        'Intrinsic::StrTrim => format!("{}.trim()", arg(0)),',
    ),
    (
        "String.length counts UTF-16 units",
        JS,
        'format!("Array.from({}).length", arg(0))',
        'format!("{}.length", arg(0))',
    ),
    (
        "an Int does not trap on overflow",
        JS,
        r'if (BigInt.asIntN(64, v) !== v) trap(\"Int overflow\");',
        r'if (false) trap(\"Int overflow\");',
    ),
    # ADR-0427: an Int's overflow found as 64 bits wrap it, and a list of
    # literals a value of the module.
    (
        "an Int's overflow is that of 63 bits",
        JS,
        r'if (BigInt.asIntN(64, v) !== v) trap(\"Int overflow\");',
        r'if (BigInt.asIntN(63, v) !== v) trap(\"Int overflow\");',
    ),
    (
        "a list of literals is made at each call",
        JS,
        "                    Some(xs) if !xs.is_empty() => {\n",
        "                    Some(xs) if false => {\n",
    ),
    (
        "an empty list is a value of the module",
        JS,
        "                    Some(xs) if !xs.is_empty() => {\n",
        "                    Some(xs) => {\n",
    ),
    (
        "a list of literals of the module is not frozen",
        JS,
        '.map(|(name, items)| format!("const {name} = Object.freeze([{items}]);")),',
        '.map(|(name, items)| format!("const {name} = [{items}];")),',
    ),
    (
        "the literals a list of the module read are written",
        JS,
        "        self.line(&format!(\"return {};\", val(*result)));\n        self.unread_dropped();\n        self.unmade_unread()\n    }\n\n    /// **Refused: a value left unmade",
        "        self.line(&format!(\"return {};\", val(*result)));\n        self.unmade_unread()\n    }\n\n    /// **Refused: a value left unmade",
    ),
    (
        "a function value's lists of literals are not the module's",
        JS,
        "        helpers.extend(e.helpers.iter().copied());\n        constants_into(&mut constants, &e.constants).map_err(refused)?;\n        let params: Vec<String> = c.function.params",
        "        helpers.extend(e.helpers.iter().copied());\n        let params: Vec<String> = c.function.params",
    ),
    # A callee's lists are the module's too; no control drops that merge, as
    # the inliner copies each callee's first call into its caller, list and
    # all, so no program yet has a list a callee alone holds (ADR-0427).
    (
        "the module's lists of literals are not given to its prelude",
        JS,
        "    constants_into(&mut e.constants, &constants).map_err(refused)?;\n",
        "",
    ),
    (
        # Two ways since ADR-0433: on two numbers, and past 2^53.
        "`/` truncates toward zero, as a number's does",
        JS,
        "return (r < 0 ? (b > 0 ? q - 1 : q + 1) : q) + 0;",
        "return q + 0;",
    ),
    (
        "`/` past 2^53 truncates toward zero, as BigInt's does",
        JS,
        "if (x % y < 0n) q = y > 0n ? q - 1n : q + 1n;",
        "/* truncated */",
    ),
    (
        "`%` takes the dividend's sign, as a number's does",
        JS,
        "return (r < 0 ? (b > 0 ? r + b : r - b) : r) + 0;",
        "return r + 0;",
    ),
    (
        "`%` past 2^53 takes the dividend's sign, as BigInt's does",
        JS,
        "return big(r < 0n ? (y > 0n ? r + y : r - y) : r);",
        "return big(r);",
    ),
    (
        "from_codepoints accepts surrogates",
        JS,
        " || (p >= 0xD800 && p <= 0xDFFF)",
        "",
    ),
    (
        "sort_by puts the left element first on a positive comparison",
        JS,
        "sort((a, b) => {sign}(f(a, b)))",
        "sort((a, b) => -{sign}(f(a, b)))",
    ),
    (
        "List.get reads one past the end",
        JS,
        "{i} >= 0 && {i} < {xs}.length",
        "{i} >= 0 && {i} <= {xs}.length",
    ),
    (
        "List.take reads a negative count from the end",
        JS,
        "{n} < 0 ? 0 : ",
        "",
    ),
    # ADR-0436: an Option a match takes apart where it is made is never made,
    # and one read again is: each read `reads` counts is a query's, the
    # match's test is its case's, and List.find takes the first that holds.
    (
        "an Option read again is taken apart where it is made",
        JS,
        "                && self.reads.get(scrutinee) == Some(&1)\n",
        "",
    ),
    (
        "a region's value is not a read",
        IR,
        "                *out.entry(r.value).or_default() += 1;\n",
        "",
    ),
    (
        "a body's value is not a read",
        IR,
        "    *out.entry(value).or_default() += 1;\n",
        "",
    ),
    (
        "a case's payload is not a read",
        IR,
        "            Instr::Variant { payload, .. } => payload.iter().copied().collect(),\n",
        "            Instr::Variant { .. } => Vec::new(),\n",
    ),
    (
        "an Option taken apart takes Some's arm where its test fails",
        JS,
        'self.line(&format!("if ({}) {{", o.test));',
        'self.line(&format!("if (!({})) {{", o.test));',
    ),
    (
        "List.find takes the last that holds",
        JS,
        "{xs}.findIndex({f});",
        "{xs}.findLastIndex({f});",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "javascript"],
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
