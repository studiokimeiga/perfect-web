#!/usr/bin/env python3
"""Mutation controls for the owned-rows ADR: a row says who may change it.

Each mutant undoes one part of PW0370's rule, of what a table's `owner`
records, or of how the platform holds every change to an owned row, and
must be killed by a test, at a native assertion (the strict producer of
`d3_postgres_mutations.py`: each test binary's complete named inventory,
every failure an assertion's; a mutant that does not build kills nothing):

1. the rule: a table a command a reader can run writes says who may change
   its rows, refused at its name beside the call that writes, through what
   the command calls, and through each command the clock runs for its
   schedule, in turn; a command the platform runs does not count; a table
   that says who is not refused, and one whose word is no owner's is told
   once (PW0335's);
2. the clause: `owner` heads a table's clause only, where its word follows,
   of three words;
3. what is recorded: `owner user` and `owner session` in the schema, `owner
   anyone` nothing; an owner recorded from now on or given up is a step of
   the migration, and one of another kind is refused (PW0366);
4. a row holds no handle (PW6901, track data-path's, carried here);
5. the enforcement, on the memory adapter: a reader's put, delete and
   prefix's delete change only its own rows; an insert records its writer
   and a put keeps the owner; a row undone keeps its owner; the adapter
   reads each table's owner from the manifest;
6. the host's writer: its scope is its reader's id of the table's kind, a
   signed-out reader owns no user's row, a reader's command is never the
   platform's, another invoker's never a reader's, and the clock's run is
   the reader's who scheduled it;
7. the enforcement, on PostgreSQL: the owned put (its guard on the row its
   conflict found) and delete statements, a row recording no owner being no
   reader's, the owner column made, a put of another's row answered as
   written, one that met a row inserted since answered alike, and the
   migration of an owner (made, given up only as stated, never of another
   kind).

They need a database: `PW_TABLES_DATABASE_URL` (or `PW_STORE_DATABASE_URL`,
`PW_FEED_DATABASE_URL`), a throwaway one; without it the PostgreSQL tests
pass doing nothing, so this refuses to run.

Run whole from the repository root. Every source is restored, whatever
happens; the unmutated baseline must pass before any mutant means anything.
Words given as arguments run only the mutants whose description holds one of
them, for a quick look while working; the record is a whole run.
"""
import os
import pathlib
import subprocess
import sys

import cargo_assertion_results
import mutation_source
import producer_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLES = ROOT / "compiler/pw-core/src/tables.rs"
POLICY = ROOT / "compiler/pw-core/src/policy.rs"
GRAMMAR = ROOT / "compiler/pw-syntax/src/grammar.rs"
HOST = ROOT / "runtime/pw-host/src/tables.rs"
MEMORY = ROOT / "runtime/pw-table-memory/src/lib.rs"
PG = ROOT / "runtime/pw-table-postgres/src/lib.rs"
SQL = ROOT / "runtime/pw-table-postgres/src/sql.rs"
MIGRATE = ROOT / "runtime/pw-table-postgres/src/migrate.rs"

# (what is undone, file, anchor, replacement)
MUTANTS = [
    # 1. the rule
    ("a table a reader's command writes is not refused", TABLES,
     '            if decl.kind != DeclKind::Command || decl.policy("invoked_by").is_some() {\n',
     "            if true {\n"),
    ("a command the platform runs counts as a reader's", TABLES,
     '            if decl.kind != DeclKind::Command || decl.policy("invoked_by").is_some() {\n',
     "            if decl.kind != DeclKind::Command {\n"),
    ("a write a reader's command schedules is not the reader's", TABLES,
     "            for (first, at) in scheduled_by(hirs, ws, unit, decl) {\n",
     "            for (first, at) in Vec::<(DefId, Span)>::new() {\n"),
    ("a schedule's own schedules are not followed", TABLES,
     "                        scheduled_by(hirs, ws, def.unit, target)\n",
     "                        Vec::<(DefId, Span)>::new()\n"),
    ("the call that writes is not named, only the command", TABLES,
     "                    .map_or_else(|| decl.name_span.clone(), |s| s.span.clone());\n",
     "                    .map_or_else(|| decl.name_span.clone(), |_| decl.name_span.clone());\n"),
    ("a table that says who may change its rows is refused anyway", TABLES,
     '        if t.owner.is_some() || hir.decl(t.decl).policy("owner").is_some() {\n',
     "        if false {\n"),
    ("an owner of no word is refused twice", TABLES,
     '        if t.owner.is_some() || hir.decl(t.decl).policy("owner").is_some() {\n',
     "        if t.owner.is_some() {\n"),
    ("the refusal is told at the table's declaration, not its name", TABLES,
     "            primary_span: t.span.clone(),\n",
     "            primary_span: hir.decl_span(t.decl),\n"),
    # 2. the clause
    ("`owner` heads a clause wherever it is written", GRAMMAR,
     '    "layout", "set", "sets", "row", "order", "index", "unique", "refers", "migrate", "owner",\n',
     '    "layout", "set", "sets", "row", "order", "index", "unique", "refers", "migrate",\n'),
    ("`owner` is no clause", GRAMMAR,
     "    // begin with, and a field's (`refers owner to Users`).\n    \"owner\",\n",
     "    // begin with, and a field's (`refers owner to Users`).\n"),
    ("`owner` is no table's clause", POLICY,
     '        "row" | "order" | "index" | "unique" | "refers" | "bounded" | "owner" => {\n',
     '        "row" | "order" | "index" | "unique" | "refers" | "bounded" => {\n'),
    ("`owner` takes a word no owner is", POLICY,
     '        "owner" => Domain::Word(OWNERS),\n',
     '        "owner" => Domain::Word(&["user", "session", "anyone", "everyone"]),\n'),
    # 3. what is recorded
    ("`owner anyone` records an owner", TABLES,
     "            Owner::Anyone => None,\n",
     "            Owner::Anyone => Some(OwnerKind::Session),\n"),
    ("the schema records no owner", TABLES,
     "            owner: t.owner.as_ref().and_then(|o| o.value.recorded()),\n",
     "            owner: None,\n"),
    ("an owner of another kind is taken (PW0366)", TABLES,
     "            (Some(was), Some(is)) => refused.push((\n",
     "            (Some(was), Some(is)) if false => refused.push((\n"),
    ("an owner recorded or given up is no step", TABLES,
     "            (was, is) if was == is => {}\n",
     "            (was, is) if was == is || was.is_none() || is.is_none() => {}\n"),
    # 4. a row holds no handle
    ("a row holding a handle is not refused", TABLES,
     "            .and_then(|ty| crate::check::handle_in(sigs, ty, &mut BTreeSet::new()))\n",
     "            .and_then(|_| None::<&'static str>)\n"),
    # 5. the memory adapter
    ("a reader's put changes another reader's row", MEMORY,
     "                Some(existing) if !scope.changes(existing.owner.as_deref()) => return Ok(()),\n",
     "                Some(existing) if false && !scope.changes(existing.owner.as_deref()) => {\n"
     "                    return Ok(());\n                }\n"),
    ("a reader's delete removes another reader's row", MEMORY,
     "                .is_some_and(|row| scope.changes(row.owner.as_deref()))\n",
     "                .is_some_and(|_| true)\n"),
    ("a prefix's delete removes every reader's rows", MEMORY,
     "                .filter(|(_, row)| scope.changes(row.owner.as_deref()))\n",
     "                .filter(|_| true)\n"),
    ("an insert records no owner", MEMORY,
     "                None => scope.owner().map(str::to_string),\n",
     "                None => None,\n"),
    ("a row put again takes its writer as its owner", MEMORY,
     "                Some(existing) => existing.owner.clone(),\n",
     "                Some(_) => scope.owner().map(str::to_string),\n"),
    ("an undone row loses its owner", MEMORY,
     "                .map(|((table, key), prior)| (table, key, prior))\n",
     "                .map(|((table, key), prior)| {\n"
     "                    let owner = None;\n"
     "                    let row = prior.map(|row| Arc::new(Held { fields: row.fields.clone(), owner }));\n"
     "                    (table, key, row)\n"
     "                })\n"),
    ("the adapter reads no owner from the manifest", MEMORY,
     "            owner: host.owner_of(name),\n",
     "            owner: host.owner_of(name).and(None),\n"),
    # 6. the host's writer
    ("a signed-out reader writes a user's table as anyone", HOST,
     "                Some(user) => Ok(Scope::Owned(user)),\n",
     "                Some(user) => Ok(Scope::Owned(user)),\n                None if true => Ok(Scope::Any),\n"),
    ("a reader changes any row of an owned table", HOST,
     "            Scope::Owned(id) => owner == Some(*id),\n",
     "            Scope::Owned(_) => owner.is_some() || owner.is_none(),\n"),
    ("a session's table records its user", HOST,
     "            (Writer::Reader(reader), Some(OwnerKind::Session)) => Ok(Scope::Owned(&reader.session)),\n",
     "            (Writer::Reader(reader), Some(OwnerKind::Session)) => {\n"
     "                Ok(Scope::Owned(reader.user.as_deref().unwrap_or(&reader.session)))\n"
     "            }\n"),
    ("a clock's run is never its reader's", HOST,
     "            (Writer::Reader(_), Some(by)) if *by != CLOCK => Err(format!(\n",
     "            (Writer::Reader(_), Some(by)) => Err(format!(\n"),
    ("any invoker's command runs for a reader", HOST,
     "            (Writer::Reader(_), Some(by)) if *by != CLOCK => Err(format!(\n",
     "            (Writer::Reader(_), Some(by)) if false && *by != CLOCK => Err(format!(\n"),
    ("a reader's command runs as the platform", HOST,
     "            (Writer::Platform, None) => Err(format!(\n",
     "            (Writer::Platform, None) if false => Err(format!(\n"),
    # 7. PostgreSQL
    ("a put on PostgreSQL changes another reader's row", SQL,
     '                format!("{conflict} WHERE {}", mine("pw_t", me, any))\n',
     '                format!("{conflict} WHERE {} OR true", mine("pw_t", me, any))\n'),
    ("a delete on PostgreSQL removes another reader's row", SQL,
     '                "DELETE FROM {t} AS t WHERE {} AND {} RETURNING {rows}",\n',
     '                "DELETE FROM {t} AS t WHERE {} AND ({} OR true) RETURNING {rows}",\n'),
    ("a row that records no owner is a reader's on PostgreSQL", SQL,
     '        "(${any}::boolean OR COALESCE({alias}.{} = ${me}::text, false))",\n',
     '        "(${any}::boolean OR COALESCE({alias}.{} = ${me}::text, true))",\n'),
    ("PostgreSQL makes no owner column", SQL,
     "    if table.owner.is_some() {\n        parts.push(format!(\"{} text\", ident(OWNER)));\n",
     "    if false {\n        parts.push(format!(\"{} text\", ident(OWNER)));\n"),
    ("PostgreSQL answers another reader's put as its own", PG,
     "                    let others = owned && old_held && !row.get::<_, bool>(3 + 2 * n);\n",
     "                    let others = false && owned && old_held && !row.get::<_, bool>(3 + 2 * n);\n"),
    ("a put that met a row inserted since is answered refused", PG,
     "                    let since = owned\n",
     "                    let since = false\n"),
    ("a table that comes to record owners gains no column", MIGRATE,
     "            (None, Some(_)) => columns_sql.push(sql::owner_column(names, table, true)),\n",
     "            (None, Some(_)) => {}\n"),
    ("an old build gives the owners up", MIGRATE,
     "            (Some(_), None) if stated_applies => {\n",
     "            (Some(_), None) if stated_applies || true => {\n"),
    ("an owner of another kind is taken on PostgreSQL", MIGRATE,
     "            (Some(before), Some(after)) if before == after => {}\n",
     "            (Some(before), Some(after)) if before == after || true => {}\n"),
]

# Each test binary apart, the compiler's first: a mutant is killed by the
# first that fails at an assertion, and the rest are not run for it. The
# parser's are its table tests (`a_table`): its library's whole inventory
# holds a `should_panic` test, which the strict producer does not count.
TESTS = [
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "who_may_change"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "handles"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-core", "--test", "generality"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-syntax", "--lib", "a_table"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-memory", "--test", "owned_rows"],
    ["cargo", "test", "--quiet", "--locked", "-p", "pw-table-postgres", "--test", "owned_rows"],
]

# Where a failing test's assertion may stand.
SOURCES = [
    ROOT / "compiler/pw-core/tests/who_may_change.rs",
    ROOT / "compiler/pw-core/tests/handles.rs",
    ROOT / "compiler/pw-core/tests/generality.rs",
    GRAMMAR,
    *sorted((ROOT / "compiler/pw-syntax/src").glob("*.rs")),
    ROOT / "runtime/pw-table-memory/tests/owned_rows.rs",
    *sorted((ROOT / "runtime/pw-table-memory/tests/common").glob("*.rs")),
    *sorted((ROOT / "runtime/pw-table-memory/tests/support").glob("*.rs")),
    ROOT / "runtime/pw-table-postgres/tests/owned_rows.rs",
    *sorted((ROOT / "runtime/pw-table-postgres/tests/common").glob("*.rs")),
    *sorted((ROOT / "runtime/pw-table-postgres/tests/support").glob("*.rs")),
]
INVENTORY = {}


def database():
    return next(
        (os.environ[name] for name in
         ("PW_TABLES_DATABASE_URL", "PW_STORE_DATABASE_URL", "PW_FEED_DATABASE_URL")
         if os.environ.get(name)),
        None,
    )


def run_tests(stop_at_kill):
    """(valid, passed, failed): every binary's whole inventory, each failure
    an assertion's; where `stop_at_kill`, none after the first that fails."""
    passed, failed = 0, 0
    for cmd in TESTS:
        full = [*cmd, "--", "--format=pretty"]
        try:
            child = producer_process.run(full, cwd=ROOT, timeout=1800)
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
        if stop_at_kill and result.failed:
            break
    return True, passed, failed


def main():
    import mutation_baseline  # keeps every command and reports a red baseline
    if not database():
        print("FAIL: no database is named (PW_TABLES_DATABASE_URL, PW_STORE_DATABASE_URL or")
        print("PW_FEED_DATABASE_URL); without one the PostgreSQL tests pass doing nothing,")
        print("and no mutant of its adapter can mean anything")
        return 2
    built, passed, failed = run_tests(stop_at_kill=False)
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
            built, passed, failed = run_tests(stop_at_kill=True)
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
