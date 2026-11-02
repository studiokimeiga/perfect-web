#!/usr/bin/env python3
"""Mutation controls for track store-pg: the store's data behind the
DataLayer seam, in memory and on PostgreSQL.

Each mutant undoes one piece:
- an order placed without its lines, in either layer, or on PostgreSQL;
- a commit without its events, its outbox's rows not written;
- a command's transaction left at the connection's default, and the
  isolation answered rather than measured;
- a commit the database refused answered as committed;
- the outbox not read back, and the commit not recorded in the materializer,
  or recorded without its rows;
- the store's connections searching more than its own schema;
- a program let name one of the host's own operations;
- a delivered event left in the materializer's outbox.

The tests then fail: the store's PostgreSQL tests, and the store's tests
of an order, its events and its host operations, run in memory and with
the store on PostgreSQL (PW_STORE_TEST_LAYER=postgres).

They need a database: `PW_STORE_DATABASE_URL`, a throwaway one, which each
test uses in a schema of its own. Without it every PostgreSQL test passes
doing nothing, and no mutant could be killed, so this refuses to run.

Run from the repository root; `just e14-store-postgres` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
STORE = ROOT / "spikes/own-renderer/server/src/store.rs"
CHECKOUT_RS = ROOT / "spikes/own-renderer/server/src/checkout.rs"
LAYER = ROOT / "spikes/own-renderer/server/src/store_pg.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "an order is placed without its lines",
        CHECKOUT_RS,
        # Re-anchored by track store-accounts: an owner's, a session's or a user's.
        # Re-anchored by track order-total: a user's order is placed with its
        # charges (`checkout.rs`), which the store's `place_order` calls.
        "    r.place(owner, lines)?;\n",
        "    r.place(owner, &[])?;\n",
    ),
    (
        "on PostgreSQL, an order's lines are not written",
        LAYER,
        '                    "INSERT INTO order_lines (order_id, position, item, name, quantity, price) \\\n',
        '                    "INSERT INTO order_lines (order_id, position, item, name, quantity, price) \\\n'
        '                     SELECT $1, $2, $3, $4, $5, $6 WHERE false \\\n',
    ),
    (
        "a commit commits without its events",
        LAYER,
        "        for (kind, name, values) in handed {\n",
        "        for (kind, name, values) in handed.take(0) {\n",
    ),
    (
        "a command's transaction is the connection's default",
        LAYER,
        'Isolation::Serializable => c.batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE"),',
        'Isolation::Serializable => c.batch_execute("BEGIN"),',
    ),
    (
        "the isolation is answered, not measured",
        LAYER,
        '            _ => "read_committed",\n',
        '            _ => "serializable",\n',
    ),
    (
        "a refused commit is answered as committed",
        LAYER,
        '        c.batch_execute("COMMIT").map_err(pg)?;\n',
        '        let _ = c.batch_execute("COMMIT");\n',
    ),
    ("the outbox is not read back", LAYER, '            Ok(read) => Ok(crate::data::Commit::Applied(read)),\n', '            Ok(_) => Ok(crate::data::Commit::Applied((Vec::new(), Vec::new()))),\n'),
    (
        "a commit on PostgreSQL is not recorded in the materializer",
        SERVER,
        "                let mut ids = Vec::new();\n"
        "                if self.data.session_entry() {\n",
        "                let mut ids = Vec::new();\n"
        "                if false {\n",
    ),
    (
        "a commit on PostgreSQL is recorded without its rows",
        SERVER,
        "                            self.materializer.command(|tx| {\n"
        "                                for (key, value) in &rows {\n",
        "                            self.materializer.command(|tx| {\n"
        "                                for (key, value) in rows.iter().take(0) {\n",
    ),
    (
        "the store's connections search more than its schema",
        LAYER,
        'c.batch_execute(&format!("SET search_path TO {}", ident(&self.schema)))',
        'c.batch_execute(&format!("SET search_path TO {}, public", ident(&self.schema)))',
    ),
    (
        "a program may name one of the host's own operations",
        SERVER,
        "                if i.key().starts_with(store::HOST_OPS) {\n",
        "                if i.key().starts_with(store::HOST_OPS) && false {\n",
    ),
    (
        "a command's delivered events stay in the materializer's outbox",
        SERVER,
        "            // `OrderChanged` in the materializer's outbox for good.\n"
        "            self.materializer.delivered(&ids);\n",
        "            // `OrderChanged` in the materializer's outbox for good.\n",
    ),
]

# The store's tests of what the mutants undo, which run on either layer.
STORE_TESTS = [
    "every_event_a_commit_stages_is_consumed_once_delivered",
    "a_program_naming_a_host_operation_is_refused",
    "an_order_is_placed_from_the_cart_and_reaches_its_open_page",
    "the_store_moving_an_order_along_reaches_its_open_page",
    "an_empty_cart_places_no_order",
]

CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--"]

# (what runs, the environment it adds)
TESTS = [
    (CARGO + STORE_TESTS, {"PW_STORE_TEST_LAYER": "memory"}),
    (CARGO + ["tests::store_pg::"] + STORE_TESTS, {"PW_STORE_TEST_LAYER": "postgres"}),
]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd, extra in TESTS:
        env = dict(os.environ, **extra)
        p = subprocess.Popen(cmd, cwd=ROOT, env=env, start_new_session=True,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            out, _ = p.communicate(timeout=BOUND)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            p.communicate()
            failed += 1
            continue
        found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not found:
            built = False
            continue
        for p_, f in found:
            passed += int(p_)
            failed += int(f)
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    if not os.environ.get("PW_STORE_DATABASE_URL"):
        print("FAIL: PW_STORE_DATABASE_URL is not set; without a database every test")
        print("passes doing nothing, and no mutant can mean anything")
        return 2
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed", flush=True)
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
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

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
