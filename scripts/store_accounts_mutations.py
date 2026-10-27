#!/usr/bin/env python3
"""Mutation controls for track store-accounts: a cart and an order a user's,
a guest's cart joining its user's at sign-in, and a record's scope the
scopes of the queries that produce it, joined (ADR-0318).

Each mutant undoes one piece:
- a user's cart read and written by the session, not the reader's handle;
- a user's order read for another owner than the reader;
- the store keying a session's cart and order by the session where the
  program reads them by the reader (`Server::owner`);
- a session's cart entry keyed by the session, so the user's event reaches
  it nowhere;
- the guest's lines joined without summing one item's quantities;
- the guest's cart left as it was after joining;
- a sum past a bigint wrapped rather than refused;
- a signed-in session that signs in again joined to the next user;
- the deployment not told of a guest's sign-in;
- on PostgreSQL, a cart read whoever owns it;
- in the compiler, `private` giving a record no scope, and disagreeing
  producers keeping the last one's scope alone;
- `user` not a visibility (the parse refuses `user query`), not lowered as
  the declaration's visibility, and not importable;
- telling by principal undone: the key's user not read, so every reader of
  the query is told, or the user's other sessions not told;
- milestone 2, delivery addresses: a store's radius read in kilometres; the
  estimate without the courier's travel, or answering a store out of
  reach; an Add or an order to a store out of reach let through; a saved
  address not chosen, or a choice leaving the last one chosen; an eleventh
  address kept; a place the table does not hold saved; on PostgreSQL, a
  reader's addresses read whoever saved them, or store 48 delivering as far
  as store 47; and the store's reach not told when the reader's addresses
  change;
- milestone 1's rest, the stale tab (Q3): the host reading no document a
  command names; a page shown to another session, or one the host no
  longer holds, answered; a request from no numbered page refused; the
  predicate's words told for the platform's; a retry of a press that
  committed refused as stale, or its kept answer looked for under the
  retry's session; and the runtime naming no document.

Each mutant names its suite, and runs that suite's tests alone (W6's ruling
of 2026-10-09, as `refusal_mutations.py` does): `server`, the store's
accounts tests (tests/store_accounts.rs), its addresses tests
(tests/addresses.rs), the places' unit tests and the join's, in memory and
with the store on PostgreSQL (PW_STORE_TEST_LAYER=postgres), and identity's
sign-in tests; `core`, the compiler's boundary and visibility tests
(boundary_matrix.rs, boundary.rs's own, user_visibility.rs); `browser`, the
stale tab in Chromium (e2e/store-accounts.spec.mjs), after the store's page
and the servers are built.

They need a database: `PW_STORE_DATABASE_URL`, a throwaway one, which each
test uses in a schema of its own (and `PW_FEED_DATABASE_URL`, the feed's,
for the notifications' test on PostgreSQL; the recipe names one for both). Without it every PostgreSQL run would read
the in-memory layer and no PostgreSQL mutant could mean anything, so this
refuses to run.

Run from the repository root; `just e14-store-accounts` records the output.
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
LAYER = ROOT / "spikes/own-renderer/server/src/store_pg.rs"
IDENTITY = ROOT / "spikes/own-renderer/server/src/identity.rs"
BOUNDARY = ROOT / "compiler/pw-core/src/boundary.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
RESOLVE = ROOT / "compiler/pw-core/src/resolve.rs"
LOWER = ROOT / "compiler/pw-core/src/lower.rs"
RUNTIME = ROOT / "spikes/own-renderer/public/pw-runtime.mjs"
PLACES = ROOT / "spikes/own-renderer/server/src/places.rs"
ADDRESSES_SQL = ROOT / "spikes/own-renderer/server/migrations/store/0004_addresses.sql"
APP = ROOT / "examples/store/app.pw"

# (what is undone, file, anchor, replacement)
# The mutants the checker refuses, by the rule that is the protection each
# takes away (`scripts/mutant_programs.py`): every other mutant here
# leaves a program that checks.
CHECKED = {
    "the store's reach is not told when the reader's addresses change": {'PW5106'},
}

MUTANTS = [
    (
        "a user's cart is read and written by the session",
        "server",
        STORE,
        "                    let (owner, rest) = if by_reader {\n"
        "                        reader_of(&op, args)?\n"
        "                    } else {\n"
        "                        (this.as_str(), own_session(&op, &this, args)?)\n"
        "                    };\n"
        "                    if fail {\n",
        "                    let (owner, rest) = if by_reader {\n"
        "                        (this.as_str(), reader_of(&op, args)?.1)\n"
        "                    } else {\n"
        "                        (this.as_str(), own_session(&op, &this, args)?)\n"
        "                    };\n"
        "                    if fail {\n",
    ),
    (
        "a user's order is read for another owner than the reader",
        "server",
        STORE,
        "                let owner = if by_reader {\n"
        "                    reader_of(&op, args)?.0\n"
        "                } else {\n"
        "                    own_session(&op, &this, args)?;\n"
        "                    this.as_str()\n"
        "                };\n"
        "                w(&mut |r| {\n"
        # Re-anchored by NEXT 6c: an awaiting order's status carries its page.
        "                    let status = match r.order(owner)? {\n",
        "                let owner = if by_reader {\n"
        "                    reader_of(&op, args)?;\n"
        "                    \"ada\"\n"
        "                } else {\n"
        "                    own_session(&op, &this, args)?;\n"
        "                    this.as_str()\n"
        "                };\n"
        "                w(&mut |r| {\n"
        "                    let status = match r.order(owner)? {\n",
    ),
    (
        "the store keys a session's cart by the session where the program reads it by the reader",
        "server",
        SERVER,
        "            Some(true) => notifications::user_of(&self.identity.principals(), session),\n",
        "            Some(true) => session.to_string(),\n",
    ),
    (
        "a session's cart entry is keyed by the session",
        "server",
        SERVER,
        "        \"store.page.Cart\",\n        &[owner],\n",
        "        \"store.page.Cart\",\n        &[session],\n",
    ),
    (
        "a guest's lines are joined without summing one item's quantities",
        "server",
        STORE,
        "                held.quantity = held.quantity.checked_add(line.quantity).ok_or_else(|| {\n",
        "                held.quantity = held.quantity.checked_add(0).ok_or_else(|| {\n",
    ),
    (
        "the guest's cart is left as it was after joining",
        "server",
        STORE,
        "            r.set_cart(user, &lines)?;\n            r.set_cart(guest, &[])?;\n",
        "            r.set_cart(user, &lines)?;\n",
    ),
    (
        "a sum past a bigint is wrapped rather than refused",
        "server",
        STORE,
        "                held.quantity = held.quantity.checked_add(line.quantity).ok_or_else(|| {\n",
        "                held.quantity = Some(held.quantity.wrapping_add(line.quantity)).ok_or_else(|| {\n",
    ),
    (
        "a signed-in session that signs in again is joined to the next user",
        "server",
        IDENTITY,
        "        let was_guest = self.principals.of(session).is_none_or(|p| p.is_guest());\n",
        "        let was_guest = true;\n",
    ),
    (
        "the deployment is not told of a guest's sign-in",
        "server",
        IDENTITY,
        "        if was_guest && !session.is_empty() {\n            on_sign_in(",
        "        if was_guest && session.is_empty() {\n            on_sign_in(",
    ),
    (
        "on PostgreSQL, a cart is read whoever owns it",
        "server",
        LAYER,
        "                \"SELECT item, name, quantity, price FROM cart_lines \\\n"
        "                 WHERE owner = $1 ORDER BY position\",\n",
        "                \"SELECT item, name, quantity, price FROM cart_lines \\\n"
        "                 WHERE owner = $1 OR true ORDER BY position\",\n",
    ),
    (
        # Re-anchored by ADR-0324: `private` gives no scope since, and
        # `user` alone is the user's.
        "in the compiler, `user` gives a record no scope",
        "core",
        BOUNDARY,
        '                    Some("user") => Restriction::User("UserId".into()),\n',
        '                    Some("nothing") => Restriction::User("UserId".into()),\n',
    ),
    (
        "in the compiler, disagreeing producers keep the last one's scope",
        "core",
        BOUNDARY,
        "                let held = f.scoped.entry(produced.semantic_key()).or_default();\n"
        "                *held = held.join(&Label::of(restriction));\n",
        "                f.scoped.insert(produced.semantic_key(), Label::of(restriction));\n",
    ),
    (
        "the key's user is not read, and every reader is told",
        "server",
        SERVER,
        "                .filter(|_| entry_is_private(&policy));\n",
        "                .filter(|_| false);\n",
    ),
    (
        "the user's other session is not told",
        "server",
        SERVER,
        "                        .is_none_or(|user| notifications::user_of(&principals, other) == user)\n",
        "                        .is_none_or(|_| false)\n",
    ),
    (
        "`user` is not a visibility",
        "core",
        GRAMMAR,
        '        if !self.at_kw("user") {\n            return false;\n        }\n',
        '        if !self.at_kw("user") || true {\n            return false;\n        }\n',
    ),
    (
        "`user` is not lowered as the declaration's visibility",
        "core",
        LOWER,
        '    matches!(first.as_str(), "public" | "session" | "private" | "user").then_some(first)\n',
        '    matches!(first.as_str(), "public" | "session" | "private").then_some(first)\n',
    ),
    (
        "`user` is not importable",
        "core",
        RESOLVE,
        '                    if decl.is_some_and(|d| d.visibility.as_deref() == Some("private")) {\n',
        '                    if decl.is_some_and(|d| matches!(d.visibility.as_deref(), Some("private") | Some("user"))) {\n',
    ),
    # Milestone 2: delivery addresses.
    (
        "a store's radius is read in kilometres where it is metres",
        "server",
        PLACES,
        "    distance_km(zone.lat_e6, zone.lon_e6, place.lat_e6, place.lon_e6) * 1000.0\n"
        "        <= zone.radius_m as f64\n",
        "    distance_km(zone.lat_e6, zone.lon_e6, place.lat_e6, place.lon_e6)\n"
        "        <= zone.radius_m as f64\n",
    ),
    (
        "the estimate leaves out the courier's travel",
        "server",
        STORE,
        "                    Some(Reach { minutes, .. }) => minutes,\n",
        "                    Some(Reach { .. }) => 0,\n",
    ),
    (
        "the estimate answers for a store out of reach",
        "server",
        STORE,
        "                    }) => return Ok(declared(\"no-coverage\")),\n",
        "                    }) => 0,\n",
    ),
    (
        "an Add to a store out of reach is let through",
        "server",
        STORE,
        "                    coverage(r, store, reader)?.is_none_or(|c| c.delivers),\n",
        "                    coverage(r, store, reader)?.is_none_or(|_| true),\n",
    ),
    (
        "an order a store out of reach is in is placed",
        "server",
        STORE,
        # Re-anchored by ADR-0332: the store places an order through
        # `#await-payment`, and no program reaches `#place` since (train-5's
        # run found this surviving there).
        "                if !cart_reaches(r, &lines, owner)? {\n",
        "                if !cart_reaches(r, &lines, owner)? && false {\n",
    ),
    (
        "a saved address is not chosen",
        "server",
        STORE,
        "            place: place.clone(),\n            chosen: true,\n",
        "            place: place.clone(),\n            chosen: false,\n",
    ),
    (
        "choosing an address leaves the last one chosen",
        "server",
        STORE,
        "            a.chosen = a.id == *id;\n",
        "            a.chosen |= a.id == *id;\n",
    ),
    (
        "an eleventh address is kept",
        "server",
        STORE,
        "        if rows.len() >= MOST_ADDRESSES {\n",
        "        if rows.len() > MOST_ADDRESSES {\n",
    ),
    (
        "a place the table does not hold is saved",
        "server",
        STORE,
        "        if crate::places::place(place).is_none() {\n            return Ok(Err(\"unknown-place\"));\n",
        "        if crate::places::place(place).is_none() && false {\n            return Ok(Err(\"unknown-place\"));\n",
    ),
    (
        "on PostgreSQL, a reader's addresses are read whoever saved them",
        "server",
        LAYER,
        "                \"SELECT id, label, place, chosen FROM addresses \\\n"
        "                 WHERE owner = $1 ORDER BY position\",\n",
        "                \"SELECT id, label, place, chosen FROM addresses \\\n"
        "                 WHERE owner = $1 OR true ORDER BY position\",\n",
    ),
    (
        "on PostgreSQL, store 48 delivers as far as store 47",
        "server",
        ADDRESSES_SQL,
        "radius_m = 2000 WHERE id = '48';\n",
        "radius_m = 4000 WHERE id = '48';\n",
    ),
    (
        "the store's reach is not told when the reader's addresses change",
        "server",
        APP,
        "    key            id, reader\n"
        "    invalidates_on AddressesChanged(reader)\n"
        "    concurrency    one_per_key\n"
        "    on_key_change  cancel\n"
        "    timeout        2.seconds\n"
        "{\n"
        "    Addresses.coverage(id, reader)\n",
        "    key            id, reader\n"
        "    concurrency    one_per_key\n"
        "    on_key_change  cancel\n"
        "    timeout        2.seconds\n"
        "{\n"
        "    Addresses.coverage(id, reader)\n",
    ),
    # Milestone 1's rest, the stale tab (Q3): a command is answered for the
    # reader its page was shown to.
    (
        "the host reads no document a command names",
        "server",
        SERVER,
        '            let stale = header("pw-document")\n',
        '            let stale = header("pw-documents")\n',
    ),
    (
        "a page shown to another session is answered as the reader's",
        "server",
        SERVER,
        '            Some(_) => Some(ANOTHER_READER),\n',
        '            Some(_) => None,\n',
    ),
    (
        "a page the host no longer holds is answered",
        "server",
        SERVER,
        "            None => Some(PAGE_NOT_HELD),\n",
        "            None => None,\n",
    ),
    (
        "a request from a page the host did not number is refused",
        "server",
        SERVER,
        "        if document == 0 {\n            return None;\n        }\n",
        "",
    ),
    (
        "the reader is told the predicate's words, not the platform's",
        "server",
        SERVER,
        '    "You signed in or out in another tab. Reload this page to go on.",\n',
        '    "Sign in to do this.",\n',
    ),
    (
        "a retry of a press that committed is refused as stale",
        "server",
        SERVER,
        "                server.kept_answer(&session, *document, id, &args, interaction.as_deref())\n",
        "                {\n                    let _ = document;\n                    None\n                }\n",
    ),
    (
        "a current reader's kept answer is ignored when its page is gone",
        "server",
        SERVER,
        '            Some(Ok(kept)) => Ok(Some(Answered::from_kept(&kept))),\n',
        '            Some(Ok(_kept)) => Ok(None),\n',
    ),
    (
        "the runtime names no document",
        "browser",
        RUNTIME,
        '          ...(documentCursor > 0 ? { "pw-document": String(documentCursor) } : {}),\n',
        "",
    ),
]

# The store's accounts tests, and the join's own, which run on either layer.
STORE_TESTS = [
    "tests::store_accounts::",
    "store::joins::",
    # Milestone 2: a reader's addresses, and the places' rule.
    "tests::addresses::",
    "places::",
    # Telling by principal reaches the feed's notifications too.
    "tests::notifications::reading_them_tells_no_other_users_page",
    "tests::notifications::on_postgres_reading_them_tells_no_other_users_page",
    "a_sessions_own_change_reaches_no_other_session",
]

CARGO = ["cargo", "test", "--quiet", "--locked", "-p", "pw-dev-server", "--"]

# Each suite's commands: (what runs, the environment it adds). A mutant runs
# its own suite's (W6's ruling of 2026-10-09, as `refusal_mutations.py`
# does: a mutant is given the tests that can see its rule). Until
# 2026-10-10 every mutant ran every command, and the recipe took 1 h 51 min
# of CI.
SUITES = {
    "server": [
        (CARGO + STORE_TESTS + ["tests::sign_in::"], {"PW_STORE_TEST_LAYER": "memory"}),
        (CARGO + STORE_TESTS, {"PW_STORE_TEST_LAYER": "postgres"}),
    ],
    "core": [
        (
            ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "boundary_matrix"],
            {},
        ),
        (["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--lib", "boundary"], {}),
        (
            ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "user_visibility"],
            {},
        ),
    ],
}

# The browser's: the stale tab in Chromium, after the store's page and the
# server are built from the source as it is.
SPEC = ["e2e/store-accounts.spec.mjs", "-g", "signed in or out", "--project=chromium"]

# How long one command may run. Past the bound it is killed with everything
# it started, and the run counts as failing.
BOUND = 1200


def bounded(cmd, **kw):
    """(output, returncode), or (output, None) when it ran past the bound."""
    p = subprocess.Popen(cmd, start_new_session=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, **kw)
    try:
        out, _ = p.communicate(timeout=BOUND)
        return out, p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        out, _ = p.communicate()
        return out, None


def cargo_tests(suite):
    """(built, passed, failed) over the suite's commands."""
    built, passed, failed = True, 0, 0
    for cmd, extra in SUITES[suite]:
        out, code = bounded(cmd, cwd=ROOT, env=dict(os.environ, **extra))
        if code is None:
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


def browser_tests(_suite="browser"):
    """(built, passed, failed), after a build of the store's page, which
    serves the runtime it was built with, and of the servers the suite
    starts, which the script does not build."""
    built = subprocess.run(
        ["bash", "spikes/own-renderer/run.sh"],
        cwd=ROOT,
        env={**os.environ, "BUILD_ONLY": "1"},
        capture_output=True,
        text=True,
    )
    if built.returncode != 0:
        return False, 0, 0
    servers = subprocess.run(
        ["cargo", "build", "--quiet", "--locked", "-p", "pw-dev-server", "-p", "kiokun-server"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if servers.returncode != 0:
        return False, 0, 0
    out, code = bounded(
        ["pnpm", "exec", "playwright", "test", *SPEC, "--reporter=line"],
        cwd=ROOT / "spikes/own-renderer",
    )
    if code is None:
        return True, 0, 1
    out = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", out)
    passed = sum(int(n) for n in re.findall(r"(\d+) passed", out))
    failed = sum(int(n) for n in re.findall(r"(\d+) failed", out))
    return passed + failed > 0, passed, failed


RUN = {"server": cargo_tests, "core": cargo_tests, "browser": browser_tests}


def main():
    import mutation_baseline  # keeps what each command says, for a red baseline
    # Stopped from outside, the source is still restored: SIGTERM raises
    # here, and the `finally` below runs.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    if not os.environ.get("PW_STORE_DATABASE_URL"):
        print("FAIL: PW_STORE_DATABASE_URL is not set; without a database the PostgreSQL")
        print("runs read the in-memory layer, and no PostgreSQL mutant can mean anything")
        return 2
    for suite, run in RUN.items():
        built, passed, failed = run(suite)
        print(f"baseline ({suite}): {passed} passed, {failed} failed", flush=True)
        if not built or failed or not passed:
            print("FAIL: the unmutated baseline is not green; no mutant can mean anything")
            mutation_baseline.explain()
            return 1

    survivors = 0
    for what, suite, path, anchor, replacement in MUTANTS:
        original = path.read_text()
        if original.count(anchor) != 1:
            print(f"{what}: ANCHOR NOT FOUND EXACTLY ONCE in {path.name}", flush=True)
            survivors += 1
            continue
        try:
            path.write_text(original.replace(anchor, replacement, 1))
            built, passed, failed = RUN[suite](suite)
        finally:
            path.write_text(original)
        if not built:
            verdict = "KILLED (does not build)"
        elif failed:
            verdict = f"KILLED ({failed} of {passed + failed} tests fail)"
        else:
            verdict = "SURVIVED"
            survivors += 1
        print(f"{what} [{suite}]: {verdict}", flush=True)
    # The page and the servers are built again from the restored source.
    browser_tests()

    print(f"{len(MUTANTS) - survivors} of {len(MUTANTS)} mutants killed")
    return 1 if survivors else 0


if __name__ == "__main__":
    sys.exit(main())
