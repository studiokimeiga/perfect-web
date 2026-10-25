//! **A command's writes, staged** (ADR-0386): the memory adapter's
//! transactions, by its own API and by the host functions it gives a
//! command, over two tables: `Users`, bounded, keyed by `id`, unique by
//! `name`; and `Posts`, keyed by `author` then `id`, ordered by `rank`,
//! indexed by `draft`, its `author` referring to `Users`.

use std::sync::Arc;
use std::sync::mpsc;

use pw_core::tables::{
    FieldSchema, FunctionJson, IndexSchema, Plan, Primitive, ReferenceSchema, Schema, Step, Stored,
    TableJson, TableSchema, TablesJson,
};
use pw_host::engine::Val;
use pw_table_memory::{Committed, Error, MemoryTables};
use serde_json::{Value, json};

fn field(name: &str, stored: Stored) -> FieldSchema {
    FieldSchema {
        name: name.into(),
        ty: name.into(),
        stored,
    }
}

fn string() -> Stored {
    Stored::String {
        min_length: None,
        max_length: None,
    }
}

fn function(
    table: &str,
    name: &str,
    primitive: Primitive,
    by: &[&str],
    sorted_by: &[&str],
) -> FunctionJson {
    FunctionJson {
        name: name.into(),
        import: format!("pw:tables/{table}#{}", name.replace('_', "-")),
        primitive,
        effect: if primitive.writes() { "write" } else { "read" }.into(),
        by: by.iter().map(|s| (*s).into()).collect(),
        sorted_by: sorted_by.iter().map(|s| (*s).into()).collect(),
        params: Vec::new(),
        returns: String::new(),
    }
}

/// `Users` at most `bound` rows, and their `Posts`.
fn manifest(bound: u64) -> TablesJson {
    let users = TableSchema {
        name: "Users".into(),
        source: None,
        fields: vec![field("id", string()), field("name", string())],
        key: vec!["id".into()],
        order: None,
        indexes: vec![IndexSchema {
            name: "by_name".into(),
            fields: vec!["name".into()],
            unique: true,
        }],
        references: vec![],
    };
    let posts = TableSchema {
        name: "Posts".into(),
        source: None,
        fields: vec![
            field("author", string()),
            field("id", string()),
            field(
                "rank",
                Stored::Int {
                    at_least: None,
                    at_most: None,
                },
            ),
            field("draft", Stored::Bool),
        ],
        key: vec!["author".into(), "id".into()],
        order: Some("rank".into()),
        indexes: vec![IndexSchema {
            name: "by_draft".into(),
            fields: vec!["draft".into()],
            unique: false,
        }],
        references: vec![ReferenceSchema {
            field: "author".into(),
            table: "Users".into(),
        }],
    };
    let hash = Schema::of(vec![users.clone(), posts.clone()]).hash;
    let u = |name, primitive, by: &[&str], sorted: &[&str]| {
        function("users", name, primitive, by, sorted)
    };
    let p = |name, primitive, by: &[&str], sorted: &[&str]| {
        function("posts", name, primitive, by, sorted)
    };
    TablesJson {
        hash: hash.clone(),
        tables: vec![
            TableJson {
                schema: posts,
                module: "Posts".into(),
                interface: "pw:tables/posts".into(),
                row: "Post".into(),
                bounded: None,
                functions: vec![
                    p("get", Primitive::Get, &["author", "id"], &[]),
                    p(
                        "get_many",
                        Primitive::GetMany,
                        &["author", "id"],
                        &["author", "id", "rank"],
                    ),
                    p(
                        "of_author",
                        Primitive::Prefix,
                        &["author"],
                        &["rank", "author", "id"],
                    ),
                    p(
                        "of_author_many",
                        Primitive::PrefixMany,
                        &["author"],
                        &["author", "rank", "id"],
                    ),
                    p(
                        "by_draft",
                        Primitive::Index,
                        &["draft"],
                        &["rank", "author", "id"],
                    ),
                    p(
                        "by_draft_many",
                        Primitive::IndexMany,
                        &["draft"],
                        &["draft", "rank", "author", "id"],
                    ),
                    p("insert", Primitive::Insert, &[], &[]),
                    p("put", Primitive::Put, &[], &[]),
                    p("delete", Primitive::Delete, &["author", "id"], &[]),
                    p(
                        "delete_of_author",
                        Primitive::DeletePrefix,
                        &["author"],
                        &[],
                    ),
                ],
            },
            TableJson {
                schema: users,
                module: "Users".into(),
                interface: "pw:tables/users".into(),
                row: "User".into(),
                bounded: Some(bound),
                functions: vec![
                    u("get", Primitive::Get, &["id"], &[]),
                    u("by_name", Primitive::Index, &["name"], &[]),
                    u("all", Primitive::All, &[], &["id"]),
                    u("insert", Primitive::Insert, &[], &[]),
                    u("put", Primitive::Put, &[], &[]),
                    u("delete", Primitive::Delete, &["id"], &[]),
                ],
            },
        ],
        migration: Plan {
            from: None,
            to: hash,
            steps: vec![
                Step::CreateTable {
                    table: "Posts".into(),
                },
                Step::CreateTable {
                    table: "Users".into(),
                },
            ],
        },
    }
}

fn user(id: &str, name: &str) -> Value {
    json!({"id": id, "name": name})
}

fn post(author: &str, id: &str, rank: i64, draft: bool) -> Value {
    json!({"author": author, "id": id, "rank": rank, "draft": draft})
}

const USERS_INSERT: &str = "pw:tables/users#insert";
const USERS_PUT: &str = "pw:tables/users#put";
const USERS_DELETE: &str = "pw:tables/users#delete";
const USERS_ALL: &str = "pw:tables/users#all";
const BY_NAME: &str = "pw:tables/users#by-name";
const POSTS_INSERT: &str = "pw:tables/posts#insert";
const POSTS_PUT: &str = "pw:tables/posts#put";
const POSTS_DELETE: &str = "pw:tables/posts#delete";
const DELETE_OF_AUTHOR: &str = "pw:tables/posts#delete-of-author";
const OF_AUTHOR: &str = "pw:tables/posts#of-author";
const BY_DRAFT: &str = "pw:tables/posts#by-draft";
const BY_DRAFT_MANY: &str = "pw:tables/posts#by-draft-many";

/// Two users; ann's three posts and bo's one, two of them drafts.
fn seeded(bound: u64) -> MemoryTables {
    let db = MemoryTables::new(manifest(bound)).unwrap();
    db.transaction(|tx| {
        tx.write(USERS_INSERT, &[user("ann", "Ann")])?;
        tx.write(USERS_INSERT, &[user("bo", "Bo")])?;
        tx.write(POSTS_INSERT, &[post("ann", "a1", 3, false)])?;
        tx.write(POSTS_INSERT, &[post("ann", "a2", 1, true)])?;
        tx.write(POSTS_INSERT, &[post("ann", "a3", 2, false)])?;
        tx.write(POSTS_INSERT, &[post("bo", "b1", 5, true)])
    })
    .unwrap();
    db
}

/// Every read the tests compare a state by.
fn state(read: impl Fn(&str, &[Value]) -> Result<Value, Error>) -> Vec<Value> {
    vec![
        read(USERS_ALL, &[]).unwrap(),
        read(OF_AUTHOR, &[json!("ann")]).unwrap(),
        read(OF_AUTHOR, &[json!("bo")]).unwrap(),
        read(BY_DRAFT_MANY, &[json!([true, false])]).unwrap(),
        read(BY_NAME, &[json!("Ann")]).unwrap(),
        read(BY_NAME, &[json!("Cy")]).unwrap(),
    ]
}

#[test]
fn a_commands_reads_see_its_writes_and_no_other_read_does() {
    let db = seeded(10);
    let before = state(|i, a| db.read(i, a));
    let mut command = db.begin().unwrap();
    command
        .write(POSTS_PUT, &[post("ann", "a1", 3, true)])
        .unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    assert_eq!(
        command.read(BY_DRAFT, &[json!(true)]).unwrap(),
        json!([
            post("ann", "a2", 1, true),
            post("ann", "a1", 3, true),
            post("bo", "b1", 5, true)
        ]),
        "its own put, through the index it kept"
    );
    assert_eq!(
        command.read(BY_NAME, &[json!("Cy")]).unwrap(),
        user("cy", "Cy")
    );
    assert_eq!(state(|i, a| db.read(i, a)), before, "no other read");
    let committed = command.commit().unwrap();
    assert_eq!(
        committed,
        Committed {
            written: [("Posts".to_string(), 2), ("Users".to_string(), 2)].into()
        },
        "each table it wrote, at the version its commit made"
    );
    assert_eq!(
        db.read(BY_DRAFT, &[json!(true)]).unwrap(),
        json!([
            post("ann", "a2", 1, true),
            post("ann", "a1", 3, true),
            post("bo", "b1", 5, true)
        ])
    );
    assert_eq!(db.read(BY_NAME, &[json!("Cy")]).unwrap(), user("cy", "Cy"));
}

/// A write refused by the row that holds its key or a unique value changes
/// nothing, and the command goes on: what it writes after commits.
#[test]
fn a_refused_write_changes_nothing_and_the_command_goes_on() {
    let db = seeded(10);
    let mut command = db.begin().unwrap();
    let inside = |c: &pw_table_memory::Command| state(|i, a| c.read(i, a));
    let before = inside(&command);
    for (import, row, holder) in [
        (USERS_INSERT, user("ann", "Other"), user("ann", "Ann")),
        (USERS_INSERT, user("cy", "Ann"), user("ann", "Ann")),
        (USERS_PUT, user("bo", "Ann"), user("ann", "Ann")),
        (
            POSTS_INSERT,
            post("ann", "a1", 9, true),
            post("ann", "a1", 3, false),
        ),
    ] {
        assert_eq!(
            command.write(import, &[row]),
            Err(Error::Conflict {
                table: if import.contains("users") {
                    "Users"
                } else {
                    "Posts"
                }
                .into(),
                existing: holder
            })
        );
        assert_eq!(inside(&command), before, "{import}: nothing changed");
    }
    // A put of its own key's row, keeping its unique name, is no conflict.
    command.write(USERS_PUT, &[user("ann", "Ann")]).unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    command.commit().unwrap();
    assert_eq!(db.read(BY_NAME, &[json!("Cy")]).unwrap(), user("cy", "Cy"));
    assert_eq!(
        db.read(BY_NAME, &[json!("Ann")]).unwrap(),
        user("ann", "Ann")
    );
}

/// **Dropped, a command is undone whole**: its puts, inserts, deletes and a
/// prefix's delete, and every index and reference entry they moved; the
/// next command and every read see the tables as they were.
#[test]
fn a_dropped_command_is_undone_whole_its_maps_too() {
    let db = seeded(10);
    let before = state(|i, a| db.read(i, a));
    let mut command = db.begin().unwrap();
    command
        .write(POSTS_PUT, &[post("ann", "a2", 7, false)])
        .unwrap();
    command.write(DELETE_OF_AUTHOR, &[json!("bo")]).unwrap();
    command.write(USERS_DELETE, &[json!("bo")]).unwrap();
    command.write(USERS_PUT, &[user("ann", "Bo")]).unwrap();
    command.write(USERS_INSERT, &[user("cy", "Ann")]).unwrap();
    command
        .write(POSTS_INSERT, &[post("cy", "c1", 0, true)])
        .unwrap();
    command
        .write(POSTS_DELETE, &[json!("ann"), json!("a1")])
        .unwrap();
    assert_ne!(state(|i, a| command.read(i, a)), before);
    drop(command);
    assert_eq!(state(|i, a| db.read(i, a)), before);
    let next = db.begin().unwrap();
    assert_eq!(
        state(|i, a| next.read(i, a)),
        before,
        "the working copy too"
    );
    // Its unique index too: Ann's name is Ann's again, and Bo's Bo's.
    let mut next = next;
    assert!(matches!(
        next.write(USERS_INSERT, &[user("dee", "Bo")]),
        Err(Error::Conflict { existing, .. }) if existing == user("bo", "Bo")
    ));
}

/// **Every index is kept on every write**: a put that moves a row between
/// an index's values, a delete, a prefix's delete; read inside the command
/// and after its commit.
#[test]
fn every_index_is_kept_on_every_write() {
    let db = seeded(10);
    let mut command = db.begin().unwrap();
    command
        .write(POSTS_PUT, &[post("ann", "a1", 3, true)])
        .unwrap();
    command
        .write(POSTS_PUT, &[post("bo", "b1", 5, false)])
        .unwrap();
    command
        .write(POSTS_DELETE, &[json!("ann"), json!("a2")])
        .unwrap();
    let drafts = json!([post("ann", "a1", 3, true)]);
    let published = json!([post("ann", "a3", 2, false), post("bo", "b1", 5, false)]);
    assert_eq!(command.read(BY_DRAFT, &[json!(true)]).unwrap(), drafts);
    assert_eq!(command.read(BY_DRAFT, &[json!(false)]).unwrap(), published);
    command.commit().unwrap();
    assert_eq!(db.read(BY_DRAFT, &[json!(true)]).unwrap(), drafts);
    assert_eq!(db.read(BY_DRAFT, &[json!(false)]).unwrap(), published);
    db.transaction(|tx| tx.write(DELETE_OF_AUTHOR, &[json!("ann")]))
        .unwrap();
    assert_eq!(db.read(BY_DRAFT, &[json!(true)]).unwrap(), json!([]));
    assert_eq!(
        db.read(BY_DRAFT_MANY, &[json!([false, true])]).unwrap(),
        json!([post("bo", "b1", 5, false)])
    );
}

/// **A unique value is held by one row, whatever the writes**: a swap of
/// two names goes through a free one, and each step that would hold a name
/// twice is refused.
#[test]
fn a_unique_value_is_held_by_one_row_through_any_writes() {
    let db = seeded(10);
    db.transaction(|tx| {
        assert!(matches!(
            tx.write(USERS_PUT, &[user("ann", "Bo")]),
            Err(Error::Conflict { .. })
        ));
        tx.write(USERS_PUT, &[user("ann", "Free")])?;
        tx.write(USERS_PUT, &[user("bo", "Ann")])?;
        tx.write(USERS_PUT, &[user("ann", "Bo")])
    })
    .unwrap();
    assert_eq!(
        db.read(BY_NAME, &[json!("Ann")]).unwrap(),
        user("bo", "Ann")
    );
    assert_eq!(db.read(BY_NAME, &[json!("Bo")]).unwrap(), user("ann", "Bo"));
    assert_eq!(db.read(BY_NAME, &[json!("Free")]).unwrap(), Value::Null);
}

/// **The bound is held at every commit, and `all` never answers past it**:
/// inside a command a table may hold more for a while, and its `all` then
/// refuses; committed so, the command is refused whole.
#[test]
fn the_bound_is_held_at_commit_and_all_never_answers_past_it() {
    let db = seeded(2);
    let mut command = db.begin().unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    assert!(matches!(
        command.read(USERS_ALL, &[]),
        Err(Error::Input(why)) if why.contains("more than its bound of 2")
    ));
    // Back within it before the commit: committed.
    command.write(USERS_DELETE, &[json!("cy")]).unwrap();
    command.commit().unwrap();
    let mut command = db.begin().unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    assert!(matches!(
        command.commit(),
        Err(Error::Input(why)) if why.contains("Users exceeds its declared bound")
    ));
    assert_eq!(
        db.read(USERS_ALL, &[]).unwrap(),
        json!([user("ann", "Ann"), user("bo", "Bo")])
    );
}

/// **References are held at commit, both ways**: a post of no user, and a
/// user deleted while a post refers to them, are refused whole; written in
/// either order within one command, a user and their post commit.
#[test]
fn references_are_held_at_commit_both_ways() {
    let db = seeded(10);
    for writes in [
        vec![(POSTS_INSERT, vec![post("cy", "c1", 1, false)])],
        vec![(USERS_DELETE, vec![json!("bo")])],
        vec![
            (USERS_DELETE, vec![json!("ann")]),
            (POSTS_DELETE, vec![json!("ann"), json!("a1")]),
        ],
    ] {
        let mut command = db.begin().unwrap();
        for (import, args) in &writes {
            command.write(import, args).unwrap();
        }
        assert!(
            matches!(command.commit(), Err(Error::Input(why)) if why.contains("references a missing")),
            "{writes:?}"
        );
    }
    let mut command = db.begin().unwrap();
    command
        .write(POSTS_INSERT, &[post("cy", "c1", 1, false)])
        .unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    command.write(DELETE_OF_AUTHOR, &[json!("bo")]).unwrap();
    command.write(USERS_DELETE, &[json!("bo")]).unwrap();
    command.commit().unwrap();
    assert_eq!(
        db.read(OF_AUTHOR, &[json!("cy")]).unwrap(),
        json!([post("cy", "c1", 1, false)])
    );
    assert_eq!(db.read(BY_NAME, &[json!("Bo")]).unwrap(), Value::Null);
}

/// **One command at a time**: a second, begun while the first is open,
/// waits its turn (which the adapter tells), and begins once the first has
/// ended, reading what it committed; the first commits whole meanwhile.
#[test]
fn a_command_waits_its_turn() {
    let db = Arc::new(seeded(10));
    let mut first = db.begin().unwrap();
    first.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    let (told, telling) = mpsc::channel();
    db.tell_on_wait(move || {
        let _ = told.send(());
    });
    let second = Arc::clone(&db);
    let waiting = std::thread::spawn(move || {
        let command = second.begin().unwrap();
        command.read(BY_NAME, &[json!("Cy")]).unwrap()
    });
    telling.recv().unwrap();
    first.commit().expect("the first is still the open command");
    assert_eq!(waiting.join().unwrap(), user("cy", "Cy"));
}

/// A command and its host functions act on nothing once it has ended: a
/// commit's prepared command writes nothing more, and an ended command's
/// functions fail rather than write into the next.
#[test]
fn an_ended_command_writes_nothing() {
    let db = seeded(10);
    let contract: pw_host::ComponentContract = serde_json::from_value(json!({
        "component_id": "users.writer",
        "capability_mapping": pw_host::CAPABILITY_MAPPING,
        "abi_schema": "test",
        "required_capabilities": [{"family": "database", "operation": "write", "argument": "Users"}],
        "allowed_placements": ["origin"],
        "imports": [{
            "interface": "pw:tables/users",
            "name": "insert",
            "capability": "database.write<Users>",
            "kind": "host_capability"
        }],
        "exports": []
    }))
    .unwrap();
    let topology = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":["database.write<Users>"]}]}"#,
    )
    .unwrap();
    let admission = pw_host::admit(&contract, &topology, "origin", &[USERS_INSERT.into()]);
    let granted = pw_host::Granted::from(&admission, &Default::default()).unwrap();
    let cy = Val::Record(vec![
        ("id".into(), Val::String("cy".into())),
        ("name".into(), Val::String("Cy".into())),
    ]);
    let command = db.begin().unwrap();
    let host = command.host_fns(&contract, &granted).unwrap();
    let insert = host.get(USERS_INSERT).unwrap().clone();
    assert_eq!(
        insert(std::slice::from_ref(&cy)),
        Ok(vec![Val::Result(Ok(None))])
    );
    assert_eq!(
        insert(std::slice::from_ref(&cy)),
        Ok(vec![Val::Result(Err(Some(Box::new(cy.clone()))))]),
        "refused by its own earlier write"
    );
    let prepared = command.prepare().unwrap();
    assert!(
        insert(std::slice::from_ref(&cy)).is_err(),
        "prepared, it writes nothing more"
    );
    prepared.publish();
    let next = db.begin().unwrap();
    let refused = insert(std::slice::from_ref(&cy)).expect_err("ended, it writes nothing");
    assert!(refused.contains("Ended"), "{refused}");
    drop(next);
    assert_eq!(db.read(BY_NAME, &[json!("Cy")]).unwrap(), user("cy", "Cy"));
}

/// A command that writes nothing commits nothing a host need record: a
/// delete of a key no row has, a row put back as it was, and a row
/// inserted and deleted again write nothing.
#[test]
fn a_command_that_writes_nothing_reports_no_table() {
    let db = seeded(10);
    let mut command = db.begin().unwrap();
    command.write(USERS_DELETE, &[json!("nobody")]).unwrap();
    command.write(DELETE_OF_AUTHOR, &[json!("nobody")]).unwrap();
    command.write(USERS_PUT, &[user("ann", "Ann")]).unwrap();
    command.write(USERS_INSERT, &[user("cy", "Cy")]).unwrap();
    command.write(USERS_DELETE, &[json!("cy")]).unwrap();
    assert!(command.written().unwrap().is_empty());
    assert!(command.commit().unwrap().written.is_empty());
    // The neighbour: a row changed is a table written.
    let mut command = db.begin().unwrap();
    command.write(USERS_PUT, &[user("ann", "Anne")]).unwrap();
    assert_eq!(command.written().unwrap(), ["Users".to_string()].into());
}

/// An `Option` of an `Option` would store its `Some(None)` as its `None`:
/// the adapter refuses such a table where it is made.
#[test]
fn an_option_of_an_option_is_refused_where_the_adapter_is_made() {
    let mut nested = manifest(10);
    nested.tables[1].schema.fields.push(field(
        "note",
        Stored::Option(Box::new(Stored::Option(Box::new(string())))),
    ));
    nested.hash = Schema::of(nested.tables.iter().map(|t| t.schema.clone()).collect()).hash;
    nested.migration.to = nested.hash.clone();
    assert!(matches!(
        MemoryTables::new(nested),
        Err(Error::Schema(why)) if why.contains("unsupported storage encoding")
    ));
    assert!(MemoryTables::new(manifest(10)).is_ok());
}

#[test]
fn changing_only_a_floats_zero_sign_is_published_and_rolled_back_exactly() {
    let mut declared = manifest(10);
    declared.tables[0]
        .schema
        .fields
        .push(field("score", Stored::Float));
    let hash = Schema::of(declared.tables.iter().map(|t| t.schema.clone()).collect()).hash;
    declared.hash = hash.clone();
    declared.migration.to = hash;
    let db = MemoryTables::new(declared).expect("float field admitted");
    let row = |score: f64| json!({"author":"ann","id":"a1","rank":1,"draft":false,"score":score});
    db.transaction(|tx| {
        tx.write(USERS_INSERT, &[user("ann", "Ann")])?;
        tx.write(POSTS_INSERT, &[row(0.0)])
    })
    .unwrap();
    let bits = |value: Value| value[0]["score"].as_f64().expect("a float").to_bits();
    let mut command = db.begin().unwrap();
    command.write(POSTS_PUT, &[row(-0.0)]).unwrap();
    assert_eq!(
        bits(command.read(OF_AUTHOR, &[json!("ann")]).unwrap()),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        bits(db.read(OF_AUTHOR, &[json!("ann")]).unwrap()),
        0.0f64.to_bits(),
        "staging isolated"
    );
    let committed = command.commit().unwrap();
    assert!(
        committed.written.contains_key("Posts"),
        "a representational change must publish"
    );
    assert_eq!(
        bits(db.read(OF_AUTHOR, &[json!("ann")]).unwrap()),
        (-0.0f64).to_bits(),
        "published bits match staged bits"
    );
    let mut rolled_back = db.begin().unwrap();
    rolled_back.write(POSTS_PUT, &[row(0.0)]).unwrap();
    drop(rolled_back);
    let next = db.begin().unwrap();
    assert_eq!(
        bits(next.read(OF_AUTHOR, &[json!("ann")]).unwrap()),
        (-0.0f64).to_bits(),
        "rollback restores exact bits"
    );
    drop(next);
    for nan in [f64::NAN, f64::from_bits(0x7ff8_0000_0000_0042)] {
        let mut command = db.begin().unwrap();
        assert!(
            command.write(POSTS_PUT, &[row(nan)]).is_err(),
            "nonfinite JSON value is refused"
        );
        drop(command);
    }
    assert_eq!(
        bits(db.read(OF_AUTHOR, &[json!("ann")]).unwrap()),
        (-0.0f64).to_bits()
    );
}

#[test]
fn integer_changes_above_float_precision_are_not_lost() {
    let db = seeded(10);
    let lower = 9_007_199_254_740_992i64;
    for rank in [lower, lower + 1] {
        let mut command = db.begin().unwrap();
        command
            .write(POSTS_PUT, &[post("ann", "a1", rank, false)])
            .unwrap();
        let committed = command.commit().unwrap();
        assert!(committed.written.contains_key("Posts"));
        let rows = db.read(OF_AUTHOR, &[json!("ann")]).unwrap();
        assert_eq!(
            rows.as_array().unwrap().last().unwrap()["rank"].as_i64(),
            Some(rank)
        );
    }
}
