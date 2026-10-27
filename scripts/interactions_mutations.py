#!/usr/bin/env python3
"""Mutation controls for ADR-0325: an interaction's answer commits with its
writes.

Each mutant undoes one piece:
- the host: a committed record not recalled before the command runs; one
  recalled with other arguments answered; the record not written in the
  materializer's transaction, or older ones not forgotten there; a record
  answering past its day; and a commit that met another host's record by
  its key answered as refused, not as the first;
- the layers: on PostgreSQL, the store's record or the feed's not written
  in the command's transaction; older ones not forgotten there; and one
  older than a day recalled.

The tests then fail: the host's in memory, and the store's and the feed's
on PostgreSQL.

They need a database: `PW_STORE_DATABASE_URL` and `PW_FEED_DATABASE_URL`, a
throwaway one, which each test uses in a schema of its own. Without them
every PostgreSQL test passes doing nothing, so this refuses to run.

Run from the repository root; `just e14-interactions` records the output.
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
STORE_PG = ROOT / "spikes/own-renderer/server/src/store_pg.rs"
FEED_PG = ROOT / "spikes/own-renderer/server/src/feed_pg.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "a committed record is not recalled before the command runs",
        SERVER,
        "        if let Some((first, answer)) = self.recalled_interaction(&key)? {\n",
        "        if let Some((first, answer)) = None::<(String, String)> {\n",
    ),
    (
        "a record recalled with other arguments answers them",
        SERVER,
        "            if first != sent {\n",
        "            if false {\n",
    ),
    (
        "the record is not written in the materializer's transaction",
        SERVER,
        "                        Materializer::record_interaction(tx, r.key, r.sent, r.answer, r.at_ms)?;\n",
        "",
    ),
    (
        "older records are not forgotten in the materializer's transaction",
        SERVER,
        "                        Materializer::prune_interactions(tx, r.forget_before_ms);\n",
        "",
    ),
    (
        "a record answers past its day",
        SERVER,
        "            .saturating_sub(INTERACTIONS_KEPT_MS);\n        if self.data.keeps_interactions() {\n",
        "            .saturating_sub(u64::MAX);\n        if self.data.keeps_interactions() {\n",
    ),
    ("a commit that met another host's record is answered as refused", SERVER, '                                Some((first, answer)) if first == sent => {\n                                    return Ok((\n                                        CommandOutcome::Answered(Answered::from_kept(&answer)),\n                                        Vec::new(),\n                                    ));\n                                }\n', ''),
    (
        "on PostgreSQL, the store's record is not written with its writes",
        STORE_PG,
        "            crate::feed_pg::record(c, r)?;\n",
        "",
    ),
    (
        "on PostgreSQL, the feed's record is not written with its writes",
        FEED_PG,
        "            record(c, r)?;\n",
        "",
    ),
    (
        "on PostgreSQL, older records are not forgotten in the transaction",
        FEED_PG,
        "    c.execute(\"DELETE FROM interactions WHERE at_ms < $1\", &[&before])\n        .map_err(pg)?;\n",
        "",
    ),
    (
        "on PostgreSQL, a record older than a day is recalled",
        FEED_PG,
        "        \"SELECT sent, answer FROM interactions WHERE key = $1 AND at_ms >= $2\",\n",
        "        \"SELECT sent, answer FROM interactions WHERE key = $1 AND (at_ms >= $2 OR true)\",\n",
    ),
]

CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "--exact"]

# (what runs, the environment it adds)
TESTS = [
    (
        CARGO + [
            "tests::interactions_kept_are_bounded_per_session",
            "tests::an_interactions_record_answers_it_for_a_day",
        ],
        {},
    ),
    (
        CARGO + [
            "tests::store_pg::on_postgres_an_interactions_answer_outlives_its_host",
            "tests::store_pg::on_postgres_a_second_hosts_commit_answers_what_the_first_committed",
            "tests::store_pg::on_postgres_an_interactions_record_answers_a_day",
            "tests::store_pg::on_postgres_a_retried_interaction_runs_its_command_once",
            "tests::feed_pg::on_postgres_a_posts_interaction_outlives_its_host",
        ],
        {},
    ),
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
    if not os.environ.get("PW_STORE_DATABASE_URL") or not os.environ.get("PW_FEED_DATABASE_URL"):
        print("FAIL: PW_STORE_DATABASE_URL or PW_FEED_DATABASE_URL is not set; without a")
        print("database every PostgreSQL test passes doing nothing, and no mutant there can")
        print("mean anything")
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
