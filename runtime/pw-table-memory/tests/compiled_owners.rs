//! **An owned table, compiled and run on the memory adapter, each function
//! its caller's** (track `row-ownership`, ADR-0384 H1).
//!
//! `examples/owned/`'s queries and commands, compiled by the compiler and
//! run through the table gate (`call_with_tables_as`) on the memory adapter,
//! each call made as one user (`Caller`), as the host makes it from the
//! session's principal: a reader reads their own notes and the notes sent to
//! them, the latter without the draft their author keeps; a handle that is
//! not the call's (a forged argument) is refused before any row is read; a
//! call no one makes reads no one's rows; a command writes and deletes its
//! caller's notes alone, and a note whose author is another is refused
//! before it is written. The manifest is held whole for its owners, and an
//! import typed otherwise than its owned function is refused where it links.

include!("support/owned.rs");

// --- reads ----------------------------------------------------------------------

#[test]
fn a_reader_reads_their_own_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    assert_eq!(
        query(&program, &tables, "Mine", &as_user(ALICE), &[handle(ALICE)]),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", CAROL, 2)
        ])])
    );
    assert_eq!(
        query(&program, &tables, "Mine", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![note(BOB, "n1", ALICE, 3)])])
    );
    // One id, two authors: each reads their own.
    let n1 = Val::String("n1".into());
    assert_eq!(
        query(
            &program,
            &tables,
            "One",
            &as_user(ALICE),
            &[handle(ALICE), n1.clone()]
        ),
        Ok(vec![Val::Option(Some(Box::new(note(ALICE, "n1", BOB, 1))))])
    );
    assert_eq!(
        query(&program, &tables, "One", &as_user(BOB), &[handle(BOB), n1]),
        Ok(vec![Val::Option(Some(Box::new(note(BOB, "n1", ALICE, 3))))])
    );
    // A set form: the handle beside the list, each key the reader's.
    let ids = Val::List(vec![
        Val::String("n2".into()),
        Val::String("n1".into()),
        Val::String("n9".into()),
    ]);
    assert_eq!(
        query(
            &program,
            &tables,
            "Picked",
            &as_user(ALICE),
            &[handle(ALICE), ids.clone()]
        ),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", CAROL, 2)
        ])])
    );
    assert_eq!(
        query(
            &program,
            &tables,
            "Picked",
            &as_user(BOB),
            &[handle(BOB), ids]
        ),
        Ok(vec![Val::List(vec![note(BOB, "n1", ALICE, 3)])])
    );
}

#[test]
fn a_reader_a_note_is_sent_to_reads_it_without_its_draft() {
    let program = program();
    let tables = seeded(&program);
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n1", BOB, 1)])])
    );
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(CAROL), &[handle(CAROL)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n2", CAROL, 2)])])
    );
    // The draft is nowhere in what the sharer is answered.
    let answer = query(&program, &tables, "ToMe", &as_user(ALICE), &[handle(ALICE)])
        .assert_ok("alice reads what bob sent her");
    assert_eq!(answer, vec![Val::List(vec![seen(BOB, "n1", ALICE, 3)])]);
    assert!(
        !format!("{answer:?}").contains("draft"),
        "a sharer was answered a private field: {answer:?}"
    );
}

#[test]
fn a_forged_reader_is_refused_before_any_row_is_read() {
    let program = program();
    let tables = seeded(&program);
    // A query's `reader` the host would make from the session; given
    // another's, as a forged request or a forged component would.
    for (id, args) in [
        ("Mine", vec![handle(ALICE)]),
        ("ToMe", vec![handle(ALICE)]),
        ("One", vec![handle(ALICE), Val::String("n1".into())]),
        (
            "Picked",
            vec![handle(ALICE), Val::List(vec![Val::String("n1".into())])],
        ),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let refused = query_counted(&program, &tables, id, &as_user(BOB), &args, &reads)
            .assert_err("a handle that is not the call's is refused");
        assert!(
            refused.contains("a handle that is not its caller's"),
            "{id}: {refused}"
        );
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "{id}: the adapter read a row"
        );
    }
}

#[test]
fn a_call_no_one_makes_reads_no_owned_row() {
    let program = program();
    let tables = seeded(&program);
    let reads = Arc::new(AtomicUsize::new(0));
    let refused = query_counted(
        &program,
        &tables,
        "Mine",
        &Caller::Nobody,
        &[handle(ALICE)],
        &reads,
    )
    .assert_err("no one's call reads an owned table");
    assert!(refused.contains("no one makes this call"), "{refused}");
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    // `call_with_tables` is a call made as no one.
    let component = program.component("Mine");
    let granted = component.granted();
    let host = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the query's reads");
    let refused = pw_host::engine::call_with_tables(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &component.export(),
        &[handle(ALICE)],
    )
    .assert_err("call_with_tables is made as no one");
    assert!(refused.contains("no one makes this call"), "{refused}");
}

// --- writes ---------------------------------------------------------------------

#[test]
fn a_command_writes_its_callers_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    // `write` makes a note of the caller's.
    let text = Val::String("hello".into());
    assert_eq!(
        command(
            &program,
            &tables,
            "write",
            CAROL,
            &[Val::String("n7".into()), handle(ALICE), text]
        ),
        Ok(vec![Val::Result(Ok(None))])
    );
    assert!(
        every_note(&tables).contains(&(CAROL.to_string(), "n7".to_string())),
        "{:?}",
        every_note(&tables)
    );
    // `keep`, given a note whose author is another, as a forged request
    // gives it: refused before it is written, and nothing commits.
    let before = every_note(&tables);
    let refused = command(&program, &tables, "keep", BOB, &[note(ALICE, "n5", BOB, 9)])
        .assert_err("a note of another's is refused");
    assert!(
        refused.contains("whose `author` is not its writer"),
        "{refused}"
    );
    assert_eq!(every_note(&tables), before, "a refused command wrote");
    // Its own: kept.
    assert_eq!(
        command(&program, &tables, "keep", BOB, &[note(BOB, "n5", ALICE, 9)]),
        Ok(vec![])
    );
    assert!(every_note(&tables).contains(&(BOB.to_string(), "n5".to_string())));
}

#[test]
fn a_command_deletes_its_callers_notes_alone() {
    let program = program();
    let tables = seeded(&program);
    // Bob removes his `n1`; alice's `n1`, the same id, stays.
    assert_eq!(
        command(
            &program,
            &tables,
            "remove",
            BOB,
            &[Val::String("n1".into()), handle(ALICE)]
        ),
        Ok(vec![])
    );
    assert_eq!(
        every_note(&tables),
        [
            (ALICE.to_string(), "n1".to_string()),
            (ALICE.to_string(), "n2".to_string())
        ]
    );
    // Alice forgets every note of hers, and no one else's.
    let tables = seeded(&program);
    assert_eq!(command(&program, &tables, "forget", ALICE, &[]), Ok(vec![]));
    assert_eq!(every_note(&tables), [(BOB.to_string(), "n1".to_string())]);
}

// --- the manifest, and the link ----------------------------------------------------

#[test]
fn the_manifest_is_held_whole_for_its_owners() {
    let program = program();
    let refused = |change: &dyn Fn(&mut serde_json::Value), says: &str| {
        let mut json = serde_json::to_value(&program.tables).expect("tables.json as JSON");
        change(&mut json["tables"][0]);
        let why = pw_host::tables::Manifest::from_json(&json.to_string())
            .assert_err("the host refuses the manifest");
        assert!(why.contains(says), "{why}");
    };
    refused(
        &|t| {
            if let Some(t) = t.as_object_mut() {
                t.remove("owner");
            }
        },
        "shares or keeps fields private, and no one owns it",
    );
    // A host knows a field by what it stores: `text` is a string, and the
    // compiler's PW0367 is what holds it to a principal's id.
    refused(
        &|t| t["owner"] = json!("text"),
        "its key leads with another",
    );
    refused(
        &|t| t["owner"] = json!("added"),
        "which is no field holding a principal's id",
    );
    refused(&|t| t["shares"] = json!(["author"]), "shares with `author`");
    refused(
        &|t| {
            t["owner"] = json!("to");
            t["shares"] = json!([]);
        },
        "is owned by `to`, and its key leads with another",
    );
    refused(&|t| t["private"] = json!(["to"]), "keeps `to` private");
    refused(
        &|t| t["private"] = json!(["added"]),
        "keeps `added` private",
    );
    // `by_to` read by `to`, which it no longer shares with.
    refused(
        &|t| t["shares"] = json!([]),
        "its index `by_to` is read by no reader of its rows alone",
    );
    refused(
        &|t| {
            t["functions"][0]["reader"] = json!("to");
        },
        "`get` is not given its reader's handle",
    );
    refused(
        &|t| {
            t["functions"][0]
                .as_object_mut()
                .map(|f| f.remove("reader"));
        },
        "`get` is not given its reader's handle",
    );
    refused(
        &|t| {
            let insert = t["functions"]
                .as_array_mut()
                .and_then(|fs| fs.iter_mut().find(|f| f["name"] == "insert"));
            if let Some(insert) = insert {
                insert["reader"] = json!("to");
            }
        },
        "`insert` is not given its reader's handle",
    );
    refused(
        &|t| {
            t["bounded"] = json!(10);
            if let Some(fs) = t["functions"].as_array_mut() {
                fs.push(json!({
                    "name": "all", "import": "pw:tables/notes#all", "primitive": "all",
                    "effect": "read", "by": [], "params": [], "returns": "List<Note>",
                    "sorted_by": ["added", "author", "id"], "reader": "author"
                }))
            }
        },
        "is read whole by `all`",
    );
}

#[test]
fn an_import_typed_as_an_unowned_tables_is_refused_where_it_links() {
    // The same program built from `Notes` with no owner: its `Picked`
    // imports `get-many` taking one list of keys, which the owned manifest
    // types as the handle beside a list of ids. A stale build, refused
    // before anything is instantiated.
    let src = std::fs::read_to_string(root().join("examples/owned/Notes.pw")).expect("Notes.pw");
    let unowned = src
        .replacen("    owned   by author\n", "", 1)
        .replacen("    shares  with to\n", "", 1)
        .replacen("    private draft\n", "", 1);
    let app = "module owned.app\n\nimport List\nimport Notes.{ Note, NotesAuthorId }\nimport Notes\n\n\
               query Picked(keys: List<NotesAuthorId>) -> List<Note>\n    consistency strong\n{\n    \
               Notes.get_many(keys)\n}\n";
    let stale = built(&[("Notes.pw", &unowned), ("app.pw", app)]);
    let program = program();
    let tables = seeded(&program);
    let component = stale.component("Picked");
    let granted = component.granted();
    let host = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the reads");
    let keys = Val::List(vec![Val::Record(vec![
        ("author".into(), Val::String(ALICE.into())),
        ("id".into(), Val::String("n1".into())),
    ])]);
    let refused = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(ALICE),
        &component.export(),
        &[Val::String(ALICE.into()), keys],
    )
    .assert_err("a stale build is refused where it links");
    assert!(
        refused.contains("typed otherwise than tables.json declares the owned table's function"),
        "{refused}"
    );
}

#[test]
fn a_sharers_read_answered_another_readers_row_is_refused() {
    // Whatever answers a sharer's read, a row for another reader is no
    // answer of it: held to what it was asked, then shown as the seen row.
    let program = program();
    let tables = seeded(&program);
    let component = program.component("ToMe");
    let granted = component.granted();
    let mut host: BTreeMap<String, HostFn> = BTreeMap::new();
    host.insert(
        "pw:tables/notes#by-to".into(),
        Arc::new(|_: &[Val]| Ok(vec![Val::List(vec![note(ALICE, "n2", CAROL, 2)])])) as HostFn,
    );
    let refused = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(BOB),
        &component.export(),
        &[handle(BOB)],
    )
    .assert_err("another reader's row is refused");
    assert!(refused.contains("was not asked for"), "{refused}");
    // The control: the adapter's own answer.
    assert_eq!(
        query(&program, &tables, "ToMe", &as_user(BOB), &[handle(BOB)]),
        Ok(vec![Val::List(vec![seen(ALICE, "n1", BOB, 1)])])
    );
}
