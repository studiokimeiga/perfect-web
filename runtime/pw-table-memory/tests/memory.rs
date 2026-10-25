use pw_core::tables::{
    FieldSchema, FunctionJson, IndexSchema, ParamJson, Plan, Primitive, ReferenceSchema, Schema,
    Step, Stored, TableJson, TableSchema, TablesJson,
};
use pw_table_memory::{Error, MemoryTables};
use serde_json::{Value, json};
use std::sync::Arc;

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
fn integer() -> Stored {
    Stored::Int {
        at_least: None,
        at_most: None,
    }
}
fn function(name: &str, primitive: Primitive, by: &[&str], sort: &[&str]) -> FunctionJson {
    FunctionJson {
        name: name.into(),
        import: format!("pw:tables/likes#{}", name.replace('_', "-")),
        primitive,
        effect: if primitive.writes() {
            "write".into()
        } else {
            "read".into()
        },
        by: by.iter().map(|s| (*s).into()).collect(),
        sorted_by: sort.iter().map(|s| (*s).into()).collect(),
        params: Vec::<ParamJson>::new(),
        returns: String::new(),
    }
}

fn manifest() -> TablesJson {
    let schema = TableSchema {
        name: "Likes".into(),
        source: None,
        fields: vec![
            field("from", string()),
            field("to", string()),
            field("rank", integer()),
            field("note", Stored::Option(Box::new(string()))),
        ],
        key: vec!["from".into(), "to".into()],
        order: Some("rank".into()),
        indexes: vec![
            IndexSchema {
                name: "by_to".into(),
                fields: vec!["to".into()],
                unique: false,
            },
            IndexSchema {
                name: "by_rank".into(),
                fields: vec!["rank".into()],
                unique: true,
            },
        ],
        references: vec![],
    };
    let hash = Schema::of(vec![schema.clone()]).hash;
    TablesJson {
        hash: hash.clone(),
        tables: vec![TableJson {
            schema,
            module: "Likes".into(),
            interface: "pw:tables/likes".into(),
            row: "Like".into(),
            bounded: Some(50),
            functions: vec![
                function("get", Primitive::Get, &["from", "to"], &[]),
                function(
                    "get_many",
                    Primitive::GetMany,
                    &["from", "to"],
                    &["from", "to"],
                ),
                function(
                    "of_from",
                    Primitive::Prefix,
                    &["from"],
                    &["rank", "from", "to"],
                ),
                function(
                    "of_from_many",
                    Primitive::PrefixMany,
                    &["from"],
                    &["from", "rank", "to"],
                ),
                function("by_to", Primitive::Index, &["to"], &["rank", "from", "to"]),
                function(
                    "by_to_many",
                    Primitive::IndexMany,
                    &["to"],
                    &["to", "rank", "from"],
                ),
                function("by_rank", Primitive::Index, &["rank"], &[]),
                function("all", Primitive::All, &[], &["rank", "from", "to"]),
                function("insert", Primitive::Insert, &[], &[]),
                function("put", Primitive::Put, &[], &[]),
                function("delete", Primitive::Delete, &["from", "to"], &[]),
                function("delete_of_from", Primitive::DeletePrefix, &["from"], &[]),
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
fn call(name: &str) -> String {
    format!("pw:tables/likes#{name}")
}
fn like(from: &str, to: &str, rank: i64) -> Value {
    json!({ "from": from, "to": to, "rank": rank, "note": null })
}

#[test]
fn compiler_manifest_drives_reads_without_app_specific_storage() {
    let db = MemoryTables::new(manifest()).unwrap();
    db.transaction(|tx| {
        tx.write(&call("insert"), &[like("a", "c", 10)])?;
        tx.write(&call("insert"), &[like("a", "b", 2)])?;
        tx.write(&call("insert"), &[like("d", "c", 5)])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        db.read(&call("get"), &[json!("a"), json!("b")]).unwrap(),
        like("a", "b", 2)
    );
    assert_eq!(
        db.read(&call("get"), &[json!("unknown"), json!("b")])
            .unwrap(),
        Value::Null
    );
    assert_eq!(
        db.read(&call("of-from"), &[json!("a")]).unwrap(),
        json!([like("a", "b", 2), like("a", "c", 10)])
    );
    assert_eq!(
        db.read(&call("by-to"), &[json!("c")]).unwrap(),
        json!([like("d", "c", 5), like("a", "c", 10)])
    );
    assert_eq!(
        db.read(&call("by-rank"), &[json!(5)]).unwrap(),
        like("d", "c", 5)
    );
    assert_eq!(
        db.read(&call("all"), &[]).unwrap(),
        json!([like("a", "b", 2), like("d", "c", 5), like("a", "c", 10)])
    );
}

#[test]
fn set_queries_deduplicate_and_sort_not_by_input_order() {
    let db = MemoryTables::new(manifest()).unwrap();
    db.transaction(|tx| {
        tx.write(&call("insert"), &[like("x", "d", 30)])?;
        tx.write(&call("insert"), &[like("z", "a", 3)])?;
        tx.write(&call("insert"), &[like("x", "b", 20)])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        db.read(
            &call("get-many"),
            &[json!([
                { "from": "x", "to": "d" }, { "from": "z", "to": "a" },
                { "from": "x", "to": "d" }, { "from": "missing", "to": "z" },
            ])]
        )
        .unwrap(),
        json!([like("x", "d", 30), like("z", "a", 3)])
    );
    assert_eq!(
        db.read(&call("of-from-many"), &[json!(["z", "x", "x"])])
            .unwrap(),
        json!([like("x", "b", 20), like("x", "d", 30), like("z", "a", 3)])
    );
    assert_eq!(db.read(&call("get-many"), &[json!([])]).unwrap(), json!([]));
    assert_eq!(
        db.read(&call("by-to-many"), &[json!(["a", "d", "d"])])
            .unwrap(),
        json!([like("z", "a", 3), like("x", "d", 30)])
    );
}

#[test]
fn rejected_and_abandoned_writes_do_not_publish() {
    let db = MemoryTables::new(manifest()).unwrap();
    db.transaction(|tx| tx.write(&call("insert"), &[like("a", "b", 1)]))
        .unwrap();
    assert!(matches!(
        db.transaction(|tx| {
            tx.write(&call("insert"), &[like("other", "c", 1)])?;
            Ok(())
        }),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(
        db.read(&call("all"), &[]).unwrap(),
        json!([like("a", "b", 1)])
    );

    let abandoned: Result<(), Error> = db.transaction(|tx| {
        tx.write(&call("insert"), &[like("other", "c", 2)])?;
        Err(Error::Input("business rule rejected".into()))
    });
    assert!(abandoned.is_err());
    assert_eq!(
        db.read(&call("all"), &[]).unwrap(),
        json!([like("a", "b", 1)])
    );

    db.transaction(|tx| {
        tx.write(&call("put"), &[like("a", "b", 3)])?;
        tx.write(&call("insert"), &[like("new", "d", 4)])?;
        tx.write(&call("delete"), &[json!("new"), json!("d")])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        db.read(&call("all"), &[]).unwrap(),
        json!([like("a", "b", 3)])
    );
}

#[test]
fn invalid_rows_and_schema_hash_fail_closed() {
    let db = MemoryTables::new(manifest()).unwrap();
    for row in [
        json!({ "from": "a", "to": "b", "rank": "not-an-int", "note": null }),
        json!({ "from": "a", "to": "b", "rank": 1 }),
        json!({ "from": "a", "to": "b", "rank": 1, "note": null, "extra": 2 }),
        json!({ "from": "a", "to": "b", "rank": 1.25, "note": null }),
    ] {
        assert!(matches!(
            db.transaction(|tx| tx.write(&call("insert"), &[row])),
            Err(Error::Input(_))
        ));
    }
    let mut bad = manifest();
    bad.hash = "incorrect".into();
    assert!(matches!(MemoryTables::new(bad), Err(Error::Schema(_))));
    assert_eq!(db.read(&call("all"), &[]).unwrap(), json!([]));
}

#[test]
fn transactions_serialize_concurrent_writers() {
    let db = Arc::new(MemoryTables::new(manifest()).unwrap());
    let mut threads = Vec::new();
    for n in 0..12 {
        let db = Arc::clone(&db);
        threads.push(std::thread::spawn(move || {
            db.transaction(|tx| {
                tx.write(&call("insert"), &[like("a", &format!("user-{n}"), n)])?;
                Ok(())
            })
            .unwrap();
        }));
    }
    for handle in threads {
        handle.join().unwrap();
    }
    assert_eq!(
        db.read(&call("all"), &[])
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        12
    );
}

#[test]
fn foreign_keys_are_validated_at_commit() {
    let mut m = manifest();
    let users = TableSchema {
        name: "Users".into(),
        source: None,
        fields: vec![field("id", string())],
        key: vec!["id".into()],
        order: None,
        indexes: vec![],
        references: vec![],
    };
    m.tables.push(TableJson {
        schema: users,
        module: "Users".into(),
        interface: "pw:tables/users".into(),
        row: "User".into(),
        bounded: None,
        functions: vec![FunctionJson {
            name: "insert".into(),
            import: "pw:tables/users#insert".into(),
            primitive: Primitive::Insert,
            effect: "write".into(),
            by: vec![],
            sorted_by: vec![],
            params: vec![],
            returns: String::new(),
        }],
    });
    m.tables[0].schema.references.push(ReferenceSchema {
        field: "to".into(),
        table: "Users".into(),
    });
    m.hash = Schema::of(m.tables.iter().map(|t| t.schema.clone()).collect()).hash;
    m.migration.to = m.hash.clone();
    let db = MemoryTables::new(m).unwrap();
    assert!(matches!(
        db.transaction(|tx| tx.write(&call("insert"), &[like("a", "missing", 1)])),
        Err(Error::Input(_))
    ));
    db.transaction(|tx| {
        tx.write("pw:tables/users#insert", &[json!({ "id": "b" })])?;
        tx.write(&call("insert"), &[like("a", "b", 1)])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        db.read(&call("get"), &[json!("a"), json!("b")]).unwrap(),
        like("a", "b", 1)
    );
}

#[test]
fn unused_imports_and_wrong_selectors_are_rejected() {
    let db = MemoryTables::new(manifest()).unwrap();
    assert!(matches!(
        db.read("pw:tables/other#get", &[]),
        Err(Error::Input(_))
    ));
    assert!(matches!(
        db.read(&call("get"), &[json!("a")]),
        Err(Error::Input(_))
    ));
    assert!(matches!(
        db.read(&call("get-many"), &[json!({"from":"a", "to":"b"})]),
        Err(Error::Input(_))
    ));
    assert!(matches!(
        db.read(&call("insert"), &[]),
        Err(Error::Input(_))
    ));
}

fn admitted_table_reader(imports: &[&str]) -> (pw_host::ComponentContract, pw_host::Granted) {
    let declared: Vec<Value> = imports
        .iter()
        .map(|key| {
            let (interface, name) = key.split_once('#').expect("interface#function");
            json!({
                "interface": interface,
                "name": name,
                "capability": "database.read<Likes>",
                "kind": "host_capability"
            })
        })
        .collect();
    let contract: pw_host::ComponentContract = serde_json::from_value(json!({
        "component_id": "likes.reader",
        "capability_mapping": pw_host::CAPABILITY_MAPPING,
        "abi_schema": "test",
        "required_capabilities": [{"family": "database", "operation": "read", "argument": "Likes"}],
        "allowed_placements": ["origin"],
        "imports": declared,
        "exports": []
    }))
    .unwrap();
    let topology = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":["database.read<Likes>"]}]}"#,
    )
    .unwrap();
    let actual: Vec<String> = imports.iter().map(|key| (*key).into()).collect();
    let admitted = pw_host::admit(&contract, &topology, "origin", &actual);
    assert!(admitted.is_admitted(), "{admitted:?}");
    let granted = pw_host::Granted::from(&admitted, &Default::default()).unwrap();
    (contract, granted)
}

#[test]
fn admitted_component_selects_only_manifest_declared_get_and_get_many() {
    let db = MemoryTables::new(manifest()).unwrap();
    let (contract, granted) =
        admitted_table_reader(&["pw:tables/likes#get-many", "pw:tables/likes#get"]);
    assert_eq!(
        db.read_host_imports(&contract, &granted).unwrap(),
        ["pw:tables/likes#get", "pw:tables/likes#get-many"]
    );

    // The manifest has get-many, but a contract that only imports get must
    // never receive the neighboring operation.
    let (contract, granted) = admitted_table_reader(&["pw:tables/likes#get"]);
    assert_eq!(
        db.read_host_imports(&contract, &granted).unwrap(),
        ["pw:tables/likes#get"]
    );
}

#[test]
fn table_host_refuses_unknown_writes_and_wildcard_imports() {
    let db = MemoryTables::new(manifest()).unwrap();
    for denied in [
        "pw:tables/likes#put",
        "pw:tables/likes#all",
        "pw:tables/other#get",
        "pw:tables/*#get",
    ] {
        let (contract, granted) = admitted_table_reader(&["pw:tables/likes#get", denied]);
        assert!(
            matches!(
                db.read_host_imports(&contract, &granted),
                Err(Error::Input(reason)) if reason.contains(denied)
            ),
            "{denied} must be refused, not silently omitted"
        );
    }
}

#[test]
fn table_host_refuses_ungranted_or_misclassified_read_and_manifest_mismatch() {
    let db = MemoryTables::new(manifest()).unwrap();
    let (mut contract, granted) = admitted_table_reader(&["pw:tables/likes#get"]);
    contract.imports[0].kind = pw_host::ImportKind::Component;
    assert!(matches!(
        db.read_host_imports(&contract, &granted),
        Err(Error::Input(_))
    ));

    let (mut contract, _) = admitted_table_reader(&["pw:tables/likes#get"]);
    contract.required_capabilities.clear();
    let topology = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":[]}]}"#,
    )
    .unwrap();
    let admission = pw_host::admit(
        &contract,
        &topology,
        "origin",
        &["pw:tables/likes#get".into()],
    );
    assert!(admission.is_admitted());
    let no_grants = pw_host::Granted::from(&admission, &Default::default()).unwrap();
    assert!(matches!(
        db.read_host_imports(&contract, &no_grants),
        Err(Error::Input(_))
    ));

    let mut changed = manifest();
    changed.tables[0]
        .functions
        .iter_mut()
        .find(|f| f.primitive == Primitive::Get)
        .unwrap()
        .effect = "write".into();
    let changed = MemoryTables::new(changed).unwrap();
    let (contract, granted) = admitted_table_reader(&["pw:tables/likes#get"]);
    assert!(matches!(
        changed.read_host_imports(&contract, &granted),
        Err(Error::Input(_))
    ));
}

#[test]
fn table_host_refuses_wrong_table_capability_even_when_admitted() {
    let db = MemoryTables::new(manifest()).unwrap();
    let (mut contract, _) = admitted_table_reader(&["pw:tables/likes#get"]);
    // A forged contract and topology may agree on a grant for another table.
    // That does not authorize the compiler manifest's Likes table import.
    contract.imports[0].capability = "database.read<Other>".into();
    contract.required_capabilities[0].argument = Some("Other".into());
    let topology = pw_host::Topology::from_json(
        r#"{"nodes":[{"name":"origin","world":"origin","grants":["database.read<Other>"]}]}"#,
    )
    .unwrap();
    let admission = pw_host::admit(
        &contract,
        &topology,
        "origin",
        &["pw:tables/likes#get".into()],
    );
    assert!(admission.is_admitted(), "{admission:?}");
    let granted = pw_host::Granted::from(&admission, &Default::default()).unwrap();
    assert!(pw_host::linkable(&contract, &granted).contains(&"pw:tables/likes#get".into()));
    assert!(matches!(
        db.read_host_imports(&contract, &granted),
        Err(Error::Input(_))
    ));
}
