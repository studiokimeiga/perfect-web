#!/usr/bin/env python3
"""Mutation controls for NEXT 4b (W9): a read is a bounded number of
statements on PostgreSQL. Eight controls restore loops that read a store
at a time: seven the store ran until 4b, and one ADR-0348's set estimates
replaced:

- the layer (`store_pg.rs`): the items' stores scanned store by store, every
  store's menu read; the stores' hours read a store at a time;
- the operations: `hours#for-stores` reading each store's hours alone; the
  search's reach reading each store it found alone; `checkout#parts`,
  `hours#for-cart` and the order's reach (`cart_reaches`) finding each line's
  store by scanning every store's menu; and `user-estimates#for-stores`
  reading each store's coverage alone (ADR-0348).

Seven more restore premature payment-count completion, remove one of the
store/feed scheduled-state write guards, or write before taking one
(ADR-0352's completion correction). Each must fail a real delayed-provider
or real PostgreSQL transaction test, and none by a stretch of time: a guard
control is killed by what the guard itself reports (`Writes::tell_on_wait`),
and the payment-count control by the count's own polls, at any speed.

A mutant must fail `tests::bounded_statements::`, the store's operations
and its pages and commands counted on PostgreSQL at the seed and with fifty
stores more (`postgres`, which needs PW_STORE_DATABASE_URL).

Run from the repository root; `just e14-bounded-statements` records the
output. The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import signal
import subprocess
import sys

from cargo_test_results import parse as cargo_result

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "spikes/own-renderer/server/src"
STORE_PG = SRC / "store_pg.rs"
HOURS_RS = SRC / "hours.rs"
SEARCH_RS = SRC / "search.rs"
CHECKOUT_RS = SRC / "checkout.rs"
STORE_RS = SRC / "store.rs"

# (what, suite, file, anchor, replacement)
MUTANTS = [
    (
        "set estimates read coverage once per store again",
        "postgres",
        STORE_RS,
        "                let coverage = coverage_of(r, &stores, reader)?;\n",
        "                let coverage = stores.iter().map(|store| Ok((store.clone(), coverage(r, store, reader)?))).collect::<Result<BTreeMap<_, _>, String>>()?;\n",
    ),
    (
        "PostgreSQL's items' stores are scanned store by store",
        "postgres",
        STORE_PG,
        "    fn stores_of_items(&mut self, items: &[String]) -> Result<BTreeMap<String, String>, String> {\n",
        "    fn stores_of_items(&mut self, items: &[String]) -> Result<BTreeMap<String, String>, String> {\n"
        "        #[allow(unreachable_code)]\n"
        "        return crate::store::scanned_stores_of_items(self, items);\n",
    ),
    (
        "PostgreSQL's stores' hours are read a store at a time",
        "postgres",
        STORE_PG,
        "        crate::hours::pg_hours_of(&mut self.0, stores)\n",
        "        crate::store::hours_one_by_one(self, stores)\n",
    ),
    (
        "hours#for-stores reads each store's hours alone",
        "postgres",
        HOURS_RS,
        "                    let found = r.hours_of(&stores)?;\n",
        "                    let found = crate::store::hours_one_by_one(r, &stores)?;\n",
    ),
    (
        "the search's reach reads each store it found alone",
        "postgres",
        SEARCH_RS,
        "        match reach.remove(&m.id).flatten() {\n",
        "        match crate::store::coverage(r, &m.id, reader)? {\n",
    ),
    (
        "checkout#parts scans every store's menu for each line's store",
        "postgres",
        CHECKOUT_RS,
        "    let of_item = r.stores_of_items(&items)?;\n",
        "    let of_item = crate::store::scanned_stores_of_items(r, &items)?;\n",
    ),
    (
        "hours#for-cart scans every store's menu for each line's store",
        "postgres",
        HOURS_RS,
        "                let of_item = r.stores_of_items(&items)?;\n",
        "                let of_item = crate::store::scanned_stores_of_items(r, &items)?;\n",
    ),
    (
        "the order's reach scans every store's menu for each line's store",
        "postgres",
        STORE_RS,
        "    let of_item = r.stores_of_items(&items)?;\n",
        "    let of_item = crate::store::scanned_stores_of_items(r, &items)?;\n",
    ),
    (
        "unchanged statement counters stand for payment completion",
        "postgres",
        SRC / "tests/bounded_statements.rs",
        'fn after_payments(s: &Server, completed: usize) -> Vec<u64> {\n',
        'fn after_payments(s: &Server, completed: usize) -> Vec<u64> {\n    #[allow(unreachable_code)]\n    return {\n        let _ = completed;\n        let mut last = s.data.statements_by_transaction();\n        loop {\n            std::thread::sleep(std::time::Duration::from_millis(300));\n            let now = s.data.statements_by_transaction();\n            if now == last { break now; }\n            last = now;\n        }\n    };\n',
    ),
    (
        'store_pg.rs ran bypasses the command write guard',
        "postgres",
        SRC / 'store_pg.rs',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n',
    ),
    (
        'store_pg.rs retry bypasses the command write guard',
        "postgres",
        SRC / 'store_pg.rs',
        '    fn retry(\n        &self,\n        id: i64,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<(), String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n',
        '    fn retry(\n        &self,\n        id: i64,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<(), String> {\n',
    ),
    (
        'feed_pg.rs ran bypasses the command write guard',
        "postgres",
        SRC / 'feed_pg.rs',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n',
    ),
    (
        'feed_pg.rs retry bypasses the command write guard',
        "postgres",
        SRC / 'feed_pg.rs',
        '    fn retry(\n        &self,\n        id: i64,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<(), String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n',
        '    fn retry(\n        &self,\n        id: i64,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<(), String> {\n',
    ),
    (
        'feed_pg.rs ran writes before it takes the command write guard',
        "postgres",
        SRC / 'feed_pg.rs',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n        self.pool.with(|c| crate::schedules::pg::ran(c, id))\n',
        '    fn ran(&self, id: i64) -> Result<(), String> {\n        let written = self.pool.with(|c| crate::schedules::pg::ran(c, id));\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n        written\n',
    ),
    (
        'store_pg.rs retry_current bypasses the command write guard',
        "postgres",
        SRC / 'store_pg.rs',
        '    fn retry_current(\n        &self,\n        expected: &pw_materialize::ScheduledRun,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<bool, String> {\n        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());\n',
        '    fn retry_current(\n        &self,\n        expected: &pw_materialize::ScheduledRun,\n        attempts: i64,\n        due_ms: i64,\n        failed: Option<&str>,\n    ) -> Result<bool, String> {\n',
    ),
]

TESTS = [
    "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
    "tests::bounded_statements::",
]
CARGO = {
    "postgres": (TESTS, {"PW_STORE_TEST_LAYER": "postgres"}),
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, env):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, cwd=ROOT, env={**os.environ, **env}, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(suite):
    """(built, passed, failed) over the suite's tests."""
    cmd, env = CARGO[suite]
    out, code = bounded(cmd, env)
    result = cargo_result(code, out)
    if not result.valid:
        return False, 0, 0
    return True, result.passed, result.failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    watch = mutation_baseline.mutation_bound.start()
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    if not os.environ.get("PW_STORE_DATABASE_URL"):
        print("FAIL: the postgres suite needs PW_STORE_DATABASE_URL, a throwaway database")
        return 1
    for suite in CARGO:
        built, passed, failed = cargo_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed or watch.stopped:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = cargo_tests(suite)
        finally:
            path.write_text(original)
        if not built or watch.stopped:
            print(f"{what} [{suite}]: REFUSED (invalid or stopped Cargo producer; no kill)", flush=True)
            mutation_baseline.explain()
            return 1
        if failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
