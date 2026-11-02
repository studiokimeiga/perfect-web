#!/usr/bin/env python3
"""Mutation controls for track `row-ownership` (ADR-0384's H1 and H2):
whose a table's rows are, read and written by their reader alone, and a
predicate the program gives a body, asked by the host.

Each mutant undoes one guarantee and must be killed by a test:
1. the compiler: an owned read takes the reader's handle, never a plain id;
   a sharer is answered the seen row; an owned table gives no `all`; tables.json
   names each function's reader; PW0367, PW0368 and PW0369 each refuse;
   `private` is a declaration's visibility where a declaration's word follows;
2. the host: a handle is its caller's, and a call no one makes reads no
   owned row; an owned row is written by its owner alone; a set form's handle
   is given back to each key; a sharer is answered the seen row; the manifest
   is held whole for its owners; an owned import is typed as owned;
3. predicates (H2): a body answers a Bool and only reads (PW6905); a query
   may require one; a requirement names the component that answers it; the
   host asks the program's predicate, never the deployment, for it; its
   reads are its command's transaction's.

Run whole from the repository root (`just e14-row-ownership`). Every source
is restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record is
a whole run.
"""
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ROOT / "compiler/pw-core/src/tables.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
PREDICATES = ROOT / "compiler/pw-core/src/predicates.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
OWNERS = ROOT / "runtime/pw-host/src/engine/table_owners.rs"
LINK = ROOT / "runtime/pw-host/src/engine/table_link.rs"
HOST_TABLES = ROOT / "runtime/pw-host/src/tables.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"

MUTANTS = [
    # 1. the compiler
    ("an owned read takes a plain id", TABLES,
     '    let handle = |f: &str| DeclaredType::new("capability.User", vec![ty(f).unwrap_or_else(unit)]);\n',
     "    let handle = |f: &str| ty(f).unwrap_or_else(unit);\n"),
    ("a sharer is answered the whole row", TABLES,
     "        (Reading::Sharer, Some(s)) => DeclaredType::new(s.name.clone(), Vec::new()),\n",
     "        (Reading::Sharer, Some(_)) => row_ty.clone(),\n"),
    ("an owned table gives all", TABLES,
     "    if let Some(b) = &t.bounded\n        && t.owner.is_none()\n",
     "    if let Some(b) = &t.bounded\n        && (t.owner.is_none() || true)\n"),
    ("tables.json names no reader", TABLES,
     "        Primitive::Insert | Primitive::Put => Some(owner.to_string()),\n        _ => by.first().cloned(),\n",
     "        Primitive::Insert | Primitive::Put => None.filter(|_: &String| owner.is_empty()),\n        _ => None.filter(|_: &String| by.is_empty()),\n"),
    ("PW0367 an owner may be no principal's id", TABLES,
     "            Some((_, Some(ty))) if !principal_id(sigs, ty) => Err(format!(\n",
     "            Some((_, Some(ty))) if false && !principal_id(sigs, ty) => Err(format!(\n"),
    ("PW0368 an owned key may lead with another", TABLES,
     "        && k.value.first().is_some_and(|f| f != owner)\n",
     "        && k.value.first().is_some_and(|f| f != owner && false)\n"),
    ("PW0368 an owned index may lead with no reader", TABLES,
     "        if index.fields.first().is_some_and(|f| !is_reader(f)) {\n",
     "        if index.fields.first().is_some_and(|f| !is_reader(f) && false) {\n"),
    ("PW0368 a unique index may hold no owner", TABLES,
     "        if index.unique && !index.fields.iter().any(|f| f == owner) {\n",
     "        if false && index.unique && !index.fields.iter().any(|f| f == owner) {\n"),
    ("PW0368 all of an owned table is not refused", TABLES,
     '    if member == "all"\n        && let Some(owner) = t.owner_field()\n',
     '    if member == "all"\n        && let Some(owner) = t.owner_field().filter(|_| false)\n'),
    ("PW0369 a private field may be the order of a shared table", TABLES,
     "        } else if !sharers.is_empty() && t.order.as_ref().is_some_and(|o| o.value == *name) {\n",
     "        } else if false && !sharers.is_empty() && t.order.as_ref().is_some_and(|o| o.value == *name) {\n"),
    ("PW0369 a private field may be the owner", TABLES,
     "        } else if name == owner {\n",
     "        } else if false && name == owner {\n"),
    ("private before a declaration's word heads a clause", GRAMMAR,
     "        if DECL_STARTERS.contains(&next) || UI_NOUNS.contains(&next) {\n",
     "        if false && (DECL_STARTERS.contains(&next) || UI_NOUNS.contains(&next)) {\n"),
    # 2. the host
    ("a forged handle is believed", OWNERS,
     "        Some(Val::String(given)) if given == user => {}\n",
     "        Some(Val::String(given)) if given == user || true => {}\n"),
    ("a call no one makes reads owned rows", OWNERS,
     "    let Caller::User(user) = caller else {\n",
     "    let nobody = String::new();\n    let user = match caller { Caller::User(user) => user, Caller::Nobody => &nobody };\n    if false {\n"),
    ("an owned row may be written for another", OWNERS,
     "                Some(Val::String(owner)) if owner == user => Ok(vec![row.clone()]),\n",
     "                Some(Val::String(owner)) if owner == user || true => Ok(vec![row.clone()]),\n"),
    ("a set form's handle is not given back to its keys", OWNERS,
     "            Ok(vec![Val::List(keys)])\n",
     "            let _ = keys;\n            Ok(args)\n"),
    ("a sharer is answered the private fields", OWNERS,
     "    let Some(seen) = read.handle.as_ref().and_then(|h| h.seen.as_ref()) else {\n",
     "    let Some(seen) = read.handle.as_ref().and_then(|h| h.seen.as_ref()).filter(|_| false) else {\n"),
    ("the manifest holds no owners", HOST_TABLES,
     "            table.owned_whole()?;\n",
     "            let _ = table.owned_whole();\n"),
    ("an owned import is typed as an unowned one", LINK,
     "    if access.handle().is_some() {\n",
     "    if access.handle().is_some() && false {\n"),
    ("the engine admits no handle", HOST,
     "                            Some(function) => table_owners::admit(function, &caller, args)\n",
     "                            Some(_) if false => table_owners::admit(function.as_ref().unwrap(), &caller, args)\n"),
    # 3. predicates
    ("a predicate's body is not held to its Bool", VALUES,
     "        || (decl.kind == DeclKind::Predicate && decl.body.is_some())\n",
     "        || (decl.kind == DeclKind::Predicate && decl.body.is_some() && false)\n"),
    ("PW6905 a predicate's body may write", PREDICATES,
     '                !matches!(family, "database.read" | "session.read")\n',
     '                !matches!(family, "database.read" | "session.read") && false\n'),
    ("a query may not require a predicate", CHECK,
     "                crate::hir::DeclKind::Command | crate::hir::DeclKind::Query\n",
     "                crate::hir::DeclKind::Command\n"),
    ("a requirement names no component", CONTRACT,
     "                requirement.component = asked_by(hirs, ws, unit, &requirement.predicate);\n",
     "                requirement.component = asked_by(hirs, ws, unit, &requirement.predicate).filter(|_| false);\n"),
    ("the deployment is asked the program's predicate", HOST,
     "                Some(component) => program(component, selected),\n",
     "                Some(_) => deployment(predicate, selected),\n"),
    ("a predicate's reads leave its command's transaction", MEMORY,
     "                    .command_read(id, &bound, args)\n                    .map_err(|error| format!(\"`{bound}`: {error:?}\"))\n            });\n            out.insert(import, table_read_host::host_fn(read, data));\n",
     "                    .read_published(&bound, args)\n                    .map_err(|error| format!(\"`{bound}`: {error:?}\"))\n            });\n            let _ = id;\n            out.insert(import, table_read_host::host_fn(read, data));\n"),
    ("a predicate's reads are given to a command", MEMORY,
     '            matches!(contract.exports.as_slice(), [only] if only.kind == "predicate");\n',
     '            matches!(contract.exports.as_slice(), [only] if only.kind == "predicate" || true);\n'),
]

# Each Cargo test binary separately: names can repeat between binaries, and
# the shared native assertion validator expects one complete inventory.
TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "table_owners"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "predicate_bodies"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-syntax", "--lib"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-host", "--features", "engine", "--lib"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_owners"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_predicates"],
]

SOURCES = [
    ROOT / "compiler/pw-core/tests/table_owners.rs",
    ROOT / "compiler/pw-core/tests/predicate_bodies.rs",
    ROOT / "compiler/pw-syntax/src/grammar.rs",
    *sorted((ROOT / "runtime/pw-host/src").rglob("*.rs")),
    ROOT / "runtime/pw-table-memory/tests/compiled_owners.rs",
    ROOT / "runtime/pw-table-memory/tests/compiled_predicates.rs",
    ROOT / "runtime/pw-table-memory/tests/support/owned.rs",
    ROOT / "runtime/pw-table-memory/tests/support/assert_result.rs",
]
INVENTORY = {}


def run_tests():
    passed, failed = 0, 0
    for cmd in TESTS:
        full = [*cmd, "--", "--format=pretty"]
        try:
            child = producer_process.run(full, cwd=ROOT, timeout=900)
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
    print(f"{len(chosen) - survivors} of {len(chosen)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
