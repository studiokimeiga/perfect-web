//! **Every primitive on PostgreSQL** (ADR-0338 D3): what each answers, in
//! tables.json's order, each row exactly its table's; what each refuses,
//! and how; and what each costs, one prepared statement, at two data sizes
//! (ADR-0336's way, ADR-0338 5.5's part 4).

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use common::*;
use pw_table_postgres::{Error, PgTables};
use serde_json::{Value, json};

/// The `(owner, n)` of each row a read answered, in its order.
fn keys(answer: &Value) -> Vec<(String, i64)> {
    answer
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    (
                        row["owner"].as_str().unwrap_or_default().to_string(),
                        row["n"].as_i64().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(list: &[(&str, i64)]) -> Vec<(String, i64)> {
    list.iter().map(|(o, n)| (o.to_string(), *n)).collect()
}

/// Owners `ids`, and for each the things `(n, rank, slug, flag)`, written in
/// one command of the adapter's own.
fn seed(tables: &PgTables, ids: &[&str], things: &[(&str, i64, i64, &str, bool)]) {
    tables
        .transaction(|tx| {
            for id in ids {
                tx.write(&owner("insert"), &[owner_row(id)])?;
            }
            for (o, n, rank, slug, flag) in things {
                tx.write(&thing("insert"), &[thing_row(o, *n, *rank, slug, *flag)])?;
            }
            Ok(())
        })
        .assert_ok("seeded");
}

#[test]
fn the_compilers_functions_are_each_served() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "functions");
    let manifest = things();
    let mut imports: Vec<String> = manifest
        .tables
        .iter()
        .flat_map(|t| t.functions.iter().map(|f| f.import.clone()))
        .collect();
    imports.sort();
    assert_eq!(
        imports,
        [
            "pw:tables/owners#all",
            "pw:tables/owners#delete",
            "pw:tables/owners#get",
            "pw:tables/owners#get-many",
            "pw:tables/owners#insert",
            "pw:tables/owners#put",
            "pw:tables/things#by-flag",
            "pw:tables/things#by-flag-many",
            "pw:tables/things#by-owner-rank",
            "pw:tables/things#by-owner-rank-many",
            "pw:tables/things#by-slug",
            "pw:tables/things#by-slug-many",
            "pw:tables/things#delete",
            "pw:tables/things#delete-of-owner",
            "pw:tables/things#get",
            "pw:tables/things#get-many",
            "pw:tables/things#insert",
            "pw:tables/things#of-owner",
            "pw:tables/things#of-owner-many",
            "pw:tables/things#put",
        ]
    );
    let tables = open_things(&scratch);
    seed(&tables, &["a"], &[("a", 1, 5, "one", true)]);
    // Every read answers; none is refused as undeclared.
    for import in imports.iter().filter(|i| {
        let f = i.rsplit('#').next().unwrap_or_default();
        !matches!(f, "insert" | "put" | "delete" | "delete-of-owner")
    }) {
        let f = import.rsplit('#').next().unwrap_or_default();
        let args: Vec<Value> = match f {
            "all" => vec![],
            "get" if import.starts_with(OWNER) => vec![json!("a")],
            "get-many" if import.starts_with(OWNER) => vec![json!(["a"])],
            "get" => vec![json!("a"), json!(1)],
            "get-many" => vec![json!([{"owner": "a", "n": 1}])],
            "of-owner" => vec![json!("a")],
            "of-owner-many" => vec![json!(["a"])],
            "by-slug" => vec![json!("one")],
            "by-slug-many" => vec![json!(["one"])],
            "by-flag" => vec![json!(true)],
            "by-flag-many" => vec![json!([true])],
            "by-owner-rank" => vec![json!("a"), json!(5)],
            _ => vec![json!([{"owner": "a", "rank": 5}])],
        };
        let answer = tables.read(import, &args);
        assert!(answer.is_ok(), "{import}: {answer:?}");
    }
}

#[test]
fn a_row_is_answered_exactly_as_it_was_written() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "exact");
    let tables = open_things(&scratch);
    // Every kind of field, at its edges: a string holding U+0000 and
    // characters of every UTF-8 width, a negative zero, a float's least
    // positive value and its largest, nested ones, an empty list, a unit.
    let row = json!({
        "owner": "a\u{0}b", "n": i64::MIN, "rank": 1000, "slug": "é漢😀\u{0}",
        "flag": false, "score": -0.0, "note": "\u{0}", "tags": ["", "\u{0}", "😀"],
        "point": {"x": -0.0, "y": i64::MAX}, "nothing": null, "maybe": [],
        "count": 1,
    });
    let other = json!({
        "owner": "a\u{0}b", "n": 2, "rank": 0, "slug": "", "flag": true,
        "score": 5e-324, "note": null, "tags": [], "point": {"x": 1.7976931348623157e308, "y": -1},
        "nothing": null, "maybe": null, "count": null,
    });
    tables
        .transaction(|tx| {
            tx.write(&owner("insert"), &[json!({"id": "a\u{0}b", "name": ""})])?;
            tx.write(&thing("insert"), &[row.clone()])?;
            tx.write(&thing("insert"), &[other.clone()])
        })
        .assert_ok("written");
    let got = tables
        .read(&thing("get"), &[json!("a\u{0}b"), json!(i64::MIN)])
        .assert_ok("read back");
    // The JSON text is the row's, a negative zero's sign kept, nested too.
    assert_eq!(got.to_string(), row.to_string());
    let got = tables
        .read(&thing("get"), &[json!("a\u{0}b"), json!(2)])
        .assert_ok("read back");
    assert_eq!(got.to_string(), other.to_string());
    // Exactly its fields: none more, none less.
    let fields: Vec<&String> = got.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    assert_eq!(fields.len(), 12);
    // A row with a key no row has: none.
    assert_eq!(
        tables.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(Value::Null)
    );
}

#[test]
fn reads_come_in_tables_json_order_strings_by_their_bytes() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "order");
    let tables = open_things(&scratch);
    // Code point order: "B" < "a" < "z" < "é" (a collation would put "a"
    // first, and "é" before "z").
    seed(
        &tables,
        &["z", "é", "a", "B"],
        &[
            ("a", 3, 20, "s1", true),
            ("a", 1, 30, "s2", false),
            ("a", 2, 20, "s3", true),
            ("é", 1, 10, "s4", true),
            ("z", 9, 10, "s5", false),
            ("B", 4, 20, "s6", true),
        ],
    );
    let all = tables.read(&owner("all"), &[]).assert_ok("all");
    let ids: Vec<&str> = all
        .as_array()
        .map(|rows| rows.iter().filter_map(|r| r["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(ids, ["B", "a", "z", "é"]);
    // A prefix's rows: by the order (rank), then the key.
    let of_a = tables.read(&thing("of-owner"), &[json!("a")]).assert_ok("of a");
    assert_eq!(keys(&of_a), pairs(&[("a", 2), ("a", 3), ("a", 1)]));
    // Its set form: grouped by what it was given, then the order, the key.
    let of_many = tables
        .read(&thing("of-owner-many"), &[json!(["é", "a", "B", "a"])])
        .assert_ok("of many");
    assert_eq!(
        keys(&of_many),
        pairs(&[("B", 4), ("a", 2), ("a", 3), ("a", 1), ("é", 1)])
    );
    // By keys: grouped by the key.
    let got = tables
        .read(
            &thing("get-many"),
            &[json!([{"owner": "z", "n": 9}, {"owner": "a", "n": 3}, {"owner": "a", "n": 3}, {"owner": "q", "n": 1}])],
        )
        .assert_ok("get many");
    assert_eq!(keys(&got), pairs(&[("a", 3), ("z", 9)]));
    // An index: its value's rows by the order, then the key.
    let flagged = tables.read(&thing("by-flag"), &[json!(true)]).assert_ok("flag");
    assert_eq!(keys(&flagged), pairs(&[("é", 1), ("B", 4), ("a", 2), ("a", 3)]));
    let both = tables
        .read(&thing("by-flag-many"), &[json!([true, false])])
        .assert_ok("flags");
    assert_eq!(
        keys(&both),
        pairs(&[("z", 9), ("a", 1), ("é", 1), ("B", 4), ("a", 2), ("a", 3)])
    );
    // A composite index and its set form.
    let at = tables
        .read(&thing("by-owner-rank"), &[json!("a"), json!(20)])
        .assert_ok("owner rank");
    assert_eq!(keys(&at), pairs(&[("a", 2), ("a", 3)]));
    let many = tables
        .read(
            &thing("by-owner-rank-many"),
            &[json!([{"owner": "a", "rank": 30}, {"owner": "B", "rank": 20}])],
        )
        .assert_ok("owner ranks");
    assert_eq!(keys(&many), pairs(&[("B", 4), ("a", 1)]));
    // A unique index: one row, or none.
    let one = tables.read(&thing("by-slug"), &[json!("s5")]).assert_ok("slug");
    assert_eq!(one["owner"], json!("z"));
    assert_eq!(
        tables.read(&thing("by-slug"), &[json!("none")]),
        Ok(Value::Null)
    );
    let slugs = tables
        .read(&thing("by-slug-many"), &[json!(["s6", "s1", "x"])])
        .assert_ok("slugs");
    assert_eq!(keys(&slugs), pairs(&[("a", 3), ("B", 4)]));
    // A set read given nothing answers nothing.
    assert_eq!(
        tables.read(&thing("get-many"), &[json!([])]),
        Ok(json!([]))
    );
}

#[test]
fn a_write_is_refused_by_the_row_holding_its_key_then_a_unique_value() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "refused");
    let tables = open_things(&scratch);
    seed(
        &tables,
        &["a", "b"],
        &[("a", 1, 5, "one", true), ("b", 1, 5, "two", true)],
    );
    let mut tx = tables.begin().assert_ok("a command");
    // An insert of a held key: the key's row, though another holds its slug.
    let refused = tx.write(&thing("insert"), &[thing_row("a", 1, 9, "two", false)]);
    assert_eq!(
        refused,
        Err(Error::Conflict {
            table: "Things".into(),
            existing: thing_row("a", 1, 5, "one", true),
        })
    );
    // A new key holding another row's slug: that row.
    let refused = tx.write(&thing("insert"), &[thing_row("a", 7, 9, "two", false)]);
    assert_eq!(
        refused,
        Err(Error::Conflict {
            table: "Things".into(),
            existing: thing_row("b", 1, 5, "two", true),
        })
    );
    // A put refused by another row's slug, never by its own.
    let refused = tx.write(&thing("put"), &[thing_row("a", 1, 6, "two", true)]);
    assert_eq!(
        refused,
        Err(Error::Conflict {
            table: "Things".into(),
            existing: thing_row("b", 1, 5, "two", true),
        })
    );
    tx.write(&thing("put"), &[thing_row("a", 1, 6, "one", false)])
        .assert_ok("its own slug, kept");
    // The transaction is whole after each refusal: the put is read back.
    let got = tx.read(&thing("get"), &[json!("a"), json!(1)]).assert_ok("read");
    assert_eq!(got, thing_row("a", 1, 6, "one", false));
    tx.commit().assert_ok("committed");
    assert_eq!(
        tables.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(thing_row("a", 1, 6, "one", false))
    );
}

#[test]
fn a_bounded_table_grows_no_further_than_its_bound() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "bound");
    let tables = open_things(&scratch);
    seed(&tables, &["a", "b", "c", "d", "e"], &[]);
    let mut tx = tables.begin().assert_ok("a command");
    let past = tx.write(&owner("insert"), &[owner_row("f")]);
    assert_eq!(
        past,
        Err(Error::Input(
            "Owners cannot grow past its declared bound of 5".into()
        ))
    );
    let past = tx.write(&owner("put"), &[owner_row("f")]);
    assert!(matches!(past, Err(Error::Input(_))), "{past:?}");
    // A row replaced at the bound is no growth; one deleted makes room.
    tx.write(&owner("put"), &[json!({"id": "a", "name": "renamed"})])
        .assert_ok("replaced at the bound");
    tx.write(&owner("delete"), &[json!("b")]).assert_ok("deleted");
    tx.write(&owner("insert"), &[owner_row("f")]).assert_ok("room made");
    let all = tx.read(&owner("all"), &[]).assert_ok("all");
    assert_eq!(all.as_array().map(Vec::len), Some(5));
    tx.commit().assert_ok("committed");
}

#[test]
fn a_delete_deletes_its_key_and_a_prefix_its_rows_alone() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "delete");
    let tables = open_things(&scratch);
    seed(
        &tables,
        &["a", "ab", "b"],
        &[
            ("a", 1, 1, "s1", true),
            ("a", 2, 2, "s2", true),
            ("ab", 1, 3, "s3", true),
            ("b", 1, 4, "s4", true),
        ],
    );
    tables
        .transaction(|tx| {
            tx.write(&thing("delete"), &[json!("b"), json!(1)])?;
            tx.write(&thing("delete"), &[json!("b"), json!(1)])?;
            tx.write(&thing("delete-of-owner"), &[json!("a")])
        })
        .assert_ok("deleted");
    let left = tables.read(&thing("by-flag"), &[json!(true)]).assert_ok("left");
    assert_eq!(keys(&left), pairs(&[("ab", 1)]));
}

#[test]
fn what_a_function_is_given_is_held_before_the_database_sees_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "held");
    let tables = open_things(&scratch);
    seed(&tables, &["a"], &[("a", 1, 5, "one", true)]);
    let before = tables.statements();
    let wrong = [
        (thing("get"), vec![json!("a")]),
        (thing("get"), vec![json!("a"), json!("1")]),
        (thing("get"), vec![json!(true), json!(1)]),
        (thing("get-many"), vec![json!({"owner": "a", "n": 1})]),
        (thing("get-many"), vec![json!([{"owner": "a"}])]),
        (thing("by-slug"), vec![json!("ninechars")]),
        (thing("of-owner"), vec![json!(1.5)]),
        (owner("get"), vec![json!("")]),
        (owner("all"), vec![json!(1)]),
        (thing("insert"), vec![]),
        ("pw:tables/things#nothing".to_string(), vec![]),
    ];
    for (import, args) in &wrong {
        let answer = tables.read(import, args);
        assert!(
            matches!(answer, Err(Error::Input(_))),
            "{import} {args:?}: {answer:?}"
        );
    }
    let mut tx = tables.begin().assert_ok("a command");
    let rows = [
        json!({"owner": "a"}),
        json!([]),
        {
            let mut r = thing_row("a", 9, 5, "x", true);
            r["rank"] = json!(1001);
            r
        },
        {
            let mut r = thing_row("a", 9, 5, "x", true);
            r["score"] = json!(null);
            r
        },
        {
            let mut r = thing_row("a", 9, 5, "x", true);
            r["count"] = json!(0);
            r
        },
        {
            let mut r = thing_row("a", 9, 5, "x", true);
            r["point"] = json!({"x": 1.0});
            r
        },
        {
            let mut r = thing_row("a", 9, 5, "x", true);
            r["extra"] = json!(1);
            r
        },
    ];
    for row in &rows {
        let answer = tx.write(&thing("insert"), std::slice::from_ref(row));
        assert!(matches!(answer, Err(Error::Input(_))), "{row}: {answer:?}");
    }
    let answer = tx.write(&thing("of-owner"), &[json!("a")]);
    assert!(matches!(answer, Err(Error::Input(_))), "{answer:?}");
    drop(tx);
    // Not one of them reached the database.
    assert_eq!(tables.statements(), before);
}

#[test]
fn a_refinement_is_held_by_its_check_whoever_writes() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "check");
    let tables = open_things(&scratch);
    seed(&tables, &["a"], &[("a", 1, 5, "eight888", true)]);
    let insert = |rank: i64, slug: &str, count: &str, score: &str| {
        scratch.sql(&format!(
            "INSERT INTO \"Things\" (owner, n, rank, slug, flag, score, note, tags, point, nothing, maybe, count) \
             VALUES ('\\x61'::bytea, {rank}, {rank}, convert_to('{slug}', 'UTF8'), true, {score}, NULL, '[]', \
             '{{\"x\": 1.0, \"y\": 1}}', 'null', NULL, {count})"
        ))
    };
    // Another writer of the database meets the declaration's bounds.
    assert!(insert(1001, "x", "NULL", "1.0").is_err(), "a rank past 1000");
    assert!(insert(-1, "x", "NULL", "1.0").is_err(), "a rank below 0");
    assert!(insert(7, "x", "0", "1.0").is_err(), "a count below 1");
    assert!(insert(8, "nine99999", "NULL", "1.0").is_err(), "a slug of nine");
    assert!(insert(9, "x", "NULL", "'NaN'").is_err(), "a float not finite");
    // Eight code points of sixteen bytes are eight.
    insert(10, "éééééééé", "NULL", "1.0").assert_ok("eight code points");
    assert!(insert(11, "ééééééééé", "NULL", "1.0").is_err(), "nine code points");
    let json_shapes = scratch.sql(
        "INSERT INTO \"Things\" (owner, n, rank, slug, flag, score, note, tags, point, nothing, maybe, count) \
         VALUES ('\\x61'::bytea, 12, 1, '\\x'::bytea, true, 1.0, NULL, '{}', '{}', 'null', NULL, NULL)",
    );
    assert!(json_shapes.is_err(), "a list held as an object");
    assert_eq!(
        tables
            .read(&thing("by-slug"), &[json!("éééééééé")])
            .map(|r| r["rank"].clone()),
        Ok(json!(10))
    );
}

#[test]
fn a_row_another_writer_left_out_of_its_type_is_refused_not_answered() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "foreign");
    let tables = open_things(&scratch);
    seed(&tables, &["a"], &[]);
    // A list of strings holding a number: no check reaches inside a list.
    scratch
        .sql(
            "INSERT INTO \"Things\" (owner, n, rank, slug, flag, score, note, tags, point, nothing, maybe, count) \
             VALUES ('\\x61'::bytea, 1, 1, '\\x'::bytea, true, 1.0, NULL, '[1]', '{\"x\": 1.0, \"y\": 1}', 'null', NULL, NULL)",
        )
        .assert_ok("written by another writer");
    let answer = tables.read(&thing("get"), &[json!("a"), json!(1)]);
    assert!(matches!(answer, Err(Error::Schema(_))), "{answer:?}");
}

/// **Each primitive's statements, at two sizes** (ADR-0336's pins): one,
/// whatever the table holds and however many keys a set read is given.
#[test]
fn each_primitive_is_one_statement_at_two_sizes() {
    let Some(url) = database() else { return };
    let mut pinned: Vec<Vec<(String, u64)>> = Vec::new();
    for size in [4_i64, 400] {
        let scratch = Scratch::new(&url, "costs");
        let tables = open_things(&scratch);
        let owners = ["a", "b", "c", "d", "e"];
        let rows: Vec<(String, i64)> = (0..size)
            .map(|i| (owners[(i % 5) as usize].to_string(), i))
            .collect();
        tables
            .transaction(|tx| {
                for id in owners {
                    tx.write(&owner("insert"), &[owner_row(id)])?;
                }
                for (o, n) in &rows {
                    tx.write(
                        &thing("insert"),
                        &[thing_row(o, *n, n % 7, &format!("s{n}"), n % 2 == 0)],
                    )?;
                }
                Ok(())
            })
            .assert_ok("seeded");
        let keys: Vec<Value> = rows
            .iter()
            .map(|(o, n)| json!({"owner": o, "n": n}))
            .collect();
        let slugs: Vec<Value> = rows.iter().map(|(_, n)| json!(format!("s{n}"))).collect();
        let calls: Vec<(String, Vec<Value>)> = vec![
            (owner("get"), vec![json!("a")]),
            (owner("get-many"), vec![json!(owners)]),
            (owner("all"), vec![]),
            (thing("get"), vec![json!("a"), json!(0)]),
            (thing("get-many"), vec![json!([keys[0]])]),
            (thing("get-many"), vec![Value::Array(keys.clone())]),
            (thing("of-owner"), vec![json!("a")]),
            (thing("of-owner-many"), vec![json!(owners)]),
            (thing("by-slug"), vec![json!("s1")]),
            (thing("by-slug-many"), vec![Value::Array(slugs.clone())]),
            (thing("by-flag"), vec![json!(true)]),
            (thing("by-flag-many"), vec![json!([true, false])]),
            (thing("by-owner-rank"), vec![json!("a"), json!(0)]),
            (thing("by-owner-rank-many"), vec![json!([{"owner": "a", "rank": 0}, {"owner": "b", "rank": 1}])]),
        ];
        let mut counts = Vec::new();
        for (import, args) in &calls {
            let before = tables.statements();
            tables.read(import, args).assert_ok("read");
            counts.push((format!("{import} {}", args.len()), tables.statements() - before));
        }
        let mut tx = tables.begin().assert_ok("a command");
        tx.read(&owner("get"), &[json!("a")]).assert_ok("the transaction begun");
        let writes: Vec<(String, Vec<Value>)> = vec![
            (thing("insert"), vec![thing_row("a", -1, 1, "new", true)]),
            (thing("insert"), vec![thing_row("a", -1, 1, "new", true)]),
            (thing("put"), vec![thing_row("a", -1, 2, "new", true)]),
            (thing("put"), vec![thing_row("a", -2, 2, "new", true)]),
            (thing("delete"), vec![json!("a"), json!(-1)]),
            (thing("delete-of-owner"), vec![json!("b")]),
            (owner("put"), vec![owner_row("a")]),
            (owner("insert"), vec![owner_row("f")]),
        ];
        for (import, args) in &writes {
            let before = tables.statements();
            let _ = tx.write(import, args);
            counts.push((format!("{import} {}", args.len()), tables.statements() - before));
        }
        drop(tx);
        pinned.push(counts);
    }
    for (at_four, at_four_hundred) in pinned[0].iter().zip(&pinned[1]) {
        assert_eq!(at_four.1, 1, "{}: at four rows", at_four.0);
        assert_eq!(at_four_hundred.1, 1, "{}: at four hundred rows", at_four_hundred.0);
    }
    assert_eq!(pinned[0].len(), 22);
}

#[test]
fn a_statement_is_prepared_once_on_a_connection_never_per_call() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "prepared");
    let options = pw_table_postgres::Options {
        connections: 1,
        ..Default::default()
    };
    let tables = PgTables::open_with(&url, scratch.schema(), things(), options).assert_ok("one connection");
    seed(&tables, &["a"], &[("a", 1, 5, "one", true)]);
    tables.read(&thing("get"), &[json!("a"), json!(1)]).assert_ok("first");
    let prepared = tables.prepared();
    for n in 0..20 {
        tables.read(&thing("get"), &[json!("a"), json!(n)]).assert_ok("again");
        tables.read(&thing("of-owner"), &[json!("a")]).assert_ok("another");
    }
    // `of-owner` prepared once, `get` not again.
    assert_eq!(tables.prepared(), prepared + 1);
}
