//! Closed variants use one lossless wire-tag encoding across table operations.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/cases.rs");

#[test]
fn compiled_closed_cases_round_trip_and_sort_by_wire_tags() {
    let program = case_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory closed cases");
    tables
        .transaction(|tx| {
            for row in case_rows() {
                tx.write("pw:tables/tags#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    case_queries(|id, args| query(&program, &tables, id, &Caller::Nobody, args));
    // A real compiled write encodes nested cases; subsequent read decodes them.
    assert_eq!(
        command(&program, &tables, "Save", ALICE, &[case_row("ab", 7, true)]),
        Ok(vec![])
    );
    assert_eq!(
        query(
            &program,
            &tables,
            "Many",
            &Caller::Nobody,
            &[Val::List(vec![case_key("ab", 7)])]
        ),
        Ok(vec![Val::List(vec![case_row("ab", 7, true)])])
    );
}
#[test]
fn malformed_whole_case_inputs_reach_no_adapter_callback() {
    let program = case_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
    let calls = Arc::new(AtomicUsize::new(0));
    for bad in malformed_tags() {
        let input = Val::List(vec![tag("ab"), bad]);
        assert!(
            query_counted(
                &program,
                &tables,
                "Counts",
                &Caller::Nobody,
                &[input],
                &calls
            )
            .is_err()
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "whole set validated before callbacks"
        );
    }
    assert_eq!(
        query(&program, &tables, "By", &Caller::Nobody, &[tag("ab")]),
        Ok(vec![Val::List(vec![])])
    );
}
#[test]
fn unsupported_case_schemas_and_invalid_stored_tags_are_refused() {
    let program = case_program();
    for tables in malformed_case_schemas(&program) {
        assert!(
            matches!(MemoryTables::new(tables),Err(pw_table_memory::Error::Schema(ref why)) if why.contains("closed-case"))
        );
    }
    let tables = MemoryTables::new(program.tables).assert_ok("valid neighbor");
    for tag in ["A_Z", "missing", ""] {
        let mut row = case_rows().remove(0);
        row["detail"]["history"][1] = json!(tag);
        assert!(
            tables
                .transaction(|tx| tx.write("pw:tables/tags#put", &[row]))
                .is_err()
        );
    }
    assert_eq!(
        tables
            .read("pw:tables/tags#all", &[])
            .assert_ok("empty after refusals"),
        json!([])
    );
}

#[test]
fn actual_variant_case_order_must_match_the_import_abi_before_callbacks() {
    let program = case_wrong_abi_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("valid reordered tags");
    let calls = Arc::new(AtomicUsize::new(0));
    let result = query_counted(
        &program,
        &tables,
        "By",
        &Caller::Nobody,
        &[tag("ab")],
        &calls,
    );
    assert!(
        matches!(result,Err(ref why) if why.contains("variant cases or payloads differ")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn host_metadata_rejects_unsupported_nested_case_shapes() {
    let program = case_program();
    for tables in malformed_case_schemas(&program) {
        assert!(
            pw_host::tables::Manifest::from_json(&serde_json::to_string(&tables).unwrap()).is_err()
        );
    }
    let mut tables = program.tables;
    let pw_core::tables::Stored::Record(fields) = &mut tables.tables[0].schema.fields[3].stored
    else {
        panic!("nested fixture")
    };
    fields[0].stored = pw_core::tables::Stored::Cases(vec![]);
    assert!(
        pw_host::tables::Manifest::from_json(&serde_json::to_string(&tables).unwrap()).is_err()
    );
}

#[test]
fn independent_case_codec_refuses_unknown_tags_payloads_and_enum_values() {
    use pw_host::engine::{table_link, table_results, table_selectors};
    use pw_host::tables::{Case, Stored};
    let stored = Stored::Cases(vec![
        Case {
            name: "AB".into(),
            carries: vec![],
        },
        Case {
            name: "A_Z".into(),
            carries: vec![],
        },
    ]);
    assert_eq!(table_link::check_value(&stored, &tag("a-z")), Ok(()));
    for bad in malformed_tags() {
        assert!(table_link::check_value(&stored, &bad).is_err(), "{bad:?}");
    }
    let program = case_program();
    let component = program.component("By");
    let Some(read) = program
        .manifest
        .admit_reads(&component.contract, &component.granted())
        .unwrap()
        .remove("pw:tables/tags#of-kind")
    else {
        panic!("prefix read")
    };
    assert_eq!(
        table_selectors::decode_read_arguments(&read, &[tag("a-z")]),
        Ok(vec![json!("a-z")])
    );
    let mut invalid = case_rows().remove(0);
    invalid["detail"]["kind"] = json!("AB");
    assert!(table_results::encode_get_many(&read, &json!([invalid])).is_err());
    let valid = case_rows().remove(0);
    assert_eq!(
        table_results::encode_get_many(&read, &json!([valid])),
        Ok(Val::List(vec![case_row("ab", 1, false)]))
    );
}
