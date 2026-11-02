#!/usr/bin/env python3
"""Mutation controls for track `data-path` (ADR-0338's D2: an app with no
Rust data layer): the development server serves a program's declared
tables through the table gate alone, each component's host map its own,
a command's rows published as the platform's transaction's last step
before its COMMIT, after its every record and its outbox, with them or not
at all, and the tables closed where the COMMIT fails after; a guarded
command answered before its tables are linked; one render reading one
state of the tables; the guestbook on the tables it declares, bounded,
its refusals said, its composer an account's; the platform's reader's
account, never a guest's or a stand-in's, a guest's identity never its
session; no row holding a handle, at the compiler and again at the host;
no upload beside tables; and the conformance suite's teeth on the memory
adapter (ADR-0338 D2's named controls: a primitive answering another
table's rows, a set read in another order, an index not kept on a write, a
staged write visible before its commit; and, since the integrator's
review of 2026-11-02, a read made three times, counted at the adapter).

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
IDENTITY = SERVER / "identity.rs"
HOST = ROOT / "runtime/pw-host/src/lib.rs"
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"
MATERIALIZER = ROOT / "runtime/pw-materialize/src/lib.rs"
COMPILER_TABLES = ROOT / "compiler/pw-core/src/tables.rs"
BOOK = ROOT / "examples/guestbook/book.pw"
PAGES = ROOT / "examples/guestbook/pages.pw"

# The materializer's transaction, as it ends (`command_then`): its outbox
# written, then the layer's last step, then its COMMIT.
OUTBOX_THEN_LAST = (
    "        let mut ids = Vec::new();\n"
    "        for ev in &events {\n"
    "            let written = serde_json::to_string(&ev.args)\n"
    "                .map_err(|e| e.to_string())\n"
    "                .and_then(|args| {\n"
    "                    tx.execute(\n"
    "                        \"INSERT INTO outbox (event, args) VALUES (?1, ?2)\",\n"
    "                        params![ev.name, args],\n"
    "                    )\n"
    "                    .map_err(|e| e.to_string())\n"
    "                });\n"
    "            if let Err(why) = written {\n"
    "                let _ = tx.rollback();\n"
    "                return Err(E::from(format!(\n"
    "                    \"the outbox refused the event `{}`: {why}\",\n"
    "                    ev.name\n"
    "                )));\n"
    "            }\n"
    "            ids.push(tx.last_insert_rowid());\n"
    "        }\n"
)
LAST = (
    "        if let Err(e) = last() {\n"
    "            let _ = tx.rollback();\n"
    "            return Err(e);\n"
    "        }\n"
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
    # 2. the commit: rows published after every record and the outbox, as
    # the transaction's last step before its COMMIT, or not at all; and the
    # tables closed where the COMMIT fails after
    ("a command's rows are published before its outbox is written", MATERIALIZER,
     OUTBOX_THEN_LAST + LAST, LAST + OUTBOX_THEN_LAST),
    ("a publication that fails keeps the command's records", MAIN,
     "                        true => layer.borrow_mut().publish_within(),\n",
     "                        true => layer.borrow_mut().publish_within().or(Ok(())),\n"),
    ("a COMMIT that fails after the publication is told no one", MATERIALIZER,
     "            uncommitted(&why);\n",
     "            let _ = &uncommitted;\n"),
    ("tables whose COMMIT failed after their publication stay open", LAYER,
     "        self.tables.fail_closed();\n",
     "        let _ = self.tables;\n"),
    ("a command that wrote reports no rows, and commits nothing", LAYER,
     "                Ok(written) if written.is_empty() => None,\n",
     "                Ok(_) => None,\n"),
    ("the layer grants no table's write", LAYER,
     "                    interned(format!(\"database.write<{table}>\")),\n",
     "                    interned(format!(\"database.read<{table}>\")),\n"),
    ("a program mixing its tables with a hand-written layer is served", LAYER,
     "                i.kind == pw_host::ImportKind::HostCapability && !i.interface.starts_with(\"pw:\")\n",
     "                false\n"),
    ("a row of a handle's type in tables.json is served", LAYER,
     "            if let Some(whose) = handle_named(written) {\n",
     "            if let Some(whose) = handle_named(written).filter(|_| false) {\n"),
    ("a handle's type written bare is not named", LAYER,
     "            if bare || platform {\n",
     "            if platform || (bare && false) {\n"),
    ("a program of tables declaring an upload is served", LAYER,
     "    if let Some(upload) = declared.first() {\n",
     "    if let Some(upload) = declared.first().filter(|_| false) {\n"),
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
    # The suite's costs, counted at the adapter (the review's S2): a layer
    # that calls the adapter three times for one read, and an adapter that
    # counts none of its statements.
    ("a read is made three times", LAYER,
     "        read.map_err(|e| format!(\"`{}`'s tables: {e:?}\", contract.component_id))\n",
     "        read.map(|ops| ops.into_iter().map(|(name, f)| { let thrice: HostFn = Arc::new(move |args: &[Val]| { let _ = f(args); let _ = f(args); f(args) }); (name, thrice) }).collect()).map_err(|e| format!(\"`{}`'s tables: {e:?}\", contract.component_id))\n"),
    ("the adapter counts no statement", MEMORY,
     "        self.statements.fetch_add(1, Ordering::SeqCst);\n",
     "        let _ = &self.statements;\n"),
    # 6. the platform's reader: their account, for their own handle alone,
    # never a guest's or a stand-in's; a guest's identity never its session;
    # and no row holding a handle
    ("the account operation answers another user's handle", NOTES,
     "        [Val::String(of)] if *of == user => Ok(vec![Val::Option(account.clone())]),\n",
     "        [Val::String(_)] => Ok(vec![Val::Option(account.clone())]),\n"),
    ("a guest is an account", NOTES,
     "    (!principal.is_guest()).then(|| {\n",
     "    true.then(|| {\n"),
    ("a stand-in is an account of its own", NOTES,
     "    if session.starts_with(crate::identity::ON_BEHALF) {\n",
     "    if false {\n"),
    ("a run on a reader's behalf is answered no account", NOTES,
     "        return recorded.map(|name| (principal.user, name));\n",
     "        return None;\n"),
    ("a sign-in records no account", IDENTITY,
     "        self.record_account(&principal);\n",
     "        let _ = Self::record_account;\n"),
    ("a guest's id is its session", IDENTITY,
     "    format!(\"u-{}\", hex(&digest[..16]))\n",
     "    let _ = digest;\n    format!(\"u-{session}\")\n"),
    ("a guest's digest is the one its session is kept by", IDENTITY,
     "    let digest = sha2::Sha256::digest(format!(\"{GUEST_DOMAIN}{session}\").as_bytes());\n",
     "    let digest = sha2::Sha256::digest(format!(\"{}{session}\", &GUEST_DOMAIN[..0]).as_bytes());\n"),
    ("a guest is named for its session", IDENTITY,
     "            name: format!(\"Guest {shown}\"),\n",
     "            name: format!(\"Guest {session}\"),\n"),
    ("a signed-out reader acts as its session", IDENTITY,
     "            .unwrap_or_else(|| guest_user(session))\n",
     "            .unwrap_or_else(|| format!(\"u-{session}\"))\n"),
    ("a row may hold a handle", COMPILER_TABLES,
     "            .and_then(|ty| crate::check::handle_in(sigs, ty, &mut BTreeSet::new()))\n",
     "            .and_then(|_| None::<&str>)\n"),
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
    ("a full guestbook is not refused `Full`", BOOK,
     "        Some(account) => if List.length(Entries.all()) >= most() {\n",
     "        Some(account) => if List.length(Entries.all()) > most() {\n"),
    ("the composer is shown to a reader with no account", PAGES,
     "            <div hidden={!me.signed_in}>\n",
     "            <div>\n"),
]

# Each Cargo test binary apart: names can repeat between binaries, and the
# shared native assertion validator expects one complete inventory each.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::tables::", "tests::guestbook::", "tests::conformance::",
      "notifications::tests::", "tables::tests::"]),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_guarded"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "compiled_snapshot"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-materialize", "--lib"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "handles"], []),
]

SOURCES = [
    SERVER / "tests/tables.rs",
    SERVER / "tests/guestbook.rs",
    SERVER / "tests/conformance.rs",
    NOTES,
    LAYER,
    MATERIALIZER,
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
