
// Compiled count proofs shared by the memory and PostgreSQL tests.
fn count_program() -> Program {
    let notes = std::fs::read_to_string(root().join("examples/owned/Notes.pw")).unwrap()
        .replace("index   by_to on to", "index   by_to on to\n    index   by_to_added on to, added");
    let mut app = std::fs::read_to_string(root().join("examples/owned/app.pw")).unwrap();
    for (name, params, returns, keys, invalidates, body) in [
        ("CountMine", "reader: User<UserId>", "Int", "reader", "NotesChanged(reader)", "Notes.count_by_author(reader)"),
        ("CountToMe", "reader: User<UserId>", "Int", "reader", "NoteSent(reader), NotesForgotten(_)", "Notes.count_by_to(reader)"),
        ("CountPicked", "reader: User<UserId>, ids: List<NoteId>", "List<Notes.NotesAuthorIdCount>", "reader, ids", "NotesChanged(reader)", "Notes.count_by_author_id_many(reader, ids)"),
        ("CountTimes", "reader: User<UserId>, times: List<Int>", "List<Notes.NotesToAddedCount>", "reader, times", "NoteSent(reader), NotesForgotten(_)", "Notes.count_by_to_added_many(reader, times)"),
    ] {
        app.push_str(&format!("\nuser query {name}({params}) -> {returns}\n    freshness 0.seconds\n    consistency read_your_writes\n    cache private\n    key {keys}\n    invalidates_on {invalidates}\n    concurrency one_per_key\n    on_key_change cancel\n    timeout 2.seconds\n{{\n    {body}\n}}\n"));
    }
    built(&[("Notes.pw", &notes), ("app.pw", &app)])
}

fn count_query(
    program: &Program,
    tables: &dyn pw_host::engine::CallTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    fuel: u64,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let mut host = tables.functions(&component.contract, &granted)?;
    let user = match caller { Caller::User(user) => user.as_str(), _ => "" };
    host.extend(platform(component, user));
    pw_host::engine::call_with_tables_as(
        &component.bytes, &component.contract, &granted,
        &pw_host::Limits { fuel: Some(fuel), ..pw_host::Limits::unbounded() },
        &host, &program.manifest, caller, &component.export(), args,
    )
}

fn count_entry(key: Val, count: i64) -> Val {
    Val::Record(vec![("key".into(), key), ("count".into(), Val::S64(count))])
}

fn assert_compiled_counts(program: &Program, tables: &dyn pw_host::engine::CallTables) {
    for (user, mine, sent) in [(ALICE, 2, 1), (BOB, 1, 1), (CAROL, 0, 1)] {
        for (id, expected) in [("CountMine", mine), ("CountToMe", sent)] {
            assert_eq!(count_query(program, tables, id, &as_user(user), &[handle(user)], 1000), Ok(vec![Val::S64(expected)]));
        }
    }
    let args = [handle(ALICE), Val::List(vec![Val::String("missing".into()), Val::String("n1".into()), Val::String("n1".into())])];
    let key = |id: &str| Val::Record(vec![("author".into(), Val::String(ALICE.into())), ("id".into(), Val::String(id.into()))]);
    assert_eq!(count_query(program, tables, "CountPicked", &as_user(ALICE), &args, 3000), Ok(vec![Val::List(vec![count_entry(key("missing"), 0), count_entry(key("n1"), 1)])]));
    let args = [handle(BOB), Val::List(vec![Val::S64(9), Val::S64(1), Val::S64(1)])];
    let key = |n| Val::Record(vec![("to".into(), Val::String(BOB.into())), ("added".into(), Val::S64(n))]);
    assert_eq!(count_query(program, tables, "CountTimes", &as_user(BOB), &args, 3000), Ok(vec![Val::List(vec![count_entry(key(1), 1), count_entry(key(9), 0)])]));
    for id in ["CountMine", "CountToMe", "CountPicked", "CountTimes"] {
        for caller in [Caller::Nobody, as_user(BOB)] {
            let watched = Watched::new(tables);
            let mut args = vec![handle(ALICE)];
            if matches!(id, "CountPicked" | "CountTimes") {
                args.push(Val::List(vec![]));
            }
            let denied = count_query(program, &watched, id, &caller, &args, 3000);
            assert!(denied.is_err(), "{id} accepted {caller:?}: {denied:?}");
            assert_eq!(watched.body.load(Ordering::SeqCst), 0, "forged reader reaches no adapter host function");
        }
    }
}
