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
//! each row it holds owned by no reader; one whose owners would be read as
//! another kind of reader's is refused at start.
//!
//! Each test makes a schema of its own and drops it; without a database
//! (`PW_TABLES_DATABASE_URL`, else `PW_STORE_DATABASE_URL`) each says so and
//! passes, as the adapter's other tests do.

mod common;

#[path = "../../pw-table-memory/tests/support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use common::*;
use pw_core::tables::Schema;
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

/// **Alice's row, inserted while Bob's put of its key runs**: committed by a
/// connection of her own while his statement waits on it, his command's
/// one write not run again. What his write answers, and the tables after.
fn race(scratch: &Scratch, isolation: Isolation, setting: &str) -> (Result<(), Error>, PgTables) {
    let name = "owned_race_put";
    let url = named(
        &with_setting(&scratch.url, "default_transaction_isolation", setting),
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
    let mut hers =
        postgres::Client::connect(&scratch.url, postgres::NoTls).assert_ok("a connection");
    hers.batch_execute(&format!("SET search_path TO \"{}\"; BEGIN", scratch.schema))
        .assert_ok("her transaction");
    hers.execute(
        "INSERT INTO \"Notes\" (shelf, id, slug, text, \"pw:owner\") VALUES ($1, $2, $3, $4, $5)",
        &[
            &b"home".to_vec(),
            &b"milk".to_vec(),
            &b"m".to_vec(),
            &b"hers".to_vec(),
            &"s-alice",
        ],
    )
    .assert_ok("her row, not yet committed");
    let answered = std::thread::scope(|scope| {
        let his = scope.spawn(|| {
            let mut command = tables.begin(bob())?;
            command.write(PUT, &[note("milk", "x", "his")])?;
            command.commit().map(|_| ())
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
        hers.batch_execute("COMMIT").assert_ok("her commit");
        assert!(waiting, "Bob's put never waited on her row");
        his.join().assert_ok("Bob's put")
    });
    (answered, tables)
}

/// **A row inserted while a put runs is left as it was.** A deployment that
/// leaves the isolation to its connections (`Isolation::ConnectionDefault`)
/// may run at read committed, where a put's statement reads one snapshot and
/// its conflict finds a row inserted after it. The update's own guard
/// decides on that row, so Bob changes nothing, and is answered as for
/// another reader's row (written, nothing changed); judged on the snapshot
/// alone, the update would have taken her row. At serializable, the
/// adapter's own, PostgreSQL refuses the interleaving itself, as one it
/// could not serialize (a command run again then finds her row, absent to
/// it).
#[test]
fn a_row_inserted_while_a_put_runs_is_left_as_it_was() {
    let Some(scratch) = database_for("owned_race") else {
        return;
    };
    let (answered, tables) = race(&scratch, Isolation::ConnectionDefault, "read committed");
    assert_eq!(answered, Ok(()), "answered as for another reader's row");
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
    assert_eq!(owners(&scratch), ["milk s-alice"]);
    drop(tables);
    let serializable = Scratch::new(&scratch.url, "owned_race_serializable");
    let (answered, tables) = race(&serializable, Isolation::Serializable, "read committed");
    assert!(
        matches!(answered, Err(Error::Serialization(_))),
        "{answered:?}"
    );
    assert_eq!(now(&tables, "milk"), note("milk", "m", "hers"));
    assert_eq!(owners(&serializable), ["milk s-alice"]);
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
/// that gives them up, by a build whose migration is from the schema
/// recorded, loses the column; an old build, whose is not, is refused. One
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
    // A build from it gives them up, and says `owner anyone`.
    let widened = PgTables::open(
        &scratch.url,
        scratch.schema(),
        built_against("    owner anyone\n", &lock("    owner session\n")),
    )
    .assert_ok("the owners given up");
    assert!(!owner_column(&scratch), "{:?}", owners(&scratch));
    write(&widened, bob(), DELETE, &[json!("home"), json!("jam")]).assert_ok("anyone's delete");
    assert_eq!(now(&widened, "jam"), Value::Null);
}
