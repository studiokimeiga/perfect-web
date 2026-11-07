//! **A row says who may change it, on PostgreSQL** (the owned-rows ADR; the
//! memory adapter's `owned_rows.rs`, answered alike).
//!
//! A table whose rows record their owner (`owner session`, `owner user`) has
//! a column of the adapter's own, `"pw:owner"`, in no field: a reader's
//! insert records the reader's id there, and its `put`, `delete` and
//! `delete_of_<fields>` statements change only the rows recording it, in the
//! same statement, so no other statement's read decides. Another reader's
//! row is absent to them. A reader with no id of the table's kind is refused
//! before the database sees anything. The platform changes any row and owns
//! none it inserts. A table that comes to record owners gains the column,
//! each row it holds owned by no reader; one gives them up only as its
//! migration states it; one whose owners would be read as another kind of
//! reader's is refused at start. The races make two connections'
//! interleavings at read committed and at serializable, and a put is
//! answered by what its statement wrote. The app's compiled `drop_item`
//! runs through the adapter's functions as a host runs a request, held to
//! its writer.
//!
//! Each test makes a schema of its own and drops it; without a database
//! (`PW_TABLES_DATABASE_URL`, else `PW_STORE_DATABASE_URL`) each says so and
//! passes, as the adapter's other tests do.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use common::*;
use pw_core::tables::Schema;
use pw_host::engine::Val;
use pw_host::tables::{Reader, Writer};
use pw_table_postgres::{Error, Isolation, Options, PgTables};
use serde_json::{Value, json};

fn writer(session: &str, user: Option<&str>) -> Writer {
    Writer::Reader(Reader::new(session, user).expect("a reader"))
}

fn alice() -> Writer {
    writer("s-alice", Some("u-alice"))
}

fn bob() -> Writer {
    writer("s-bob", Some("u-bob"))
}

/// A shelf's notes, a slug each, keyed by the shelf first. `OWNER` is where
/// its `owner` is written.
const NOTES: &str = "module Notes

opaque type NoteId = String
opaque type Shelf = String

type Note = Note { shelf: Shelf, id: NoteId, slug: String, text: String }

table Notes
    row     Note
    key     shelf, id
    unique  by_slug on slug
OWNER";

fn notes_sources(owner: &str) -> String {
    NOTES.replace("OWNER", owner)
}

fn notes(owner: &str) -> pw_core::tables::TablesJson {
    let src = notes_sources(owner);
    tables(&[("Notes.pw", &src)])
}

const INSERT: &str = "pw:tables/notes#insert";
const PUT: &str = "pw:tables/notes#put";
const DELETE: &str = "pw:tables/notes#delete";
const CLEAR: &str = "pw:tables/notes#delete-of-shelf";
const GET: &str = "pw:tables/notes#get";

fn note(id: &str, slug: &str, text: &str) -> Value {
    json!({"shelf": "home", "id": id, "slug": slug, "text": text})
}

fn write(tables: &PgTables, who: Writer, import: &str, args: &[Value]) -> Result<(), Error> {
    tables.transaction(who, |tx| tx.write(import, args))
}

fn now(tables: &PgTables, id: &str) -> Value {
    tables
        .read(GET, &[json!("home"), json!(id)])
        .assert_ok("a read")
}

/// Each row's owner as the database holds it, by its id.
fn owners(scratch: &Scratch) -> Vec<String> {
    scratch.texts(
        "SELECT convert_from(id, 'UTF8') || ' ' || coalesce(\"pw:owner\", 'none') \
         FROM \"Notes\" ORDER BY convert_from(id, 'UTF8')",
    )
}

/// Whether `Notes` has the adapter's owner column, as the catalog says.
fn owner_column(scratch: &Scratch) -> bool {
    scratch.texts(&format!(
        "SELECT count(*)::text FROM information_schema.columns \
         WHERE table_schema = '{}' AND table_name = 'Notes' AND column_name = 'pw:owner'",
        scratch.schema
    )) == ["1"]
}

/// The database, or a note that this test passes for want of one.
fn database_for(what: &str) -> Option<Scratch> {
    match database() {
        Some(url) => Some(Scratch::new(&url, what)),
        None => {
            eprintln!("{what}: skipped, no PW_TABLES_DATABASE_URL or PW_STORE_DATABASE_URL");
            None
        }
    }
}

/// The notes on a schema of the test's own, owned by `owner`, holding
/// Alice's milk and eggs.
fn alices(scratch: &Scratch, owner: &str) -> PgTables {
    let tables =
        PgTables::open(&scratch.url, scratch.schema(), notes(owner)).assert_ok("the adapter opens");
    for (id, slug) in [("milk", "m"), ("eggs", "e")] {
        write(&tables, alice(), INSERT, &[note(id, slug, "hers")]).assert_ok("Alice's note");
    }
    tables
}

#[test]
fn another_readers_rows_are_absent_to_every_write() {
    for owner in ["    owner session\n", "    owner user\n"] {
        let Some(scratch) = database_for("owned_absent") else {
            return;
        };
        let tables = alices(&scratch, owner);
        let before = (now(&tables, "milk"), now(&tables, "eggs"));
        write(&tables, bob(), PUT, &[note("milk", "m", "Bob's now")]).assert_ok("a put");
        write(&tables, bob(), DELETE, &[json!("home"), json!("milk")]).assert_ok("a delete");
        write(&tables, bob(), CLEAR, &[json!("home")]).assert_ok("a clear");
        assert_eq!(
            (now(&tables, "milk"), now(&tables, "eggs")),
            before,
            "{owner}"
        );
        // The control: Alice changes each.
        write(&tables, alice(), PUT, &[note("milk", "m", "changed")]).assert_ok("her put");
        assert_eq!(now(&tables, "milk"), note("milk", "m", "changed"));
        write(&tables, alice(), DELETE, &[json!("home"), json!("milk")]).assert_ok("her delete");
        assert_eq!(now(&tables, "milk"), Value::Null);
        write(&tables, alice(), CLEAR, &[json!("home")]).assert_ok("her clear");
        assert_eq!(now(&tables, "eggs"), Value::Null);
    }
}

/// The owner is the adapter's column, as the database holds it; a read
/// answers the row's fields alone.
#[test]
fn the_owner_is_a_column_of_the_adapters_and_in_no_field() {
    let Some(scratch) = database_for("owned_column") else {
        return;
    };
    let tables = alices(&scratch, "    owner user\n");
    assert_eq!(owners(&scratch), ["eggs u-alice", "milk u-alice"]);
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
    let session = Scratch::new(&scratch.url, "owned_column_session");
    let _ = alices(&session, "    owner session\n");
    assert_eq!(owners(&session), ["eggs s-alice", "milk s-alice"]);
    // A table no reader owns has no such column.
    let anyone = Scratch::new(&scratch.url, "owned_column_anyone");
    let _ = alices(&anyone, "    owner anyone\n");
    assert!(owner_column(&scratch), "the control: an owned table has it");
    assert!(!owner_column(&anyone), "{:?}", owners(&anyone));
}

/// A prefix's delete removes its reader's rows of the prefix alone, and a
/// command that changed no row of its reader's wrote nothing.
#[test]
fn a_prefixs_delete_takes_its_readers_rows_alone() {
    let Some(scratch) = database_for("owned_prefix") else {
        return;
    };
    let tables = alices(&scratch, "    owner session\n");
    let mut command = tables.begin(bob()).assert_ok("its turn");
    command
        .write(DELETE, &[json!("home"), json!("eggs")])
        .assert_ok("a delete");
    command.write(CLEAR, &[json!("home")]).assert_ok("a clear");
    assert!(command.written().assert_ok("written").is_empty());
    command.commit().assert_ok("its commit");
    write(&tables, bob(), INSERT, &[note("bread", "b", "his")]).assert_ok("Bob's note");
    write(&tables, bob(), CLEAR, &[json!("home")]).assert_ok("Bob's clear");
    assert_eq!(owners(&scratch), ["eggs s-alice", "milk s-alice"]);
}

/// An insert is refused by the row holding its key, whoever's, and answered
/// with the row's fields; a put of one's own row is refused by another's
/// unique value; a put of another's row writes nothing, whatever it holds.
#[test]
fn refusals_are_the_rows_fields_and_another_readers_put_writes_nothing() {
    let Some(scratch) = database_for("owned_refusals") else {
        return;
    };
    let tables = alices(&scratch, "    owner session\n");
    assert_eq!(
        write(&tables, bob(), INSERT, &[note("milk", "x", "his")]),
        Err(Error::Conflict {
            table: "Notes".into(),
            existing: note("milk", "m", "hers"),
        })
    );
    assert_eq!(
        write(&tables, bob(), PUT, &[note("bread", "e", "his")]),
        Err(Error::Conflict {
            table: "Notes".into(),
            existing: note("eggs", "e", "hers"),
        }),
        "his own new row, its slug Alice's"
    );
    write(&tables, bob(), PUT, &[note("milk", "e", "his")])
        .assert_ok("Alice's row, a slug another holds: absent, nothing written");
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
}

/// A reader signed in as no one owns no row of an `owner user` table: every
/// write is refused before the database sees it.
#[test]
fn a_signed_out_reader_writes_no_row_of_a_users_table() {
    let Some(scratch) = database_for("owned_guest") else {
        return;
    };
    let tables = alices(&scratch, "    owner user\n");
    let guest = writer("s-guest", None);
    for (import, args) in [
        (INSERT, vec![note("jam", "j", "guest's")]),
        (PUT, vec![note("jam", "j", "guest's")]),
        (DELETE, vec![json!("home"), json!("milk")]),
        (CLEAR, vec![json!("home")]),
    ] {
        let refused = write(&tables, guest.clone(), import, &args);
        assert!(
            matches!(&refused, Err(Error::Unowned(why)) if why.starts_with("Notes: ")),
            "{import}: {refused:?}"
        );
    }
    assert_eq!(owners(&scratch), ["eggs u-alice", "milk u-alice"]);
}

/// A user's rows from any of their sessions; a session's from it alone.
#[test]
fn a_user_owns_across_sessions_and_a_session_alone() {
    let Some(scratch) = database_for("owned_kinds") else {
        return;
    };
    let phone = writer("s-alice-phone", Some("u-alice"));
    let tables = alices(&scratch, "    owner user\n");
    write(
        &tables,
        phone.clone(),
        DELETE,
        &[json!("home"), json!("milk")],
    )
    .assert_ok("her other session's delete");
    assert_eq!(now(&tables, "milk"), Value::Null);
    let by_session = Scratch::new(&scratch.url, "owned_kinds_session");
    let tables = alices(&by_session, "    owner session\n");
    write(&tables, phone, DELETE, &[json!("home"), json!("milk")]).assert_ok("a delete");
    assert_ne!(now(&tables, "milk"), Value::Null);
}

/// The platform's command changes any row and keeps each row's owner; a row
/// it inserts is no reader's.
#[test]
fn the_platform_changes_any_row_and_owns_none() {
    let Some(scratch) = database_for("owned_platform") else {
        return;
    };
    let tables = alices(&scratch, "    owner session\n");
    write(
        &tables,
        Writer::Platform,
        PUT,
        &[note("milk", "m", "swept")],
    )
    .assert_ok("its put");
    assert_eq!(now(&tables, "milk"), note("milk", "m", "swept"));
    write(
        &tables,
        Writer::Platform,
        INSERT,
        &[note("salt", "s", "its")],
    )
    .assert_ok("its insert");
    assert_eq!(
        owners(&scratch),
        ["eggs s-alice", "milk s-alice", "salt none"]
    );
    for who in [alice(), bob()] {
        write(&tables, who, DELETE, &[json!("home"), json!("salt")]).assert_ok("a delete");
    }
    assert_ne!(now(&tables, "salt"), Value::Null, "no reader's");
    write(&tables, Writer::Platform, CLEAR, &[json!("home")]).assert_ok("its clear");
    assert!(owners(&scratch).is_empty(), "{:?}", owners(&scratch));
}

/// Alice's row at `milk`, as a connection of her own writes it.
const HER_MILK: &str = "INSERT INTO \"Notes\" (shelf, id, slug, text, \"pw:owner\") VALUES \
     (convert_to('home', 'UTF8'), convert_to('milk', 'UTF8'), convert_to('m', 'UTF8'), \
     convert_to('hers', 'UTF8'), 's-alice')";

/// The row at `milk`, whoever's, deleted by that connection.
const NO_MILK: &str = "DELETE FROM \"Notes\" WHERE id = convert_to('milk', 'UTF8')";

/// How Alice's transaction begins: at her connection's default, read
/// committed, or serializable, as a command of the adapter's own is.
const HER_DEFAULT: &str = "BEGIN";
const HER_SERIALIZABLE: &str = "BEGIN ISOLATION LEVEL SERIALIZABLE";

/// What Bob's put writes at `milk`.
fn his() -> Value {
    note("milk", "x", "his")
}

/// **Bob's put of `milk` ([`his`]), while a transaction of Alice's own
/// connection holds its key**: the table holding a row there first, its
/// writer's and its fields, or none; her transaction begun by `begin` and
/// running `hers`, then committed while his statement waits on it; his
/// command's one write not run again. What his command answers (the tables
/// its commit wrote), and the tables after. His connections are named
/// `name`, at read committed unless `isolation` says otherwise.
fn race(
    scratch: &Scratch,
    isolation: Isolation,
    name: &str,
    held: Option<(Writer, Value)>,
    (begin, hers): (&str, &[&str]),
) -> (Result<Vec<String>, Error>, PgTables) {
    let url = named(
        &with_setting(
            &scratch.url,
            "default_transaction_isolation",
            "read committed",
        ),
        name,
    );
    let options = Options {
        isolation,
        ..Options::default()
    };
    let tables = PgTables::open_with(
        &url,
        scratch.schema(),
        notes("    owner session\n"),
        options,
    )
    .assert_ok("the adapter opens");
    if let Some((who, row)) = held {
        write(&tables, who, INSERT, &[row]).assert_ok("the row held");
    }
    let mut her =
        postgres::Client::connect(&scratch.url, postgres::NoTls).assert_ok("a connection");
    her.batch_execute(&format!(
        "SET search_path TO \"{}\"; {begin}; {}",
        scratch.schema,
        hers.join("; ")
    ))
    .assert_ok("her transaction, not yet committed");
    let answered = std::thread::scope(|scope| {
        let his = scope.spawn(|| {
            let mut command = tables.begin(bob())?;
            command.write(PUT, &[his()])?;
            command
                .commit()
                .map(|committed| committed.written.into_keys().collect())
        });
        let waiting = until(|| {
            scratch
                .texts(&format!(
                    "SELECT count(*)::text FROM pg_stat_activity \
                     WHERE application_name = '{name}' AND wait_event_type = 'Lock'"
                ))
                .first()
                .is_some_and(|n| n != "0")
        });
        her.batch_execute("COMMIT").assert_ok("her commit");
        assert!(waiting, "{name}: Bob's put never waited on her transaction");
        his.join().assert_ok("Bob's put")
    });
    (answered, tables)
}

/// **A row inserted while a put runs is left as it was.** A deployment that
/// leaves the isolation to its connections (`Isolation::ConnectionDefault`)
/// may run at read committed, where a put's statement reads one snapshot and
/// its conflict finds a row inserted after it. The update's own guard
/// decides on that row, so Bob changes nothing, and is answered as for
/// another reader's row (answered written, and his commit wrote no table);
/// judged on the snapshot alone, the update would have taken her row. At
/// serializable, the adapter's own, PostgreSQL refuses the interleaving
/// itself, as one it could not serialize (a command run again then finds
/// her row, absent to it).
#[test]
fn a_row_inserted_while_a_put_runs_is_left_as_it_was() {
    let Some(scratch) = database_for("owned_race") else {
        return;
    };
    let (answered, tables) = race(
        &scratch,
        Isolation::ConnectionDefault,
        "owned_race_inserted",
        None,
        (HER_DEFAULT, &[HER_MILK]),
    );
    assert_eq!(
        answered,
        Ok(Vec::new()),
        "answered as for another reader's row"
    );
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
    assert_eq!(owners(&scratch), ["milk s-alice"]);
    drop(tables);
    let serializable = Scratch::new(&scratch.url, "owned_race_serializable");
    let (answered, tables) = race(
        &serializable,
        Isolation::Serializable,
        "owned_race_inserted_serializable",
        None,
        (HER_DEFAULT, &[HER_MILK]),
    );
    assert!(
        matches!(answered, Err(Error::Serialization(_))),
        "{answered:?}"
    );
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
    assert_eq!(owners(&serializable), ["milk s-alice"]);
}

/// **A put is answered by what its statement wrote, not by what its
/// snapshot held** (the review of 2026-11-07). Bob's statement reads
/// Alice's row at the key, and its insert, waiting on her delete, writes
/// his own row once she commits: his commit wrote `Notes`, and says so. It
/// was answered as a put of her row, which writes nothing, so his commit
/// published no table, and every view of `Notes` missed his row. Her row
/// holds what his put writes, so his row anew is a write only because it
/// replaced none of his: taken for the row it replaced, hers, it would be
/// his row put back as it was. His own row, replaced by hers while his put
/// waits, is the other way about: the update's guard leaves her row, and
/// his command wrote nothing, where it was answered as a write of his. Each
/// at read committed, and at serializable, the adapter's own: PostgreSQL
/// refuses each interleaving with a transaction of its kind, and her
/// transaction at read committed (another writer of the database) too
/// where it replaced his row, but not where it left the key free: his
/// insert there commits, as her delete then his put would, and is answered
/// as written.
#[test]
fn a_put_is_answered_by_what_it_wrote_not_by_its_snapshot() {
    let Some(first) = database_for("owned_race_answer") else {
        return;
    };
    let deleted: &[&str] = &[NO_MILK];
    let replaced: &[&str] = &[NO_MILK, HER_MILK];
    let hers = note("milk", "m", "hers");
    let (her_row, his_row) = ((alice(), his()), (bob(), note("milk", "m", "his before")));
    // Each race by its name: his command's isolation, her transaction,
    // whose row the table held at `milk`; what his command answers (the
    // tables its commit wrote, or `None`, refused as one PostgreSQL could
    // not serialize), the row at `milk` after, and each row's owner.
    let races = [
        (
            "owned_race_deleted",
            Isolation::ConnectionDefault,
            (HER_DEFAULT, deleted),
            her_row.clone(),
            Some(vec!["Notes"]),
            his(),
            vec!["milk s-bob"],
        ),
        (
            "owned_race_replaced",
            Isolation::ConnectionDefault,
            (HER_DEFAULT, replaced),
            his_row.clone(),
            Some(vec![]),
            hers.clone(),
            vec!["milk s-alice"],
        ),
        (
            "owned_race_deleted_beside",
            Isolation::Serializable,
            (HER_DEFAULT, deleted),
            her_row.clone(),
            Some(vec!["Notes"]),
            his(),
            vec!["milk s-bob"],
        ),
        (
            "owned_race_replaced_beside",
            Isolation::Serializable,
            (HER_DEFAULT, replaced),
            his_row.clone(),
            None,
            hers.clone(),
            vec!["milk s-alice"],
        ),
        (
            "owned_race_deleted_serializable",
            Isolation::Serializable,
            (HER_SERIALIZABLE, deleted),
            her_row.clone(),
            None,
            Value::Null,
            vec![],
        ),
        (
            "owned_race_replaced_serializable",
            Isolation::Serializable,
            (HER_SERIALIZABLE, replaced),
            his_row.clone(),
            None,
            hers.clone(),
            vec!["milk s-alice"],
        ),
    ];
    for (name, isolation, transaction, held, wrote, at_milk, left) in races {
        let scratch = Scratch::new(&first.url, name);
        let (answered, tables) = race(&scratch, isolation, name, Some(held), transaction);
        match wrote {
            Some(wrote) => assert_eq!(
                answered,
                Ok(wrote.iter().map(|t| t.to_string()).collect()),
                "{name}"
            ),
            None => assert!(
                matches!(answered, Err(Error::Serialization(_))),
                "{name}: {answered:?}"
            ),
        }
        assert_eq!(now(&tables, "milk"), at_milk, "{name}");
        assert_eq!(owners(&scratch), left, "{name}");
    }
}

// --- the app's compiled command, run as a host runs a request -----------------

/// `examples/tables`' `drop_item(list, item)`.
const DROP_ITEM: &str = "tables.app.drop_item";

/// `examples/tables`' app, its lists and items owned by a session (the
/// memory adapter's `common::app_owned`): its tables.json, and `drop_item`
/// compiled.
fn owned_app() -> (pw_core::tables::TablesJson, Compiled) {
    let (manifest, mut components) = compiled_edited("app.pw", &[DROP_ITEM], &|src| {
        src.replace("    owner   anyone\n", "    owner   session\n")
            .replace("    owner  anyone\n", "    owner  session\n")
    });
    let drop_item = components
        .remove(DROP_ITEM)
        .assert_ok("drop_item, compiled");
    (manifest, drop_item)
}

/// **A reader's command is never the platform's, on PostgreSQL** (the
/// review of 2026-11-07; the memory adapter's test of the same name): the
/// app's compiled `drop_item`, begun for the platform, is refused its table
/// functions before it runs, so a host that ran a request's command as the
/// platform, which changes any row, changes none. Begun for a reader, it is
/// admitted, and changes that reader's rows alone: Bob's request names
/// Alice's item and changes nothing, and hers deletes it.
#[test]
fn a_readers_command_is_never_the_platforms() {
    let Some(scratch) = database_for("owned_command") else {
        return;
    };
    let (manifest, drop_item) = owned_app();
    let tables =
        PgTables::open(&scratch.url, scratch.schema(), manifest).assert_ok("the adapter opens");
    let list = json!({"id": "groceries", "name": "Groceries", "slug": "food", "added": 1});
    let item = json!({
        "list": "groceries", "id": "milk", "title": "Milk", "done": false, "count": 1,
        "added": 1, "note": null
    });
    write(&tables, alice(), "pw:tables/lists#insert", &[list]).assert_ok("her list");
    write(&tables, alice(), "pw:tables/items#insert", &[item]).assert_ok("her item");
    let granted = drop_item.granted();
    let run = |who: Writer| -> Result<Vec<Val>, String> {
        let command = tables.begin(who).assert_ok("its turn");
        let mut host = command
            .host_fns(&drop_item.contract, &granted)
            .map_err(|error| format!("{error:?}"))?;
        host.extend(drop_item.platform().0);
        let args = [Val::String("groceries".into()), Val::String("milk".into())];
        let answer = drop_item.call(&granted, &host, tables.host_tables(), &args)?;
        command.commit().assert_ok("its commit");
        Ok(answer)
    };
    let milk = || {
        tables
            .read("pw:tables/items#get", &[json!("groceries"), json!("milk")])
            .assert_ok("a read")
    };
    let refused = run(Writer::Platform).assert_err("a reader's command, begun for the platform");
    assert!(
        refused.contains("is a command a reader runs, and its writes are its reader's"),
        "{refused}"
    );
    assert_ne!(milk(), Value::Null, "refused before it ran");
    run(bob()).assert_ok("Bob's request");
    assert_ne!(
        milk(),
        Value::Null,
        "Bob's request names her item, and changes nothing"
    );
    run(alice()).assert_ok("her request");
    assert_eq!(milk(), Value::Null, "her request deletes it");
}

// --- the migration -------------------------------------------------------------

/// tables.json of the notes owned by `owner`, built against `released`.
fn built_against(owner: &str, released: &Schema) -> pw_core::tables::TablesJson {
    let src = notes_sources(owner);
    build(&[("Notes.pw", &src)], Some(released)).tables
}

fn lock(owner: &str) -> Schema {
    let src = notes_sources(owner);
    build(&[("Notes.pw", &src)], None).schema
}

/// A table that comes to record owners gains the column: every row it held
/// is no reader's, so no request changes it, and the platform still may. One
/// that gives them up loses the column only as its migration from the schema
/// recorded states it (`migrate owner session dropped`, as a field is
/// dropped: the review of 2026-11-07); an old build, whose migration is not
/// from it, is refused, and so is one from it that does not state it. One
/// whose owners would be read as another kind of reader's is refused.
#[test]
fn a_table_comes_to_record_owners_and_gives_them_up_only_as_stated() {
    let Some(scratch) = database_for("owned_migrate") else {
        return;
    };
    // Released with no owner, two rows written.
    let unowned =
        PgTables::open(&scratch.url, scratch.schema(), notes("")).assert_ok("the release");
    for (id, slug) in [("milk", "m"), ("eggs", "e")] {
        write(&unowned, alice(), INSERT, &[note(id, slug, "before")]).assert_ok("a row");
    }
    drop(unowned);
    let owned = PgTables::open(
        &scratch.url,
        scratch.schema(),
        built_against("    owner session\n", &lock("")),
    )
    .assert_ok("the column made");
    assert_eq!(owners(&scratch), ["eggs none", "milk none"]);
    write(&owned, alice(), DELETE, &[json!("home"), json!("milk")]).assert_ok("a delete");
    assert_ne!(
        now(&owned, "milk"),
        Value::Null,
        "a row held before is no reader's"
    );
    write(&owned, alice(), INSERT, &[note("jam", "j", "hers")]).assert_ok("her row");
    write(
        &owned,
        Writer::Platform,
        DELETE,
        &[json!("home"), json!("milk")],
    )
    .assert_ok("the platform's delete");
    assert_eq!(now(&owned, "milk"), Value::Null);
    drop(owned);
    // The same database, reopened by the same build: nothing to do.
    let again = PgTables::open(
        &scratch.url,
        scratch.schema(),
        built_against("    owner session\n", &lock("")),
    )
    .assert_ok("reopened");
    drop(again);
    // Another kind of reader: refused (the compiler refuses the same build
    // against the schema released, PW0366, so this is a build of no release).
    let kind = PgTables::open(&scratch.url, scratch.schema(), notes("    owner user\n")).err();
    assert!(
        matches!(&kind, Some(Error::Migration(why)) if why.contains("each owner recorded would be read as another kind of reader")),
        "{kind:?}"
    );
    // An old build, whose migration is not from the schema recorded, does
    // not give the owners up.
    let old = PgTables::open(&scratch.url, scratch.schema(), notes("")).err();
    assert!(
        matches!(&old, Some(Error::Migration(why)) if why.contains("the owners would be lost")),
        "{old:?}"
    );
    assert_eq!(owners(&scratch), ["eggs none", "jam s-alice"]);
    // A build from it that states it gives them up, and says `owner anyone`;
    // its migration without the statement's step gives up nothing.
    let given_up = built_against(
        "    owner anyone\n    migrate owner session dropped\n",
        &lock("    owner session\n"),
    );
    let mut unstated = given_up.clone();
    unstated
        .migration
        .steps
        .retain(|step| !matches!(step, pw_core::tables::Step::ChangeOwner { .. }));
    let refused = PgTables::open(&scratch.url, scratch.schema(), unstated).err();
    assert!(
        matches!(&refused, Some(Error::Migration(why)) if why.contains("state `migrate owner session dropped`")),
        "{refused:?}"
    );
    assert!(owner_column(&scratch), "the control: the column kept");
    let widened =
        PgTables::open(&scratch.url, scratch.schema(), given_up).assert_ok("the owners given up");
    assert!(!owner_column(&scratch), "{:?}", owners(&scratch));
    write(&widened, bob(), DELETE, &[json!("home"), json!("jam")]).assert_ok("anyone's delete");
    assert_eq!(now(&widened, "jam"), Value::Null);
}
