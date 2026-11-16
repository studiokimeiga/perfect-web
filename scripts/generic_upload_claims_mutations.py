#!/usr/bin/env python3
"""Mutation controls for the server's claim bridge (ADR-0440, L5b): a compiled
command claims, publishes and commits a declared upload through the server,
on memory and on PostgreSQL.

Each mutant undoes one rule and must be killed by a test that fails at its
assertion (the strict producer: `cargo_assertion_results`, each binary's
inventory frozen at the baseline; a build failure, a crash, or a failure
anywhere but an assertion is no kill):

1. the commit carries the attempt: its answer recorded unsealed (no nonce),
   its ledger not frozen before the commit, a committed attempt's claims not
   applied, a claiming command that writes no row keeping no record;
2. each outcome settles its claims: an attempt that kept nothing inspected
   all the same (its claim held for good), a failed attempt's receipt never
   inspected (a landed commit's claim given back), the layers' word on
   what kept nothing (PostgreSQL's refusal and another attempt's record
   answering first, memory's unpublished rows) taken for an unknown outcome;
3. the bridge and staging: a query reading its session's leases by another
   handle; a lease keyed by another declaration than its route's; a lease
   claimed, or shown, whoever its reader;
4. the memory layer: a new generation's bytes not kept before its rows;
5. the ledger: an absent receipt giving the claim back (absence is never
   rollback), a live attempt offered to the periodic pass, another attempt's
   receipt (alike but for its nonce) settling this one, an inspection that
   could not be made holding its claim for good, the periodic pass inspecting
   nothing;
6. the PostgreSQL adapter: a keep that fails not refusing its commit.

Not here, each for its reason: memory's collection of an unnamed generation
is storage-internal, unseen at the server; the receipt marker is required by
the ledger's own freeze anyway; the serving handler's check of a live
generation's bytes and the storage's own (`check_bytes`) are each the
other's backstop, so neither alone can be killed, and
`a_live_generations_corrupt_bytes_are_answered_503_on_postgresql` holds the
two together.

The tests: `tests::upload_bridge` (the compiled program through the server,
on memory and on PostgreSQL), `uploads::bridge` and `uploads::generic`. They
need a database for their PostgreSQL half: `PW_TABLES_DATABASE_URL` (or
`PW_STORE_DATABASE_URL`, `PW_FEED_DATABASE_URL`), a throwaway one; without
it this refuses to run.

Run whole from the repository root (`just e14-generic-upload-claims`). Every
source is restored in finally; the unmutated baseline must pass before any
mutant counts.
"""

import os
import pathlib
import subprocess
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import cargo_assertion_results  # noqa: E402 - after the path, as the scripts beside it
import mutation_source  # noqa: E402
import producer_process  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src"
MAIN = SERVER / "main.rs"
BRIDGE = SERVER / "uploads/bridge.rs"
GENERIC = SERVER / "uploads/generic.rs"
LAYER_PG = SERVER / "tables_pg.rs"
LAYER_MEMORY = SERVER / "tables.rs"
STAGING = SERVER / "uploads.rs"
PG_ASSETS = ROOT / "runtime/pw-table-postgres/src/assets/command.rs"

MUTANTS = [
    # 1. the commit carries the attempt
    ("an attempt's answer is recorded unsealed", MAIN,
     "                Some(claims) => held_claims(claims)?.receipt_answer(&answer)?,\n",
     "                Some(_) => answer,\n"),
    ("the ledger is not frozen before the commit", MAIN,
     "            held_claims(claims)?.begin_commit(interaction)?;\n",
     "            let _ = claims;\n"),
    ("a committed attempt's claims are not applied", MAIN,
     "                    CommitOutcome::Applied(_) => invocation.applied(),\n",
     "                    CommitOutcome::Applied(_) => Ok(()),\n"),
    ("a claiming command that writes no row keeps no record", MAIN,
     "            None if claimed => Vec::new(),\n",
     "            None if claimed && false => Vec::new(),\n"),
    # 2. each outcome settles its claims
    ("a failed attempt that kept nothing is inspected all the same", MAIN,
     "                Some(ticket) if !kept_nothing => {\n",
     "                Some(ticket) => {\n"),
    ("a failed attempt's receipt is never inspected", MAIN,
     "                Some(ticket) if !kept_nothing => {\n",
     "                Some(ticket) if false => {\n"),
    ("a refused commit's claims are left pending on PostgreSQL", LAYER_PG,
     "        self.refused || self.recorded\n",
     "        self.recorded\n"),
    ("a refused publication's claims are left pending in memory", LAYER_MEMORY,
     "        !self.published\n",
     "        false\n"),
    ("a commit another attempt recorded first leaves its claims pending", LAYER_PG,
     "        self.refused || self.recorded\n",
     "        self.refused\n"),
    # 3. the bridge
    ("a query reads its session's leases by another handle", BRIDGE,
     "                let cookie = asked.session(\"upload.attached\", &handle)?;\n",
     "                let cookie = {\n                    let _ = &asked;\n                    handle.as_str()\n                };\n"),
    ("a lease is keyed by another declaration than its route's", STAGING,
     "        let lease_key = lease_key(session, &declared.name);\n",
     "        let lease_key = lease_key(session, &self.declared.iter().find(|d| d.name != declared.name).unwrap_or(declared).name);\n"),
    ("a lease is claimed whoever its reader", STAGING,
     "            .filter(|l| !l.claimed && l.reader == *reader)?;\n",
     "            .filter(|l| !l.claimed)?;\n"),
    ("a lease is shown whoever its reader", STAGING,
     "            .filter(|lease| lease.reader == *reader)?;\n",
     "            .filter(|_| true)?;\n"),
    # 4. the memory layer
    ("a new generation's bytes are not kept in memory", LAYER_MEMORY,
     "                if plan.keep_new {\n",
     "                if plan.keep_new && false {\n"),
    # 5. the ledger
    ("an absent receipt gives the claim back", GENERIC,
     "                if !applied {\n                    self.leases.inspected(&ticket, true)?;\n                }\n",
     "                if !applied {\n                    self.leases.finish_attempt(&ticket, false)?;\n                }\n"),
    ("a live attempt is offered to the periodic pass", GENERIC,
     "            .filter(due)\n",
     "            .filter(|_| true)\n"),
    ("another attempt's receipt, alike but for its nonce, settles this one", GENERIC,
     "            || receipt.answer != ticket.proof.answer\n",
     ""),
    ("an inspection that could not be made holds its claim for good", GENERIC,
     "                self.leases.inspected(&ticket, false)?;\n",
     "                self.leases.inspected(&ticket, true)?;\n"),
    ("the periodic pass inspects no pending upload", MAIN,
     "        self.reconcile_uploads(16);\n",
     "        let _ = 16;\n"),
    # 6. the PostgreSQL adapter
    ("a keep that fails does not refuse its commit on PostgreSQL", PG_ASSETS,
     '                .map_err(|e| Error::Refused(format!("upload keep: {e}")))?;\n',
     "                .ok();\n"),
]

# One command, its filters, as the inventory is kept per command.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::upload_bridge::", "uploads::bridge::", "uploads::generic::"]),
]

SOURCES = [
    SERVER / "tests/upload_bridge.rs",
    SERVER / "tests/tables.rs",
    SERVER / "tests/uploads.rs",
    BRIDGE,
    SERVER / "uploads/bridge/tests.rs",
    GENERIC,
    SERVER / "uploads/generic/tests.rs",
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
    import mutation_baseline  # noqa: F401 - keeps every command and reports a red baseline
    if not database():
        print("FAIL: no database is named (PW_TABLES_DATABASE_URL, PW_STORE_DATABASE_URL or")
        print("PW_FEED_DATABASE_URL); without one the PostgreSQL half passes doing nothing")
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
