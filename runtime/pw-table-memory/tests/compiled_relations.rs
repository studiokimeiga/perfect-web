//! Global relation windows execute actual compiled owned/sharer imports.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/relations.rs");

#[test]
fn compiled_relation_windows_are_global_and_sets_keep_overlapping_empty_groups() {
    let program = relation_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
    tables
        .transaction(|tx| {
            for row in relation_roots() {
                tx.write("pw:tables/roots#insert", &[row])?;
            }
            for row in relation_links() {
                tx.write("pw:tables/links#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed independent public posts/private edges");
    for (name, ids) in [("RelFirst", vec![1, 2]), ("RelLast", vec![4, 3])] {
        assert_eq!(
            query(
                &program,
                &tables,
                name,
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(10), Val::S64(2)]
            ),
            Ok(vec![Val::List(
                ids.into_iter().map(relation_root_value).collect()
            )])
        );
    }
    assert_eq!(
        query(
            &program,
            &tables,
            "RelMany",
            &as_user(ALICE),
            &[
                handle(ALICE),
                Val::List(vec![
                    Val::S64(20),
                    Val::S64(999),
                    Val::S64(10),
                    Val::S64(10)
                ]),
                Val::S64(2)
            ]
        ),
        Ok(vec![Val::List(vec![
            relation_group("reader", ALICE, 10, &[4, 3]),
            relation_group("reader", ALICE, 20, &[3, 1]),
            relation_group("reader", ALICE, 999, &[])
        ])])
    );
    assert_eq!(
        query(
            &program,
            &tables,
            "RelLast",
            &as_user(BOB),
            &[handle(BOB), Val::S64(10), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![relation_root_value(5)])])
    );
    assert_eq!(
        query(
            &program,
            &tables,
            "RelSharedMany",
            &as_user(BOB),
            &[
                handle(BOB),
                Val::List(vec![Val::S64(999), Val::S64(10)]),
                Val::S64(2)
            ]
        ),
        Ok(vec![Val::List(vec![
            relation_group("to", BOB, 10, &[1, 2]),
            relation_group("to", BOB, 999, &[])
        ])])
    );
    for n in [0, -9] {
        assert_eq!(
            query(
                &program,
                &tables,
                "RelLast",
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(10), Val::S64(n)]
            ),
            Ok(vec![Val::List(vec![])])
        );
    }
    assert_eq!(
        query(
            &program,
            &tables,
            "RelLast",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(10), Val::S64(i64::MAX)]
        ),
        Ok(vec![Val::List(
            [4, 3, 2, 1].into_iter().map(relation_root_value).collect()
        )])
    );
}

#[test]
fn a_relation_never_uses_another_readers_memberships() {
    let program = relation_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
    let before = tables.statements();
    let forged = query(
        &program,
        &tables,
        "RelLast",
        &as_user(ALICE),
        &[handle(BOB), Val::S64(10), Val::S64(0)],
    );
    assert!(forged.is_err(), "forged reader even at zero: {forged:?}");
    let absent = query(
        &program,
        &tables,
        "RelLast",
        &Caller::Nobody,
        &[handle(ALICE), Val::S64(10), Val::S64(2)],
    );
    assert!(absent.is_err(), "no caller: {absent:?}");
    assert_eq!(tables.statements() - before, 0, "no adapter calls");
}

#[test]
fn relation_manifest_admission_holds_both_tables_and_both_grants_independently() {
    let program = relation_program();
    let component = program.component("RelLast");
    let reads = program
        .manifest
        .admit_reads(&component.contract, &component.granted())
        .assert_ok("dual relation grant");
    let read = reads
        .get("pw:tables/links#last-posts-by-reader-topic")
        .unwrap();
    assert_eq!(read.table, "Links");
    assert_eq!(
        read.source_key.as_deref(),
        Some(
            [
                "reader".to_string(),
                "topic".to_string(),
                "author".to_string(),
                "edge".to_string()
            ]
            .as_slice()
        )
    );
    assert_eq!(read.target.as_deref(), Some("Roots"));
    assert_eq!(
        read.by.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["reader", "topic"]
    );
    assert_eq!(
        read.row.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["id", "author", "stamp", "text"]
    );
    assert_eq!(read.key, ["id"]);
    let shared = program.component("RelShared");
    let shared_reads = program
        .manifest
        .admit_reads(&shared.contract, &shared.granted())
        .assert_ok("shared source relation");
    assert!(
        shared_reads
            .values()
            .all(|r| r.handle.as_ref().is_some_and(|h| h.seen.is_none())),
        "public target must not receive source's seen projection"
    );
    for missing in ["Links", "Roots"] {
        let mut contract = component.contract.clone();
        contract
            .required_capabilities
            .retain(|cap| cap.name() != format!("database.read<{missing}>"));
        for import in &mut contract.imports {
            if import.interface == "pw:tables/links" {
                import.capability = format!(
                    "database.read<{}>",
                    if missing == "Roots" { "Links" } else { "Roots" }
                );
            }
        }
        let absent = Component {
            bytes: component.bytes.clone(),
            imports: component.imports.clone(),
            contract,
        };
        let result = program
            .manifest
            .admit_reads(&absent.contract, &absent.granted());
        assert!(
            result.is_err(),
            "missing contract AND actual grant {missing}: {result:?}"
        );
        assert!(result.unwrap_err().contains(missing));
    }
    let mut contract = component.contract.clone();
    for import in &mut contract.imports {
        if import.interface == "pw:tables/links" {
            import.capability = "database.read<Roots>".into();
        }
    }
    assert!(
        program
            .manifest
            .admit_reads(&contract, &component.granted())
            .is_ok(),
        "either canonical primary claim is valid when both grants are required/held"
    );
    let original = serde_json::to_value(&program.tables).unwrap();
    for kind in [
        "source-absent",
        "source-other",
        "target-owned",
        "target-order",
        "match-kind",
        "match-refinement",
        "repeated-relation",
        "function-relation",
        "source-sort",
    ] {
        let mut value = original.clone();
        let tables = value["tables"].as_array_mut().unwrap();
        let source = tables.iter().position(|t| t["name"] == "Links").unwrap();
        let target = tables.iter().position(|t| t["name"] == "Roots").unwrap();
        match kind {
            "source-absent" => {
                tables[source]["source"] = serde_json::Value::Null;
            }
            "source-other" => {
                tables[target]["source"] = json!("AnotherSource");
            }
            "target-owned" => {
                tables[target]["owner"] = json!("author");
            }
            "target-order" => {
                tables[target]["order"] = serde_json::Value::Null;
            }
            "match-kind" | "match-refinement" => {
                let field = tables[target]["fields"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|f| f["name"] == "author")
                    .unwrap();
                field["stored"] = if kind == "match-kind" {
                    json!({"int":{}})
                } else {
                    json!({"string":{"min_length":1}})
                };
            }
            "repeated-relation" => {
                let duplicate = tables[source]["windows"][0].clone();
                tables[source]["windows"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            "function-relation" | "source-sort" => {
                let function = tables[source]["functions"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|f| f["name"] == "last_posts_by_reader_topic")
                    .unwrap();
                if kind == "function-relation" {
                    function.as_object_mut().unwrap().remove("relation");
                } else {
                    function["sorted_by"] = json!(["reader", "topic"]);
                }
            }
            _ => unreachable!(),
        }
        let result = pw_host::tables::Manifest::from_json(&value.to_string());
        assert!(
            result.is_err(),
            "forged {kind} manifest admitted: {result:?}"
        );
    }
}

fn relation_fake(
    program: &Program,
    id: &str,
    args: &[Val],
    answer: serde_json::Value,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let reads = program
        .manifest
        .admit_reads(&component.contract, &granted)
        .assert_ok("fake callback admitted");
    let mut host = program.platform(ALICE);
    for (key, read) in reads {
        let answer = answer.clone();
        host.insert(
            key,
            pw_host::engine::table_read_host::host_fn(read, Arc::new(move |_| Ok(answer.clone()))),
        );
    }
    pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(ALICE),
        &component.export(),
        args,
    )
}

#[test]
fn the_relation_reply_gate_checks_nested_groups_and_target_rows_whole() {
    let program = relation_program();
    let rows = relation_roots();
    let scalar_args = [handle(ALICE), Val::S64(10), Val::S64(2)];
    assert_eq!(
        relation_fake(&program, "RelLast", &scalar_args, json!([rows[3], rows[2]])),
        Ok(vec![Val::List(vec![
            relation_root_value(4),
            relation_root_value(3)
        ])])
    );
    for bad in [
        json!([rows[2], rows[3]]),
        json!([rows[3], rows[3]]),
        json!([rows[3], rows[2], rows[1]]),
        json!([{"id":4,"author":CAROL,"stamp":3,"text":"C3","secret":"foreign source metadata"}]),
        json!([{"id":4,"author":CAROL,"stamp":3}]),
    ] {
        let result = relation_fake(&program, "RelLast", &scalar_args, bad.clone());
        assert!(result.is_err(), "malformed scalar target {bad}: {result:?}");
    }
    let group = |topic, ids: &[usize]| json!({"key":{"reader":ALICE,"topic":topic},"rows":ids.iter().map(|i|rows[*i].clone()).collect::<Vec<_>>()});
    let args = [
        handle(ALICE),
        Val::List(vec![
            Val::S64(20),
            Val::S64(10),
            Val::S64(10),
            Val::S64(999),
        ]),
        Val::S64(2),
    ];
    let valid = json!([group(10, &[3, 2]), group(20, &[2, 0]), group(999, &[])]);
    assert_eq!(
        relation_fake(&program, "RelMany", &args, valid.clone()),
        Ok(vec![Val::List(vec![
            relation_group("reader", ALICE, 10, &[4, 3]),
            relation_group("reader", ALICE, 20, &[3, 1]),
            relation_group("reader", ALICE, 999, &[])
        ])])
    );
    let mut foreign = valid.clone();
    foreign[0]["key"]["reader"] = json!(BOB);
    let mut extra = valid.clone();
    extra[0]["extra"] = json!(true);
    for bad in [
        json!([group(10, &[3, 2]), group(20, &[2, 0])]),
        json!([group(20, &[2, 0]), group(10, &[3, 2]), group(999, &[])]),
        json!([
            group(10, &[3, 2]),
            group(10, &[]),
            group(20, &[2, 0]),
            group(999, &[])
        ]),
        json!([group(10, &[3, 2, 1]), group(20, &[2, 0]), group(999, &[])]),
        json!([group(10, &[2, 3]), group(20, &[2, 0]), group(999, &[])]),
        foreign,
        extra,
        json!([]),
    ] {
        let result = relation_fake(&program, "RelMany", &args, bad.clone());
        assert!(
            result.is_err(),
            "malformed nested relation reply {bad}: {result:?}"
        );
    }
}

#[test]
fn a_relation_predicate_sees_both_staged_tables_and_pins_membership_with_the_body() {
    let program = relation_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");

    tables
        .transaction(|tx| {
            for row in relation_roots() {
                tx.write("pw:tables/roots#insert", &[row])?;
            }
            for row in relation_links() {
                tx.write("pw:tables/links#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed relation snapshot");
    let root = json!({"id":6,"author":BOB,"stamp":100,"text":"STAGED ORIGINAL"});
    let edge = json!({"reader":ALICE,"topic":77,"author":BOB,"edge":7,"to":BOB,"secret":"only alice's membership"});
    let mut command = tables.begin().assert_ok("command");
    command
        .write("pw:tables/roots#insert", std::slice::from_ref(&root))
        .assert_ok("stage target");
    command
        .write("pw:tables/links#insert", &[edge])
        .assert_ok("stage source membership");
    let args = [handle(ALICE), Val::S64(77), Val::S64(6)];
    let asked = AtomicUsize::new(0);
    let staged = authorized(
        &program,
        "RelGuarded",
        ALICE,
        &command,
        &pw_host::Limits::unbounded(),
        &[Val::S64(77), Val::S64(6)],
        false,
        &asked,
    );
    assert!(
        staged.is_ok(),
        "compiled predicate sees BOTH staged tables: {staged:?}"
    );
    let staged = staged.unwrap();
    assert!(matches!(staged.as_slice(), [Val::Option(Some(_))]));
    assert_eq!(asked.load(Ordering::SeqCst), 0, "no deployment predicate");
    assert_eq!(
        tables
            .read("pw:tables/roots#get", &[json!(6)])
            .assert_ok("published target before commit"),
        serde_json::Value::Null
    );
    command.commit().assert_ok("publish source and target");
    let snapshot = tables.snapshot().assert_ok("held relation snapshot");
    let held = authorized(
        &program,
        "RelHeld",
        ALICE,
        &snapshot,
        &pw_host::Limits::unbounded(),
        &args,
        false,
        &asked,
    )
    .assert_ok("pin membership AND body");
    assert_eq!(held, staged);
    tables.transaction(|tx|{
        let mut changed=root.clone();changed["text"]=json!("CHANGED AFTER PIN");
        tx.write("pw:tables/roots#put",&[changed])?;
        tx.write("pw:tables/links#delete",&[json!(ALICE),json!(77),json!(BOB),json!(7)])?;
        tx.write("pw:tables/links#insert",&[json!({"reader":ALICE,"topic":77,"author":CAROL,"edge":8,"to":BOB,"secret":"changed membership"})])
    }).assert_ok("later writer changes source membership AND target body");
    assert_eq!(
        tables
            .read("pw:tables/roots#get", &[json!(6)])
            .assert_ok("live target body")["text"],
        "CHANGED AFTER PIN"
    );
    assert_eq!(
        authorized(
            &program,
            "RelHeld",
            ALICE,
            &snapshot,
            &pw_host::Limits::unbounded(),
            &args,
            false,
            &asked
        ),
        Ok(held)
    );
    let fresh = tables.snapshot().assert_ok("fresh relation snapshot");
    let watched = Watched::new(&fresh);
    let denied = authorized(
        &program,
        "RelHeld",
        ALICE,
        &watched,
        &pw_host::Limits::unbounded(),
        &args,
        false,
        &asked,
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("Selected")),
        "fresh predicate follows new membership: {denied:?}"
    );
    assert_eq!(
        watched.body.load(Ordering::SeqCst),
        0,
        "fresh refused body never runs"
    );
}

#[test]
fn public_relations_use_scalar_bool_group_keys_and_allow_anonymous_callers() {
    let program = relation_public_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");

    tables
        .transaction(|tx| {
            for row in relation_roots() {
                tx.write("pw:tables/roots#insert", &[row])?;
            }
            for row in relation_public_links() {
                tx.write("pw:tables/links#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("public Bool memberships");
    assert_eq!(
        query(
            &program,
            &tables,
            "PlainLast",
            &Caller::Nobody,
            &[Val::Bool(false), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![
            relation_root_value(3),
            relation_root_value(1)
        ])])
    );
    let args = [
        Val::List(vec![Val::Bool(true), Val::Bool(false), Val::Bool(true)]),
        Val::S64(2),
    ];
    assert_eq!(
        query(&program, &tables, "PlainMany", &Caller::Nobody, &args),
        Ok(vec![Val::List(vec![
            relation_public_group(false, &[3, 1]),
            relation_public_group(true, &[4, 3])
        ])])
    );
    let args = [
        Val::List(vec![Val::Bool(true), Val::Bool(false)]),
        Val::S64(0),
    ];
    assert_eq!(
        query(&program, &tables, "PlainMany", &Caller::Nobody, &args),
        Ok(vec![Val::List(vec![
            relation_public_group(false, &[]),
            relation_public_group(true, &[])
        ])])
    );
    tables
        .transaction(|tx| tx.write("pw:tables/links#delete-of-topic", &[json!(true)]))
        .assert_ok("remove one public selector's members");
    assert_eq!(
        query(
            &program,
            &tables,
            "PlainMany",
            &Caller::Nobody,
            &[
                Val::List(vec![Val::Bool(true), Val::Bool(false)]),
                Val::S64(2)
            ]
        ),
        Ok(vec![Val::List(vec![
            relation_public_group(false, &[3, 1]),
            relation_public_group(true, &[])
        ])])
    );
}
