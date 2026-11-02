use pw_core::tables::{
    FieldSchema, FunctionJson, IndexSchema, Plan, Primitive, Schema, Step, Stored, TableJson,
    TableSchema, TablesJson,
};
#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use pw_table_memory::{Error, MemoryTables};
use serde_json::{Value, json};

fn field(name: &str, stored: Stored) -> FieldSchema {
    FieldSchema {
        name: name.into(),
        ty: name.into(),
        stored,
    }
}

fn function(name: &str, primitive: Primitive, by: &[&str], sorted_by: &[&str]) -> FunctionJson {
    FunctionJson {
        name: name.into(),
        import: format!("pw:tables/likes#{}", name.replace('_', "-")),
        primitive,
        effect: if primitive.writes() { "write" } else { "read" }.into(),
        by: by.iter().map(|name| (*name).into()).collect(),
        sorted_by: sorted_by.iter().map(|name| (*name).into()).collect(),
        params: Vec::new(),
        returns: String::new(),
    }
}

fn manifest() -> TablesJson {
    let short_string = || Stored::String {
        min_length: None,
        max_length: Some(3),
    };
    let table = TableSchema {
        name: "Likes".into(),
        source: None,
        fields: vec![
            field("from", short_string()),
            field("to", short_string()),
            field(
                "rank",
                Stored::Int {
                    at_least: Some(0),
                    at_most: Some(9),
                },
            ),
        ],
        key: vec!["from".into(), "to".into()],
        order: Some("rank".into()),
        indexes: vec![IndexSchema {
            name: "by_rank".into(),
            fields: vec!["rank".into()],
            unique: true,
        }],
        references: Vec::new(),
    };
    let hash = Schema::of(vec![table.clone()]).hash;
    TablesJson {
        hash: hash.clone(),
        tables: vec![TableJson {
            schema: table,
            module: "Likes".into(),
            interface: "pw:tables/likes".into(),
            row: "Like".into(),
            bounded: Some(10),
            functions: vec![
                function("get", Primitive::Get, &["from", "to"], &[]),
                function(
                    "get_many",
                    Primitive::GetMany,
                    &["from", "to"],
                    &["from", "to", "rank"],
                ),
                function("by_rank", Primitive::Index, &["rank"], &[]),
                function(
                    "of_from_many",
                    Primitive::PrefixMany,
                    &["from"],
                    &["from", "rank", "to"],
                ),
                function("insert", Primitive::Insert, &[], &[]),
            ],
        }],
        migration: Plan {
            from: None,
            to: hash,
            steps: vec![Step::CreateTable {
                table: "Likes".into(),
            }],
        },
    }
}

fn seeded() -> MemoryTables {
    let db = MemoryTables::new(manifest()).assert_ok("valid domain result");
    db.transaction(|tx| {
        tx.write(
            "pw:tables/likes#insert",
            &[json!({"from": "a", "to": "b", "rank": 3})],
        )?;
        tx.write(
            "pw:tables/likes#insert",
            &[json!({"from": "c", "to": "d", "rank": 4})],
        )?;
        Ok(())
    })
    .assert_ok("valid domain result");
    db
}

fn rejected(db: &MemoryTables, operation: &str, args: &[Value]) {
    assert!(
        matches!(
            db.read(operation, args),
            Err(Error::Input(reason)) if reason.contains("selector") && reason.contains("type or bound")
        ),
        "{operation} accepted a selector that its declared schema refuses: {args:?}"
    );
}

#[test]
fn scalar_reads_refuse_wrong_types_and_schema_bounds_instead_of_missing() {
    let db = seeded();
    let get = "pw:tables/likes#get";
    rejected(&db, get, &[json!(true), json!("b")]);
    rejected(&db, get, &[json!("a"), json!(3)]);
    rejected(&db, get, &[json!("long"), json!("b")]);
    rejected(&db, "pw:tables/likes#by-rank", &[json!("3")]);
    rejected(&db, "pw:tables/likes#by-rank", &[json!(10)]);

    // A valid key that is absent still returns none. A present key is unchanged.
    assert_eq!(db.read(get, &[json!("a"), json!("z")]), Ok(Value::Null));
    assert_eq!(
        db.read(get, &[json!("a"), json!("b")]),
        Ok(json!({"from": "a", "to": "b", "rank": 3}))
    );
    assert_eq!(
        db.read("pw:tables/likes#by-rank", &[json!(3)]),
        Ok(json!({"from": "a", "to": "b", "rank": 3}))
    );
}

#[test]
fn set_reads_refuse_one_bad_member_without_losing_valid_neighbors() {
    let db = seeded();
    let many = "pw:tables/likes#get-many";
    rejected(
        &db,
        many,
        &[json!([
            {"from": "a", "to": "b"},
            {"from": false, "to": "d"},
        ])],
    );
    rejected(&db, many, &[json!([{"from": "a", "to": "long"}])]);
    rejected(&db, "pw:tables/likes#of-from-many", &[json!(["a", 1])]);

    assert_eq!(
        db.read(
            many,
            &[json!([
                {"from": "c", "to": "d"},
                {"from": "a", "to": "b"},
                {"from": "c", "to": "d"},
            ])],
        ),
        Ok(json!([
            {"from": "a", "to": "b", "rank": 3},
            {"from": "c", "to": "d", "rank": 4},
        ]))
    );
    assert_eq!(db.read(many, &[json!([])]), Ok(json!([])));
}
