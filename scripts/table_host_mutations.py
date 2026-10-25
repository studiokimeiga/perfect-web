#!/usr/bin/env python3
"""Mutation controls for D2's read path: a compiled component reads its
tables through the table gate, typed, exact, and held to what tables.json
declares (ADR-0338 D2; PRs #29 to #40, integrated, and the integrator's
rulings of 2026-10-25 on them).

Each mutant undoes one piece and must be killed by a test:
1. the manifest held whole where the host reads it (`pw_host::tables`):
   each table's interface its name's, each import at its own table's
   interface and name, one function an import, each effect its
   primitive's, every field a function names its table's, a get and a
   get-many by the table's key;
2. the gate (`Manifest::admit_reads`, PR #29's): a get or get-many the
   manifest declares, the host's own import, its table's capability,
   granted; the tables' namespace however it is spelt;
3. the gate the only way a table import links (`gate_links`, the engine's
   calls and instantiation);
4. a table import typed as tables.json declares its read, where it is
   linked (`engine::table_link::check_import`): a get's key values and row,
   a get-many's keys and rows, a row's fields counted, named and typed;
5. a key held to its declared type and bounds before an adapter reads it
   (`table_selectors`), a composite key's fields by their own names;
6. a row encoded as its declared record (`table_results`): its order, its
   WIT names, its types and bounds, no field more;
7. every answer held to its read where the engine passes it on
   (`table_link::check_answer`): a get's row its key's; a get-many's rows of
   keys asked, each once, in order;
8. a row exactly its table's there: no field more, none less, none out of
   place, each of its type and within its bounds.

Run whole from the repository root (`just e14-table-host`). Every source is
restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record is
a whole run.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ROOT / "runtime/pw-host/src/tables.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"
LINK = ROOT / "runtime/pw-host/src/engine/table_link.rs"
KEYS = ROOT / "runtime/pw-host/src/engine/table_selectors.rs"
ROWS = ROOT / "runtime/pw-host/src/engine/table_results.rs"

MUTANTS = [
    # 1. the manifest, whole
    ("a table's interface need not be its name's", TABLES,
     "            if table.interface != interface {\n",
     "            if false && table.interface != interface {\n"),
    ("a table may repeat a field", TABLES,
     "            if fields.len() != table.fields.len() {\n",
     "            if false && fields.len() != table.fields.len() {\n"),
    ("a key may name no field", TABLES,
     "            if table.key.is_empty() || !table.key.iter().all(known) {\n",
     "            if table.key.is_empty() {\n"),
    ("an order may name no field", TABLES,
     "            if !table.order.iter().all(known) {\n",
     "            if false && !table.order.iter().all(known) {\n"),
    ("an import may sit outside its table's interface", TABLES,
     "                if function.import != import {\n",
     "                if false && function.import != import {\n"),
    ("an import may be two functions'", TABLES,
     "                if !imports.insert(function.import.as_str()) {\n",
     "                if !imports.insert(function.import.as_str()) && false {\n"),
    ("a get may be imported elsewhere than #get", TABLES,
     "                    && function.import != format!(\"{interface}#{served}\")\n",
     "                    && false\n"),
    ("an effect need not be its primitive's", TABLES,
     "                if function.effect != effect {\n",
     "                if false && function.effect != effect {\n"),
    ("a function may name a field its table lacks", TABLES,
     "                if !function.by.iter().chain(&function.sorted_by).all(known) {\n",
     "                if false && !function.by.iter().chain(&function.sorted_by).all(known) {\n"),
    ("a get may read by other than the key", TABLES,
     "                    && function.by != table.key\n",
     "                    && false\n"),
    # 2. the gate
    ("the gate leaves out a table import its manifest does not declare", TABLES,
     "                return Err(refused(\n"
     "                    \"tables.json declares no get or get-many there\".into(),\n"
     "                ));\n",
     "                continue;\n"),
    ("the gate binds a component's import", TABLES,
     "            if import.kind != ImportKind::HostCapability {\n",
     "            if false && import.kind != ImportKind::HostCapability {\n"),
    ("the gate trusts an import's claim of its capability", TABLES,
     "            if import.capability != capability {\n",
     "            if false && import.capability != capability {\n"),
    ("the gate binds an ungranted read", TABLES,
     "            if !linkable.contains(&key) {\n",
     "            if false && !linkable.contains(&key) {\n"),
    ("a versioned spelling of the namespace is no table's", TABLES,
     '        || interface.starts_with("pw:tables@")\n',
     "        || false\n"),
    ("the bare namespace is no table's", TABLES,
     '    interface == "pw:tables"\n',
     "    false\n"),
    # 3. the only way
    ("a call links table imports without the gate", HOST,
     "        let table_reads = crate::tables::gate_links(contract, granted, tables)?;\n",
     "        let table_reads = { let _ = tables; std::collections::BTreeMap::<String, crate::tables::Read>::new() };\n"),
    ("instantiation links table imports with stubs", HOST,
     "        crate::tables::gate_links(contract, granted, None)?;\n",
     ""),
    ("the gate is skipped when a manifest is given", TABLES,
     "        Some(tables) => tables.admit_reads(contract, granted),\n",
     "        Some(_) => Ok(BTreeMap::new()),\n"),
    ("no import is a table's", TABLES,
     "    is_table_interface(key.split('#').next().unwrap_or(key))\n",
     "    is_table_interface(key.split('#').next().unwrap_or(key)) && false\n"),
    # 4. a table import's type, where it is linked
    ("a table import links whatever its type", HOST,
     "                    table_link::check_import(read, &ty)?;\n",
     "                    let _ = (read, ty);\n"),
    ("a get-many's key type goes unchecked", LINK,
     "        key(read, &keys.ty()).map_err(refused)?;\n",
     "        let _ = key(read, &keys.ty());\n"),
    ("a get-many's rows go unchecked", LINK,
     "        record(&read.row, &rows.ty()).map_err(refused)?;\n",
     "        let _ = record(&read.row, &rows.ty());\n"),
    ("a get's row goes unchecked", LINK,
     "        record(&read.row, &row.ty()).map_err(refused)?;\n",
     "        let _ = record(&read.row, &row.ty());\n"),
    ("a get takes any number of key values", LINK,
     "        if params.len() != read.by.len() {\n",
     "        if false && params.len() != read.by.len() {\n"),
    ("a get's key values go untyped", LINK,
     "            stored(&column.stored, ty)\n"
     "                .map_err(|why| refused(format!(\"its key's `{}`: {why}\", column.name)))?;\n",
     "            let _ = (stored(&column.stored, ty), column);\n"),
    ("a row may hold more fields than tables.json", LINK,
     "    if fields.len() != columns.len() {\n",
     "    if false && fields.len() != columns.len() {\n"),
    ("a row's fields go by any names", LINK,
     "        if field.name != column.wit {\n",
     "        if false && field.name != column.wit {\n"),
    ("a row's fields go untyped", LINK,
     "        stored(&column.stored, &field.ty).map_err(|why| format!(\"`{}`: {why}\", column.name))?;\n",
     "        let _ = stored(&column.stored, &field.ty);\n"),
    # 5. a key, before an adapter reads it
    ("a key goes unchecked against its declared type and bounds", KEYS,
     "    check_value(&column.stored, value)\n"
     "        .map_err(|why| format!(\"key field `{}`: {why}\", column.name))?;\n",
     "    let _ = (check_value(&column.stored, value), column);\n"),
    ("a composite key's fields go by place, not name", KEYS,
     "                if *name != column.wit {\n",
     "                if false && *name != column.wit {\n"),
    ("a composite key may have fewer fields", KEYS,
     "            if fields.len() != columns.len() {\n",
     "            if false && fields.len() != columns.len() {\n"),
    ("a get takes any number of key fields", KEYS,
     "    if args.len() != read.by.len() {\n",
     "    if false && args.len() != read.by.len() {\n"),
    # 6. a row, encoded as its record
    ("an encoded row keeps a field it does not declare", ROWS,
     "        .find(|name| !columns.iter().any(|column| &column.name == *name))\n",
     "        .find(|name| false && !columns.iter().any(|column| &column.name == *name))\n"),
    ("an encoded row comes in another order", ROWS,
     "        fields.push((column.wit.clone(), encoded));\n",
     "        fields.insert(0, (column.wit.clone(), encoded));\n"),
    ("an encoded row keeps the Pleris names", ROWS,
     "        fields.push((column.wit.clone(), encoded));\n",
     "        fields.push((column.name.clone(), encoded));\n"),
    ("an encoded value goes unbounded", ROWS,
     "    check_value(stored, &encoded)?;\n",
     "    let _ = check_value(stored, &encoded);\n"),
    # 7. an answer, its read's
    ("a get may answer another key's row", LINK,
     "                if key == asked {\n",
     "                if true || key == asked {\n"),
    ("a get-many may answer a key not asked for", LINK,
     "        if !asked.contains(&key) {\n",
     "        if false && !asked.contains(&key) {\n"),
    ("a get-many may answer a key twice", LINK,
     "        if !seen.insert(key.clone()) {\n",
     "        if !seen.insert(key.clone()) && false {\n"),
    ("a get-many may answer out of order", LINK,
     "        if last.as_ref().is_some_and(|last| *last > place) {\n",
     "        if false && last.as_ref().is_some_and(|last| *last > place) {\n"),
    ("no answer is held to its read", HOST,
     "                            table_link::check_answer(read, &args, &out).map_err(|why| {\n",
     "                            { let _ = (read, &args); Ok::<(), String>(()) }.map_err(|why| {\n"),
    # 8. a row, exactly its table's
    ("an answered row may lack a field", LINK,
     "            return Err(format!(\"the row lacks `{}`\", column.wit));\n",
     "            continue;\n"),
    ("an answered row's fields go by any names", LINK,
     "        if *name != column.wit {\n",
     "        if false && *name != column.wit {\n"),
    ("an answered row may hold a field more", LINK,
     "    if let Some((extra, _)) = given.get(columns.len()) {\n",
     "    if let Some((extra, _)) = given.get(usize::MAX) {\n"),
    ("an answered row's values go untyped", LINK,
     "        check_value(&column.stored, value).map_err(|why| format!(\"`{}`: {why}\", column.wit))?;\n",
     "        let _ = check_value(&column.stored, value);\n"),
    ("an Int goes unbounded", LINK,
     "            if at_least.is_some_and(|least| *n < least) || at_most.is_some_and(|most| *n > most) {\n",
     "            if false && (at_least.is_some_and(|least| *n < least) || at_most.is_some_and(|most| *n > most)) {\n"),
    ("a String goes unbounded", LINK,
     "            if min_length.is_some_and(|least| length < least)\n",
     "            if false && min_length.is_some_and(|least| length < least)\n"),
    ("a Float need not be finite", LINK,
     "        (Stored::Float, Val::Float64(x)) if x.is_finite() => Ok(()),\n",
     "        (Stored::Float, Val::Float64(x)) if x.is_finite() || x.is_nan() => Ok(()),\n"),
]

TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-host", "--features", "engine", "--lib"],
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
    chosen = [m for m in MUTANTS if not sys.argv[1:] or any(w in m[0] for w in sys.argv[1:])]
    survivors = 0
    for what, path, anchor, replacement in chosen:
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
    print(f"{len(chosen) - survivors} of {len(chosen)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
