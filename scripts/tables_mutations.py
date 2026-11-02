#!/usr/bin/env python3
"""D1's declaration, generated interfaces, warnings and released-schema boundary.

Run whole from the repository root. Every source is restored in finally;
the unmutated baseline must pass before any mutant means anything.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ROOT / "compiler/pw-core/src/tables.rs"
LOWER = ROOT / "compiler/pw-core/src/lower.rs"
BUILD = ROOT / "compiler/pw-core/src/build.rs"
PROGRAM = ROOT / "compiler/pw-core/src/program.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
CLI = ROOT / "compiler/pw-cli/src/main.rs"
BACKEND = ROOT / "compiler/pw-core/src/backend/mod.rs"
WIT = ROOT / "compiler/pw-core/src/wit.rs"
RESOLVE = ROOT / "compiler/pw-core/src/resolve.rs"

MUTANTS = [
    ("lowering gives no table functions", LOWER,
     "    crate::tables::generate(&mut lo.hir);\n", ""),
    ("table imports belong to the app instead of the platform", BACKEND,
     "    let binding = match [PLATFORM_PACKAGE, TABLES_PACKAGE]\n",
     "    let binding = match [PLATFORM_PACKAGE]\n"),
    ("an empty Result case is emitted as an invalid empty tuple", WIT,
     "            Builtin::Result => match (unit(0), unit(1)) {\n",
     "            Builtin::Result => match (unit(0) && false, unit(1) && false) {\n"),
    ("a key names a field twice", TABLES,
     "        if out.iter().any(|o| o == n) {\n", "        if false && out.iter().any(|o| o == n) {\n"),
    ("a table has any module's name", TABLES,
     "    if t.name != module || second {\n", "    if second {\n"),
    ("duplicate source names hide a database", TABLES,
     "        found.extend(unambiguous_sources(hirs, unit, ws, t));\n", ""),
    ("an alternate own module counts as another database", TABLES,
     "            .filter(|(source_unit, _)| !alternate_own_module(*source_unit))\n",
     "            .filter(|(source_unit, _)| true || !alternate_own_module(*source_unit))\n"),
    ("a sibling module replaces a table reference's own module", TABLES,
     "    let m = ws.visible_module(unit, name)?;\n",
     "    let m = ws.modules.iter().find(|m| m.name == name)?;\n"),
    ("a qualified own-module name resolves through a namesake", RESOLVE,
     "        if own.name == name {\n", "        if false && own.name == name {\n"),
    ("table interfaces collide after WIT naming", TABLES,
     "        if other.name != t.name && interface_of(&other.name) == interface {\n",
     "        if false && other.name != t.name && interface_of(&other.name) == interface {\n"),
    ("function imports collide after WIT naming", TABLES,
     "        found.extend(distinct_imports(hirs, unit, t));\n", ""),
    ("generated record names can hold two tuple shapes", TABLES,
     "            .is_some_and(|first| first.as_slice() != fields)\n",
     "            .is_some_and(|first| false && first.as_slice() != fields)\n"),
    ("nested generic parameters lose their stored arguments", TABLES,
     "            .get(&binding)\n", "            .get(&binding).filter(|_| false)\n"),
    ("a key's fields are unchecked", TABLES,
     '        Some(k) => fields_of("key", &k.value, &k.span, &mut out),\n', "        Some(_) => {},\n"),
    ("an index's fields are unchecked", TABLES,
     "        fields_of(clause, &index.fields, &i.span, &mut out);\n", ""),
    ("order names any field", TABLES,
     '        fields_of("order", std::slice::from_ref(&o.value), &o.span, &mut out);\n', ""),
    ("an index need not start by_", TABLES,
     '        if !index.name.starts_with("by_") {\n', '        if false && !index.name.starts_with("by_") {\n'),
    ("two indexes give the same function", TABLES,
     "            if let Some(first) = seen.get(&name) {\n", "            if let Some(first) = seen.get(&name).filter(|_| false) {\n"),
    ("a unique index is ordinary", TABLES,
     '            "index" | "unique" => match index_clause(v, p.name == "unique") {\n',
     '            "index" | "unique" => match index_clause(v, false) {\n'),
    ("the whole key has no set form", TABLES,
     '            name: "get_many".to_string(),\n', '            name: "get_batch".to_string(),\n'),
    ("the longest proper key prefix is missing", TABLES,
     "        (1..k.value.len()).map(|j| k.value[..j].to_vec()).collect()\n",
     "        (1..k.value.len().saturating_sub(1)).map(|j| k.value[..j].to_vec()).collect()\n"),
    ("bounded gives no whole-table read", TABLES,
     "    if let Some(b) = &t.bounded\n        && t.owner.is_none()\n", "    if let Some(b) = t.bounded.as_ref().filter(|_| false)\n        && t.owner.is_none()\n"),
    ("a set form answers an Option", TABLES,
     "                    ret: list(answer(r)),\n                    primitive: Primitive::GetMany,\n",
     "                    ret: option(answer(r)),\n                    primitive: Primitive::GetMany,\n"),
    ("a put ignores uniqueness", TABLES,
     "            ret: match unique {\n", "            ret: match unique && false {\n"),
    ("a reference may hold the wrong key", TABLES,
     "    if held {\n", "    if held || true {\n"),
    ("a reference can hold one field of a composite key", TABLES,
     "    if key.value.len() != 1 {\n", "    if false && key.value.len() != 1 {\n"),
    ("cross-source references are silent", TABLES,
     "            (theirs != mine).then(|| {\n", "            (false && theirs != mine).then(|| {\n"),
    ("cross-source references are errors", TABLES,
     '                    "refers_across_sources",\n                    Severity::Warning,\n',
     '                    "refers_across_sources",\n                    Severity::Error,\n'),
    ("bindings written by hand are allowed", TABLES,
     "    out.extend(bindings(hirs, unit, ws));\n", ""),
    ("all without a bound uses the wrong diagnostic", TABLES,
     '    if member == "all" && t.bounded.is_none() {\n',
     '    if false && member == "all" && t.bounded.is_none() {\n'),
    ("a changed schema lock is accepted", TABLES,
     "        if hash != lock.hash {\n", "        if false && hash != lock.hash {\n"),
    ("an unknown lock format is accepted", TABLES,
     "        if lock.schema != LOCK_FORMAT {\n", "        if false && lock.schema != LOCK_FORMAT {\n"),
    ("destructive field changes are planned silently", TABLES,
     "        for f in &old.fields {\n", "        for f in old.fields.iter().take(0) {\n"),
    ("renames are not applied", TABLES,
     "                renamed.insert(from.clone(), m.value.field.clone());\n", ""),
    ("new optional fields are not additive", TABLES,
     "            if let Stored::Option(_) = g.stored {\n",
     "            if let Stored::Option(_) = g.stored && false {\n"),
    ("a changed key is accepted", TABLES,
     "        if old_key != schema.key {\n", "        if false && old_key != schema.key {\n"),
    ("a table can move between sources silently", TABLES,
     "        if old.source != schema.source {\n", "        if false && old.source != schema.source {\n"),
    ("an unused migration is accepted", TABLES,
     "            if used.contains(&i) || !shaped(&m.value, &schema.fields) {\n",
     "            if true || used.contains(&i) || !shaped(&m.value, &schema.fields) {\n"),
    ("a backfill of the wrong type is accepted", TABLES,
     "                Some((_, Some(ty))) => fits(sigs, ty, value)\n",
     "                Some((_, Some(ty))) => fits(sigs, ty, value).or(Ok::<(), String>(()))\n"),
    ("a prefix's rows have no declared order", TABLES,
     "    for f in grouped.iter().chain(t.order.iter()).chain(t.key.iter()) {\n",
     "    for f in grouped.iter().chain(t.key.iter()) {\n"),
    ("build ignores the released schema", PROGRAM,
     "            crate::tables::tables_json(cx.hirs, cx.ws, cx.sigs, released);\n",
     "            crate::tables::tables_json(cx.hirs, cx.ws, cx.sigs, None);\n"),
    ("check ignores the released schema", CHECK,
     "        for (unit, diagnostic) in crate::tables::held_to(&hirs, &ws, &sigs, released).1 {\n",
     "        for (unit, diagnostic) in crate::tables::held_to(&hirs, &ws, &sigs, None).1 {\n"),
    ("a no-table rebuild keeps old metadata", BUILD,
     '            for name in ["tables.json", "schema.lock"] {\n',
     '            for name in ["tables.json", "schema.lock"].into_iter().take(0) {\n'),
    ("CLI silently ignores a missing schema-lock value", CLI,
     "    if release_path == Some(None) {\n", "    if false && release_path == Some(None) {\n"),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "tables"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-cli", "--test", "tables"],
]

def run_tests():
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", r.stdout + r.stderr)
        if not results:
            built = False
        for p, f in results:
            passed += int(p)
            failed += int(f)
    return built, passed, failed

def main():
    import mutation_baseline  # keeps every command and reports a red baseline
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
        print(f"{what}: {verdict}", flush=True)
    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0

if __name__ == "__main__":
    sys.exit(main())
