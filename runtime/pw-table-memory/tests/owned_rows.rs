//! **A row says who may change it, and the memory adapter holds it** (the
//! owned-rows ADR).
//!
//! `examples/tables`, its lists and items saying `owner session` or `owner
//! user` (`common::app_owned`): each command is begun for a writer, a
//! request's reader or the platform's own command. A reader's insert records
//! the reader as the row's owner, in no field; its `put`, `delete` and
//! `delete_of_list` change only the rows recording it, and another reader's
//! row is absent to them: nothing changes, and the answer is a missing
//! row's. A reader with no id of the table's kind is refused every write.
//! The platform changes any row and owns none it inserts. The first tests
//! use the adapter's own API; the last run the app's compiled commands
//! through the engine, as a host runs a request.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::collections::BTreeMap;

use common::*;
use pw_host::engine::{HostFn, Val};
use pw_host::tables::{OwnerKind, Reader, Scope, Writer};
use pw_table_memory::{Error, MemoryTables};
use serde_json::{Value, json};

fn writer(session: &str, user: Option<&str>) -> Writer {
    Writer::Reader(Reader::new(session, user).expect("a reader"))
}

/// Alice and Bob, each signed in, each in a session of their own.
fn alice() -> Writer {
    writer("s-alice", Some("u-alice"))
}

fn bob() -> Writer {
    writer("s-bob", Some("u-bob"))
}

const INSERT_LIST: &str = "pw:tables/lists#insert";
const PUT_LIST: &str = "pw:tables/lists#put";
const INSERT_ITEM: &str = "pw:tables/items#insert";
const PUT_ITEM: &str = "pw:tables/items#put";
const DELETE_ITEM: &str = "pw:tables/items#delete";
const CLEAR_ITEMS: &str = "pw:tables/items#delete-of-list";
const GET_ITEM: &str = "pw:tables/items#get";
const GET_LIST: &str = "pw:tables/lists#get";

/// One write, in a command of its own for `who`, committed where it is
/// answered.
fn write(tables: &MemoryTables, who: Writer, import: &str, args: &[Value]) -> Result<(), Error> {
    tables.transaction(who, |tx| tx.write(import, args))
}

fn item_now(tables: &MemoryTables, list: &str, id: &str) -> Value {
    tables
        .read(GET_ITEM, &[json!(list), json!(id)])
        .assert_ok("a read")
}

/// The tables, with Alice's groceries, its milk and eggs.
fn alices(owner: &str) -> MemoryTables {
    let program = app_owned(owner);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    write(
        &tables,
        alice(),
        INSERT_LIST,
        &[list_json("groceries", "food", 1)],
    )
    .assert_ok("Alice's list");
    for (id, added) in [("milk", 1), ("eggs", 2)] {
        write(
            &tables,
            alice(),
            INSERT_ITEM,
            &[item_json("groceries", id, false, added)],
        )
        .assert_ok("Alice's item");
    }
    tables
}

/// The milk, taken in hand by someone else: done, and renamed.
fn changed_milk() -> Value {
    let mut milk = item_json("groceries", "milk", true, 1);
    milk["title"] = json!("Bob's now");
    milk
}

#[test]
fn another_readers_rows_are_absent_to_every_write() {
    for owner in ["session", "user"] {
        let tables = alices(owner);
        let before = (
            item_now(&tables, "groceries", "milk"),
            item_now(&tables, "groceries", "eggs"),
            tables
                .read(GET_LIST, &[json!("groceries")])
                .assert_ok("a read"),
        );
        // Each answered as a write to a missing row is: done, nothing said.
        write(&tables, bob(), PUT_ITEM, &[changed_milk()]).assert_ok("a put");
        write(
            &tables,
            bob(),
            DELETE_ITEM,
            &[json!("groceries"), json!("milk")],
        )
        .assert_ok("a delete");
        write(&tables, bob(), CLEAR_ITEMS, &[json!("groceries")]).assert_ok("a clear");
        let mut renamed = list_json("groceries", "food", 1);
        renamed["name"] = json!("Bob's list");
        write(&tables, bob(), PUT_LIST, &[renamed]).assert_ok("a put");
        assert_eq!(
            (
                item_now(&tables, "groceries", "milk"),
                item_now(&tables, "groceries", "eggs"),
                tables
                    .read(GET_LIST, &[json!("groceries")])
                    .assert_ok("a read"),
            ),
            before,
            "owner {owner}: Bob changed none of Alice's rows"
        );
        // The control: Alice changes each.
        write(&tables, alice(), PUT_ITEM, &[changed_milk()]).assert_ok("her put");
        assert_eq!(item_now(&tables, "groceries", "milk"), changed_milk());
        write(
            &tables,
            alice(),
            DELETE_ITEM,
            &[json!("groceries"), json!("milk")],
        )
        .assert_ok("her delete");
        assert_eq!(item_now(&tables, "groceries", "milk"), Value::Null);
        write(&tables, alice(), CLEAR_ITEMS, &[json!("groceries")]).assert_ok("her clear");
        assert_eq!(item_now(&tables, "groceries", "eggs"), Value::Null);
    }
}

/// A prefix's delete removes its reader's rows of the prefix alone.
#[test]
fn a_prefixs_delete_takes_its_readers_rows_and_leaves_the_others() {
    let tables = alices("session");
    write(
        &tables,
        bob(),
        INSERT_ITEM,
        &[item_json("groceries", "bread", false, 3)],
    )
    .assert_ok("Bob's item in the same list");
    write(&tables, bob(), CLEAR_ITEMS, &[json!("groceries")]).assert_ok("Bob's clear");
    assert_eq!(item_now(&tables, "groceries", "bread"), Value::Null);
    assert_ne!(item_now(&tables, "groceries", "milk"), Value::Null);
    assert_ne!(item_now(&tables, "groceries", "eggs"), Value::Null);
}

/// A command that changed nothing of anyone's publishes nothing: its
/// tables are not written, and no version moves.
#[test]
fn a_command_that_touched_only_anothers_rows_wrote_nothing() {
    let tables = alices("session");
    let mut command = tables.begin(bob()).assert_ok("its turn");
    command
        .write(PUT_ITEM, &[changed_milk()])
        .assert_ok("a put");
    command
        .write(DELETE_ITEM, &[json!("groceries"), json!("eggs")])
        .assert_ok("a delete");
    assert!(command.written().assert_ok("written").is_empty());
    let committed = command.commit().assert_ok("its commit");
    assert!(committed.written.is_empty(), "{committed:?}");
}

/// An insert is refused by the row holding its key, whoever's: the key is
/// taken. Its answer is the row, as any read of it answers (reads are
/// unchanged), and holds only the row's fields.
#[test]
fn an_insert_is_refused_by_the_key_whoever_holds_it() {
    let tables = alices("session");
    let refused = write(
        &tables,
        bob(),
        INSERT_ITEM,
        &[item_json("groceries", "milk", true, 9)],
    );
    assert_eq!(
        refused,
        Err(Error::Conflict {
            table: "Items".into(),
            existing: item_json("groceries", "milk", false, 1),
        })
    );
}

/// The owner is in no field: a read answers the row's fields alone.
#[test]
fn the_owner_is_in_no_field() {
    let tables = alices("user");
    assert_eq!(
        item_now(&tables, "groceries", "milk"),
        item_json("groceries", "milk", false, 1)
    );
    let every = tables
        .read("pw:tables/items#of-list", &[json!("groceries")])
        .assert_ok("a prefix's read");
    assert_eq!(
        every,
        json!([
            item_json("groceries", "milk", false, 1),
            item_json("groceries", "eggs", false, 2)
        ])
    );
}

/// `owner user`: the user's rows, from any of their sessions; `owner
/// session`: the session's alone, whoever is signed in.
#[test]
fn a_user_owns_across_sessions_and_a_session_alone() {
    let alice_elsewhere = writer("s-alice-phone", Some("u-alice"));
    let by_user = alices("user");
    write(
        &by_user,
        alice_elsewhere.clone(),
        DELETE_ITEM,
        &[json!("groceries"), json!("milk")],
    )
    .assert_ok("her other session's delete");
    assert_eq!(item_now(&by_user, "groceries", "milk"), Value::Null);

    let by_session = alices("session");
    write(
        &by_session,
        alice_elsewhere,
        DELETE_ITEM,
        &[json!("groceries"), json!("milk")],
    )
    .assert_ok("another session's delete");
    assert_ne!(
        item_now(&by_session, "groceries", "milk"),
        Value::Null,
        "a session's rows are its own"
    );
}

/// A reader signed in as no one owns no row of an `owner user` table: every
/// write to it is refused, whatever the row, and writes nothing. In an
/// `owner session` table the same reader writes as its session.
#[test]
fn a_signed_out_reader_writes_no_row_of_a_users_table() {
    let tables = alices("user");
    let guest = writer("s-guest", None);
    for (import, args) in [
        (INSERT_ITEM, vec![item_json("groceries", "jam", false, 4)]),
        (PUT_ITEM, vec![item_json("groceries", "jam", false, 4)]),
        (PUT_ITEM, vec![changed_milk()]),
        (DELETE_ITEM, vec![json!("groceries"), json!("milk")]),
        (DELETE_ITEM, vec![json!("groceries"), json!("none")]),
        (CLEAR_ITEMS, vec![json!("groceries")]),
    ] {
        let refused = write(&tables, guest.clone(), import, &args);
        assert!(
            matches!(&refused, Err(Error::Unowned(why)) if why.starts_with("Items: ")),
            "{import} {args:?}: {refused:?}"
        );
    }
    assert_eq!(
        item_now(&tables, "groceries", "milk"),
        item_json("groceries", "milk", false, 1)
    );
    assert_eq!(item_now(&tables, "groceries", "jam"), Value::Null);
    // The control: a session's table takes the guest's rows.
    let by_session = alices("session");
    write(
        &by_session,
        guest.clone(),
        INSERT_ITEM,
        &[item_json("groceries", "jam", false, 4)],
    )
    .assert_ok("the guest's insert");
    write(
        &by_session,
        guest,
        DELETE_ITEM,
        &[json!("groceries"), json!("jam")],
    )
    .assert_ok("the guest's delete");
    assert_eq!(item_now(&by_session, "groceries", "jam"), Value::Null);
}

/// The platform's command changes any row; a row it inserts is owned by no
/// reader, so no request changes it. A put keeps the owner a row records.
#[test]
fn the_platform_changes_any_row_and_owns_none() {
    let tables = alices("session");
    write(&tables, Writer::Platform, PUT_ITEM, &[changed_milk()]).assert_ok("its put");
    assert_eq!(item_now(&tables, "groceries", "milk"), changed_milk());
    // Put again by the platform, the milk is still Alice's.
    write(
        &tables,
        bob(),
        DELETE_ITEM,
        &[json!("groceries"), json!("milk")],
    )
    .assert_ok("Bob's delete");
    assert_ne!(item_now(&tables, "groceries", "milk"), Value::Null);
    write(
        &tables,
        alice(),
        DELETE_ITEM,
        &[json!("groceries"), json!("milk")],
    )
    .assert_ok("Alice's delete");
    assert_eq!(item_now(&tables, "groceries", "milk"), Value::Null);
    // The platform's own row is no reader's.
    write(
        &tables,
        Writer::Platform,
        INSERT_ITEM,
        &[item_json("groceries", "salt", false, 5)],
    )
    .assert_ok("its insert");
    for who in [alice(), bob()] {
        write(
            &tables,
            who,
            DELETE_ITEM,
            &[json!("groceries"), json!("salt")],
        )
        .assert_ok("a reader's delete");
    }
    assert_ne!(item_now(&tables, "groceries", "salt"), Value::Null);
    write(
        &tables,
        Writer::Platform,
        CLEAR_ITEMS,
        &[json!("groceries")],
    )
    .assert_ok("its clear");
    assert_eq!(item_now(&tables, "groceries", "salt"), Value::Null);
    assert_eq!(item_now(&tables, "groceries", "eggs"), Value::Null);
}

/// A command undone restores each row with its owner; one committed
/// publishes each with it. A row put back as it was for another owner is no
/// write a read sees (PostgreSQL's `written` alike), and is published so.
#[test]
fn undone_and_published_rows_keep_their_owners() {
    let tables = alices("session");
    let platform_puts_back = |commit: bool| {
        let mut command = tables.begin(Writer::Platform).assert_ok("its turn");
        command
            .write(DELETE_ITEM, &[json!("groceries"), json!("milk")])
            .assert_ok("its delete");
        command
            .write(INSERT_ITEM, &[item_json("groceries", "milk", false, 1)])
            .assert_ok("the same row, inserted again");
        let written = command.written().assert_ok("written");
        if commit {
            command.commit().assert_ok("its commit");
        }
        written
    };
    let delete_milk = |who: Writer| {
        write(
            &tables,
            who,
            DELETE_ITEM,
            &[json!("groceries"), json!("milk")],
        )
        .assert_ok("a delete");
        item_now(&tables, "groceries", "milk")
    };
    // Undone: the row is Alice's again, Bob's delete absent, hers not.
    platform_puts_back(false);
    assert_ne!(delete_milk(bob()), Value::Null, "Bob's delete changed it");
    assert_eq!(
        delete_milk(alice()),
        Value::Null,
        "the row restored is hers"
    );
    // Committed: the same fields, now the platform's, no reader's.
    write(
        &tables,
        alice(),
        INSERT_ITEM,
        &[item_json("groceries", "milk", false, 1)],
    )
    .assert_ok("hers again");
    assert_eq!(
        platform_puts_back(true),
        std::collections::BTreeSet::new(),
        "no read sees a row's owner"
    );
    assert_ne!(delete_milk(alice()), Value::Null, "her delete changed it");
    assert_ne!(delete_milk(bob()), Value::Null, "his delete changed it");
    assert_eq!(
        delete_milk(Writer::Platform),
        Value::Null,
        "the platform's does"
    );
}

// --- the app's compiled commands, as a host runs a request -------------------

const NODE: &[&str] = &[
    "database.read<Lists>",
    "database.read<Items>",
    "database.write<Lists>",
    "database.write<Items>",
    "outbox.write",
];

/// A compiled command, run for `who` as a host runs a request: begun for
/// them, its table functions the gate admits, committed where it answers.
fn run(
    program: &Program,
    tables: &MemoryTables,
    who: Writer,
    id: &str,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    let reader = program.reader(id);
    let granted = reader.admitted(&reader.contract, &grants(NODE));
    let staged = tables.begin(who).assert_ok("its turn");
    let mut host: BTreeMap<String, HostFn> = staged
        .host_fns(&reader.contract, &granted)
        .map_err(|error| format!("{error:?}"))?;
    host.extend(platform(reader).0);
    let answer = reader.call(&granted, &host, tables.host_tables(), args)?;
    staged.commit().assert_ok("its commit");
    Ok(answer)
}

fn text(value: &str) -> Val {
    Val::String(value.into())
}

/// The class closed: `drop_item(list, item)` deletes the item a request
/// names only where it is the request's reader's. Bob's requests name
/// Alice's item and change nothing; hers change it.
#[test]
fn a_compiled_command_changes_only_its_readers_rows() {
    let program = app_owned("session");
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    run(
        &program,
        &tables,
        alice(),
        MAKE_LIST,
        &[list("groceries", "food", 1)],
    )
    .assert_ok("Alice's list");
    run(
        &program,
        &tables,
        alice(),
        KEEP_ITEM,
        &[item("groceries", "milk", false, 1)],
    )
    .assert_ok("Alice's item");
    let hers = item_now(&tables, "groceries", "milk");
    assert_eq!(hers, item_json("groceries", "milk", false, 1));
    for (id, args) in [
        (DROP_ITEM, vec![text("groceries"), text("milk")]),
        (CLEAR_LIST, vec![text("groceries")]),
        (KEEP_ITEM, vec![item("groceries", "milk", true, 1)]),
    ] {
        run(&program, &tables, bob(), id, &args).assert_ok("Bob's request is answered");
        assert_eq!(item_now(&tables, "groceries", "milk"), hers, "{id}");
    }
    run(
        &program,
        &tables,
        alice(),
        DROP_ITEM,
        &[text("groceries"), text("milk")],
    )
    .assert_ok("Alice's request");
    assert_eq!(item_now(&tables, "groceries", "milk"), Value::Null);
}

/// A reader signed in as no one runs a command that writes an `owner user`
/// table, and it fails: the write traps the command, which commits nothing.
#[test]
fn a_compiled_command_of_a_signed_out_reader_writes_no_users_row() {
    let program = app_owned("user");
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let failed = run(
        &program,
        &tables,
        writer("s-guest", None),
        KEEP_ITEM,
        &[item("groceries", "milk", false, 1)],
    );
    let why = failed.assert_err("the guest's write traps its command");
    assert!(why.contains("Unowned"), "{why}");
    assert_eq!(item_now(&tables, "groceries", "milk"), Value::Null);
}

/// A reader's command is never run as the platform, and the platform's never
/// for a reader: the adapter refuses its table functions.
#[test]
fn a_readers_command_is_never_the_platforms() {
    let program = app_owned("session");
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let refused = run(
        &program,
        &tables,
        Writer::Platform,
        DROP_ITEM,
        &[text("groceries"), text("milk")],
    )
    .assert_err("a reader's command as the platform");
    assert!(
        refused.contains("is a command a reader runs, and its writes are its reader's"),
        "{refused}"
    );
}

// --- the host's reading: the manifest, a writer's scope ----------------------

/// The host reads each table's owner from tables.json as the compiler wrote
/// it (`pw_core::tables::OwnerKind`, mirrored by name), none for `owner
/// anyone`; a kind it does not know refuses the manifest rather than leaving
/// the table's rows unowned.
#[test]
fn the_host_reads_each_tables_owner_and_refuses_a_kind_it_does_not_know() {
    let session = app_owned("session");
    assert_eq!(session.manifest.owner_of("Lists"), Some(OwnerKind::Session));
    assert_eq!(session.manifest.owner_of("Items"), Some(OwnerKind::Session));
    let user = app_owned("user");
    assert_eq!(user.manifest.owner_of("Items"), Some(OwnerKind::User));
    let anyone = app();
    assert_eq!(anyone.manifest.owner_of("Items"), None);
    assert_eq!(session.manifest.owner_of("Nothing"), None);
    let refused = manifest_where(&session.tables, |json| {
        json["tables"][0]["owner"] = json!("organization");
    })
    .assert_err("an owner of a kind the host does not know");
    assert!(refused.contains("organization"), "{refused}");
}

/// Which rows a writer changes: any, for the platform or a table no reader
/// owns; else its reader's own, by the id of the table's kind. A reader
/// with no id of that kind changes none, and an empty id names no one.
#[test]
fn a_writers_scope_is_its_readers_id_of_the_tables_kind() {
    let both = writer("s-1", Some("u-1"));
    assert_eq!(both.scope(None), Ok(Scope::Any));
    assert_eq!(
        both.scope(Some(OwnerKind::Session)),
        Ok(Scope::Owned("s-1"))
    );
    assert_eq!(both.scope(Some(OwnerKind::User)), Ok(Scope::Owned("u-1")));
    let guest = writer("s-2", None);
    assert_eq!(
        guest.scope(Some(OwnerKind::Session)),
        Ok(Scope::Owned("s-2"))
    );
    assert!(guest.scope(Some(OwnerKind::User)).is_err());
    assert_eq!(guest.scope(None), Ok(Scope::Any));
    for kind in [None, Some(OwnerKind::Session), Some(OwnerKind::User)] {
        assert_eq!(Writer::Platform.scope(kind), Ok(Scope::Any));
    }
    assert!(Reader::new("", None).is_err());
    assert!(Reader::new("s", Some("")).is_err());
    // A row records its owner, or none: the scope changes it, or not.
    let mine = Scope::Owned("u-1");
    assert!(mine.changes(Some("u-1")));
    assert!(!mine.changes(Some("u-2")));
    assert!(!mine.changes(None), "a row no reader owns is no reader's");
    assert!(Scope::Any.changes(None) && Scope::Any.changes(Some("u-2")));
    assert_eq!(mine.owner(), Some("u-1"));
    assert_eq!(Scope::Any.owner(), None);
}

/// A reader's writer is a command a request runs; the platform's, a command
/// another of its own invokes; and a command the clock invokes is run for
/// the writer whose command scheduled it, of a kind its contract says some
/// command schedules it for (`runs_for`, the review of 2026-11-07): a run
/// only readers schedule is never the platform's, nor one only the platform
/// schedules a reader's; one both schedule is either's, and one nothing
/// schedules no one's. A contract that says otherwise refuses it.
#[test]
fn a_writer_is_admitted_for_its_own_kind_of_command() {
    let program = app_owned("session");
    let contract = program.reader(DROP_ITEM).contract.clone();
    assert_eq!(alice().admits(&contract), Ok(()));
    assert!(Writer::Platform.admits(&contract).is_err());
    let invoked_by = |by: &str, runs_for: &[&str]| {
        let mut invoked = contract.clone();
        let export = invoked.exports[0]
            .component
            .as_mut()
            .assert_ok("its component's export");
        export.invoked_by = Some(by.into());
        export.runs_for = runs_for.iter().map(|w| w.to_string()).collect();
        invoked
    };
    let payments = invoked_by("Payments", &[]);
    assert_eq!(Writer::Platform.admits(&payments), Ok(()));
    let refused = alice()
        .admits(&payments)
        .assert_err("a reader's writes for the platform's command");
    assert!(refused.contains("is invoked by Payments"), "{refused}");
    let readers = invoked_by("Clock", &["reader"]);
    assert_eq!(
        alice().admits(&readers),
        Ok(()),
        "the reader who scheduled it"
    );
    let refused = Writer::Platform
        .admits(&readers)
        .assert_err("a run only readers schedule, as the platform");
    assert!(
        refused.contains("its writes are that reader's, never the platform's"),
        "{refused}"
    );
    let platforms = invoked_by("Clock", &["platform"]);
    assert_eq!(
        Writer::Platform.admits(&platforms),
        Ok(()),
        "the platform's schedule"
    );
    let refused = alice()
        .admits(&platforms)
        .assert_err("a run only the platform schedules, as a reader");
    assert!(
        refused.contains("for the platform's own schedules"),
        "{refused}"
    );
    let both = invoked_by("Clock", &["platform", "reader"]);
    assert_eq!(alice().admits(&both), Ok(()));
    assert_eq!(Writer::Platform.admits(&both), Ok(()));
    let none = invoked_by("Clock", &[]);
    for writer in [alice(), Writer::Platform] {
        let refused = writer
            .admits(&none)
            .assert_err("a run no command schedules");
        assert!(refused.contains("no command schedules it"), "{refused}");
    }
}

/// **The compiler's `runs_for` reaches the host**: the app with a reader's
/// command that schedules one the clock invokes, built; that command's
/// contract, as the host reads it, admits the reader and refuses the
/// platform.
#[test]
fn a_run_a_readers_command_schedules_is_never_the_platforms() {
    const LATER: &str = "\nopaque type InteractionId = String\n\n\
        command drop_later(list: ListId, item: ItemId) -> Result<Int, String> \
        !{ clock.schedule }\n    requires Anyone\n{\n    \
        schedule drop_due(list, item) after minutes(30)\n    Ok(1)\n}\n\n\
        command drop_due(list: ListId, item: ItemId) -> Result<Int, String>\n    \
        invoked_by    Clock\n    idempotent_by InteractionId\n    \
        invalidates   ItemsOf(list), OpenItems(), EveryItem()\n{\n    \
        Items.delete(list, item)\n    Ok(1)\n}\n";
    let built = build_edited("app.pw", |src| {
        let src = src.replacen(
            "import Items\n",
            "import Items\nimport clock.{ minutes }\n",
            1,
        );
        format!("{src}{LATER}")
    });
    assert!(built.as_ref().err().is_none(), "{:?}", built.as_ref().err());
    let build = built.assert_ok("it builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contracts = pw_host::ComponentContract::from_json(
        &serde_json::to_string(&build.contracts).assert_ok("contracts"),
    )
    .assert_ok("the host reads the compiler's contracts");
    let due = contracts
        .iter()
        .find(|c| c.component_id == "tables.app.drop_due")
        .assert_ok("drop_due's contract");
    let export = due.exports[0].component.as_ref().assert_ok("its export");
    assert_eq!(export.runs_for, ["reader"]);
    assert_eq!(alice().admits(due), Ok(()));
    assert!(Writer::Platform.admits(due).is_err());
}
