#!/usr/bin/env python3
"""Mutation controls for ADR-0266's amendment (2026-11-17): a call's
arguments past the flat limit, to a host's operation, are stored in the
component's own memory, a tuple of them laid out as a record is, and the
call takes the one pointer.

Each mutant undoes one piece, and must be killed by a test of
`compiler/pw-conformance/tests/wide_arguments.rs` that fails at its
assertion (the strict producer: `cargo_assertion_results`, the binary's
inventory frozen at the baseline; a build failure, a crash, or a failure
anywhere but an assertion is no kill):

1. each argument's alignment within the tuple;
2. the offsets accumulating;
3. an argument held flat stored at its own offset, not the tuple's start;
4. an argument held in memory copied to its own offset;
5. an argument held in memory copied whole;
6. the tuple allocated at its own alignment;
7. the call passed the tuple's own pointer.

Not here, each for its reason: the call taking the one pointer at all (a
core module that passes flat values to an import that takes one pointer
does not validate, so no test runs it); the tuple's size (nothing is
allocated between the stores and the call but the answer's area, which the
host writes after it has read the arguments, so a tuple allocated short is
read whole all the same, until the region's end, which only a test that
sized the region to the allocation, a fragile one, could place).

Run whole from the repository root (`just e14-wide-parameters`). The source
is restored in finally; the unmutated baseline must pass before any mutant
counts.
"""

import pathlib
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import cargo_assertion_results  # noqa: E402 - after the path, as the scripts beside it
import mutation_source  # noqa: E402
import producer_process  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
WASM = ROOT / "compiler/pw-core/src/backend/wasm.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    ("a call's argument is stored where the last ended, unaligned", WASM,
     "                        end = end.next_multiple_of(a);\n",
     "                        end = end.next_multiple_of(a.min(1));\n"),
    ("a call's arguments' offsets do not accumulate", WASM,
     "                        end += size;\n",
     "                        end += size * 0;\n"),
    ("a call's argument held flat is stored at the tuple's start", WASM,
     "                                u64::from(*at),\n",
     "                                0,\n"),
    ("a call's argument held in memory is copied to the tuple's start", WASM,
     "                                    I::LocalGet(tuple),\n                                    I::I32Const(*at as i32),\n",
     "                                    I::LocalGet(tuple),\n                                    I::I32Const(0),\n"),
    ("a call's argument held in memory is copied short", WASM,
     "                                    I::I32Const(size as i32),\n                                    I::MemoryCopy {\n",
     "                                    I::I32Const((size / 2) as i32),\n                                    I::MemoryCopy {\n"),
    ("a call's arguments' tuple is allocated unaligned", WASM,
     "                        I::I32Const(align as i32),\n                        I::I32Const(end.next_multiple_of(align) as i32),\n",
     "                        I::I32Const(1),\n                        I::I32Const(end.next_multiple_of(align) as i32),\n"),
    ("a call passes another pointer than its tuple's", WASM,
     "                    self.ops.push(I::LocalGet(tuple));\n",
     "                    self.ops.push(I::I32Const(8));\n"),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-conformance", "--test", "wide_arguments"],
]

SOURCES = [ROOT / "compiler/pw-conformance/tests/wide_arguments.rs"]

INVENTORY = {}


def run_tests():
    passed, failed = 0, 0
    for cmd in TESTS:
        full = [*cmd, "--", "--format=pretty"]
        try:
            child = producer_process.run(full, cwd=ROOT, timeout=1200)
        except subprocess.TimeoutExpired:
            return False, passed, failed
        key = tuple(cmd)
        result = cargo_assertion_results.validate(child.returncode, child.stdout,
            root=ROOT, sources=SOURCES, expected=INVENTORY.get(key))
        if not result.valid:
            print(child.stdout[-4000:], flush=True)
            return False, passed, failed
        INVENTORY.setdefault(key, cargo_assertion_results.names(child.stdout))
        passed += result.passed
        failed += result.failed
    return True, passed, failed


def main():
    import mutation_baseline  # noqa: F401 - keeps every command and reports a red baseline
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1
    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}", flush=True)
            survivors += 1
            continue
        with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
            built, passed, failed = run_tests()
        if not built:
            verdict = "INVALID (build, infrastructure or native assertion producer)"
            survivors += 1
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
