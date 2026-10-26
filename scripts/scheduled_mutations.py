#!/usr/bin/env python3
"""Mutation controls for the scheduled-commands track: a command runs when
the clock passes its time.

Each mutant undoes one piece.

The language (step one), which the compiler's tests of scheduling
(`tests/scheduled.rs`), of `invoked_by` (`tests/invoked_by.rs`) or the
corpus's (`generality.rs`, `checking_source.rs`) must catch:
- the grammar: a schedule's time read as words;
- the checker: a command a reader presses scheduled; a schedule outside a
  command taken; a schedule that says no time taken; a run's time of any
  type, or of a program's own type spelt as the clock's;
- the relations: a schedule's arguments related to nothing; its time
  related as the command's argument;
- the effects: a schedule performing nothing;
- `invoked_by Clock`: the clock no invoker; a payment's one `Settled`, or
  its one command, asked of the clock.

The host (steps two and three), which the server's tests of its runs and
the store's sweep (`tests::store_scheduled::`, `schedules::`) must catch,
the store in memory:
- the compiler's side: a schedules' import that says nothing of what it
  schedules; a schedule's time not given;
- staging: every run of a command one run, whatever its arguments; a
  scheduling that writes nothing else committing nothing; a duration
  counted from 1970;
- the materializer's: a run due a minute early; a run failed for good run
  again; a run scheduled again a second run; a cancel cancelling nothing;
- running: a run made as no reader; a run answered kept; a refused run
  taken as answered, run again at once, or never kept failed; the host
  granting no scheduling; a run's commit told to no one reading it;
- the store: a payment declined cancelling its sweep; one paid keeping
  it; an order taken back swept still; a sweep taking back an order it
  was not scheduled for.

And on PostgreSQL, which the same tests with the store there
(`PW_STORE_TEST_LAYER=postgres`) and the database's own
(`tests::store_pg::on_postgres_a_run…`, `…two_hosts…`, and the feed's
`tests::feed_pg::on_postgres_a_feeds_run…`) must catch: the store's or the
feed's commit writing no runs; a run due a minute early, or kept once
answered; a run keyed by its host, so a second host's run of it commits
again. They need a database: `PW_STORE_DATABASE_URL`, a throwaway one,
which each test uses in a schema of its own; without it this refuses to
run.

Run from the repository root; `just e14-scheduled` records the output. The
sources are restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import signal
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
SCHEDULED = ROOT / "compiler/pw-core/src/scheduled.rs"
VALUES = ROOT / "compiler/pw-core/src/values.rs"
EFFECTS = ROOT / "compiler/pw-core/src/effects.rs"
INVOKED = ROOT / "compiler/pw-core/src/invoked.rs"
CONTRACT = ROOT / "compiler/pw-core/src/contract.rs"
LOWER = ROOT / "compiler/pw-core/src/backend/lower.rs"
MATERIALIZE = ROOT / "runtime/pw-materialize/src/lib.rs"
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"
SCHEDULES = ROOT / "spikes/own-renderer/server/src/schedules.rs"
STORE_PG = ROOT / "spikes/own-renderer/server/src/store_pg.rs"
FEED_PG = ROOT / "spikes/own-renderer/server/src/feed_pg.rs"
STORE = ROOT / "examples/store/app.pw"

COMPILER, MEMORY, POSTGRES = "compiler", "memory", "postgres"

# (what is undone, the suite that must kill it, file, anchor, replacement)
MUTANTS = [
    (
        "a schedule's time is read as words",
        COMPILER,
        GRAMMAR,
        '        if keyword == "schedule"\n            && matches!(self.cur_text(), "at" | "after")\n',
        '        if keyword == "never"\n            && matches!(self.cur_text(), "at" | "after")\n',
    ),
    (
        "a command a reader presses is scheduled",
        COMPILER,
        SCHEDULED,
        '                    && t.policy("invoked_by")\n                        .is_some_and(|p| p.value.trim() == "Clock")\n',
        "                    && true\n",
    ),
    (
        "a schedule outside a command is taken",
        COMPILER,
        SCHEDULED,
        "            if decl.kind != DeclKind::Command {\n",
        "            if false {\n",
    ),
    (
        "a schedule that says no time is taken",
        COMPILER,
        SCHEDULED,
        '                        && matches!(modifiers[1].as_str(), "at" | "after")\n',
        "                        && true\n",
    ),
    (
        "a run's time is of any type",
        COMPILER,
        SCHEDULED,
        "                (Ty::Nominal(d, _), Some(want)) => *d == want,\n",
        "                (Ty::Nominal(_, _), Some(_)) => true,\n",
    ),
    (
        "a program's own type spelt as the clock's is the clock's",
        COMPILER,
        SCHEDULED,
        "                (Ty::Nominal(d, _), Some(want)) => *d == want,\n                // A type",
        "                (Ty::Nominal(_, _), Some(_)) => shown.ends_with(expected),\n                // A type",
    ),
    (
        "a schedule's arguments are related to nothing",
        COMPILER,
        VALUES,
        '                "query" | "subscription" | "navigate" | "schedule" | "unschedule"\n',
        '                "query" | "subscription" | "navigate"\n',
    ),
    (
        "a schedule's time is related as the command's argument",
        COMPILER,
        VALUES,
        '                        ("schedule", Some("at" | "after")) => &args[..args.len().saturating_sub(1)],\n',
        '                        ("schedule", Some("never")) => &args[..args.len().saturating_sub(1)],\n',
    ),
    (
        "a schedule performs nothing",
        COMPILER,
        EFFECTS,
        '                && matches!(keyword.as_str(), "schedule" | "unschedule")\n            {\n                let effect',
        '                && matches!(keyword.as_str(), "never")\n            {\n                let effect',
    ),
    (
        "the clock invokes nothing",
        COMPILER,
        INVOKED,
        'pub const INVOKERS: &[&str] = &["Payments", "Clock"];\n',
        'pub const INVOKERS: &[&str] = &["Payments"];\n',
    ),
    (
        "the clock's command is given one `Settled`",
        COMPILER,
        INVOKED,
        '        if invoker == "Payments" && !given_settled(unit, ws, decl) {\n',
        "        if !given_settled(unit, ws, decl) {\n",
    ),
    (
        "the clock runs one command",
        COMPILER,
        INVOKED,
        '        if invoker == "Payments"\n            && let Some(each) = all.get(invoker)\n',
        "        if true\n            && let Some(each) = all.get(invoker)\n",
    ),
    # --- the host, the store in memory ----------------------------------
    (
        "a schedules' import says nothing of what it schedules",
        MEMORY,
        CONTRACT,
        "                schedules: Some(Schedules {\n"
        "                    command: component_id(hirs[def.unit], crate::hir::DeclId(def.decl)),\n"
        "                    how: how.to_string(),\n"
        "                }),\n",
        "                schedules: None,\n",
    ),
    (
        "a schedule's time is not given",
        MEMORY,
        LOWER,
        "            values.push(value);\n",
        "            let _ = value;\n",
    ),
    (
        "every run of a command is one run, whatever its arguments",
        MEMORY,
        SERVER,
        "                            key: schedules::key(&scheduling.command, &args),\n",
        '                            key: schedules::key(&scheduling.command, ""),\n',
    ),
    ("a scheduling that writes nothing else commits nothing", MEMORY, SERVER, '            None if !scheduled.is_empty() || completing.is_some() => Vec::new(),\n', '            None if completing.is_some() => Vec::new(),\n'),
    (
        "a duration is counted from 1970",
        MEMORY,
        SCHEDULES,
        "                When::After(ms) => Some(now_ms.saturating_add(ms)),\n",
        "                When::After(ms) => Some(ms),\n",
    ),
    (
        "a run is due a minute early",
        MEMORY,
        MATERIALIZE,
        '            "WHERE failed IS NULL AND due_ms <= ?1 ORDER BY due_ms, id LIMIT ?2",\n',
        '            "WHERE failed IS NULL AND due_ms <= ?1 + 60000 ORDER BY due_ms, id LIMIT ?2",\n',
    ),
    (
        "a run failed for good is run again",
        MEMORY,
        MATERIALIZE,
        '            "WHERE failed IS NULL AND due_ms <= ?1 ORDER BY due_ms, id LIMIT ?2",\n',
        '            "WHERE due_ms <= ?1 ORDER BY due_ms, id LIMIT ?2",\n',
    ),
    (
        "a run scheduled again is a second run",
        MEMORY,
        MATERIALIZE,
        "        Materializer::unschedule(tx, key)?;\n",
        "",
    ),
    (
        "a cancel cancels nothing",
        MEMORY,
        SCHEDULES,
        "            None => Materializer::unschedule(tx, &s.key)?,\n",
        "            None => {}\n",
    ),
    (
        "a run is made as no reader",
        MEMORY,
        SERVER,
        '            &format!("schedule-{}", run.id),\n            &run.user,\n',
        '            &format!("schedule-{}", run.id),\n            "nobody",\n',
    ),
    ("a run answered is kept", MEMORY, MATERIALIZE.with_name("scheduled.rs"), '        let changed = tx.execute(\n            "DELETE FROM schedules WHERE id = ?1 AND command = ?2 AND args = ?3 AND user_id = ?4 AND due_ms = ?5 AND attempts = ?6 AND failed IS NULL AND protocol = 1",\n            params![expected.id, expected.command, expected.args, expected.user, expected.due_ms, expected.attempts],\n        ).map_err(|_| "scheduled completion was refused".to_string())?;\n', '        let changed = 1;\n'),
    ("a refused run is taken as answered", MEMORY, SERVER, '            Err(why) => why,\n        };\n        let failures = run.attempts.saturating_add(1);\n', '            Err(_why) => return false,\n        };\n        let failures = run.attempts.saturating_add(1);\n'),
    (
        "a refused run runs again at once",
        MEMORY,
        SCHEDULES,
        "    2000_i64\n",
        "    0_i64\n",
    ),
    ("a refused run is never kept failed", MEMORY, SERVER, '        let terminal = run.attempts >= schedules::RETRIES;\n', '        let terminal = false;\n'),
    (
        "a run's commit is told to no one reading it",
        MEMORY,
        SERVER,
        "        if !due.is_empty() {\n            self.tell_waiting();\n        }\n",
        "",
    ),
    (
        "the host grants no scheduling",
        MEMORY,
        SERVER,
        '                "clock.schedule",\n            ]\n',
        "            ]\n",
    ),
    (
        "a payment declined cancels its order's sweep",
        MEMORY,
        STORE,
        "        _ => false,\n",
        "        _ => true,\n",
    ),
    (
        "a payment paid keeps its order's sweep",
        MEMORY,
        STORE,
        "    if settled_for_good(settled.status) {\n",
        "    if false {\n",
    ),
    (
        "an order taken back is swept still",
        MEMORY,
        STORE,
        "            unschedule expire_order(awaited.order)\n",
        "",
    ),
    (
        "a sweep takes back an order it was not scheduled for",
        MEMORY,
        STORE,
        "        Some(awaited) => if awaited.order == order {\n",
        "        Some(awaited) => if true {\n",
    ),
    # --- the host, on PostgreSQL ----------------------------------------
    (
        "the store's commit on PostgreSQL writes no runs",
        POSTGRES,
        STORE_PG,
        "        crate::schedules::pg::write(c, scheduled).map_err(pg)?;\n",
        "",
    ),
    (
        "the feed's commit on PostgreSQL writes no runs",
        POSTGRES,
        FEED_PG,
        "        crate::schedules::pg::write(c, scheduled).map_err(pg)?;\n",
        "",
    ),
    (
        "a run on PostgreSQL is due a minute early",
        POSTGRES,
        SCHEDULES,
        "             WHERE failed IS NULL AND due_ms <= $1 ORDER BY due_ms, id LIMIT $2\",\n",
        "             WHERE failed IS NULL AND due_ms <= $1 + 60000 ORDER BY due_ms, id LIMIT $2\",\n",
    ),
    (
        "a run on PostgreSQL is kept once answered",
        POSTGRES,
        SCHEDULES,
        '        c.execute("DELETE FROM schedules WHERE id = $1", &[&id])\n',
        '        c.execute("DELETE FROM schedules WHERE id = -$1", &[&id])\n',
    ),
    (
        "a run is keyed by its host, and a second host's commits again",
        POSTGRES,
        SERVER,
        '        let interaction = format!("schedule-{}", run.id);\n',
        '        let interaction = format!("schedule-{}-{}", run.id, self.epoch);\n',
    ),
]

SERVER_TESTS = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--"]

# The tests each suite's mutants must fail.
TESTS = {
    COMPILER: (
        [
            "cargo", "test", "--locked", "-p", "pw-core", "--test", "scheduled",
            "--test", "invoked_by", "--test", "generality", "--test", "checking_source",
        ],
        {},
    ),
    MEMORY: (
        SERVER_TESTS + ["tests::store_scheduled::", "schedules::"],
        {"PW_STORE_TEST_LAYER": "memory"},
    ),
    POSTGRES: (
        SERVER_TESTS + [
            "tests::store_scheduled::",
            "tests::store_pg::on_postgres_a_run",
            "tests::store_pg::on_postgres_two_hosts",
            "tests::feed_pg::on_postgres_a_feeds_run",
        ],
        {"PW_STORE_TEST_LAYER": "postgres"},
    ),
}

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def run_tests(suite):
    """(built, passed, failed) for one suite's tests."""
    cmd, extra = TESTS[suite]
    env = {**os.environ, **extra}
    p = subprocess.Popen(cmd, cwd=ROOT, env=env, start_new_session=True,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        out, _ = p.communicate(timeout=BOUND)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.communicate()
        return True, 0, 1
    found = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
    if not found:
        return False, 0, 0
    return True, sum(int(a) for a, _ in found), sum(int(b) for _, b in found)


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    url = os.environ.get("PW_STORE_DATABASE_URL")
    if not url:
        print("FAIL: PW_STORE_DATABASE_URL is not set: the PostgreSQL mutants "
              "could not be killed, and the baseline would not hold the layer")
        return 1
    # The feed's test reads its own variable: the same throwaway database.
    os.environ.setdefault("PW_FEED_DATABASE_URL", url)
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    for suite in (COMPILER, MEMORY, POSTGRES):
        built, passed, failed = run_tests(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
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
            built, passed, failed = run_tests(suite)
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
