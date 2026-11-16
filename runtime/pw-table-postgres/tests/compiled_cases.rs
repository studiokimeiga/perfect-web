//! The same compiled closed cases on actual PostgreSQL, including bytea keys.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/cases.rs");
mod common;
use pw_host::engine::CallTables;
use pw_table_postgres::PgTables;
fn case_query(
    program: &Program,
    tables: &PgTables,
    id: &str,
    args: &[Val],
    calls: &Arc<AtomicUsize>,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let snapshot = tables.snapshot().map_err(|e| format!("{e:?}"))?;
    let host = snapshot
        .functions(&component.contract, &granted)?
        .into_iter()
        .map(|(key, call)| (key, counted(call, calls)))
        .collect();
    pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &Caller::Nobody,
        &component.export(),
        args,
    )
}
#[test]
fn compiled_closed_cases_round_trip_and_sort_by_wire_tags() {
    let program = case_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "closed_cases");
    let tables = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("PostgreSQL closed cases");
    tables
        .transaction(|tx| {
            for row in case_rows() {
                tx.write("pw:tables/tags#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    case_queries(|id, args| case_query(&program, &tables, id, args, &Arc::default()));
    let component = program.component("Save");
    let granted = component.granted();
    let staged = tables.begin().assert_ok("command");
    let mut host = staged
        .host_fns(&component.contract, &granted)
        .assert_ok("write binding");
    host.extend(platform(component, ""));
    assert_eq!(
        pw_host::engine::call_with_tables_as(
            &component.bytes,
            &component.contract,
            &granted,
            &pw_host::Limits::unbounded(),
            &host,
            &program.manifest,
            &Caller::Nobody,
            &component.export(),
            &[case_row("ab", 7, true)]
        ),
        Ok(vec![])
    );
    staged.commit().assert_ok("commit nested cases");
    assert_eq!(
        case_query(
            &program,
            &tables,
            "Many",
            &[Val::List(vec![case_key("ab", 7)])],
            &Arc::default()
        ),
        Ok(vec![Val::List(vec![case_row("ab", 7, true)])])
    );
    assert_eq!(
        scratch.texts("SELECT pg_typeof(kind)::text FROM \"Tags\" LIMIT 1"),
        vec!["bytea"]
    );
    drop(tables);
    let reopened = PgTables::open(&url, scratch.schema(), program.tables).assert_ok("reopen");
    assert_eq!(
        reopened
            .read("pw:tables/tags#get", &[json!("ab"), json!(7)])
            .assert_ok("reopened"),
        json!({"kind":"ab","id":7,"lane":true,"detail":{"kind":"ab","history":["ab","a-z"],"maybe":"a-z"}})
    );
}
#[test]
fn malformed_whole_case_inputs_reach_no_adapter_or_data_sql() {
    let program = case_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "closed_cases_bad");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let calls = Arc::new(AtomicUsize::new(0));
    for bad in malformed_tags() {
        assert!(
            case_query(
                &program,
                &tables,
                "Counts",
                &[Val::List(vec![tag("ab"), bad])],
                &calls
            )
            .is_err()
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "whole set validated before SQL callbacks"
        );
    }
    assert_eq!(
        case_query(&program, &tables, "By", &[tag("ab")], &calls),
        Ok(vec![Val::List(vec![])])
    );
}
#[test]
fn unsupported_case_schemas_are_refused_before_schema_creation() {
    let program = case_program();
    let Some(url) = common::database() else {
        return;
    };
    for tables in malformed_case_schemas(&program) {
        let scratch = common::Scratch::new(&url, "closed_cases_schema");
        assert!(
            matches!(PgTables::open(&url, scratch.schema(), tables),Err(pw_table_postgres::Error::Schema(ref why)) if why.contains("closed-case"))
        );
        assert_eq!(
            scratch.texts(&format!(
                "SELECT count(*)::text FROM information_schema.schemata WHERE schema_name='{}'",
                scratch.schema
            )),
            vec!["0"]
        );
    }
}

#[test]
fn actual_variant_abi_mismatch_reaches_no_postgresql_callback() {
    let program = case_wrong_abi_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "closed_case_abi");
    let tables = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("valid reordered tags");
    let calls = Arc::new(AtomicUsize::new(0));
    let result = case_query(&program, &tables, "By", &[tag("ab")], &calls);
    assert!(
        matches!(result,Err(ref why) if why.contains("variant cases or payloads differ")),
        "{result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn malformed_nested_case_tags_are_refused_without_writes() {
    let program = case_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "closed_case_values");
    let tables = PgTables::open(&url, scratch.schema(), program.tables).assert_ok("PostgreSQL");
    for tag in ["AB", "missing", ""] {
        let mut row = case_rows().remove(0);
        row["detail"]["history"][1] = json!(tag);
        assert!(
            tables
                .transaction(|tx| tx.write("pw:tables/tags#put", std::slice::from_ref(&row)))
                .is_err()
        );
    }
    assert_eq!(
        tables
            .read("pw:tables/tags#all", &[])
            .assert_ok("empty after refusals"),
        json!([])
    );
    tables
        .transaction(|tx| tx.write("pw:tables/tags#put", &[case_rows().remove(0)]))
        .assert_ok("valid neighbor writes");
}

#[test]
fn an_added_optional_case_column_uses_the_same_wire_type_as_fresh_creation() {
    let old = case_migration_program(false, None);
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "case_upgrade");
    let tables = PgTables::open(&url, scratch.schema(), old.tables.clone()).assert_ok("old schema");
    tables
        .transaction(|tx| tx.write("pw:tables/evolving#put", &[json!({"id":1})]))
        .assert_ok("existing row");
    drop(tables);
    let released =
        pw_core::tables::Schema::of(old.tables.tables.iter().map(|t| t.schema.clone()).collect());
    let program = case_migration_program(true, Some(&released));
    let tables = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("add optional closed case");
    assert_eq!(
        scratch.texts("SELECT pg_typeof(maybe)::text FROM \"Evolving\" LIMIT 1"),
        vec!["bytea"],
        "an additive column has the same encoding as a fresh table"
    );
    assert_eq!(
        case_query(&program, &tables, "Every", &[], &Arc::default()),
        Ok(vec![Val::List(vec![Val::Record(vec![
            ("id".into(), Val::S64(1)),
            ("maybe".into(), Val::Option(None))
        ])])])
    );
    let row = Val::Record(vec![
        ("id".into(), Val::S64(2)),
        ("maybe".into(), Val::Option(Some(Box::new(tag("a-z"))))),
    ]);
    let component = program.component("Save");
    let granted = component.granted();
    let staged = tables.begin().assert_ok("command");
    let mut host = staged
        .host_fns(&component.contract, &granted)
        .assert_ok("write binding");
    host.extend(platform(component, ""));
    assert_eq!(
        pw_host::engine::call_with_tables_as(
            &component.bytes,
            &component.contract,
            &granted,
            &pw_host::Limits::unbounded(),
            &host,
            &program.manifest,
            &Caller::Nobody,
            &component.export(),
            std::slice::from_ref(&row)
        ),
        Ok(vec![])
    );
    staged.commit().assert_ok("save Some case");
    drop(tables);
    let tables = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("reopen successor");
    assert_eq!(
        tables
            .read("pw:tables/evolving#get", &[json!(2)])
            .assert_ok("reopened Some"),
        json!({"id":2,"maybe":"a-z"})
    );
    assert_eq!(
        case_query(&program, &tables, "Every", &[], &Arc::default()),
        Ok(vec![Val::List(vec![
            Val::Record(vec![
                ("id".into(), Val::S64(1)),
                ("maybe".into(), Val::Option(None))
            ]),
            row
        ])])
    );
}
