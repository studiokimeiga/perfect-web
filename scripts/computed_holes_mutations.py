#!/usr/bin/env python3
"""Mutation controls for ADR-0226: a value the template computes compiles.

Each mutant undoes one piece: what a computed hole and a computed attribute
read; each refusal of what a host does not compute; the function run as the
last step, and the attribute set again; the component found by its name; the
host running it and rendering with it; the checker's rule that it performs
nothing, and what that rule leaves to others; and what A-032's first drafts
found, the qualified call check and a character between tags that the lexer
has no rule for. The tests of each must then fail.

The browser suite's `e2e/feed.spec.mjs` shows the thread's counts with
scripts off, and a like's reaching the open thread, in three engines.

Run from the repository root; `just e14-computed-holes` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "compiler/pw-core/src/template_ir.rs"
VALUES = ROOT / "compiler/pw-core/src/page_values.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
COMPONENT = ROOT / "compiler/pw-core/src/backend/component.rs"
WIT = ROOT / "compiler/pw-core/src/wit.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
SPECULATION = ROOT / "compiler/pw-core/src/backend/speculation.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
TYPES = ROOT / "compiler/pw-core/src/values.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a computed hole reads nothing",
        TEMPLATE,
        # Re-anchored by ADR-0228, which names its path by what it reads.
        "                let inputs = inputs_of(body, *e, ctx);\n                let value = ix.computed_path(id, &inputs);\n",
        "                let inputs = Vec::new();\n                let value = ix.computed_path(id, &inputs);\n",
    ),
    (
        "a computed attribute reads nothing",
        TEMPLATE,
        # Re-anchored by ADR-0228, as the hole's above.
        "                        let inputs = inputs_of(body, *e, ctx);\n",
        "                        let inputs = Vec::new();\n",
    ),
    (
        "a name a lambda binds is read from outside",
        TEMPLATE,
        "            Some(crate::lexical::Binder::Pattern(p)) => !inner.contains(&p),\n",
        "            Some(crate::lexical::Binder::Pattern(_)) => true,\n",
    ),
    (
        "a view that contains itself computes",
        TEMPLATE,
        "        if !computed_values(view).is_empty() {\n",
        "        if false && !computed_values(view).is_empty() {\n",
    ),
    (
        "a handler is a value the template computes",
        TEMPLATE,
        "                    if directive\n",
        "                    if false\n",
    ),
    (
        "a computed value may perform an effect at build",
        VALUES,
        "        .find(|s| s.span.start >= region.start && s.span.end <= region.end)\n",
        "        .find(|s| false && s.span.start >= region.start)\n",
    ),
    # "a host computes a value in a block" is retired (2026-10-06): since
    # ADR-0229 a host does, and the line it mutated is the refusal of a value
    # from a signal in a block, which computed_conditions_mutations.py's "a
    # value the browser would compute in a block is built" mutates. It
    # survived the chain at c7f999b, whose tests here do not read that one.
    (
        "a value of two queries is computed from the first",
        VALUES,
        "    let [(name, read)] = inputs else {\n",
        "    let [(name, read), ..] = inputs else {\n",
    ),
    # "a signal's value is not said to be the browser's" is retired: since
    # ADR-0227 the browser computes it, and computed_signals_mutations.py's
    # "a signal's value is a query's to compute" stands in its place.
    (
        "the lifted function is not run",
        VALUES,
        # Re-anchored by ADR-0228, whose host's branch is its own.
        "    out.push(Step::Derived(component_id));\n",
        "",
    ),
    (
        "a computed attribute is not set again",
        VALUES,
        "            && (found.iter().any(|(n, ..)| n == root) || computed_here)\n",
        "            && found.iter().any(|(n, ..)| n == root)\n",
    ),
    (
        "a computed value in a signal's block is said as a path",
        VALUES,
        # Re-anchored by track `device-store`: one the browser computes from
        # the signals it holds is read there (decision 7).
        "                    .any(|v| v.starts_with('#') && !computed.contains_key(v))\n",
        "                    .any(|v| false && v.starts_with('#') && !computed.contains_key(v))\n",
    ),
    (
        "the lifted function's contract is a query's",
        CONTRACT,
        '            kind: "derived".to_string(),\n',
        '            kind: "query".to_string(),\n',
    ),
    (
        "a world finds any function beside its declaration",
        COMPONENT,
        "            Some(name) => f.export == *name,\n",
        "            Some(_) => true,\n",
    ),
    (
        "a world does not name its lifted function",
        WIT,
        "            derived: lifted,\n",
        "            derived: None,\n",
    ),
    (
        "the lifted function is given nothing",
        LOWER,
        # Re-anchored by track `device-store`: its inputs, one or several.
        "        &inputs,\n",
        "        &[],\n",
    ),
    # "a value computed from a speculated one is built" is retired: since
    # ADR-0228 the speculation computes a text part from the value whole, and
    # computed_rows_mutations.py's "an attribute computed from a speculated
    # value is built" stands for what it still refuses.
    (
        "a value the template computes may perform an effect",
        CHECK,
        "    for value in computed {\n",
        "    for value in computed.into_iter().take(0) {\n",
    ),
    (
        "a page placed at build may not read its input in its template",
        CHECK,
        "        Reuse::Build => Vec::new(),\n",
        "        Reuse::Build => crate::template_ir::computed_values(body),\n",
    ),
    (
        "an escape hatch's effect is said to be impure",
        CHECK,
        '            if crate::effects::family_of(&source.effect) == "unsafe"\n',
        "            if false\n",
    ),
    (
        "an import's name is a local again",
        CHECK,
        # Tables, like imports, are module heads rather than local values.
        "                crate::hir::DeclKind::Import | crate::hir::DeclKind::Table\n",
        "                crate::hir::DeclKind::Table\n",
    ),
    (
        "a module a path starts with is a term",
        CHECK,
        "                if wildcards.contains(&nid) || heads.contains(&nid) {\n",
        "                if wildcards.contains(&nid) {\n",
    ),
    (
        "an inferred `Int` has no type",
        TYPES,
        "            Ty::Primitive(p) => TypeKey::Primitive(*p),\n",
        "            Ty::Primitive(_) => return None,\n",
    ),
    (
        "the host does not run the lifted function",
        SERVER,
        '                let out = self.run(derived, "", &BTreeMap::new(), &no_tables, &[value])?;\n',
        '                let out: Vec<Val> = vec![value];\n',
    ),
    (
        "a character with no rule between tags is unknown",
        GRAMMAR,
        "                self.toks[i].kind = Kind::MarkupText;\n",
        "                self.toks[i].kind = Kind::Unknown;\n",
    ),
    (
        "the host renders without a computed attribute's value",
        SERVER,
        '            .chain(plan["derived"].as_array().into_iter().flatten())\n',
        "",
    ),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-syntax", "--test", "corpus_lossless"],
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-core",
        "--test", "computed_holes", "--test", "template_values", "--test", "checking_source",
        "--test", "corpus_history", "--test", "rule_fixtures", "--test", "effects_through_values",
    ],
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "computed",
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
