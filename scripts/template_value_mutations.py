#!/usr/bin/env python3
"""Mutation controls for ADR-0073: a template reads each value by path.

Each mutant undoes one piece of how a template's values are read: a computed
hole read by the path the compiler names (ADR-0226; until then lowered as a
blocked part), `pw build` refusing a blocked part, a directive other than
`on:`, and a loop's key read as the path from its element, by the checker,
the template IR and the renderer. The template-value tests or the renderer's
key tests must then fail.

Run from the repository root; `just e10-template-values` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
IR = ROOT / "compiler/pw-core/src/template_ir.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
RENDER = ROOT / "runtime/pw-render/src/lib.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "`pw build` writes a blocked part",
        BUILD,
        "        for blocked in t.blocked() {",
        "        for blocked in t.blocked().into_iter().take(0) {",
    ),
    (
        "a computed text hole lowers with an empty path",
        IR,
        # Re-anchored by ADR-0232: the path as the template reads it.
        "        Node::Interpolation(e) => out.push(Chunk::Dynamic(match ctx.path(body, *e) {",
        "        Node::Interpolation(e) => out.push(Chunk::Dynamic(match Some(crate::infer::path_of(body, *e)) {",
    ),
    (
        "a computed attribute lowers with an empty path",
        IR,
        # Re-anchored by ADR-0226: a computed attribute is read by the path the
        # compiler names, where it was refused.
        "                let (value, inputs) = match ctx.path(body, *e).map(|v| ctx.read(v)) {",
        "                let (value, inputs) = match Some(crate::infer::path_of(body, *e)) {",
    ),
    (
        "a field of a computed value is a path",
        IR,
        # Re-anchored by ADR-0232: one function reads every path, given which
        # `.value` is a representation.
        "        Expr::Field { base, name } => {\n"
        '            path_where(body, *base, representation).map(|b| format!("{b}.{name}"))\n',
        "        Expr::Field { base, name } => {\n"
        '            Some(format!("{}.{name}", path_where(body, *base, representation).unwrap_or_default()))\n',
    ),
    (
        "a directive is written as an attribute",
        IR,
        '            && !matches!(prefix, "xml" | "xlink" | "xmlns")',
        '            && matches!(prefix, "never")',
    ),
    (
        "an XML namespace is read as a directive",
        IR,
        '            && !matches!(prefix, "xml" | "xlink" | "xmlns")',
        '            && !matches!(prefix, "xlink" | "xmlns")',
    ),
    (
        "a computed list is read as a path",
        IR,
        # Re-anchored by track `device-store`, which lifts a list computed
        # from the signals it holds and refuses every other.
        "        let computed = value_path(body, each.list).is_none();\n"
        "        let collection = value_path(body, each.list).unwrap_or_default();\n",
        "        let computed = false;\n"
        "        let collection = value_path(body, each.list).unwrap_or(each.written.list.clone());\n",
    ),
    (
        "a key read from another name keys on the element",
        IR,
        "                if segments.next() != Some(binding.as_str()) {",
        "                if segments.next().is_none() {",
    ),
    (
        "a key keeps its last segment",
        IR,
        '                Some(segments.collect::<Vec<_>>().join("."))',
        "                Some(path.rsplit('.').next().unwrap_or_default().to_string())",
    ),
    (
        "a key's head is not read",
        CHECK,
        "        if from_row {\n",
        "        if true || from_row {\n",
    ),
    (
        "the renderer reads a key as one field",
        RENDER,
        "    path.split('.')\n"
        "        .filter(|s| !s.is_empty())\n"
        "        .try_fold(item, |v, segment| match v {\n"
        "            Value::Record(fields) => fields.get(segment),\n"
        "            _ => None,\n"
        "        })\n"
        "        .and_then(Value::as_str)\n"
        "        .unwrap_or_default()",
        "    match item {\n"
        "        Value::Record(fields) => fields.get(path).and_then(|v| v.as_str()).unwrap_or_default(),\n"
        "        other => other.as_str().unwrap_or_default(),\n"
        "    }",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "template_values"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-render", "--test", "keys"],
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
