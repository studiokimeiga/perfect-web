#!/usr/bin/env python3
"""Mutation controls for track `durable-tables`: a program's declared tables
served durably from PostgreSQL by the development server, through the same
gate as in memory (`spikes/own-renderer/server/src/tables_pg.rs`, over
ADR-0393's adapter), with the platform's runs, sessions and fence the
adapter keeps (`runtime/pw-table-postgres/src/platform.rs`).

Each mutant undoes one guarantee and must be killed by a test that fails at
its assertion (the strict producer of `d3_postgres_mutations.py` and
`data_path_mutations.py`: `cargo_assertion_results`, each binary's
inventory frozen at the baseline; a build failure, a crash, or a failure
anywhere but an assertion is no kill):

1. the routing: a database named for the tables, not read, or not used;
2. one transaction: the interaction's record, the runs, or the recall left
   to the host's materializer; a command's runs not written, a completed
   run not taken, a run no longer current completed anyway; a refused
   commit answered committed;
3. one state a render: each query of a render in a seat of its own; a
   render with no seat reading the latest commit;
4. the authority: a command not fenced on its session, a fence admitting
   the reader as no one, a guest's command refused, a fence's last check
   not made, a fence holding a command to any session, the session's lock
   not held to the command's end or never given back, a session's change
   taking no lock, a session living past its lifetimes, an admission
   forgetting nothing, a sign-in taken twice, sessions kept in the host's
   memory, a test's schema honoured outside a development deployment;
5. the runs the adapter keeps: a run scheduled again keeping its old
   generation, a cancelled run kept, a run taken or retried whatever the
   generation it was selected as, a stale taking answered taken.

The tests: the adapter's (`runtime/pw-table-postgres/tests/platform.rs`),
the server's on PostgreSQL (`tests::tables_pg::`, the conformance suite over
this layer among them) and the server as a process, killed and stopped
(`spikes/own-renderer/server/tests/durable_tables.rs`).

They need a database: `PW_TABLES_DATABASE_URL` (or `PW_STORE_DATABASE_URL`,
`PW_FEED_DATABASE_URL`), a throwaway one, which each test uses in a schema of
its own. Without it every test passes doing nothing, and no mutant could be
killed, so this refuses to run.

Run whole from the repository root (`just e14-durable-tables`). Every source
is restored in finally; the unmutated baseline must pass before any mutant
means anything. Words given as arguments run only the mutants whose
description holds one of them, for a quick look while working; the record is
a whole run.
"""
import os
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
ADAPTER = ROOT / "runtime/pw-table-postgres"
PLATFORM = ADAPTER / "src/platform.rs"
LIB = ADAPTER / "src/lib.rs"
SERVER = ROOT / "spikes/own-renderer/server"
LAYER = SERVER / "src/tables_pg.rs"
MAIN = SERVER / "src/main.rs"

MUTANTS = [
    # 1. the routing
    ("a database named for the tables is not read", MAIN,
     '    let tables = match std::env::var("PW_TABLES_DATABASE_URL") {\n',
     '    let tables = match std::env::var("PW_TABLES_DATABASE_URL_NOT_NAMED") {\n'),
    ("a database named for the tables is not used", MAIN,
     "                let layer: Arc<dyn data::DataLayer> = match tables {\n",
     "                let layer: Arc<dyn data::DataLayer> = match None::<&tables_pg::Database> {\n"),
    # 2. one transaction
    ("an interaction's record is left to the host's materializer", LAYER,
     "    fn keeps_interactions(&self) -> bool {\n        true\n    }\n",
     "    fn keeps_interactions(&self) -> bool {\n        false\n    }\n"),
    ("an interaction's committed record is never recalled", LAYER,
     "            .recalled(key, since_ms)\n"
     "            .map_err(|e| format!(\"the interaction's record: {e:?}\"))\n",
     "            .recalled(key, since_ms)\n"
     "            .map(|_| None)\n"
     "            .map_err(|e| format!(\"the interaction's record: {e:?}\"))\n"),
    ("the runs are left to the host's materializer", LAYER,
     "    fn runs(&self) -> Option<&dyn schedules::Runs> {\n        Some(self)\n    }\n",
     "    fn runs(&self) -> Option<&dyn schedules::Runs> {\n        None\n    }\n"),
    ("a command's scheduled runs are not written", LAYER,
     "            .schedule(&schedulings)\n",
     "            .schedule(&schedulings[..0])\n"),
    ("a completed run is not taken in its command's transaction", LAYER,
     "            match command.consume(&run, completion.now_ms) {\n",
     "            match Ok::<bool, pw_table_postgres::Error>(!run.command.is_empty()) {\n"),
    ("a run no longer current completes anyway", LAYER,
     "                Ok(false) => return Ok(Commit::NotCurrent),\n",
     "                Ok(false) => {}\n"),
    ("a commit the database refused is answered committed", LAYER,
     "            .map_err(|e| format!(\"the tables did not commit: {e:?}\"))?;\n"
     "        self.turn = Turn::Committed;\n",
     "            .unwrap_or(pw_table_postgres::Committed {\n"
     "                written: Default::default(),\n"
     "                outbox: Vec::new(),\n"
     "            });\n"
     "        self.turn = Turn::Committed;\n"),
    # 3. one state a render
    ("each query of a render reads in a seat of its own", LAYER,
     "                Some(PgSnapshot(Ok(taken))) => taken.read_host_fns(contract, granted),\n",
     "                Some(PgSnapshot(Ok(_))) => match self.tables.snapshot() {\n"
     "                    Ok(own) => own.read_host_fns(contract, granted),\n"
     "                    Err(e) => Err(e),\n"
     "                },\n"),
    ("a render with no seat reads the latest commit", LAYER,
     "                Some(PgSnapshot(Err(why))) => return Err(why.clone()),\n",
     "                Some(PgSnapshot(Err(_))) => self.tables.read_host_fns(contract, granted),\n"),
    # 4. the authority
    ("a command is not fenced on its reader's session", LAYER,
     "            .fence(fence)\n"
     "            .map_err(|e| format!(\"session authority changed before commit: {e:?}\"))?;\n",
     "            .revalidate()\n"
     "            .map_err(|e| format!(\"session authority changed before commit: {e:?}{fence:?}\"))?;\n"),
    ("a fence admits a signed-in reader as no one", LAYER,
     "        ReaderIdentity::Account(identity) => Some(session_of(identity)),\n",
     "        ReaderIdentity::Account(_) => None,\n"),
    ("a guest's command is refused as another adapter's", LAYER,
     "        if matches!(actor.reader, ReaderIdentity::Guest) {\n"
     "            actor.ensure_local_current()?;\n",
     "        if false {\n"
     "            actor.ensure_local_current()?;\n"),
    ("sessions are kept in the host's memory", LAYER,
     "        Some(Arc::clone(&self.kept))\n",
     "        None::<Arc<dyn identity::Kept>>.filter(|_| Arc::strong_count(&self.kept) == 0)\n"),
    ("a test's schema is honoured outside a development deployment", LAYER,
     "        Some(s) if deployment.development && deployment.loopback() => Ok(Some(s.to_string())),\n",
     "        Some(s) => Ok(Some(s.to_string())),\n"),
    ("a fenced command's last check is not made", LIB,
     "            if let Err(error) = platform::still_fenced(&inner, conn, fence.as_ref()) {\n",
     "            if let Err(error) = platform::still_fenced(&inner, conn, fence.as_ref().filter(|_| false)) {\n"),
    ("a fence holds a command to any session", PLATFORM,
     "    if now == fence.expected {\n",
     "    if now == fence.expected || now != fence.expected {\n"),
    ("a fenced command's session lock is not held to its end", PLATFORM,
     '                "SELECT pg_advisory_lock($1)",\n',
     '                "SELECT pg_advisory_xact_lock($1)",\n'),
    ("a fenced command's session lock is never given back", LIB,
     "                self.unfence(&mut lease, fenced.key);\n",
     "                let _ = (&mut lease, fenced.key);\n"),
    ("a session's change takes no lock", PLATFORM,
     '                "SELECT pg_advisory_xact_lock($1)",\n',
     '                "SELECT $1::bigint",\n'),
    ("a session lives past its lifetimes", PLATFORM,
     "             AND opened > clock_timestamp() - make_interval(secs => $2) \\\n"
     "             AND seen > clock_timestamp() - make_interval(secs => $3)\";\n",
     "             AND $2::float8 IS NOT NULL \\\n"
     "             AND $3::float8 IS NOT NULL\";\n"),
    ("an admission forgets no session past its lifetime", PLATFORM,
     "                 gone AS (DELETE FROM {sessions} WHERE hash = $1 \\\n",
     "                 gone AS (DELETE FROM {sessions} WHERE false AND hash = $1 \\\n"),
    ("a sign-in is taken twice", PLATFORM,
     "                \"DELETE FROM {sign_ins} WHERE hash = $1 RETURNING nonce, verifier, \\\n"
     "                 (extract(epoch FROM clock_timestamp() - started) * 1000)::bigint\"\n",
     "                \"SELECT nonce, verifier, \\\n"
     "                 (extract(epoch FROM clock_timestamp() - started) * 1000)::bigint \\\n"
     "                 FROM {sign_ins} WHERE hash = $1\"\n"),
    # 5. the runs the adapter keeps
    ("a run scheduled again keeps its old generation", PLATFORM,
     '            unschedule: format!("DELETE FROM {runs} WHERE key = $1"),\n',
     '            unschedule: format!("SELECT 1 FROM {runs} WHERE key = $1"),\n'),
    ("a cancelled run is kept", PLATFORM,
     "                if let Some(due) = run.due_ms {\n",
     "                if let Some(due) = run.due_ms.or(Some(0)) {\n"),
    ("a run is taken whatever the generation it was selected as", PLATFORM,
     "                 AND user_id = $4 AND due_ms = $5 AND attempts = $6 AND failed IS NULL \\\n",
     "                 AND $4::text IS NOT NULL AND $5::bigint IS NOT NULL AND $6::bigint IS NOT NULL \\\n"
     "                 AND failed IS NULL \\\n"),
    ("a stale taking is answered taken", PLATFORM,
     "            Ok(rows) => Ok(rows.len() == 1),\n",
     "            Ok(rows) => Ok(rows.len() <= 1),\n"),
    ("a failed run is retried whatever its generation", PLATFORM,
     "                 AND due_ms = $5 AND attempts = $6 AND failed IS NOT DISTINCT FROM $10 \\\n",
     "                 AND $5::bigint IS NOT NULL AND $6::bigint IS NOT NULL \\\n"
     "                 AND failed IS NOT DISTINCT FROM $10 \\\n"),
]

# Each Cargo test binary apart: names can repeat between binaries, and the
# native assertion validator expects one complete inventory each.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "platform"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::tables_pg::"]),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--test", "durable_tables"], []),
]

SOURCES = [
    ADAPTER / "tests/platform.rs",
    ADAPTER / "tests/support/assert_result.rs",
    ADAPTER / "tests/common/mod.rs",
    SERVER / "src/tests/tables_pg.rs",
    SERVER / "src/tests/tables.rs",
    SERVER / "src/tests/conformance.rs",
    SERVER / "tests/durable_tables.rs",
]
INVENTORY = {}


def database():
    return next(
        (os.environ[name] for name in
         ("PW_TABLES_DATABASE_URL", "PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL")
         if os.environ.get(name)),
        None,
    )


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
    if not database():
        print("FAIL: no database is named (PW_TABLES_DATABASE_URL, PW_STORE_DATABASE_URL or")
        print("PW_FEED_DATABASE_URL); without one every test passes doing nothing, and no")
        print("mutant can mean anything")
        return 2
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
