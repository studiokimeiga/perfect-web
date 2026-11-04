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
   run not taken, a run no longer current completed anyway, a run's next
   generation written before the run is taken; a refused commit answered
   committed;
3. one state a render: each query of a render in a seat of its own; a
   render with no seat reading the latest commit; a render reading again
   after a read that failed;
4. the authority: a command not fenced on its session, a fence admitting
   the reader as no one, a guest's command refused, a fence's last check
   not made, a fence holding a command to any session, the session's lock
   not held to the command's end or never given back, a session's change
   taking no lock, a session living past its lifetimes, an admission
   forgetting nothing, a sign-in taken twice, sessions kept in the host's
   memory, a test's schema honoured outside a development deployment, a
   command left usable after its authority came too late, a failed
   session's change keeping its connection, an admission waiting for a
   command holding its session or making its due write under that
   command's lock (ADR-0389, amended by PR #67);
5. the runs the adapter keeps: a run scheduled again keeping its old
   generation, a cancelled run kept, a run taken or retried whatever the
   generation it was selected as, a taking another hand suppressed answered
   taken or not current;
6. the review of 3ccd083a (2026-11-02): a command waiting for its session
   while it holds its host's turn, a command holding the turn waiting for a
   connection; the shared outbox consumed too soon, its old rows never
   trimmed; the host never sweeping, or sweeping at every pass, sessions or
   sign-ins never swept, a sweep unbounded or waiting for a row a command
   holds; a rolled-back commit answered as unknown, by the layer or by the
   host, and an unanswered one answered refused.

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
    ("an owned Clock action forgets its scheduling owner", MAIN,
     '            &format!("schedule-{}", run.id),\n            &run.user,\n',
     '            &format!("schedule-{}", run.id),\n            "nobody",\n'),
    # 1. the routing
    ("a database named for the tables is not read", MAIN,
     '    let tables = match std::env::var("PW_TABLES_DATABASE_URL") {\n',
     '    let tables = match std::env::var("PW_TABLES_DATABASE_URL_NOT_NAMED") {\n'),
    ("a database named for the tables is not used", MAIN,
     "    let tables_layer = match &tables {\n",
     "    let tables_layer = match &None::<tables_pg::Database> {\n"),
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
     "        if let Err(e) = command.schedule(&schedulings) {\n",
     "        if let Err(e) = command.schedule(&schedulings[..0]) {\n"),
    ("a completed run is not taken in its command's transaction", LAYER,
     "            match command.consume(&run, completion.now_ms) {\n",
     "            match Ok::<bool, pw_table_postgres::Error>(!run.command.is_empty()) {\n"),
    ("a run's next generation is written before the run is taken", LAYER,
     "            match command.consume(&run, completion.now_ms) {\n",
     "            let _ = command.schedule(&schedulings);\n"
     "            match command.consume(&run, completion.now_ms) {\n"),
    ("a run no longer current completes anyway", LAYER,
     "                Ok(false) => return Ok(Commit::NotCurrent),\n",
     "                Ok(false) => {}\n"),
    ("a commit the database refused is answered committed", LAYER,
     "            Err(e) => return self.refuse(format!(\"the tables did not commit: {e:?}\")),\n",
     "            Err(_) => pw_table_postgres::Committed {\n"
     "                written: Default::default(),\n"
     "                outbox: Vec::new(),\n"
     "            },\n"),
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
    ("a render reads again after a read that failed", LIB,
     "            *self.ended.lock().map_err(|_| Error::StoragePoisoned)? = Some(error.clone());\n",
     "            let _ = error;\n"),
    # 4. the authority
    ("a command is not fenced on its reader's session", LAYER,
     "            .begin_fenced(fence)\n",
     "            .begin()\n"
     "            .map_err(|e| (e, &fence).0)\n"),
    ("a command whose authority came too late is left usable", LAYER,
     "                *self.turn.get_mut() = Turn::Failed(why.clone());\n",
     "                let _ = why;\n"),
    ("a failed session's change keeps its connection", PLATFORM,
     "        if !self.ended {\n",
     "        if !self.ended && false {\n"),
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
     "        let lifetimes = \"opened > clock_timestamp() - make_interval(secs => $2) \\\n"
     "             AND seen > clock_timestamp() - make_interval(secs => $3)\";\n",
     "        let lifetimes = \"$2::float8 IS NOT NULL \\\n"
     "             AND $3::float8 IS NOT NULL\";\n"),
    ("an admission waits for a command holding its session", PLATFORM,
     "                    \"SELECT pg_try_advisory_xact_lock($1)\",\n",
     "                    \"SELECT (pg_advisory_xact_lock($1) IS NULL) OR true\",\n"),
    ("an admission's due write is made under a command's lock", PLATFORM,
     "            if free != Some(true) {\n",
     "            if free != Some(true) && false {\n"),
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
     "            if held.first().map(run_of).transpose()?.as_ref() != Some(run) {\n",
     "            if held.first().map(run_of).transpose()?.map(|h| h.id) != Some(run.id) {\n"),
    ("a taking another hand suppressed is answered taken", PLATFORM,
     "            if removed.len() == 1 {\n",
     "            if removed.len() <= 1 {\n"),
    ("a taking another hand suppressed is answered not current", PLATFORM,
     "                Err(Error::Refused(format!(\n"
     "                    \"run {} is the run selected, and its deletion removed nothing\",\n"
     "                    run.id\n"
     "                )))\n",
     "                Ok::<bool, Error>(false)\n"),
    ("a failed run is retried whatever its generation", PLATFORM,
     "                 AND due_ms = $5 AND attempts = $6 AND failed IS NOT DISTINCT FROM $10 \\\n",
     "                 AND $5::bigint IS NOT NULL AND $6::bigint IS NOT NULL \\\n"
     "                 AND failed IS NOT DISTINCT FROM $10 \\\n"),
    # 6. the review of 3ccd083a
    ("a command waits for its session holding its host's turn", PLATFORM,
     "        let place = self.inner.lock_session(key)?;\n",
     "        let holding = self.begin()?;\n"
     "        let place = self.inner.lock_session(key)?;\n"
     "        drop(holding);\n"),
    ("a command holding the turn waits for a connection", LIB,
     "        let lease = open.lease.as_mut().ok_or(Error::Ended)?;\n",
     "        drop(self.pool.take()?);\n"
     "        let lease = open.lease.as_mut().ok_or(Error::Ended)?;\n"),
    ('a delivered outbox is deleted before another host can read it', LAYER,
     '        // Every host has a cursor. Local delivery leaves the retained log\n',
     (
            '        let ids: Vec<i64> = committed.outbox.iter().map(|(id, _)| *id).collect();\n'
            '        let _ = self.layer.tables.forget_outbox(&ids);\n'
            '        // Every host has a cursor. Local delivery leaves the retained log\n'
        )),
    ("the outbox rows a host left behind are never trimmed", LAYER,
     "                self.tables.trim_outbox(OUTBOX_LEFT, SWEPT),\n",
     "                self.tables.trim_outbox(OUTBOX_LEFT, 0),\n"),
    ("the host never sweeps", MAIN,
     "        self.data\n"
     "            .housekeeping(now, &self.identity.principals().lifetimes());\n",
     "        let _ = &self.data;\n"),
    ("the host sweeps at every pass of its scheduler", LAYER,
     "        if now_ms < *next {\n",
     "        if now_ms < *next && false {\n"),
    ("sessions past their lifetimes are never swept", LAYER,
     "                self.tables.sweep_sessions(lifetimes_of(sessions), SWEPT),\n",
     "                self.tables.sweep_sessions(lifetimes_of(sessions), 0),\n"),
    ("sign-ins past their lifetime are never swept", LAYER,
     "                    .sweep_sign_ins(identity::SIGN_IN_LIFETIME, SWEPT),\n",
     "                    .sweep_sign_ins(identity::SIGN_IN_LIFETIME, 0),\n"),
    ("a sweep of sessions is unbounded", PLATFORM,
     "                 LIMIT $3 FOR UPDATE SKIP LOCKED) RETURNING 1\"\n",
     "                 LIMIT $3::bigint * 0 + 1000000 FOR UPDATE SKIP LOCKED) RETURNING 1\"\n"),
    ("a sweep waits for a session a command holds", PLATFORM,
     "                 LIMIT $3 FOR UPDATE SKIP LOCKED) RETURNING 1\"\n",
     "                 LIMIT $3 FOR UPDATE NOWAIT) RETURNING 1\"\n"),
    ("a commit the database rolled back is answered as unknown", LAYER,
     "    fn definitely_refused(&self) -> bool {\n        self.refused\n    }\n",
     "    fn definitely_refused(&self) -> bool {\n        self.refused && false\n    }\n"),
    ("the host answers a refused commit as unknown", MAIN,
     "                    if refused {\n                        return Err(why);\n                    }\n",
     "                    if refused && false {\n                        return Err(why);\n                    }\n"),
    ("a commit never answered is answered refused", LAYER,
     "            ) => return Err(format!(\"the tables did not commit: {e:?}\")),\n",
     "            ) => return self.refuse(format!(\"the tables did not commit: {e:?}\")),\n"),
    ('a durable owned command has no staged predicate reads', LAYER,
     '    fn table_predicate_reads(\n        &self,\n        contract: &ComponentContract,\n        granted: &Granted,\n    ) -> Result<Ops, String> {\n        let mut turn = self.turn.borrow_mut();\n        if matches!(*turn, Turn::Committed) {\n            return Err("a command\'s predicate reads are linked before it commits".into());\n        }\n        turn.begun(&self.layer.tables)?\n            .predicate_host_fns(contract, granted)\n            .map_err(|e| format!("`{}`\'s predicate reads: {e:?}", contract.component_id))\n    }\n\n',
     '    fn table_predicate_reads(&self, _contract: &ComponentContract, _granted: &Granted) -> Result<Ops, String> {\n        Err("this data adapter supplies no predicate reads in a command".into())\n    }\n\n'),
    ('a durable table read is made three times', LAYER,
     '        read.map_err(|e| format!("`{}`\'s tables: {e:?}", contract.component_id))\n',
     '        read.map(|ops| ops.into_iter().map(|(name, f)| {\n            let thrice: HostFn = Arc::new(move |args: &[Val]| { let _ = f(args); let _ = f(args); f(args) });\n            (name, thrice)\n        }).collect()).map_err(|e| format!("`{}`\'s tables: {e:?}", contract.component_id))\n'),
    # Cross-host compatibility required by the protected feed/store migration.
    ('a generic PostgreSQL host has no shared-log reader', LAYER,
     '    fn tail(&self) -> Option<&hosts::Tail> {\n        Some(&self.tail)\n    }\n',
     '    fn tail(&self) -> Option<&hosts::Tail> {\n        None\n    }\n'),
    ('a generic PostgreSQL host has no distinct document instance', LAYER,
     '    fn instance(&self) -> Option<u64> {\n        Some(self.instance)\n    }\n',
     '    fn instance(&self) -> Option<u64> {\n        None\n    }\n'),
    ('a generic PostgreSQL commit sends no wakeup', LIB,
     "pg_notify($6, '')",
     '$6::text'),
    ('a generic outbox trim exceeds its bounded batch', LAYER,
     '                self.tables.trim_outbox(OUTBOX_LEFT, SWEPT),\n',
     '                self.tables.trim_outbox(OUTBOX_LEFT, SWEPT + 1),\n'),
]

# Each Cargo test binary apart: names can repeat between binaries, and the
# native assertion validator expects one complete inventory each.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "compiled_owned"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "platform"], []),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::tables_pg::", "tests::protected_migration::", "identity::transition_tests::"]),
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--test", "durable_tables"], []),
]

SOURCES = [
    ADAPTER / "tests/compiled_owned.rs",
    ROOT / "runtime/pw-table-memory/tests/support/owned.rs",
    ROOT / "runtime/pw-table-memory/tests/support/assert_result.rs",
    ADAPTER / "tests/platform.rs",
    ADAPTER / "tests/support/assert_result.rs",
    ADAPTER / "tests/common/mod.rs",
    SERVER / "src/tests/tables_pg.rs",
    SERVER / "src/tests/protected_migration.rs",
    SERVER / "src/identity.rs",
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
