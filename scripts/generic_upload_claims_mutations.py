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
4. the memory layer: a new generation's bytes not kept before its rows; and
   (ADR-0440's asset contract suite, 2026-11-17) its commit, or its
   housekeeping, collecting no generation left unnamed, named bytes it lost
   answered not found, and a commit that failed before its rows were
   published answered unknown;
5. the ledger: an absent receipt giving the claim back (absence is never
   rollback), a live attempt offered to the periodic pass, another attempt's
   receipt (alike but for its nonce) settling this one, an inspection that
   could not be made holding its claim for good, the periodic pass inspecting
   nothing;
6. the PostgreSQL adapter: a keep that fails not refusing its commit, and
   a commit, or housekeeping, collecting no generation left unnamed;
7. the review of a7c3377b: a commit its ledger refused before its COMMIT
   answered unknown; the periodic pass leaving an absent receipt's claim due
   for ever; an attempt dropped uninspected never offered to the pass; each
   pass taking up its turn from the first again.

Not here, each for its reason: the review's robustness repairs (a poisoned counter after a landed COMMIT,
a ledger that fails while it reconciles, a removal racing memory's read)
need a panic or a race no test makes; a readerless command's refusing
imports are held by `uploads::bridge`'s unit test, the server path by no
fixture; the receipt marker is required by
the ledger's own freeze anyway; the serving handler's check of a live
generation's bytes and the storage's own (`check_bytes`) are each the
other's backstop, on either layer, so neither alone can be killed, and
`tests::asset_contract`'s `changed_bytes_are_answered_503_before_any_304`
holds the two together, on each.

The tests: `tests::upload_bridge` (the compiled program through the server,
on memory and on PostgreSQL), `tests::asset_contract` (one contract, each
case on both layers), `uploads::bridge` and `uploads::generic`. They
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
PG_LIB = ROOT / "runtime/pw-table-postgres/src/lib.rs"
PG_ENROLL = ROOT / "runtime/pw-table-postgres/src/assets.rs"
BLOBS = SERVER / "blob/assets.rs"

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
     "    fn claims_kept_nothing(&self) -> bool {\n        !self.published\n",
     "    fn claims_kept_nothing(&self) -> bool {\n        false\n"),
    ("a commit another attempt recorded first leaves its claims pending", LAYER_PG,
     "        self.refused || self.recorded\n",
     "        self.refused\n"),
    # The review of a7c3377b: a commit its ledger refused before its COMMIT.
    ("a commit its ledger refused before its COMMIT is answered unknown", MAIN,
     "                            held_claims(claims).is_ok_and(|c| c.before_commit())\n",
     "                            held_claims(claims).is_ok_and(|c| c.before_commit() && false)\n"),
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
    # The review of a7c3377b: what the pass concludes, and its turn.
    ("the periodic pass leaves an absent receipt's claim due for ever", MAIN,
     "                    leases.inspected(&ticket, true)\n",
     "                    leases.inspected(&ticket, false)\n"),
    ("an attempt dropped uninspected is not offered to the periodic pass", GENERIC,
     "                let _ = self.leases.inspected(ticket, false);\n",
     "                let _ = ticket;\n"),
    ("each pass takes up the attempts due from the first again", GENERIC,
     "            state.due_after = Some(last.attempt.clone());\n",
     "            let _ = last;\n"),
    # The review of a7c3377b: a removed row's generation collected.
    ("a commit collects no generation it left unnamed on PostgreSQL", PG_LIB,
     "        let _ = assets::collect_committed(&inner, open);\n",
     "        let _ = &open;\n"),
    # L5a's review: a namespace is its database's.
    ("a namespace's owner is not held to on an enrolled blob root", BLOBS,
     "            Ok(found) if found == marker => Ok(()),\n",
     "            Ok(_) => Ok(()),\n"),
    ("an enrolled namespace's owner is never asked on PostgreSQL", PG_ENROLL,
     "                .own(&namespace, &owner, false)\n",
     "                .own(&namespace, &owner, false)\n                .or(Ok::<(), std::io::Error>(()))\n"),
    ("a failing collection stops its pass on PostgreSQL", PG_ASSETS,
     "                Err(e) => failed.push(format!(\"{}: {e:?}\", d.reference.generation)),\n",
     "                Err(e) => return Err(e),\n"),
    ("housekeeping collects no generation a commit failed to on PostgreSQL", LAYER_PG,
     "            match self.tables.collect_assets(SWEPT) {\n",
     "            match Ok::<usize, String>(0) {\n"),
    # 6. the PostgreSQL adapter
    # L5a's review: a read takes no lifetime lock.
    ("an image read waits for the namespace's lock on PostgreSQL", PG_ASSETS,
     "        let Some(found) = self.current_descriptor(declared, declaration, key, kind)? else {\n",
     "        let mut held = self.inner.pool.take()?;\n        let _lifetime = lock_lifetime(&self.inner, &mut held)?;\n        let Some(found) = self.current_descriptor(declared, declaration, key, kind)? else {\n"),
    ("a keep that fails does not refuse its commit on PostgreSQL", PG_ASSETS,
     '                .map_err(|e| Error::Refused(format!("upload keep: {e}")))?;\n',
     "                .ok();\n"),
    # 8. the asset contract suite (`tests::asset_contract`, 2026-11-17):
    # memory's collection and its verified read, seen at the server through
    # the test's hands on its storage, and its refused commit's answer.
    ("memory's commit collects no generation left unnamed", LAYER_MEMORY,
     "        for descriptor in std::mem::take(&mut self.collect) {\n",
     "        for descriptor in std::mem::take(&mut self.collect).into_iter().filter(|_| false) {\n"),
    ("memory's housekeeping collects no generation a commit left", LAYER_MEMORY,
     "        let pending = match self.tables.pending_asset_collections(500) {\n",
     "        let pending = match self.tables.pending_asset_collections(0) {\n"),
    ("memory answers named bytes it lost not found", LAYER_MEMORY,
     "                    Some(_) => Err(missing(&descriptor)),\n",
     "                    Some(_) => Ok(None),\n"),
    ("memory answers a commit refused before its publication unknown", LAYER_MEMORY,
     "    fn definitely_refused(&self) -> bool {\n        !self.published\n",
     "    fn definitely_refused(&self) -> bool {\n        false\n"),
]

# One command, its filters, as the inventory is kept per command.
TESTS = [
    (["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--bin", "pw-dev-server"],
     ["tests::upload_bridge::", "tests::asset_contract::", "uploads::bridge::", "uploads::generic::"]),
]

SOURCES = [
    SERVER / "tests/upload_bridge.rs",
    SERVER / "tests/asset_contract.rs",
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
