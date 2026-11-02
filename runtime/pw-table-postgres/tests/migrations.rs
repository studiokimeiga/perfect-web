//! **The schema, from the declarations** (ADR-0338 1.4, on PostgreSQL):
//! made once, under the lock, whoever opens it; an additive change made; a
//! change that loses or reinterprets data made only where the program's
//! `migrate` states it, from the schema the database holds; and a database
//! whose recorded schema or catalog is not the build's refused at start.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::sync::Arc;

use common::*;
use pw_core::tables::{Schema, TablesJson};
use pw_table_postgres::{Error, PgTables};
use serde_json::json;

/// The things' program with `Things.pw` edited by `edit`, built against
/// `released`: its tables.json, and its schema (its `schema.lock`).
fn things_where(edit: impl Fn(&str) -> String, released: Option<&Schema>) -> (TablesJson, Schema) {
    let things = edit(THINGS);
    let build = build(
        &[
            ("Owners.pw", OWNERS),
            ("Things.pw", &things),
            ("ThingData.pw", THING_DATA),
        ],
        released,
    );
    (build.tables, build.schema)
}

fn released() -> Schema {
    things_where(str::to_string, None).1
}

fn open(scratch: &Scratch, manifest: TablesJson) -> Result<PgTables, Error> {
    PgTables::open(&scratch.url, scratch.schema(), manifest)
}

fn seeded(scratch: &Scratch) -> PgTables {
    let tables = open_things(scratch);
    tables
        .transaction(|tx| {
            tx.write(&owner("insert"), &[owner_row("a")])?;
            tx.write(&thing("insert"), &[thing_row("a", 1, 5, "eight888", true)])?;
            tx.write(&thing("insert"), &[thing_row("a", 2, 6, "two", false)])
        })
        .assert_ok("seeded");
    tables
}

#[test]
fn a_database_holding_nothing_is_made_from_tables_json_and_recorded() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fresh");
    let manifest = things();
    let hash = manifest.hash.clone();
    let tables = open(&scratch, manifest).assert_ok("made");
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema"), [hash]);
    assert_eq!(
        scratch.texts("SELECT version FROM pw_platform"),
        ["0001_platform"]
    );
    // The tables, their keys, checks, indexes and references, as the
    // catalog holds them.
    let made = scratch.texts(
        "SELECT conname::text FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace \
         WHERE n.nspname = current_schema() AND c.contype IN ('p', 'c', 'f') ORDER BY 1",
    );
    for expected in [
        "Owners__id__check",
        "Owners__pkey",
        "Things__count__check",
        "Things__maybe__check",
        "Things__nothing__check",
        "Things__owner__fkey",
        "Things__pkey",
        "Things__point__check",
        "Things__rank__check",
        "Things__score__check",
        "Things__slug__check",
        "Things__tags__check",
    ] {
        assert!(
            made.contains(&expected.to_string()),
            "{expected} in {made:?}"
        );
    }
    let indexes = scratch.texts(
        "SELECT indexdef FROM pg_indexes WHERE schemaname = current_schema() \
         AND indexname LIKE 'Things__by%' ORDER BY 1",
    );
    assert_eq!(indexes.len(), 3, "{indexes:?}");
    assert!(
        indexes
            .iter()
            .any(|i| i.contains("UNIQUE") && i.contains("(slug)")),
        "{indexes:?}"
    );
    assert!(
        indexes.iter().any(|i| i.contains("(flag, rank, owner, n)")),
        "a plain index holds the order and the key: {indexes:?}"
    );
    drop(tables);
}

/// **Applied once** (ADR-0338 D3's control "a migration applied twice"):
/// eight hosts opening one empty schema at once make it once, and a host
/// opening it again makes nothing.
#[test]
fn a_schema_is_applied_once_whoever_opens_it_and_whenever() {
    let Some(url) = database() else { return };
    let scratch = Arc::new(Scratch::new(&url, "once"));
    let manifest = things();
    let at_once = Arc::new(std::sync::Barrier::new(8));
    let opened: Vec<_> = (0..8)
        .map(|_| {
            let scratch = Arc::clone(&scratch);
            let manifest = manifest.clone();
            let at_once = Arc::clone(&at_once);
            std::thread::spawn(move || {
                at_once.wait();
                open(&scratch, manifest).map(|_| ())
            })
        })
        .collect();
    for host in opened {
        let joined = host.join();
        assert!(matches!(joined, Ok(Ok(()))), "{joined:?}");
    }
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema").len(), 1);
    let again = open(&scratch, manifest).assert_ok("opened again");
    drop(again);
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema").len(), 1);
    assert_eq!(scratch.texts("SELECT version FROM pw_platform").len(), 1);
}

/// **An additive change is made**, from any schema: a table, an `Option`
/// field, an index; the rows kept.
#[test]
fn an_additive_change_is_made_and_keeps_every_row() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "additive");
    drop(seeded(&scratch));
    let (v2, _) = things_where(
        |t| {
            t.replace(
                "    count: Option<Positive>,\n",
                "    count: Option<Positive>,\n    extra: Option<String>,\n",
            )
            .replace(
                "    refers  owner to Owners\n",
                "    refers  owner to Owners\n    index   by_rank on rank\n",
            )
        },
        None,
    );
    let tables = open(&scratch, v2.clone()).assert_ok("the additive change made");
    let got = tables
        .read(&thing("get"), &[json!("a"), json!(1)])
        .assert_ok("a row kept");
    assert_eq!(got["extra"], json!(null));
    assert_eq!(got["slug"], json!("eight888"));
    let ranked = tables
        .read(&thing("by-rank"), &[json!(6)])
        .assert_ok("the new index");
    assert_eq!(ranked.as_array().map(Vec::len), Some(1));
    assert_eq!(
        scratch
            .texts("SELECT hash FROM pw_schema ORDER BY applied")
            .len(),
        2
    );
    drop(tables);
    // An older build on the newer database: its change would lose a field,
    // and is refused, the database as it was.
    let older = open(&scratch, things());
    assert!(
        matches!(older, Err(Error::Migration(_))),
        "{:?}",
        older.err()
    );
    assert_eq!(
        scratch.texts("SELECT hash FROM pw_schema ORDER BY applied DESC LIMIT 1"),
        [v2.hash]
    );
}

/// The destructive change of [`destructive`]: a field dropped, a field
/// renamed, a field retyped, a field added that is no `Option`.
fn destructive(t: &str, stated: bool) -> String {
    let mut out = t
        .replace("    note: Option<String>,\n", "")
        .replace("    tags: List<String>,\n", "    labels: List<String>,\n")
        .replace("    flag: Bool,\n", "    flag: Int,\n")
        .replace(
            "    count: Option<Positive>,\n",
            "    count: Option<Positive>,\n    weight: Int,\n",
        );
    if stated {
        out = out.replace(
            "    refers  owner to Owners\n",
            "    refers  owner to Owners\n    migrate note dropped\n    migrate labels renamed from tags\n    migrate flag backfilled with 0\n    migrate weight backfilled with 7\n",
        );
    }
    out
}

/// **A change that loses or reinterprets data is refused without its
/// `migrate`** (ADR-0338 1.4), naming each change, and the database left as
/// it was; with it, from the schema the database holds, it is made.
#[test]
fn a_change_that_loses_data_is_made_only_where_the_program_states_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "destructive");
    drop(seeded(&scratch));
    // Unstated (no schema released, so the compiler plans nothing of it).
    let (unstated, _) = things_where(|t| destructive(t, false), None);
    let refused = open(&scratch, unstated).err();
    assert!(
        matches!(refused, Some(Error::Migration(_))),
        "not refused as a migration: {refused:?}"
    );
    let why = format!("{refused:?}");
    for named in [
        "Things.note",
        "Things.tags",
        "Things.flag",
        "Things.labels",
        "Things.weight",
    ] {
        assert!(why.contains(named), "{named} in {why}");
    }
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema").len(), 1);
    // Stated, from the schema released, which the database holds.
    let (stated, _) = things_where(|t| destructive(t, true), Some(&released()));
    let tables = open(&scratch, stated).assert_ok("the stated change made");
    let got = tables
        .read(&thing("get"), &[json!("a"), json!(1)])
        .assert_ok("its row, migrated");
    assert_eq!(got["labels"], json!(["t1"]), "renamed, its values kept");
    assert_eq!(got["flag"], json!(0), "retyped, backfilled");
    assert_eq!(got["weight"], json!(7), "added, backfilled");
    assert!(got.get("note").is_none() && got.get("tags").is_none());
    let flagged = tables
        .read(&thing("by-flag"), &[json!(0)])
        .assert_ok("its index, made again");
    assert_eq!(flagged.as_array().map(Vec::len), Some(2));
    // Every row written from now on gives the backfilled fields.
    let mut tx = tables.begin().assert_ok("a command");
    let mut row = thing_row("a", 3, 7, "three", true);
    if let Some(o) = row.as_object_mut() {
        o.remove("note");
        o.remove("tags");
        o.insert("labels".into(), json!([]));
        o.insert("flag".into(), json!(1));
        o.insert("weight".into(), json!(1));
    }
    tx.write(&thing("insert"), &[row])
        .assert_ok("a row of the new shape");
    tx.commit().assert_ok("committed");
}

/// **Drift refuses the start** (ADR-0338 1.4): a column, a check, an index
/// or a trigger changed by another hand, under the host's recorded schema.
#[test]
fn a_catalog_changed_by_hand_refuses_the_start() {
    let Some(url) = database() else { return };
    for (what, change) in [
        (
            "a column added",
            "ALTER TABLE \"Things\" ADD COLUMN sneaky integer",
        ),
        (
            "a check dropped",
            "ALTER TABLE \"Things\" DROP CONSTRAINT \"Things__rank__check\"",
        ),
        ("an index added", "CREATE INDEX extra ON \"Owners\" (name)"),
        (
            "a column's type changed",
            "ALTER TABLE \"Owners\" ALTER COLUMN name TYPE text",
        ),
        (
            "a column made nullable",
            "ALTER TABLE \"Things\" ALTER COLUMN score DROP NOT NULL",
        ),
        (
            "a reference dropped",
            "ALTER TABLE \"Things\" DROP CONSTRAINT \"Things__owner__fkey\"",
        ),
        (
            "a trigger added",
            "CREATE FUNCTION nothing() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$; \
             CREATE TRIGGER nothing BEFORE INSERT ON \"Owners\" FOR EACH ROW EXECUTE FUNCTION nothing()",
        ),
        (
            "the code points counted otherwise",
            "CREATE OR REPLACE FUNCTION pw_code_points(bytea) RETURNS bigint LANGUAGE sql IMMUTABLE AS $$ SELECT 0::bigint $$",
        ),
        // The review of 2026-11-02: what changes a statement's answer or a
        // commit's fate and is no column, constraint, index or trigger.
        (
            "a rule added (a delete that deletes nothing)",
            "CREATE RULE kept AS ON DELETE TO \"Owners\" DO INSTEAD NOTHING",
        ),
        (
            "a table inheriting one (its rows read with it)",
            "CREATE TABLE heir () INHERITS (\"Owners\")",
        ),
        (
            "its references' triggers disabled",
            "ALTER TABLE \"Things\" DISABLE TRIGGER ALL",
        ),
        (
            "its references' triggers firing only on a replica",
            "DO $$ DECLARE t text; BEGIN FOR t IN SELECT tgname FROM pg_trigger \
             WHERE tgrelid = '\"Things\"'::regclass AND tgisinternal LOOP \
             EXECUTE format('ALTER TABLE \"Things\" ENABLE REPLICA TRIGGER %I', t); END LOOP; END $$",
        ),
        (
            "a table made unlogged (emptied by a crash)",
            "ALTER TABLE \"Things\" SET UNLOGGED",
        ),
    ] {
        let scratch = Scratch::new(&url, "drift");
        drop(seeded(&scratch));
        scratch.sql(change).assert_ok(what);
        let opened = open(&scratch, things());
        assert!(
            matches!(opened, Err(Error::Drift(_))),
            "{what}: {:?}",
            opened.err()
        );
    }
}

#[test]
fn a_table_no_schema_recorded_or_a_record_edited_by_hand_refuses_the_start() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "unrecorded");
    scratch
        .sql(&format!(
            "CREATE SCHEMA IF NOT EXISTS \"{0}\"; CREATE TABLE \"{0}\".\"Owners\" (id text)",
            scratch.schema
        ))
        .assert_ok("a table made by another hand");
    let opened = open(&scratch, things());
    assert!(matches!(opened, Err(Error::Drift(_))), "{:?}", opened.err());
    let scratch = Scratch::new(&url, "forged");
    drop(seeded(&scratch));
    for (what, edit) in [
        (
            "its schema's hash edited",
            "UPDATE pw_schema SET schema = jsonb_set(schema::jsonb, '{hash}', '\"feedfacefeedface\"')::json",
        ),
        (
            "its hash edited",
            "UPDATE pw_schema SET hash = 'feedfacefeedface'",
        ),
    ] {
        let scratch = Scratch::new(&url, "forged");
        drop(seeded(&scratch));
        scratch.sql(edit).assert_ok(what);
        let opened = open(&scratch, things());
        assert!(
            matches!(opened, Err(Error::Drift(_))),
            "{what}: {:?}",
            opened.err()
        );
    }
}

/// A bound tightened past a row the database holds: the check refuses it,
/// and nothing of the migration is kept.
#[test]
fn a_bound_no_row_released_holds_refuses_the_migration_whole() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "bounds");
    drop(seeded(&scratch));
    let (tightened, _) = things_where(
        |t| {
            t.replace("String.length(value) <= 8", "String.length(value) <= 4")
                .replace(
                    "    count: Option<Positive>,\n",
                    "    count: Option<Positive>,\n    extra: Option<Int>,\n",
                )
        },
        Some(&released()),
    );
    let refused = open(&scratch, tightened).err();
    assert!(
        matches!(refused, Some(Error::Migration(_))),
        "not refused: {refused:?}"
    );
    let why = format!("{refused:?}");
    assert!(why.contains("Things__slug__check"), "{why}");
    // Nothing of it kept: the field it added is not there, and the build
    // released opens as before.
    assert!(
        scratch
            .texts("SELECT attname::text FROM pg_attribute WHERE attrelid = '\"Things\"'::regclass AND attname = 'extra'")
            .is_empty()
    );
    let tables = open(&scratch, things()).assert_ok("as it was");
    assert_eq!(
        tables
            .read(&thing("by-slug"), &[json!("eight888")])
            .map(|r| r["n"].clone()),
        Ok(json!(1))
    );
}

/// **An older build never takes a newer database back**, even where its only
/// change from the schema recorded would drop an index, which loses no row:
/// its migration is not from the schema recorded, so only what adds is made.
#[test]
fn an_older_build_that_would_only_drop_an_index_is_refused() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "older");
    drop(seeded(&scratch));
    let (newer, _) = things_where(
        |t| {
            t.replace(
                "    refers  owner to Owners
",
                "    refers  owner to Owners
    index   by_rank on rank
",
            )
        },
        Some(&released()),
    );
    drop(open(&scratch, newer).assert_ok("the newer build, its index made"));
    let older = open(&scratch, things()).err();
    assert!(matches!(older, Some(Error::Migration(_))), "{older:?}");
    let why = format!("{older:?}");
    assert!(why.contains("Things__by_rank"), "{why}");
}

/// **A key field retyped is refused, its key kept** (the review of
/// 2026-11-02): a `backfilled with` stated for a field of the key would
/// drop the column, and with it the primary key, and give every row one
/// key; the program states a new table for that, never a retype.
#[test]
fn a_key_field_retyped_is_refused_and_its_key_kept() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "rekeyed");
    drop(seeded(&scratch));
    let (retyped, _) = things_where(
        |t| {
            t.replace("    n: Int,\n", "    n: String,\n").replace(
                "    refers  owner to Owners\n",
                "    refers  owner to Owners\n    migrate n backfilled with \"x\"\n",
            )
        },
        Some(&released()),
    );
    let refused = open(&scratch, retyped).err();
    assert!(
        matches!(refused, Some(Error::Migration(_))),
        "not refused: {refused:?}"
    );
    let why = format!("{refused:?}");
    assert!(why.contains("Things.n"), "{why}");
    // Nothing of it kept: the build released opens, each row by its key.
    let tables = open(&scratch, things()).assert_ok("as it was");
    assert_eq!(
        tables
            .read(&thing("get"), &[json!("a"), json!(2)])
            .map(|r| r["slug"].clone()),
        Ok(json!("two"))
    );
}

/// The advisory lock's key the adapter takes for `schema`'s migration:
/// FNV-1a over its text (`migrate::lock_key`).
fn migration_lock(schema: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("pw_tables_migrations:{schema}").as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    i64::from_ne_bytes(h.to_ne_bytes())
}

/// **Hosts racing for an empty schema each open it, where every
/// transaction is serializable by default** (the review of 2026-11-02): a
/// transaction at repeatable read or serializable takes its snapshot at its
/// first statement, the lock's own, before the lock is granted; the
/// migration reads at read committed, so the host after the first sees what
/// the first made. The lock is held here until every host waits for it.
#[test]
fn hosts_racing_under_a_serializable_default_each_open_it_made_once() {
    let Some(url) = database() else { return };
    let scratch = Arc::new(Scratch::new(&url, "racing"));
    let serial = Arc::new(with_setting(
        &named(&url, &scratch.schema),
        "default_transaction_isolation",
        "serializable",
    ));
    let key = migration_lock(&scratch.schema);
    let mut holder = postgres::Client::connect(&url, postgres::NoTls).assert_ok("the holder");
    holder
        .query("SELECT pg_advisory_lock($1)", &[&key])
        .assert_ok("the lock held");
    let hosts: Vec<_> = (0..4)
        .map(|_| {
            let scratch = Arc::clone(&scratch);
            let serial = Arc::clone(&serial);
            std::thread::spawn(move || {
                PgTables::open(&serial, scratch.schema(), things()).map(|_| ())
            })
        })
        .collect();
    let waited = until(|| waiting_on_advisory(&scratch, &scratch.schema) == 4);
    holder
        .query("SELECT pg_advisory_unlock($1)", &[&key])
        .assert_ok("the lock given up");
    for host in hosts {
        let joined = host.join();
        assert!(matches!(joined, Ok(Ok(()))), "{joined:?}");
    }
    assert!(waited, "every host waited for the migration's lock");
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema").len(), 1);
}

/// **Two objects a declaration would give one name are refused at open**
/// (the review of 2026-11-02): an object's name is `<table>__<what>`, and
/// a name may hold `__`, so an index `by_rank__check` is named as the check
/// of a field `by_rank`; the schema's objects are kept by name, and one
/// would silently replace the other (here the check, never made).
#[test]
fn a_name_two_objects_would_share_is_refused_at_open() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "names");
    let (manifest, _) = things_where(
        |t| {
            t.replace(
                "    count: Option<Positive>,\n",
                "    count: Option<Positive>,\n    by_rank: Rank,\n",
            )
            .replace(
                "    refers  owner to Owners\n",
                "    refers  owner to Owners\n    index   by_rank__check on rank\n",
            )
        },
        None,
    );
    let opened = open(&scratch, manifest).err();
    assert!(
        matches!(&opened, Some(Error::Schema(why)) if why.contains("Things__by_rank__check")),
        "{opened:?}"
    );
    assert!(
        scratch.texts("SELECT hash FROM pw_schema").len() <= 1,
        "nothing recorded"
    );
}

/// **A backfill is the value stated, whatever the server's string syntax**
/// (the review of 2026-11-02): with `standard_conforming_strings` off, a
/// backslash in an ordinary string literal is an escape, so a value written
/// `'\x6162'` would be stored as `a62`; and the least `Int` is no negated
/// literal (`-9223372036854775808::bigint` is out of range).
#[test]
fn a_backfill_is_the_value_stated_whatever_the_servers_string_syntax() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "backfill");
    drop(seeded(&scratch));
    let (mut added, _) = things_where(
        |t| {
            t.replace(
                "    count: Option<Positive>,\n",
                "    count: Option<Positive>,\n    title: String,\n    least: Int,\n",
            )
            .replace(
                "    refers  owner to Owners\n",
                "    refers  owner to Owners\n    migrate title backfilled with \"ab\"\n    migrate least backfilled with 0\n",
            )
        },
        Some(&released()),
    );
    // The least `Int` as its backfill (written into the plan, as a program
    // stating it would have it).
    let mut found = false;
    for step in &mut added.migration.steps {
        if let pw_core::tables::Step::AddField {
            field,
            backfill: Some(value),
            ..
        } = step
            && field == "least"
        {
            *value = pw_core::tables::Literal::Int(i64::MIN);
            found = true;
        }
    }
    assert!(found, "{:?}", added.migration.steps);
    let off = with_setting(&scratch.url, "standard_conforming_strings", "off");
    let opened = PgTables::open(&off, scratch.schema(), added);
    assert!(
        opened.is_ok(),
        "the backfills made: {:?}",
        opened.as_ref().err()
    );
    let tables = opened.assert_ok("opened");
    let got = tables
        .read(&thing("get"), &[json!("a"), json!(1)])
        .assert_ok("a row, backfilled");
    assert_eq!(got["title"], json!("ab"));
    assert_eq!(got["least"], json!(i64::MIN));
}
