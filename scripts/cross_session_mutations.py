#!/usr/bin/env python3
"""Mutation controls for ADR-0219: what a commit drops reaches every
session that reads it, and only those.

Each mutant undoes one piece: the other sessions told, the line between a
session's own entry and a shared one, a whole query's drop, the pages read
again, the committing session left out, and the telling deferred until the
author is answered. The tests of each must then fail.

Run from the repository root; `just e14-cross-session` records the output.
The source is restored after every mutant, whatever happens.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SERVER = ROOT / "spikes/own-renderer/server/src/main.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    (
        "no other session is told",
        SERVER,
        # Re-anchored by ADR-0297: the sessions are told at once.
        "        let others: Vec<String> = others.into_iter().collect();\n",
        "        let others: Vec<String> = others.into_iter().take(0).collect();\n",
    ),
    (
        "a session's own entry reaches the others",
        SERVER,
        # Re-anchored by track store-accounts: a drop says whose it was.
        "            if entry_is_private(&policy) && pinned {\n",
        "            if false && entry_is_private(&policy) && pinned {\n",
    ),
    (
        "a whole query's drop reaches no one",
        SERVER,
        # Re-anchored by track store-accounts: a drop says whose it was.
        "            if entry_is_private(&policy) && pinned {\n",
        "            if !args.iter().any(Option::is_some) || (entry_is_private(&policy) && pinned) {\n",
    ),
    (
        "every open page is read again",
        SERVER,
        # Re-anchored by track store-accounts: a drop says whose it was.
        "            .filter(|doc| self.reads_any(&self.page_of(doc), &reaches(&doc.0)))\n",
        "            .filter(|_| true)\n",
    ),
    (
        "the committing session is told again",
        SERVER,
        "            .filter(|(s, _)| s != session)\n",
        "            .filter(|_| true)\n",
    ),
    (
        "the author waits for every reader",
        SERVER,
        "            self.telling\n"
        "                .lock()\n"
        '                .expect("telling")\n'
        "                .push((session.to_string(), dropped));\n",
        "            self.tell_others(session, &dropped);\n",
    ),
    (
        "what waits is never told",
        SERVER,
        "            .flat_map(|(session, reached)| self.others_reading(session, reached))\n",
        "            .take(0)\n            .flat_map(|(session, reached)| self.others_reading(session, reached))\n",
    ),
    (
        "a connection tells no one",
        SERVER,
        # Re-anchored by the keep-alive ADR: told after each request of a
        # kept connection, in `handle`'s loop.
        "        server.tell_waiting();\n"
        "        if !kept || !next_request(&conn, &mut reader, server.keep_alive) {\n",
        "        if !kept || !next_request(&conn, &mut reader, server.keep_alive) {\n",
    ),
]

TESTS = [
    [
        "cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--",
        "a_post_reaches_every_open_timeline",
        "a_post_over_http_reaches_another_reader_after_its_answer",
        "a_page_reading_nothing_dropped_is_told_nothing",
        "a_sessions_own_change_reaches_no_other_session",
        # The canonical store's cart is a user's since track store-accounts,
        # told by principal; a session's own entry, the benchmark's cart.
        "a_session_keyed_carts_change_reaches_no_other_session",
        "the_feed_is_served_by_the_host_its_data_the_deployments",
    ],
]


def run_tests():
    """(built, passed, failed) over every test command."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        out = r.stdout + r.stderr
        results = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", out)
        if not results:
            built = False
            continue
        for p, f in results:
            passed += int(p)
            failed += int(f)
    return built, passed, failed


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    built, passed, failed = run_tests()
    print(f"baseline: {passed} passed, {failed} failed")
    if not built or failed or not passed:
        print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
        mutation_baseline.explain()
        return 1

    survivors = 0
    for what, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
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
        print(f"{what}: {verdict}")

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
