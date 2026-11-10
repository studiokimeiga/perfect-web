#!/usr/bin/env python3
"""Mutation controls for ADR-0267: a component's trap says why it stopped.

Each mutant undoes one piece: the functions named by their causes, each
tested cause's trap calling its own cause's function, the causes' functions
where a trap looks for them, and the host reading a cause from the frame a
trap stopped in and Wasm's own by its code. The tests of
`compiler/pw-conformance/tests/trap_causes.rs` must then fail.

Run from the repository root; `just e14-trap-causes` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WASM = ROOT / "compiler/pw-core/src/backend/wasm.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "no function is named by its cause",
        WASM,
        "    section.functions(&names);\n",
        "",
    ),
    (
        "the causes' functions are looked for one place on",
        WASM,
        "    realloc_index + 3 + cause as u32\n",
        "    realloc_index + 4 + cause as u32\n",
    ),
    (
        "an overflow calls another cause's function",
        WASM,
        "        let trap = trap_index(self.realloc_index, Cause::IntOverflow);\n        let ops = &mut self.ops;\n",
        "        let trap = trap_index(self.realloc_index, Cause::RepeatedKey);\n        let ops = &mut self.ops;\n",
    ),
    (
        "a repeated key calls another cause's function",
        WASM,
        "                    trap_index(self.realloc_index, Cause::RepeatedKey),\n",
        "                    trap_index(self.realloc_index, Cause::IntOverflow),\n",
    ),
    (
        "lists of two lengths call another cause's function",
        WASM,
        "                    trap_index(self.realloc_index, Cause::ListsOfTwoLengths),\n",
        "                    trap_index(self.realloc_index, Cause::RepeatedKey),\n",
    ),
    (
        "a code point not a scalar calls another cause's function",
        WASM,
        "I::Call(trap_index(realloc_index, Cause::NotAScalar)),\n",
        "I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),\n",
    ),
    (
        "memory that does not grow calls another cause's function",
        WASM,
        "        I::BrIf(0),\n        I::Call(trap_index(realloc_index, Cause::MemoryExhausted)),\n",
        "        I::BrIf(0),\n        I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),\n",
    ),
    (
        "the host reads no cause from the frame a trap stopped in",
        HOST,
        "                    .find_map(|f| f.func_name()?.strip_prefix(TRAP_PREFIX))\n",
        "                    .find_map(|f| f.func_name()?.strip_prefix(\"no such name\"))\n",
    ),
    (
        "a zero divisor is not named",
        HOST,
        "                Trap::IntegerDivisionByZero => \"division by zero\",\n",
        "",
    ),
    (
        "calls nested too deep are not named",
        HOST,
        "                Trap::StackOverflow => \"calls nested too deep\",\n",
        "",
    ),
    (
        "the least Int divided by -1 is not named",
        HOST,
        "                Trap::IntegerOverflow => \"Int overflow\",\n",
        "",
    ),
    (
        "fuel spent is not named",
        HOST,
        "                Trap::OutOfFuel => \"out of fuel\",\n",
        "",
    ),
    (
        "a join too long for memory calls another cause's function",
        WASM,
        # Its own bound's, not `List.range`'s (the ranges' ADR).
        "                I::I64Const(i32::MAX as i64),\n"
        "                I::I64GtU,\n"
        "                I::If(Empty),\n"
        "                I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),\n",
        "                I::I64Const(i32::MAX as i64),\n"
        "                I::I64GtU,\n"
        "                I::If(Empty),\n"
        "                I::Call(trap_index(realloc_index, Cause::MemoryExhausted)),\n",
    ),
    (
        "nodes that are no tree call another cause's function",
        WASM,
        "            I::Call(trap_index(realloc_index, Cause::NotATree)),\n",
        "            I::Call(trap_index(realloc_index, Cause::TooLongForMemory)),\n",
    ),
    (
        "the host's engine captures no backtrace",
        HOST,
        "        config.wasm_backtrace_max_frames(std::num::NonZeroUsize::new(20));\n",
        "        config.wasm_backtrace_max_frames(None);\n",
    ),
    (
        "the host's engine inlines",
        HOST,
        "        config.compiler_inlining(wasmtime::Inlining::No);\n",
        "        config.compiler_inlining(wasmtime::Inlining::Yes);\n",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "trap_causes"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "recursive_types"],
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
