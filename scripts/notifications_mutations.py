#!/usr/bin/env python3
"""Mutation controls for track `notifications` (ADR-0270, ADR-0274).

Each mutant undoes one piece: the listener rule as ADR-0270 amends ADR-0091
(a handle binds an event's id, in a listener alone, for `User` alone, over
the event's own type); the host answering the reader's user; a like, a reply
or a follow writing a notification, and one's own act writing none; a
deleted post taking its notifications; a user reading their own alone, kept
in their session's partition and in no cache every reader shares; an event
naming a user dropping that user's entries and no other's; and reading them
reaching each of the reader's sessions. The tests in
`compiler/pw-core/tests/principal.rs` or
`spikes/own-renderer/server/src/tests/notifications.rs` must then fail.

The PostgreSQL mutants run where `PW_FEED_DATABASE_URL` names a database,
and are reported as not run otherwise.

Run from the repository root; `just e14-notifications` records the output.
The source is restored after every mutant, whatever happens.
"""

import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VALUES = ROOT / "compiler/pw-core/src/values.rs"
NOTES = ROOT / "spikes/own-renderer/server/src/notifications.rs"
FEED = ROOT / "spikes/own-renderer/server/src/feed.rs"
PG = ROOT / "spikes/own-renderer/server/src/feed_pg.rs"
MAIN = ROOT / "spikes/own-renderer/server/src/main.rs"
APP = ROOT / "examples/feed/app.pw"
MIGRATION = ROOT / "spikes/own-renderer/server/migrations/feed/0006_notifications.sql"

COMPILER = "compiler"
SERVER = "server"
POSTGRES = "postgres"
BROWSER = "browser"

# (what is undone, which tests, file, anchor, replacement)
# The mutants the checker refuses, by the rule that is the protection each
# takes away (`scripts/mutant_programs.py`): every other mutant here
# leaves a program that checks.
CHECKED = {
    "the count is cached shared, every reader's": {'PW5001'},
    'mark-read reaches no other session: it invalidates nothing': {'PW5106', 'PW5107'},
    "a like tells the post's author nothing: the count listens for no `Notified`": {'PW5106'},
}

MUTANTS = [
    (
        "a listener's handle binds no event's id (ADR-0091 unamended)",
        COMPILER,
        VALUES,
        "                            Verdict::Disagree if self.handle_over(*value, &a, &e) => Outcome::Agree,\n",
        "",
    ),
    (
        "any scoping handle binds an event's id, a session's too",
        COMPILER,
        VALUES,
        "                Some(crate::signatures::PrivacyQualifier::User)\n",
        "                Some(_)\n",
    ),
    (
        "a handle binds an id outside a listener: an emit, a call, an invalidation",
        COMPILER,
        VALUES,
        "        self.listens_with(id)\n            && matches!(\n",
        "        matches!(\n",
    ),
    (
        "a handle binds an event's value of any type",
        COMPILER,
        VALUES,
        "            && unify(&mut Subst::default(), expected, over) == Verdict::Agree\n",
        "            && !matches!(over, Ty::Unknown)\n",
    ),
    (
        "the host answers the reader's user with the session",
        SERVER,
        NOTES,
        "    let user = user_of(principals, session);\n    Arc::new(move |_args: &[Val]| Ok(vec![Val::String(user.clone())]))\n",
        "    let user = session.to_string();\n    Arc::new(move |_args: &[Val]| Ok(vec![Val::String(user.clone())]))\n",
    ),
    (
        "a page's `current_user()` is given another user's id",
        SERVER,
        MAIN,
        "                    notifications::user_of(&self.identity.principals(), session),\n",
        "                    \"u-ada\".to_string(),\n",
    ),
    (
        "one's own act notifies its actor",
        SERVER,
        NOTES,
        "    (recipient != actor && !recipient.is_empty()).then(|| Note {\n",
        "    (!recipient.is_empty()).then(|| Note {\n",
    ),
    (
        "a like writes no notification",
        SERVER,
        FEED,
        "                                staged.push(Change::Notify(note));\n"
        "                            }\n"
        "                            staged.push(Change::Like(id.clone()));\n",
        "                                let _ = note;\n"
        "                            }\n"
        "                            staged.push(Change::Like(id.clone()));\n",
    ),
    (
        "following again notifies again",
        SERVER,
        FEED,
        "                    let new = !s.follows.contains(&(follower.clone(), user.clone()));\n",
        "                    let new = !s.follows.contains(&(follower.clone(), user.clone())) || true;\n",
    ),
    (
        "another user's notifications are listed",
        SERVER,
        NOTES,
        "        .filter(|n| n.recipient == reader)\n        .take(limit.max(0) as usize)\n",
        "        .filter(|n| !n.recipient.is_empty())\n        .take(limit.max(0) as usize)\n",
    ),
    (
        "another user's notifications are counted",
        SERVER,
        NOTES,
        "        .filter(|n| n.recipient == reader && !n.read)\n",
        "        .filter(|n| !n.read)\n",
    ),
    (
        "a notification survives its post's deletion",
        SERVER,
        FEED,
        "                        .retain(|n| !n.post.as_ref().is_some_and(|p| gone.contains(p)));\n",
        "                        .retain(|n| n.post.is_some() || n.post.is_none());\n",
    ),
    (
        "reading them reads nothing",
        SERVER,
        FEED,
        "                        .filter(|n| n.recipient == reader)\n",
        "                        .filter(|n| n.recipient == reader && n.read)\n",
    ),
    (
        "mark-read reaches no other session: it invalidates nothing",
        SERVER,
        APP,
        "    invalidates   Notifications(current_user(), _), Unread(current_user())\n",
        "",
    ),
    (
        "a like tells the post's author nothing: the count listens for no `Notified`",
        SERVER,
        APP,
        "    key            reader\n    invalidates_on Notified(reader), Followed(_, reader), Deleted(_)\n",
        "    key            reader\n    invalidates_on Followed(_, reader), Deleted(_)\n",
    ),
    (
        "the event's user drops the entries of a user it does not name",
        SERVER,
        MAIN,
        "                Some(Some(v)) => serde_json::from_str::<serde_json::Value>(part)\n"
        "                    .is_ok_and(|p| p == val_to_json(v)),\n",
        "                Some(Some(v)) => serde_json::from_str::<serde_json::Value>(part)\n"
        "                    .is_ok_and(|p| p == val_to_json(v) || p.is_string()),\n",
    ),
    (
        "the count is cached shared, every reader's",
        SERVER,
        APP,
        "user query Unread(reader: capability.User<UserId>) -> Int\n"
        "    freshness      0.seconds\n"
        "    consistency    read_your_writes\n"
        "    cache          private\n",
        "user query Unread(reader: capability.User<UserId>) -> Int\n"
        "    freshness      0.seconds\n"
        "    consistency    read_your_writes\n"
        "    cache          shared\n",
    ),
    (
        "a private entry is kept for every reader",
        SERVER,
        MAIN,
        "    policy[\"cache\"] == \"private\" || policy[\"privacy\"] != \"public\"\n}\n",
        "    policy[\"cache\"] == \"private\" && policy[\"privacy\"] == \"none\"\n}\n",
    ),
    (
        "on PostgreSQL, a like writes no notification",
        POSTGRES,
        PG,
        "                    crate::notifications::pg::liked(c, id, &liker).map_err(pg)?;\n",
        "                    let _ = &liker;\n",
    ),
    (
        "on PostgreSQL, a deleted post's notifications are kept, unlinked",
        POSTGRES,
        MIGRATION,
        "    post         text REFERENCES posts (id) ON DELETE CASCADE,\n",
        "    post         text,\n",
    ),
    (
        "on PostgreSQL, one's own like notifies its liker",
        POSTGRES,
        NOTES,
        "             SELECT author, $2, 'liked', id FROM posts WHERE id = $1 AND author <> $2\",\n",
        "             SELECT author, $2, 'liked', id FROM posts WHERE id = $1\",\n",
    ),
    (
        "in the browser, the reader's user is their session",
        BROWSER,
        NOTES,
        "    let user = user_of(principals, session);\n    Arc::new(move |_args: &[Val]| Ok(vec![Val::String(user.clone())]))\n",
        "    let user = format!(\"{session} \");\n    Arc::new(move |_args: &[Val]| Ok(vec![Val::String(user.clone())]))\n",
    ),
    (
        "in the browser, another user's notifications are listed",
        BROWSER,
        NOTES,
        "        .filter(|n| n.recipient == reader)\n        .take(limit.max(0) as usize)\n",
        "        .filter(|n| n.recipient != reader || n.recipient == reader)\n"
        "        .take(limit.max(0) as usize)\n",
    ),
]

# The browser's: the server built again, then the suite in three engines on
# hosts of their own ports, away from any other suite's (`PORT`). A mutant of
# the program's source would leave the feed's build stale, and the suite
# unrun, so these mutate the server alone.
BUILD = ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server"]
PLAYWRIGHT = ["pnpm", "exec", "playwright", "test", "e2e/notifications.spec.mjs", "--reporter=line"]

TESTS = {
    COMPILER: [["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "principal"]],
    SERVER: [["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "notifications"]],
    BROWSER: [BUILD, PLAYWRIGHT],
    POSTGRES: [
        ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--", "on_postgres_", "notifications"]
    ],
}

DATABASE = os.environ.get("PW_FEED_DATABASE_URL", "")


def run_tests(group):
    """(built, passed, failed) over the group's test commands."""
    built, passed, failed = True, 0, 0
    for cmd in TESTS[group]:
        if cmd is BUILD:
            if subprocess.run(cmd, cwd=ROOT).returncode != 0:
                return False, 0, 0
            continue
        if cmd is PLAYWRIGHT:
            env = {**os.environ, "PORT": os.environ.get("PW_NOTIFICATIONS_PORT", "6100")}
            r = subprocess.run(
                cmd, cwd=ROOT / "spikes/own-renderer", capture_output=True, text=True, env=env
            )
            out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", r.stdout + r.stderr)
            ok = re.search(r"^ +(\d+) passed", out, re.M)
            bad = re.search(r"^ +(\d+) failed", out, re.M)
            if not ok and not bad:
                built = False
                continue
            passed += int(ok.group(1)) if ok else 0
            failed += int(bad.group(1)) if bad else 0
            continue
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
    groups = [COMPILER, SERVER, BROWSER] + ([POSTGRES] if DATABASE else [])
    for group in groups:
        built, passed, failed = run_tests(group)
        print(f"baseline ({group}): {passed} passed, {failed} failed")
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors, run = 0, 0
    for what, group, path, anchor, replacement in MUTANTS:
        if group not in groups:
            print(f"{what}: NOT RUN (PW_FEED_DATABASE_URL is not set)")
            continue
        run += 1
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}")
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = run_tests(group)
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

    # The server the browser's mutants built, built again as it is.
    subprocess.run(BUILD, cwd=ROOT)
    print(f"{run - survivors} of {run} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
