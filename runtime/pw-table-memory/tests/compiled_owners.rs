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
    // compiler's PW0370 is what holds it to a principal's id.
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
    // A bound with no `all` (the review's finding 2, probe B): it counts
    // every owner's rows, so one owner's would refuse another's write.
    refused(
        &|t| t["bounded"] = json!(10),
        "is owned, and its bound of 10 rows counts every owner's",
    );
}

/// An owned table keyed by its owner alone, and one owned by `from` whose
/// `from` refers to it: each crush names its owner's own premium.
const PREMIUMS: &str = "module Premiums\n\nimport capability.{ UserId }\n\ntype Premium = Premium { user: UserId, tier: Int }\n\ntable Premiums\n    row     Premium\n    key     user\n    owned   by user\n";

const CRUSHES: &str = "module Crushes\n\nimport capability.{ UserId }\nimport Premiums\n\ntype Crush = Crush { from: UserId, target: UserId, added: Int }\n\ntable Crushes\n    row     Crush\n    key     from, target\n    owned   by from\n    refers  from to Premiums\n";

const CRUSHES_APP: &str = "module owned.app\n\nimport capability.{ User, UserId, id_of }\nimport context.{ current_user }\nimport Crushes.{ Crush }\nimport Crushes\nimport Premiums.{ Premium }\nimport Premiums\n\ncommand crush(target: UserId) -> Result<(), Crush>\n    requires    Anyone\n    transaction serializable\n{\n    let me = current_user()\n    Crushes.insert(me, Crush { from: id_of(me), target: target, added: 0 })\n}\n\ncommand subscribe() -> Result<(), Premium>\n    requires    Anyone\n    transaction serializable\n{\n    let me = current_user()\n    Premiums.insert(me, Premium { user: id_of(me), tier: 1 })\n}\n\ncommand cancel() -> ()\n    requires    Anyone\n    transaction serializable\n{\n    Premiums.delete(current_user())\n}\n";

#[test]
fn a_reference_stays_within_one_owners_rows() {
    // The review's finding 1 (probe A): `refers target to Premiums` let
    // eve learn whether bob held a premium row, by whether her write
    // committed, and kept bob from cancelling his while hers named him.
    let program = built_from(&[
        ("Premiums.pw", PREMIUMS),
        ("Crushes.pw", CRUSHES),
        ("app.pw", CRUSHES_APP),
    ]);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let eve = "eve";
    // Eve's crush names her own premium, which she has not taken.
    let refused =
        command(&program, &tables, "crush", eve, &[handle(BOB)]).assert_err("eve holds no premium");
    assert!(
        refused.contains("references a missing Premiums"),
        "{refused}"
    );
    assert_eq!(
        command(&program, &tables, "subscribe", eve, &[]),
        Ok(vec![Val::Result(Ok(None))])
    );
    // Now hers commits, whatever bob holds.
    assert_eq!(
        command(&program, &tables, "crush", eve, &[handle(BOB)]),
        Ok(vec![Val::Result(Ok(None))])
    );
    // Bob's premium is his alone: taken and cancelled, whoever's crush
    // names him.
    assert_eq!(
        command(&program, &tables, "subscribe", BOB, &[]),
        Ok(vec![Val::Result(Ok(None))])
    );
    assert_eq!(command(&program, &tables, "cancel", BOB, &[]), Ok(vec![]));
    // Eve's own crush names her own premium: hers to keep.
    let refused =
        command(&program, &tables, "cancel", eve, &[]).assert_err("her own crush names it");
    assert!(
        refused.contains("references a missing Premiums"),
        "{refused}"
    );
    // The host refuses every reference across owners' rows, whatever
    // compiled it (the compiler's PW6924): by another field, from a table
    // no one owns, into one no one owns; and one it cannot hold to its
    // tables.
    let refused = |change: &dyn Fn(&mut serde_json::Value), says: &str| {
        let mut json = serde_json::to_value(&program.tables).expect("tables.json as JSON");
        change(&mut json);
        let why = pw_host::tables::Manifest::from_json(&json.to_string())
            .assert_err("the host refuses the manifest");
        assert!(why.contains(says), "{why}");
    };
    let table = |json: &mut serde_json::Value, name: &str| -> serde_json::Value {
        json["tables"]
            .as_array()
            .and_then(|tables| tables.iter().position(|t| t["name"] == name))
            .map(|at| json["tables"][at].take())
            .unwrap_or_default()
    };
    let put = |json: &mut serde_json::Value, t: serde_json::Value| {
        if let Some(tables) = json["tables"].as_array_mut()
            && let Some(at) = tables.iter().position(|old| old.is_null())
        {
            tables[at] = t;
        }
    };
    // No one owns it: no owner, and no function given a handle.
    let unowned = |t: &mut serde_json::Value| {
        if let Some(t) = t.as_object_mut() {
            t.remove("owner");
        }
        if let Some(functions) = t["functions"].as_array_mut() {
            for f in functions {
                if let Some(f) = f.as_object_mut() {
                    f.remove("reader");
                }
            }
        }
    };
    let across = "across owners' rows";
    refused(
        &|json| {
            let mut t = table(json, "Crushes");
            t["references"][0]["field"] = json!("target");
            put(json, t);
        },
        &format!("Crushes refers `target` to Premiums, {across}"),
    );
    refused(
        &|json| {
            let mut t = table(json, "Crushes");
            unowned(&mut t);
            put(json, t);
        },
        &format!("Crushes refers `from` to Premiums, {across}"),
    );
    refused(
        &|json| {
            let mut t = table(json, "Premiums");
            unowned(&mut t);
            put(json, t);
        },
        &format!("Crushes refers `from` to Premiums, {across}"),
    );
    refused(
        &|json| {
            let mut t = table(json, "Crushes");
            t["references"][0]["table"] = json!("Profiles");
            put(json, t);
        },
        "Crushes refers `from` to Profiles, which it does not declare",
    );
    refused(
        &|json| {
            let mut t = table(json, "Crushes");
            t["references"][0]["field"] = json!("since");
            put(json, t);
        },
        "Crushes refers by `since`, which is not its field",
    );
}

/// Likes, whose row names the one liked first: its owner, `from`, is no
/// row's first field (the review's finding 6).
const LIKES: &str = "module Likes\n\nimport capability.{ UserId }\n\ntype Like = Like { to: UserId, from: UserId, added: Int }\n\ntable Likes\n    row     Like\n    key     from, to\n    index   by_to on to\n    owned   by from\n    shares  with to\n";

const LIKES_APP: &str = "module owned.app\n\nimport capability.{ User, UserId, id_of }\nimport context.{ current_user }\nimport Likes.{ Like }\nimport Likes\n\ncommand like(to: UserId) -> Result<(), Like>\n    requires    Anyone\n    transaction serializable\n{\n    let me = current_user()\n    Likes.insert(me, Like { to: to, from: id_of(me), added: 0 })\n}\n\ncommand keep(like: Like) -> ()\n    requires    Anyone\n    transaction serializable\n{\n    Likes.put(current_user(), like)\n}\n";

/// A like as the component's WIT declares it.
fn like(to: &str, from: &str) -> Val {
    Val::Record(vec![
        ("to".into(), Val::String(to.into())),
        ("from".into(), Val::String(from.into())),
        ("added".into(), Val::S64(0)),
    ])
}

#[test]
fn an_owner_who_is_not_the_rows_first_field_writes_their_rows_alone() {
    // Every fixture put its owner first, so an owner check that read a
    // row's first field passed every test (the review's finding 6).
    let program = built_from(&[("Likes.pw", LIKES), ("app.pw", LIKES_APP)]);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    // Alice likes bob: her own row, its first field bob's.
    assert_eq!(
        command(&program, &tables, "like", ALICE, &[handle(BOB)]),
        Ok(vec![Val::Result(Ok(None))])
    );
    // A like from bob, its first field alice's, kept by alice: refused
    // before it is written.
    let refused = command(&program, &tables, "keep", ALICE, &[like(ALICE, BOB)])
        .assert_err("a like from bob is bob's");
    assert!(
        refused.contains("whose `from` is not its writer"),
        "{refused}"
    );
    // Her own, its first field carol's: kept.
    assert_eq!(
        command(&program, &tables, "keep", ALICE, &[like(CAROL, ALICE)]),
        Ok(vec![])
    );
    let liked: Vec<(String, String)> = [ALICE, BOB]
        .iter()
        .flat_map(|from| {
            tables
                .read("pw:tables/likes#of-from", &[json!(from)])
                .assert_ok("the adapter reads a liker's likes")
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|row| {
            (
                row["from"].as_str().unwrap_or_default().to_string(),
                row["to"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        liked,
        [
            (ALICE.to_string(), BOB.to_string()),
            (ALICE.to_string(), CAROL.to_string())
        ]
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

// --- L2 (ADR-0395): a reader's command, never the platform's -------------------

/// **A reader's command is never the platform's** (PR #91's test of the
/// same name, rewritten for design A, ADR-0407): `remove`, a command a
/// reader runs, made by a host as no one, is refused its owned table before
/// it writes, so a host that ran a request's command for no reader changes
/// no row, whoever its platform's principal says the reader is. Made as
/// bob, it removes bob's note of the id alone, and alice's of the same id
/// stays; made as alice, hers goes.
#[test]
fn a_readers_command_is_never_the_platforms() {
    let program = program();
    let tables = seeded(&program);
    let before = every_note(&tables);
    let n1 = || [Val::String("n1".into()), handle(ALICE)];
    let refused = command_as(&program, &tables, "remove", &Caller::Nobody, ALICE, &n1())
        .assert_err("a reader's command made as no one");
    assert!(refused.contains("no one makes this call"), "{refused}");
    assert_eq!(
        every_note(&tables),
        before,
        "a command made as no one wrote"
    );
    assert_eq!(
        command(
            &program,
            &tables,
            "remove",
            BOB,
            &[Val::String("n1".into()), handle(BOB)]
        ),
        Ok(vec![])
    );
    assert_eq!(
        every_note(&tables),
        [
            (ALICE.to_string(), "n1".to_string()),
            (ALICE.to_string(), "n2".to_string())
        ],
        "bob's request removed alice's note"
    );
    assert_eq!(
        command(&program, &tables, "remove", ALICE, &n1()),
        Ok(vec![])
    );
    assert_eq!(every_note(&tables), [(ALICE.to_string(), "n2".to_string())]);
}

// --- L2 (ADR-0395's ruling 4): a public table, read by anyone -------------------

/// Posts, public and owned by their author: read by anyone by any index,
/// written by their author alone.
const POSTS: &str = "module Posts\n\nimport capability.{ UserId }\n\nopaque type PostId = String\n\ntype Post = Post { author: UserId, id: PostId, day: Int, slug: String }\n\npublic table Posts\n    row     Post\n    key     author, id\n    order   day\n    index   by_day on day\n    unique  by_slug on slug\n    owned   by author\n    bounded 100\n";

const POST_DATA: &str = "module PostData\n\nimport Posts\n\nsource PostData\n    holds        Posts\n    transactions serializable\n    reads        strong\n    changes      none\n    sets         one_call\n";

const POSTS_APP: &str = "module owned.app\n\nimport capability.{ User, UserId, id_of }\nimport context.{ current_user }\nimport Posts.{ Post, PostId }\nimport Posts\n\npublic query OnDay(day: Int) -> List<Post>\n    consistency strong\n{\n    Posts.by_day(day)\n}\n\ncommand post(id: PostId, day: Int, slug: String) -> Result<(), Post>\n    requires    Anyone\n    transaction serializable\n    invalidates OnDay(day)\n{\n    let me = current_user()\n    Posts.insert(me, Post { author: id_of(me), id: id, day: day, slug: slug })\n}\n\ncommand keep(post: Post) -> Result<(), Post>\n    requires    Anyone\n    transaction serializable\n    invalidates OnDay(post.day)\n{\n    Posts.put(current_user(), post)\n}\n\ncommand take_down(id: PostId) -> ()\n    requires    Anyone\n    transaction serializable\n    invalidates OnDay(_)\n{\n    Posts.delete(current_user(), id)\n}\n";

fn posts_program() -> Program {
    built_from(&[
        ("Posts.pw", POSTS),
        ("PostData.pw", POST_DATA),
        ("app.pw", POSTS_APP),
    ])
}

/// A post as the adapter stores it.
fn post_json(author: &str, id: &str, day: i64, slug: &str) -> serde_json::Value {
    json!({ "author": author, "id": id, "day": day, "slug": slug })
}

/// A post as the component's WIT declares it.
fn post(author: &str, id: &str, day: i64, slug: &str) -> Val {
    Val::Record(vec![
        ("author".into(), Val::String(author.into())),
        ("id".into(), Val::String(id.into())),
        ("day".into(), Val::S64(day)),
        ("slug".into(), Val::String(slug.into())),
    ])
}

/// Alice's `p1` on day 1 and bob's `p1`, the same id, on day 2.
fn seeded_posts(program: &Program) -> MemoryTables {
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    tables
        .transaction(|tx| {
            tx.write("pw:tables/posts#insert", &[post_json(ALICE, "p1", 1, "a")])?;
            tx.write("pw:tables/posts#insert", &[post_json(BOB, "p1", 2, "b")])?;
            Ok(())
        })
        .assert_ok("seeded through the adapter's own transaction");
    tables
}

/// Every post, by author and id, in the table's order.
fn every_post(tables: &MemoryTables) -> Vec<(String, String)> {
    let rows = tables
        .read("pw:tables/posts#all", &[])
        .assert_ok("the adapter reads every post");
    rows.as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|row| {
            (
                row["author"].as_str().unwrap_or_default().to_string(),
                row["id"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn a_public_tables_rows_are_read_by_anyone_and_written_by_their_owner() {
    let program = posts_program();
    let tables = seeded_posts(&program);
    // Read by anyone, even a call no one makes: its rows are every reader's.
    for caller in [as_user(CAROL), Caller::Nobody] {
        assert_eq!(
            query(&program, &tables, "OnDay", &caller, &[Val::S64(1)]),
            Ok(vec![Val::List(vec![post(ALICE, "p1", 1, "a")])]),
            "{caller:?}"
        );
    }
    // A post whose author is another, as a forged request gives it:
    // refused before it is written, and nothing commits.
    let before = every_post(&tables);
    let refused = command(&program, &tables, "keep", BOB, &[post(ALICE, "p1", 9, "z")])
        .assert_err("a post of another's is refused");
    assert!(
        refused.contains("whose `author` is not its writer"),
        "{refused}"
    );
    assert_eq!(every_post(&tables), before, "a refused command wrote");
    // A delete names its writer's own post of the id: bob's goes, and
    // alice's of the same id stays.
    assert_eq!(
        command(
            &program,
            &tables,
            "take_down",
            BOB,
            &[Val::String("p1".into())]
        ),
        Ok(vec![])
    );
    assert_eq!(every_post(&tables), [(ALICE.to_string(), "p1".to_string())]);
    // A write is its writer's: carol posts her own.
    assert_eq!(
        command(
            &program,
            &tables,
            "post",
            CAROL,
            &[
                Val::String("p1".into()),
                Val::S64(3),
                Val::String("c".into())
            ]
        ),
        Ok(vec![Val::Result(Ok(None))])
    );
    assert_eq!(
        every_post(&tables),
        [
            (ALICE.to_string(), "p1".to_string()),
            (CAROL.to_string(), "p1".to_string())
        ]
    );
    // And no one's write is made by a call no one makes.
    let refused = command_as(
        &program,
        &tables,
        "take_down",
        &Caller::Nobody,
        ALICE,
        &[Val::String("p1".into())],
    )
    .assert_err("a write made as no one");
    assert!(refused.contains("no one makes this call"), "{refused}");
    assert_eq!(every_post(&tables).len(), 2);
}

#[test]
fn a_public_tables_manifest_is_held_whole() {
    let program = posts_program();
    let posts = program
        .tables
        .tables
        .iter()
        .position(|t| t.schema.name == "Posts")
        .assert_ok("Posts is in tables.json");
    let refused = |change: &dyn Fn(&mut serde_json::Value), says: &str| {
        let mut json = serde_json::to_value(&program.tables).expect("tables.json as JSON");
        change(&mut json["tables"][posts]);
        let why = pw_host::tables::Manifest::from_json(&json.to_string())
            .assert_err("the host refuses the manifest");
        assert!(why.contains(says), "{why}");
    };
    let function = |t: &mut serde_json::Value, name: &str| -> Option<usize> {
        t["functions"]
            .as_array()
            .and_then(|fs| fs.iter().position(|f| f["name"] == name))
    };
    // A read given a handle, and a write given none.
    refused(
        &|t| {
            if let Some(at) = function(t, "by_day") {
                t["functions"][at]["reader"] = json!("author");
            }
        },
        "is public, and `by_day` is not given its owner's handle where it writes, or is given one where it reads",
    );
    refused(
        &|t| {
            if let Some(at) = function(t, "insert") {
                t["functions"][at]
                    .as_object_mut()
                    .map(|f| f.remove("reader"));
            }
        },
        "`insert` is not given its owner's handle where it writes",
    );
    refused(
        &|t| {
            if let Some(at) = function(t, "delete") {
                t["functions"][at]["reader"] = json!("id");
            }
        },
        "`delete` is not given its owner's handle where it writes",
    );
    // Still led by its owner.
    refused(
        &|t| t["owner"] = json!("slug"),
        "its key leads with another",
    );
    // The fixture's notes, made public, would share them and keep a field.
    let notes = program_tables_of_notes();
    let mut json = serde_json::to_value(&notes).expect("tables.json as JSON");
    json["tables"][0]["public"] = json!(true);
    let why = pw_host::tables::Manifest::from_json(&json.to_string())
        .assert_err("the host refuses a public table that shares");
    assert!(
        why.contains("is public, and shares its rows or keeps fields private"),
        "{why}"
    );
    // Not public, its bound and its index another field leads are refused
    // as an owned table's.
    refused(
        &|t| {
            t.as_object_mut().map(|t| t.remove("public"));
        },
        "is owned, and its index `by_day` is read by no reader of its rows alone",
    );
}

/// The fixture's tables.json, whose notes share and keep a draft.
fn program_tables_of_notes() -> pw_core::tables::TablesJson {
    program().tables
}

/// A table no one owns, which a public table's posts name.
const TOPICS: &str = "module Topics\n\nopaque type TopicId = String\n\ntype Topic = Topic { id: TopicId, title: String }\n\ntable Topics\n    row     Topic\n    key     id\n";

#[test]
fn a_public_table_refers_where_the_host_reads_it_as_a_table_no_one_owns() {
    // A public table's rows are every reader's, so its reference to a
    // table no one owns tells no one anything, as the guestbook's entries
    // name their book (ADR-0395's ruling 4; the host's half of PW6924).
    let posts = POSTS
        .replacen("import capability.{ UserId }\n", "import capability.{ UserId }\nimport Topics\n", 1)
        .replacen(
            "type Post = Post { author: UserId, id: PostId, day: Int, slug: String }",
            "type Post = Post { author: UserId, id: PostId, day: Int, slug: String, topic: Topics.TopicId }",
            1,
        )
        .replacen("    owned   by author\n", "    owned   by author\n    refers  topic to Topics\n", 1);
    let data = POST_DATA
        .replacen("import Posts\n", "import Posts\nimport Topics\n", 1)
        .replacen("holds        Posts\n", "holds        Posts, Topics\n", 1);
    let units = units_of(
        [
            ("Posts.pw", posts.as_str()),
            ("PostData.pw", data.as_str()),
            ("Topics.pw", TOPICS),
        ]
        .iter()
        .map(|(n, s)| (n.to_string(), s.to_string()))
        .collect(),
    );
    let build = pw_core::build::build(&units).assert_ok("the program builds");
    let manifest = pw_host::tables::Manifest::from_json(
        &serde_json::to_string(&build.tables).expect("serialize tables.json"),
    );
    assert!(manifest.is_ok(), "{manifest:?}");
}
