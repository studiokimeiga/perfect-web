#!/usr/bin/env python3
"""Mutation controls for D2's writes and other reads (ADR-0386): a compiled
command writes its tables through the table gate, typed, staged, its writes
published whole at its commit or not at all, and every read primitive is
gated, typed and held to what it was asked.

Each mutant undoes one guarantee and must be killed by a test:
1. the manifest held whole for every primitive (`pw_host::tables`): what
   each reads by, its name, D1's order, an index one of its fields, `all`
   only of a bounded table;
2. the gate: a table's write only into a command, with its table's write
   capability; the engine's command is the contract's;
3. each import typed as tables.json declares it, where it links: a written
   row, a refusable write's result, a delete's key, a unique index's
   option, no Option of an Option;
4. each answer its function's: `all` within its bound, a write refused only
   by the row that holds what it would write, a write that cannot be
   refused answering nothing;
5. a written row exactly its table's before the adapter sees it;
6. the memory adapter: a staged write invisible to every other read until
   its commit (ADR-0338's control), visible to its own command's reads; an
   index kept on a write (ADR-0338's control); a unique value held once; an
   insert refused where its key is held; a command not committed undone
   whole, never published; a commit published whole; the bound and the
   references held at commit; one command at a time; an ended or prepared
   command writing nothing; a delete of a prefix that prefix's rows only;
   the tables a command wrote, what a host commits its outbox by; a refused
   write a value, not a trap; no Option of an Option stored;
7. which queries a write invalidates: what the command declares, which
   PW5106 holds whole for a table's write as for any other.

The read path's controls (`get`, `get-many`, the gate's reads, the row
codec) are `scripts/table_host_mutations.py`'s, re-anchored where ADR-0386
generalized them.

Run whole from the repository root (`just e14-d2-writes`). Every source is
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
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"
CHECK = ROOT / "compiler/pw-core/src/check.rs"

MUTANTS = [
    # 1. the manifest, whole for every primitive
    ("a prefix's read may be by other than a proper prefix of the key", TABLES,
     "        let prefix = !by.is_empty() && by.len() < self.key.len() && self.key[..by.len()] == *by;\n",
     "        let prefix = !by.is_empty();\n"),
    ("an index's read may be by other than one of its indexes", TABLES,
     "                    .find(|index| index.fields == by)\n",
     "                    .find(|_| true)\n"),
    ("all may be of an unbounded table", TABLES,
     "            Primitive::All if self.bounded.is_none() => Err(format!(\n",
     "            Primitive::All if false && self.bounded.is_none() => Err(format!(\n"),
    ("a function's rows may come in another order than D1's", TABLES,
     "                if function.sorted_by != sorted {\n",
     "                if false && function.sorted_by != sorted {\n"),
    ("a unique index's read is ordered as a list's", TABLES,
     "        if primitive.writes() || primitive == Primitive::Get || self.unique_index(primitive, by) {\n",
     "        if primitive.writes() || primitive == Primitive::Get {\n"),
    ("an insert, a put or all may be given fields", TABLES,
     "            Primitive::All | Primitive::Insert | Primitive::Put if !by.is_empty() => Err(format!(\n",
     "            Primitive::All | Primitive::Insert | Primitive::Put if false && !by.is_empty() => Err(format!(\n"),
    ("an index may be named as no index read is", TABLES,
     "                if !index.name.starts_with(\"by_\")\n",
     "                if false\n"),
    ("an index may be of no field", TABLES,
     "                    || index.fields.is_empty()\n",
     "                    || false\n"),
    ("an index may name a field its table lacks", TABLES,
     "                    || !index.fields.iter().all(known)\n",
     "                    || false\n"),
    ("an index may name a field twice", TABLES,
     "                    || each_once(&index.fields).len() != index.fields.len()\n",
     "                    || false\n"),
    ("an index's read may be named as another's set form", TABLES,
     "                            || other.name == reads\n",
     "                            || false\n"),
    ("two indexes may be on one's fields", TABLES,
     "                        other.fields == index.fields\n",
     "                        false\n"),
    ("a table may be bounded to no row", TABLES,
     "            if table.bounded == Some(0) {\n",
     "            if false && table.bounded == Some(0) {\n"),
    ("a key may name a field twice", TABLES,
     "            if each_once(&table.key).len() != table.key.len() {\n",
     "            if false && each_once(&table.key).len() != table.key.len() {\n"),
    # 2. the gate: a write, into a command, with its table's write capability
    ("a table's write links into a query", TABLES,
     "            if write && !command {\n",
     "            if false && write && !command {\n"),
    ("a write's capability is its table's read", TABLES,
     "                Access::Write(_) => (\"write\", true),\n",
     "                Access::Write(_) => (\"read\", true),\n"),
    ("the engine takes every export for a command", HOST,
     "        let command = export_kind(contract, export) == Some(\"command\");\n",
     "        let command = { let _ = export_kind(contract, export); true };\n"),
    # 3. typed where it links
    ("a written row goes untyped where it links", LINK,
     "            record(&write.row, row).map_err(|why| format!(\"its row: {why}\"))\n",
     "            { let _ = record(&write.row, row); Ok(()) }\n"),
    ("a refusable write may answer nothing where it links", LINK,
     "        let [Type::Result(answer)] = results else {\n"
     "            return Err(format!(\n"
     "                \"it answers ({}), not a result of the row that refuses it\",\n"
     "                listed(results)\n"
     "            ));\n"
     "        };\n",
     "        let [Type::Result(answer)] = results else {\n"
     "            return Ok(());\n"
     "        };\n"),
    ("a refusal's row goes untyped where it links", LINK,
     "        record(&write.row, &held).map_err(|why| format!(\"its refusal's row: {why}\"))?;\n",
     "        let _ = record(&write.row, &held);\n"),
    ("a write that cannot be refused may answer something where it links", LINK,
     "    } else if !results.is_empty() {\n",
     "    } else if false && !results.is_empty() {\n"),
    ("a delete's key goes untyped where it links", LINK,
     "        primitive => fields(primitive.by_words(), &write.by, params),\n",
     "        primitive => {\n"
     "            let _ = fields(primitive.by_words(), &write.by, params);\n"
     "            Ok(())\n"
     "        }\n"),
    ("a unique index's read is typed as a list where it links", LINK,
     "    if read.one() {\n"
     "        let [Type::Option(row)] = results else {\n",
     "    if read.primitive == Primitive::Get {\n"
     "        let [Type::Option(row)] = results else {\n"),
    ("an Option of an Option links", LINK,
     "        (Stored::Option(inner), _) if matches!(**inner, Stored::Option(_)) => Err(\n",
     "        (Stored::Option(inner), _) if false && matches!(**inner, Stored::Option(_)) => Err(\n"),
    # 4. each answer its function's
    ("all may answer past its table's bound", LINK,
     "        && rows.len() as u64 > bound\n",
     "        && false\n"),
    ("an insert may be refused by a row that holds nothing it would", LINK,
     "        Primitive::Insert => its == mine || shares,\n",
     "        Primitive::Insert => true,\n"),
    ("a put may be refused by its own key's row", LINK,
     "        _ => its != mine && shares,\n",
     "        _ => shares,\n"),
    ("a write that cannot be refused may answer a value", LINK,
     "            [] => Ok(()),\n"
     "            _ => Err(format!(\n"
     "                \"{} values where a {} answers nothing\",\n",
     "            _ if true => Ok(()),\n"
     "            _ => Err(format!(\n"
     "                \"{} values where a {} answers nothing\",\n"),
    ("a write that cannot be refused may be answered refused", ROWS,
     "        (false, None) => Ok(Vec::new()),\n",
     "        (false, _) => Ok(Vec::new()),\n"),
    # 5. a written row, exactly its table's, before the adapter sees it
    ("a written row reaches the adapter unchecked", KEYS,
     "    let fields = exact_row(columns, value).map_err(|why| format!(\"a row: {why}\"))?;\n",
     "    let Val::Record(fields) = value else {\n"
     "        return Err(\"a row is a record\".into());\n"
     "    };\n"),
    ("an Option of an Option is written as none", KEYS,
     "        (Stored::Option(inner), Val::Option(Some(_))) if matches!(**inner, Stored::Option(_)) => {\n",
     "        (Stored::Option(inner), Val::Option(Some(_))) if false && matches!(**inner, Stored::Option(_)) => {\n"),
    # 6. the memory adapter: staged, kept, published whole or not at all
    ("a staged write is visible before its commit", MEMORY,
     "        let tables = Arc::clone(&*self.published.lock().map_err(|_| Error::StoragePoisoned)?);\n"
     "        read(&self.shape, &tables, import, args)\n",
     "        let working = self.working.lock().map_err(|_| Error::StoragePoisoned)?;\n"
     "        read(&self.shape, &working.tables, import, args)\n"),
    ("a command's reads miss its own writes", MEMORY,
     "        let working = self.working_of(id)?;\n"
     "        read(&self.shape, &working.tables, import, args)\n",
     "        drop(self.working_of(id)?);\n"
     "        self.read_published(import, args)\n"),
    ("an index not kept on a write: a row's new values missed", MEMORY,
     "                index.insert((values, key.clone()));\n",
     "                let _ = (index, values);\n"),
    ("an index not kept on a write: a row's old values kept", MEMORY,
     "                index.remove(&(values, key.clone()));\n",
     "                let _ = (index, values);\n"),
    ("a unique value may be held twice", MEMORY,
     "                if declared.unique\n",
     "                if false && declared.unique\n"),
    ("an insert replaces its key's row", MEMORY,
     "            if function.primitive == Primitive::Insert\n"
     "                && let Some(existing) = data.rows.get(&key)\n",
     "            if false\n"
     "                && let Some(existing) = data.rows.get(&key)\n"),
    ("a command not committed is published", MEMORY,
     "            self.inner.end(self.id, false);\n",
     "            self.inner.end(self.id, true);\n"),
    ("a command not committed is not undone", MEMORY,
     "            replay(&self.shape, tables, &rows);\n",
     "            let _ = rows;\n"),
    ("a commit publishes part of a command", MEMORY,
     "            let rows = changed(tables, &ended.touched);\n",
     "            let rows: Vec<_> = changed(tables, &ended.touched).into_iter().take(1).collect();\n"),
    ("a commit's bound goes unheld", MEMORY,
     "                && tables[table].rows.len() as u64 > bound\n",
     "                && false\n"),
    ("all answers past its bound inside a command", MEMORY,
     "            if data.rows.len() as u64 > bound {\n",
     "            if false && data.rows.len() as u64 > bound {\n"),
    ("a reference to a missing row commits", MEMORY,
     "                        if !tables[reference.target].rows.contains_key(&referred) {\n",
     "                        if false && !tables[reference.target].rows.contains_key(&referred) {\n"),
    ("a row referred to may be deleted at commit", MEMORY,
     "                                && holding(&tables[referring].referring[place], key)\n"
     "                                    .next()\n"
     "                                    .is_some()\n",
     "                                && holding(&tables[referring].referring[place], key)\n"
     "                                    .next()\n"
     "                                    .is_some()\n"
     "                                && false\n"),
    ("a command begins while another is open", MEMORY,
     "        while working.open.is_some() {\n",
     "        while false && working.open.is_some() {\n"),
    ("an ended command's functions act on the open one", MEMORY,
     "            Some(open) if open.id == id => Ok(working),\n",
     "            Some(_) => Ok(working),\n"),
    ("a prepared command writes on", MEMORY,
     "        if open.prepared {\n",
     "        if false && open.prepared {\n"),
    ("a delete of a prefix deletes past it", MEMORY,
     "            .take_while(move |(key, _)| key.starts_with(&prefix))\n",
     "            .take_while(move |_| !prefix.is_empty())\n"),
    ("a command reports no table it wrote", MEMORY,
     "        Ok(changed(&working.tables, &open.touched)\n",
     "        Ok(changed(&working.tables, &BTreeMap::new())\n"),
    ("a row put back as it was is a write", MEMORY,
     "            (now != *before).then(|| (*table, key.clone(), now))\n",
     "            Some((*table, key.clone(), now))\n"),
    ("a refused write traps its command", MEMORY,
     "                            Err(Error::Conflict { existing, .. }) => Ok(Some(existing)),\n",
     "                            Err(Error::Conflict { existing, .. }) => Err(existing.to_string()),\n"),
    ("the adapter stores an Option of an Option", MEMORY,
     "        Stored::Option(inner) => !matches!(**inner, Stored::Option(_)) && supported(inner),\n",
     "        Stored::Option(inner) => supported(inner),\n"),
    # 7. which queries a write invalidates: the command's, held whole (PW5106)
    ("a table's write leaves its readers as they were (PW5106 reads no write)", CHECK,
     "        let writes: BTreeSet<String> = inference\n"
     "            .effective_effects(unit, hir, id)\n"
     "            .iter()\n"
     "            .filter_map(|e| crate::effects::database_domain(e, \"write\"))\n",
     "        let writes: BTreeSet<String> = inference\n"
     "            .effective_effects(unit, hir, id)\n"
     "            .iter()\n"
     "            .filter_map(|e| crate::effects::database_domain(e, \"no-write\"))\n"),
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
