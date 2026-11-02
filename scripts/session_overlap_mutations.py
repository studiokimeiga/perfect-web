#!/usr/bin/env python3
"""ADR-0389, amended 2026-11-02 (track session-overlap): two requests of one
session whose admissions overlap are each answered, every request is
admitted, and on PostgreSQL an admission takes no session lock.

Each control undoes one rule of `Principals::admit`, of the request's
admission (`main.rs`) or of the admission's statements (`hosts.rs`):
- an admission another admission overtook is refused `Superseded` again;
- a read this host's open, close or expiry overtook answers its stale read,
  or is refused instead of read again; the reads are not bounded;
- this host's close moves no authority ticket;
- a read another admission overtook installs its principal anyway; a removal,
  a failed read's removal or an install that moves no ticket;
- (from the review) a removal, of no one or of a failed read, that another
  admission overtook removes nothing; the tickets compared before the gate
  and the sessions lock are taken;
- the runtime's three files answered before admission (PR #63's exemption);
- a refused admission, a page's or a command's, not logged by its kind;
- (from the review, on PostgreSQL) an admission's read that waits for the
  session's lock; a write due at every admission; a due write that waits for
  the lock; with the lock held, a live session refused, an expired one
  answered as its principal, or the write made anyway; a use never recorded;
  an expired row never deleted; the lock free, the read answering instead of
  the write; a session connection that bounds no lock wait.

The tests: identity.rs's admission tests, and `tests::session_overlap::`'s
four in this process and its seven on PostgreSQL, in one frozen inventory.
The PostgreSQL tests skip without a database, so this script refuses to run
without PW_FEED_DATABASE_URL: it never passes by skipping.

Strict, as the other ADR-0389 controls: the shared native assertion
validator (each failing test fails at one `assert` in the named sources),
the exact test inventory on the baseline and every control, every source
restored and checked by digest. A build, setup, timeout or other panic is an
invalid run, never a kill; the first invalid or surviving control stops it.
Run from the repository root, whole; `just e14-session-overlap` records it.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
IDENTITY = SERVER / "identity.rs"
MAIN = SERVER / "main.rs"
HOSTS = SERVER / "hosts.rs"
OVERLAP = SERVER / "tests/session_overlap.rs"
# The files a failing test may fail in: its assertion's own.
SOURCES = [IDENTITY, OVERLAP]
TEST_NAMES = [
    "identity::invocation_tests::authority_source_failures_are_bounded_without_borrowing_cached_identity",
    "identity::tests::a_close_between_the_read_and_the_locks_is_seen",
    "identity::tests::a_failed_read_another_admission_overtook_still_removes_the_copy",
    "identity::tests::a_read_another_admission_overtook_never_installs_over_another_reader",
    "identity::tests::a_read_another_admission_overtook_never_reinstalls_a_revoked_principal",
    "identity::tests::a_read_another_admission_overtook_never_reinstalls_over_a_failed_read",
    "identity::tests::a_read_of_no_one_another_admission_overtook_still_removes_the_copy",
    "identity::tests::an_admission_overtaken_at_every_read_is_refused_after_its_bound",
    "identity::tests::an_admission_whose_read_a_sign_out_overtook_reads_again",
    "identity::tests::an_unrelated_admission_does_not_suppress_a_revoked_session",
    "identity::tests::an_unrelated_admission_does_not_suppress_an_authority_read_failure",
    "identity::tests::two_overlapping_admissions_of_one_session_are_each_answered",
    "tests::session_overlap::every_request_is_admitted_and_a_failed_read_refuses_it",
    "tests::session_overlap::in_memory_a_request_admitted_while_another_is_under_way_is_answered",
    "tests::session_overlap::on_postgres_a_due_touch_is_skipped_under_a_commands_lock_and_made_once_free",
    "tests::session_overlap::on_postgres_a_session_signed_out_between_the_read_and_the_write_is_no_one",
    "tests::session_overlap::on_postgres_admissions_under_a_commands_lock_delay_no_other_readers_page",
    "tests::session_overlap::on_postgres_an_admission_never_waits_for_a_command_holding_its_session",
    "tests::session_overlap::on_postgres_an_admission_waits_on_a_locked_table_no_longer_than_the_lock_timeout",
    "tests::session_overlap::on_postgres_an_expired_session_is_no_one_under_a_commands_lock_and_deleted_once_free",
    "tests::session_overlap::on_postgres_two_requests_of_one_session_whose_admissions_overlap_are_each_answered",
    "tests::session_overlap::two_requests_of_one_session_whose_admissions_overlap_are_each_answered",
]
COMMAND = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server",
           "--", *TEST_NAMES, "--exact", "--test-threads=1", "--format=pretty"]

# (what, file, anchor, replacement)
MUTANTS = [
    ("an admission another admission overtook is refused Superseded again", IDENTITY,
     "            let unwritten = Arc::ptr_eq(&now.written, &before.written);\n",
     "            let unwritten = Arc::ptr_eq(&now.written, &before.written);\n"
     "            if !unwritten {\n"
     "                return Err(AuthorityError::Superseded);\n"
     "            }\n"),
    ("a read this host's open, close or expiry overtook answers its stale read", IDENTITY,
     "            if !Arc::ptr_eq(&now.authority, &before.authority) {\n",
     "            if false && !Arc::ptr_eq(&now.authority, &before.authority) {\n"),
    ("a read this host's open, close or expiry overtook is refused, not read again", IDENTITY,
     "            if !Arc::ptr_eq(&now.authority, &before.authority) {\n"
     "                continue;\n",
     "            if !Arc::ptr_eq(&now.authority, &before.authority) {\n"
     "                return Err(AuthorityError::Superseded);\n"),
    ("an admission reads its session without a bound", IDENTITY,
     "        for _ in 0..ADMISSION_READS {\n",
     "        for _ in 0.. {\n"),
    ("this host's open, close or expiry moves no authority ticket", IDENTITY,
     "        revision.authority = Arc::new(());\n",
     ""),
    ("a read another admission overtook installs its principal anyway", IDENTITY,
     "                    if unwritten {\n",
     "                    if true || unwritten {\n"),
    ("an install moves no ticket", IDENTITY,
     "                        Self::install(&mut sessions, session, identity.clone());\n"
     "                        self.copy_written(session);\n",
     "                        Self::install(&mut sessions, session, identity.clone());\n"),
    ("a removal moves no ticket", IDENTITY,
     "                    sessions.remove(session);\n"
     "                    self.copy_written(session);\n"
     "                    Ok(ReaderIdentity::Anonymous)\n",
     "                    sessions.remove(session);\n"
     "                    Ok(ReaderIdentity::Anonymous)\n"),
    ("a failed read's removal moves no ticket", IDENTITY,
     "                    sessions.remove(session);\n"
     "                    self.copy_written(session);\n"
     "                    Err(AuthorityError::SourceUnavailable)\n",
     "                    sessions.remove(session);\n"
     "                    Err(AuthorityError::SourceUnavailable)\n"),
    # From the review of 2026-11-02 (its M1, M2 and M3): each survived the
    # 13 tests the repair had.
    ("a read of no one another admission overtook removes nothing", IDENTITY,
     "                Ok(None) => {\n"
     "                    sessions.remove(session);\n"
     "                    self.copy_written(session);\n"
     "                    Ok(ReaderIdentity::Anonymous)\n"
     "                }\n",
     "                Ok(None) => {\n"
     "                    if unwritten {\n"
     "                        sessions.remove(session);\n"
     "                        self.copy_written(session);\n"
     "                    }\n"
     "                    Ok(ReaderIdentity::Anonymous)\n"
     "                }\n"),
    ("a failed read another admission overtook removes nothing", IDENTITY,
     "                Err(_) => {\n"
     "                    sessions.remove(session);\n"
     "                    self.copy_written(session);\n"
     "                    Err(AuthorityError::SourceUnavailable)\n"
     "                }\n",
     "                Err(_) => {\n"
     "                    if unwritten {\n"
     "                        sessions.remove(session);\n"
     "                        self.copy_written(session);\n"
     "                    }\n"
     "                    Err(AuthorityError::SourceUnavailable)\n"
     "                }\n"),
    ("the tickets are compared before the gate and the sessions lock are taken", IDENTITY,
     "            let read = kept.session(&kept_hash(session), &self.lifetimes());\n"
     "            let gate = self.command_gate(session);\n"
     '            let _authority = gate.write().expect("session authority gate");\n'
     '            let mut sessions = self.0.sessions.write().expect("principals");\n'
     '            let now = revision.lock().expect("admission revision").clone();\n',
     "            let read = kept.session(&kept_hash(session), &self.lifetimes());\n"
     '            let now = revision.lock().expect("admission revision").clone();\n'
     "            let gate = self.command_gate(session);\n"
     '            let _authority = gate.write().expect("session authority gate");\n'
     '            let mut sessions = self.0.sessions.write().expect("principals");\n'),
    ("the runtime's three files are answered before admission (PR #63's exemption)", MAIN,
     '    let route = path.split(\'?\').next().unwrap_or("/");\n'
     "    let upload_request = server.uploads.claims(method, route);\n",
     '    let route = path.split(\'?\').next().unwrap_or("/");\n'
     '    if method == "GET"\n'
     "        && matches!(\n"
     "            route,\n"
     '            "/pw-runtime.mjs" | "/pw-resume.wasm" | "/pw-render.wasm"\n'
     "        )\n"
     "    {\n"
     "        serve_file(server, &mut stream, route);\n"
     "        return;\n"
     "    }\n"
     "    let upload_request = server.uploads.claims(method, route);\n"),
    ("a page's refused admission is not logged", MAIN,
     "            Err(why) => {\n"
     "                refused_admission(server, &why);\n"
     "                respond_json(\n",
     "            Err(why) => {\n"
     "                let _ = why;\n"
     "                respond_json(\n"),
    ("a command's refused admission is not logged", MAIN,
     "                    refused_admission(server, &why);\n"
     "                    session_changed(&mut stream, &session, fresh);\n",
     "                    let _ = why;\n"
     "                    session_changed(&mut stream, &session, fresh);\n"),
    # From the review of 2026-11-02: on PostgreSQL an admission takes no
    # session lock (`SessionsPg::session`, `written_if_free`).
    ("an admission's read waits for its session's lock, as a command holds it", HOSTS,
     "        let mut taken = self.take()?;\n"
     "        self.counted();\n"
     "        let read = taken\n"
     "            .client\n"
     "            .as_mut()\n"
     '            .expect("a connection taken")\n'
     "            .query_opt(ADMISSION_READ, &params)\n"
     "            .map_err(pg)?;\n",
     "        let read = self.mutation(hash, |c| c.query_opt(ADMISSION_READ, &params))?;\n"
     "        let mut taken = self.take()?;\n"),
    ("a write is due at every admission", HOSTS,
     "        let due = !live || stale;\n",
     "        let due = stale || true;\n"),
    ("a due write waits for the session's lock", HOSTS,
     '                    &format!("SELECT pg_try_advisory_xact_lock({SESSION_AUTHORITY_KEY})"),\n',
     '                    &format!("SELECT pg_advisory_xact_lock({SESSION_AUTHORITY_KEY}) IS NOT NULL"),\n'),
    ("the session's lock held, a live session's admission is refused", HOSTS,
     "            None => Ok(answer),\n",
     '            None => Err("the session\'s lock is held".into()),\n'),
    ("the session's lock held, an expired session is answered as its principal", HOSTS,
     "        let answer = if live {\n"
     "            Some(session_identity(read)?)\n"
     "        } else {\n"
     "            None\n"
     "        };\n",
     "        let answer = Some(session_identity(read)?);\n"),
    ("the session's lock held, the due write is made anyway, without it", HOSTS,
     "            if !free {\n",
     "            if false && !free {\n"),
    ("a session's use is never due to be recorded", HOSTS,
     "        let due = !live || stale;\n",
     "        let due = !live || (stale && false);\n"),
    ("an expired session's deletion is never due", HOSTS,
     "        let due = !live || stale;\n",
     "        let due = stale;\n"),
    ("the session's lock free, the read answers instead of the write", HOSTS,
     "            Some(written) => written.map(session_identity).transpose(),\n",
     "            Some(_) => Ok(answer),\n"),
    ("a session connection bounds no lock wait", HOSTS,
     '                    "SET search_path TO {}; {SESSION_CONNECTION_LOCK_TIMEOUT}",\n',
     '                    "SET search_path TO {}",\n'),
]

# How long one run of the tests may take. Past it the run is invalid.
TIMEOUT = 1200


def database_named():
    return bool(os.environ.get("PW_FEED_DATABASE_URL"))


def run_tests():
    try:
        child = producer_process.run(COMMAND, cwd=ROOT, timeout=TIMEOUT)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"INVALID test producer: {type(error).__name__}", flush=True)
        return None
    print(child.stdout, flush=True)
    return cargo_assertion_results.validate(child.returncode, child.stdout,
                                            root=ROOT, sources=SOURCES, expected=TEST_NAMES)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    files = sorted({path for _, path, _, _ in MUTANTS} | set(SOURCES))
    before = {str(path.relative_to(ROOT)): digest(path) for path in files}
    report = {"scope": "overlapping admissions of one session (ADR-0389, amended 2026-11-02)",
              "command": COMMAND, "producer_sha256": digest(Path(__file__)),
              "sources": before, "baseline": None, "controls": [], "restored": False,
              "valid": False}
    try:
        if not database_named():
            print("FAIL: the PostgreSQL test needs PW_FEED_DATABASE_URL, a throwaway database; "
                  "without it the test skips, and a skip proves nothing")
            report["error"] = "no database"
            return 1
        baseline = run_tests()
        report["baseline"] = baseline._asdict() if baseline else None
        if (not baseline or not baseline.valid or baseline.failed
                or baseline.passed != len(TEST_NAMES)):
            print("FAIL: the baseline is not complete and green; no control is credited")
            return 1
        for name, path, anchor, replacement in MUTANTS:
            original = path.read_text()
            if original.count(anchor) != 1 or anchor == replacement:
                print(f"INVALID anchor: {name}")
                report["error"] = f"anchor: {name}"
                return 1
            with mutation_source.changed(path, original, original.replace(anchor, replacement, 1)):
                result = run_tests()
            restored = digest(path) == before[str(path.relative_to(ROOT))]
            killed = bool(restored and result and result.valid and result.failed > 0)
            report["controls"].append({"name": name, "killed": killed, "restored": restored,
                                       "native": result._asdict() if result else None})
            print(f"{name}: {'KILLED' if killed else 'INVALID OR SURVIVED'}"
                  + (f" ({result.failed} of {result.passed + result.failed} tests fail)"
                     if killed else ""), flush=True)
            if not killed:
                return 1
        report["valid"] = True
    finally:
        report["restored"] = all(digest(path) == before[str(path.relative_to(ROOT))]
                                 for path in files)
        report["valid"] = report["valid"] and report["restored"]
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        killed = sum(control["killed"] for control in report["controls"])
        print(f"{killed} of {len(MUTANTS)} mutants killed; valid={report['valid']}, "
              f"restored={report['restored']}", flush=True)
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
