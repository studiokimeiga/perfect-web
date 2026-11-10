#!/usr/bin/env python3
"""Mutation controls for ADR-0055: slicing, and the standard library's
placeholders computed.

Each mutant undoes one piece of `List.drop`, `List.slice`, `List.reverse`,
`String.slice`, `List.sum`, `List.maximum`, `List.range` or `List.swap`, in the
component, the JavaScript module or the standard library's own Pleris, and
the tests must then fail.

Run from the repository root; `just e10-slices` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WASM = ROOT / "compiler/pw-core/src/backend/wasm.rs"
JS = ROOT / "compiler/pw-core/src/backend/js_pure.rs"
LIST = ROOT / "packages/pw-std/list.pw"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a slice starts at the list's first element",
        WASM,
        "let from = self.element_address(ptr, start, esize);",
        "let from = ptr;",
    ),
    (
        "an end before the start is kept",
        WASM,
        "                            I::I32GtU,\n                            I::Select,\n"
        "                            I::LocalSet(end),",
        "                            I::Drop,\n                            I::Drop,\n"
        "                            I::Drop,\n                            I::LocalSet(end),",
    ),
    (
        "a reversal copies in order",
        WASM,
        "                    I::LocalGet(len),\n                    I::I32Const(1),\n"
        "                    I::I32Sub,\n                    I::LocalGet(i),\n"
        "                    I::I32Sub,",
        "                    I::LocalGet(i),",
    ),
    (
        "a string's slice ignores its end",
        WASM,
        "            body.extend(at_bound(3, 7));",
        "",
    ),
    (
        "a string's negative start is not zero",
        WASM,
        "            for bound in [2, 3] {",
        "            for bound in [3] {",
    ),
    (
        "the module clamps no negative bound",
        JS,
        "const clamp = (i) => (i < 0n ? 0n : i > n ? n : i);",
        "const clamp = (i) => (i > n ? n : i);",
    ),
    (
        "the module's reversal is the list",
        JS,
        'Intrinsic::ListReverse => format!("[...{}].reverse()", arg(0)),',
        'Intrinsic::ListReverse => format!("[...{}]", arg(0)),',
    ),
    (
        "sum adds nothing",
        LIST,
        "fold(items, 0.0, (total, x) => total + x)",
        "fold(items, 0.0, (total, x) => total)",
    ),
    (
        "maximum keeps the first element",
        LIST,
        "                Some(x)\n            } else {\n                Some(b)\n",
        "                Some(b)\n            } else {\n                Some(b)\n",
    ),
    (
        "maximum passes a NaN over",
        LIST,
        "if x > b | x != x | (x == b",
        "if x > b | (x == b",
    ),
    # `List.range`.
    (
        "a range's bound reads a wrapped difference as signed",
        WASM,
        "                I::I64Const(0x0fff_ffff),\n                I::I64GtU,",
        "                I::I64Const(0x0fff_ffff),\n                I::I64GtS,",
    ),
    (
        "a range too long for a list traps for no cause",
        WASM,
        "                I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),\n"
        "                I::Unreachable,\n                I::End,\n"
        "                // count = end > start",
        "                I::Unreachable,\n                I::End,\n"
        "                // count = end > start",
    ),
    (
        "an empty range's count is its difference",
        WASM,
        "                I::I32WrapI64,\n                I::I32Const(0),\n"
        "                I::LocalGet(1),\n                I::LocalGet(0),\n"
        "                I::I64GtS,\n                I::Select,\n                I::LocalSet(2),",
        "                I::I32WrapI64,\n                I::LocalSet(2),",
    ),
    (
        "a range's numbers start at zero",
        WASM,
        "                    I::LocalGet(0),\n                    I::LocalGet(4),\n"
        "                    I::I64ExtendI32U,\n                    I::I64Add,",
        "                    I::LocalGet(4),\n                    I::I64ExtendI32U,",
    ),
    (
        "the module's range traps for another cause",
        JS,
        r'trap(\"a value too long for memory\")',
        r'trap(\"a range too long\")',
    ),
    (
        "the module's range starts at zero",
        JS,
        "for (let i = start; i < end; i++)",
        "for (let i = 0n; i < end; i++)",
    ),
    # `List.swap`.
    (
        "a swap moves one element",
        WASM,
        "                self.ops.extend(moved(i, j));\n                self.ops.extend(moved(j, i));\n",
        "                self.ops.extend(moved(i, j));\n",
    ),
    (
        "a swap copies only the two it moves",
        WASM,
        "                    I::LocalGet(copy),\n                    I::LocalGet(ptr),\n"
        "                    I::LocalGet(len),\n                    I::I32Const(size),\n"
        "                    I::I32Mul,\n",
        "                    I::LocalGet(copy),\n                    I::LocalGet(ptr),\n"
        "                    I::I32Const(0),\n                    I::I32Const(size),\n"
        "                    I::I32Mul,\n",
    ),
    (
        "a place past the end is a place to swap",
        WASM,
        "                        I::I64ExtendI32U,\n                        I::I64LtS,\n"
        "                        I::I32And,\n                    ]\n                };\n"
        "                self.ops.extend(place(i));\n",
        "                        I::I64ExtendI32U,\n                        I::Drop,\n"
        "                        I::Drop,\n                    ]\n                };\n"
        "                self.ops.extend(place(i));\n",
    ),
    (
        "the module swaps the list it was given",
        JS,
        "const out = [...xs];\\n  const a = Number(i);",
        "const out = xs;\\n  const a = Number(i);",
    ),
    (
        "the module takes a negative place",
        JS,
        "if (i < 0n || j < 0n || i >= n || j >= n) return xs;",
        "if (i >= n || j >= n) return xs;",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "stdlib"],
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
