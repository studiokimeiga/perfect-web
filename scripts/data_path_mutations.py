#!/usr/bin/env python3
"""Mutation controls for track `data-path` (ADR-0338's D2: an app with no
Rust data layer): the development server serves a program's declared
tables through the table gate alone, each component's host map its own,
a command's rows published as the platform's transaction's last step,
with its records or not at all; a guarded command answered before its
tables are linked; one render reading one state of the tables; the
guestbook on the tables it declares; the platform's reader's account; and
the conformance suite's teeth on the memory adapter (ADR-0338 D2's named
controls: a primitive answering another table's rows, a set read in
another order, an index not kept on a write, a staged write visible
before its commit).

Each mutant undoes one guarantee and must be killed by a test that fails
at its assertion (D2's strict producer: `cargo_assertion_results`, each
binary's inventory frozen at the baseline; a build failure, a crash or a
failure anywhere but an assertion is no kill). Each `.pw` mutant is a
well-typed program. The guide's test, which holds the guestbook to the
tutorial byte for byte, is not run: a guestbook mutant must be killed by
what the app does, not by its text differing from a page.

Run whole from the repository root (`just e14-data-path`). Every source is
restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record
is a whole run.
"""
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
LAYER = SERVER / "tables.rs"
NOTES = SERVER / "notifications.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"
BOOK = ROOT / "examples/guestbook/book.pw"

# The materializer's transaction, as it ends: the program's tables
# published last, after every record.
COMMIT_TAIL = (
    "                    staging.prepare_materialized()?;\n"
    "                    for (key, value) in &rows {\n"
    "                        Materializer::set_state(tx, key, value);\n"
    "                    }\n"
    "                    if let Some(r) = interaction {\n"
    "                        Materializer::prune_interactions(tx, r.forget_before_ms);\n"
    "                        Materializer::record_interaction(tx, r.key, r.sent, r.answer, r.at_ms)?;\n"
    "                    }\n"
    "                    // And the runs it scheduled, or cancelled (the\n"
    "                    // scheduled-commands track).\n"
    "                    schedules::write(tx, scheduled)?;\n"
    "                    if let Some(actor) = actor.filter(|actor| actor.is_local()) {\n"
    "                        actor.ensure_local_current()?;\n"
    "                    }\n"
    "                    // TRACK SEAM (tables): **what the layer staged, the\n"
    "                    // transaction's last step** (track `data-path`): a\n"
    "                    // program's tables published after every record above is\n"
    "                    // written, before it commits; refused, all of it rolls\n"
    "                    // back.\n"
    "                    staging.publish_within()?;\n"
)
COMMIT_FIRST = (
    "                    staging.publish_within()?;\n"
    + COMMIT_TAIL[: COMMIT_TAIL.index("                    // TRACK SEAM (tables)")]
)

MUTANTS = [
    # 1. routing: the gate the only way, each component's own functions
    ("a tables program's calls go around the gate", MAIN,
     "        let results = match self.data.tables() {\n",
     "        let results = match None::<&pw_host::tables::Manifest> {\n"),
    ("a command links the published reads, not its staging", MAIN,
     "                staging.table_ops(contract, granted)\n",
     "                self.data.table_reads(contract, granted, None)\n"),
    ("a component's host map holds another component's table functions", MAIN,
     "                linked.extend(tables(contract, &granted)?);\n",
     "                linked.extend(tables(self.contracts.iter().find(|c| crate::tables::imports_tables(std::slice::from_ref(c))).unwrap_or(contract), &granted)?);\n"),
    ("a query's reads ignore its render's snapshot", MAIN,
     "                .table_reads(contract, granted, snapshot)?\n",
     "                .table_reads(contract, granted, None)?\n"),
    ("a build with no page is refused", MAIN,
     "            Ok(pages) => pages.collect::<Vec<_>>(),\n"
     "            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),\n",
     "            Ok(pages) => pages.collect::<Vec<_>>(),\n"),
    # 2. the commit: rows and records together, or not at all
    ("a command's rows are published before its records are written", MAIN,
     COMMIT_TAIL, COMMIT_FIRST),
    ("a publication that fails keeps the command's records", MAIN,
     "                    staging.publish_within()?;\n",
     "                    let _ = staging.publish_within();\n"),
    ("a command that wrote reports no rows, and commits nothing", LAYER,
     "                Ok(written) if written.is_empty() => None,\n",
     "                Ok(_) => None,\n"),
    ("the layer grants no table's write", LAYER,
     "                    interned(format!(\"database.write<{table}>\")),\n",
     "                    interned(format!(\"database.read<{table}>\")),\n"),
    ("a program mixing its tables with a hand-written layer is served", LAYER,
     "                    i.kind == pw_host::ImportKind::HostCapability && !i.interface.starts_with(\"pw:\")\n",
     "                    false\n"),
    # 3. the engine: a guarded command answered first, then the gate
    ("a guarded tables command runs unanswered", HOST,
     "            // anything is instantiated or linked.\n"
     "            self.hold_arguments(contract, export, args)?;\n"
     "            authorize_export(contract, export, args, evaluate)?;\n",
     "            // anything is instantiated or linked.\n"
     "            self.hold_arguments(contract, export, args)?;\n"),
    ("a guarded tables command links its tables without their manifest", HOST,
     "            authorize_export(contract, export, args, evaluate)?;\n"
     "            run(\n"
     "                &self.engine,\n"
     "                &self.component,\n"
     "                contract,\n"
     "                granted,\n"
     "                limits,\n"
     "                host,\n"
     "                Some(tables),\n",
     "            authorize_export(contract, export, args, evaluate)?;\n"
     "            run(\n"
     "                &self.engine,\n"
     "                &self.component,\n"
     "                contract,\n"
     "                granted,\n"
     "                limits,\n"
     "                host,\n"
     "                None,\n"),
    # 4. the adapter's snapshot
    ("a snapshot reads the tables as last committed", MEMORY,
     "                    .read_snapshot(&tables, &bound, args)\n",
     "                    .read_published(&bound, args)\n"),
    ("a snapshot another adapter took is read", MEMORY,
     "        if !Arc::ptr_eq(&snapshot.inner, &self.inner) {\n",
     "        if false {\n"),
    # 5. the conformance suite's teeth on the memory adapter
    ("a read answers its rows in no order (a set read in its keys')", MEMORY,
     "    found.sort_by_cached_key(|row| place(row, &function.sorted_by, &table.key));\n",
     ""),
    ("a prefix read answers past its prefix", MEMORY,
     "            .take_while(move |(key, _)| key.starts_with(&prefix))\n",
     "            .take_while(move |_| !prefix.is_empty())\n"),
    ("a primitive answers another table's rows", MEMORY,
     "    let data = tables\n"
     "        .get(at)\n"
     "        .map(AsRef::as_ref)\n"
     "        .ok_or_else(|| Error::Schema(\"missing table state\".into()))?;\n"
     "    let mut found: Vec<&Row> = Vec::new();\n",
     "    let data = tables\n"
     "        .get((at + 1) % tables.len())\n"
     "        .map(AsRef::as_ref)\n"
     "        .ok_or_else(|| Error::Schema(\"missing table state\".into()))?;\n"
     "    let mut found: Vec<&Row> = Vec::new();\n"),
    ("an index is not kept on a write", MEMORY,
     "                index.insert((values, key.clone()));\n",
     "                let _ = (index, values);\n"),
    ("a staged write is visible before its commit", MEMORY,
     "    fn read_published(&self, import: &str, args: &[Value]) -> Result<Value, Error> {\n"
     "        self.healthy()?;\n",
     "    fn read_published(&self, import: &str, args: &[Value]) -> Result<Value, Error> {\n"
     "        self.healthy()?;\n"
     "        if let Ok(working) = self.working.lock() {\n"
     "            return read(&self.shape, &working.tables, import, args);\n"
     "        }\n"),
    # 6. the platform's reader: their account, for their own handle alone
    ("the account operation answers another user's handle", NOTES,
     "        [Val::String(of)] if *of == user => Ok(vec![Val::Option(account.clone())]),\n",
     "        [Val::String(_)] => Ok(vec![Val::Option(account.clone())]),\n"),
    ("a guest is an account, its session handed a program", NOTES,
     "    let account = principals.of(session).filter(|p| !p.is_guest()).map(|p| {\n",
     "    let account = principals.of(session).map(|p| {\n"),
    # 7. the guestbook, on the tables it declares
    ("a removed entry's number is given again", BOOK,
     "                Some(book) => book.signed + 1,\n",
     "                Some(_) => List.length(Entries.of_book(guestbook())) + 1,\n"),
    ("anyone removes an entry but its author", BOOK,
     "        Some(entry) => if mine(entry, account_of(current_user())) {\n",
     "        Some(entry) => if !mine(entry, account_of(current_user())) {\n"),
    ("the newest entries are not first", BOOK,
     "    List.map(List.take(List.reverse(Entries.of_book(guestbook())), limit), e => line(e, account))\n",
     "    List.map(List.take(Entries.of_book(guestbook()), limit), e => line(e, account))\n"),
    ("every entry is the reader's", BOOK,
     "        Some(account) => entry.author == account.id,\n",
     "        Some(_) => true,\n"),
]

# Each Cargo test binary apart: names can repeat between binaries, and the
# shared native assertion validator expects one complete inventory each.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::tables::", "tests::guestbook::", "tests::conformance::",
      "notifications::tests::the_account_operation"]),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_guarded"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_snapshot"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "handles"], []),
]

SOURCES = [
    SERVER / "tests/tables.rs",
    SERVER / "tests/guestbook.rs",
    SERVER / "tests/conformance.rs",
    NOTES,
    ROOT / "runtime/pw-table-memory/tests/compiled_guarded.rs",
    ROOT / "runtime/pw-table-memory/tests/compiled_snapshot.rs",
    ROOT / "runtime/pw-table-memory/tests/support/assert_result.rs",
    ROOT / "compiler/pw-core/tests/handles.rs",
]
INVENTORY = {}


def run_tests():
    passed, failed = 0, 0
    for cmd, filters in TESTS:
        full = [*cmd, "--", *filters, "--format=pretty"]
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
