#!/usr/bin/env python3
"""Mutation controls for ADR-0057: maps and sets.

Each mutant undoes one piece of how a map or a set is ordered, searched,
changed, built, merged or checked, in the component, the lowering or the
JavaScript module, and the map and differential tests must then fail.

Run from the repository root; `just e10-maps` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WASM = ROOT / "compiler/pw-core/src/backend/wasm.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
JS = ROOT / "compiler/pw-core/src/backend/js_pure.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "an Int key is never above another",
        WASM,
        "                I::LocalGet(x[0]),\n                I::LocalGet(y[0]),\n                I::I64GtS,",
        "                I::LocalGet(x[0]),\n                I::LocalGet(y[0]),\n                I::I64LtS,",
    ),
    (
        "String keys are ordered backwards",
        WASM,
        "                    I::LocalGet(x[0]),\n                    I::LocalGet(x[1]),\n"
        "                    I::LocalGet(y[0]),\n                    I::LocalGet(y[1]),",
        "                    I::LocalGet(y[0]),\n                    I::LocalGet(y[1]),\n"
        "                    I::LocalGet(x[0]),\n                    I::LocalGet(x[1]),",
    ),
    (
        "a search passes over the key it seeks",
        WASM,
        "            I::LocalGet(o),\n            I::I32Const(0),\n            I::I32LtS,\n"
        "            I::If(Empty),\n            I::LocalGet(mid),",
        "            I::LocalGet(o),\n            I::I32Const(0),\n            I::I32LeS,\n"
        "            I::If(Empty),\n            I::LocalGet(mid),",
    ),
    (
        "a replaced entry is kept beside its replacement",
        WASM,
        "                    I::LocalGet(at),\n                    I::LocalGet(found),\n"
        "                    I::I32Add,\n                    I::LocalSet(skip),",
        "                    I::LocalGet(at),\n                    I::LocalSet(skip),",
    ),
    (
        "a map's value is read from its key's place",
        WASM,
        "                    I::LocalGet(from),\n                    I::I32Const(voff as i32),",
        "                    I::LocalGet(from),\n                    I::I32Const(0),",
    ),
    (
        "a sort of entries is not stable",
        WASM,
        # `Map.from_lists`' sort: the arrival sort's comparator is the same
        # text, deeper (ADR-0259), and its stability shows nowhere, since a
        # key twice traps there.
        "this.ops\n                .extend([I::LocalGet(o), I::I32Const(0), I::I32LeS, I::LocalSet(take)]);",
        "this.ops\n                .extend([I::LocalGet(o), I::I32Const(0), I::I32LtS, I::LocalSet(take)]);",
    ),
    (
        "a repeated key is kept",
        WASM,
        "            I::LocalGet(o),\n            I::I32Const(0),\n            I::I32Ne,\n"
        "            I::LocalSet(keep),",
        "            I::I32Const(1),\n            I::LocalSet(keep),",
    ),
    (
        "a union leaves out what only the second has",
        WASM,
        "        if op == N::SetUnion {\n            take(self, pb, j);\n        }",
        "",
    ),
    (
        "the entry check lets a key through twice",
        WASM,
        # Re-anchored by ADR-0259: sorted on arrival, a key twice is two
        # neighbours of one key.
        "self.ops.extend([I::LocalGet(o), I::I32Eqz]);",
        "self.ops.extend([I::LocalGet(o), I::Drop, I::I32Const(0)]);",
    ),
    (
        "a map from outside is not checked",
        LOWER,
        "            Type::Map(..) => Intrinsic::MapCheck,",
        "            Type::Map(..) => continue,",
    ),
    (
        "a map a host answers is not checked",
        LOWER,
        "                    Type::Map(..) => Some(Intrinsic::MapCheck),",
        "                    Type::Map(..) => None,",
    ),
    (
        "the module orders Int keys backwards",
        JS,
        "compare(a, b) : a < b ? -1 : a > b ? 1 : 0;",
        "compare(a, b) : a < b ? 1 : a > b ? -1 : 0;",
    ),
    (
        "the module keeps the first of a repeated key",
        JS,
        "i + 1 === s.length || key_order(of(x), of(s[i + 1])) !== 0",
        "i === 0 || key_order(of(s[i - 1]), of(x)) !== 0",
    ),
    (
        "the module's union leaves out what only the second has",
        JS,
        "else if (o > 0) { if (second) out.push(b[j]); j++; }",
        "else if (o > 0) { j++; }",
    ),
    (
        "the module's entry check lets a key through twice",
        JS,
        # Re-anchored by ADR-0259.
        "=== 0) trap(",
        "=== 2) trap(",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "maps"],
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
