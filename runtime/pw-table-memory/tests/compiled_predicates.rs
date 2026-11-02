//! **A predicate the program gives a body, asked by the host** (track
//! `row-ownership`, ADR-0384's H2; ADR-0115 as amended).
//!
//! `examples/owned/`'s `Wrote(id)` reads the reader's note; the command
//! `edit` and the query `Shown` require it. The host asks it before either
//! runs (`call_authorized_with_tables_as`), by calling its compiled
//! component, the command's inside the command's own transaction
//! (`Command::predicate_host_fns`), and never asks the deployment for it; a
//! requirement with no component is the deployment's, as before.
#![allow(dead_code)]

include!("support/owned.rs");

/// What the host asked: the program's predicates, and the deployment's.
#[derive(Default)]
struct Asked {
    program: AtomicUsize,
    deployment: AtomicUsize,
}

/// The program's predicate `component`, run as `user` with `host`'s table
/// reads, answering whether it holds.
fn ask(
    program: &Program,
    component: &str,
    user: &str,
    host: BTreeMap<String, HostFn>,
    args: &[&Val],
) -> Result<bool, String> {
    let predicate = program
        .components
        .get(component)
        .ok_or_else(|| format!("no component {component}"))?;
    let granted = predicate.granted();
    let mut host = host;
    host.extend(platform(predicate, user));
    let args: Vec<Val> = args.iter().map(|v| (*v).clone()).collect();
    let answer = pw_host::engine::call_with_tables_as(
        &predicate.bytes,
        &predicate.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(user),
        &predicate.export(),
        &args,
    )?;
    match answer.as_slice() {
        [Val::Bool(holds)] => Ok(*holds),
        other => Err(format!("a predicate answered {other:?}")),
    }
}

/// **A command, made as `user`, its predicates asked first**: the
/// program's in its own transaction, the deployment's by `deployment`.
fn command_asked(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    user: &str,
    args: &[Val],
    asked: &Asked,
    deployment: bool,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let staged = tables.begin().assert_ok("its turn");
    let mut host = staged
        .host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the command's reads and writes");
    host.extend(platform(component, user));
    let prepared =
        pw_host::engine::Prepared::compile(&component.bytes).assert_ok("the command compiles");
    let answer = prepared.call_authorized_with_tables_as(
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(user),
        &component.export(),
        args,
        |predicate, selected| {
            asked.program.fetch_add(1, Ordering::SeqCst);
            let reads = staged
                .predicate_host_fns(
                    &program
                        .components
                        .get(predicate)
                        .ok_or_else(|| format!("no component {predicate}"))?
                        .contract,
                    &program.components[predicate].granted(),
                )
                .map_err(|e| format!("{e:?}"))?;
            ask(program, predicate, user, reads, selected)
        },
        |_, _| {
            asked.deployment.fetch_add(1, Ordering::SeqCst);
            Ok(deployment)
        },
    );
    if answer.is_ok() {
        staged.commit().assert_ok("its commit");
    }
    answer
}

/// **A query, made as `user`, its predicates asked first** on the published
/// tables.
fn query_asked(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    user: &str,
    args: &[Val],
    asked: &Asked,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let mut host = tables
        .read_host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the query's reads");
    host.extend(platform(component, user));
    let prepared =
        pw_host::engine::Prepared::compile(&component.bytes).assert_ok("the query compiles");
    prepared.call_authorized_with_tables_as(
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(user),
        &component.export(),
        args,
        |predicate, selected| {
            asked.program.fetch_add(1, Ordering::SeqCst);
            let contract = &program.components[predicate].contract;
            let reads = tables
                .read_host_fns(contract, &program.components[predicate].granted())
                .map_err(|e| format!("{e:?}"))?;
            ask(program, predicate, user, reads, selected)
        },
        |_, _| {
            asked.deployment.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        },
    )
}

fn text_of(tables: &MemoryTables, author: &str, id: &str) -> Option<String> {
    let row = tables
        .read("pw:tables/notes#get", &[json!(author), json!(id)])
        .assert_ok("the adapter reads a note");
    row["text"].as_str().map(str::to_string)
}

#[test]
fn a_command_runs_where_its_programs_predicate_holds() {
    let program = program();
    let tables = seeded(&program);
    let asked = Asked::default();
    let args = [
        Val::String("n1".into()),
        handle(BOB),
        Val::String("edited".into()),
    ];
    assert_eq!(
        command_asked(&program, &tables, "edit", ALICE, &args, &asked, false),
        Ok(vec![])
    );
    assert_eq!(text_of(&tables, ALICE, "n1").as_deref(), Some("edited"));
    assert_eq!(asked.program.load(Ordering::SeqCst), 1);
    // A predicate the program gives a body is never the deployment's.
    assert_eq!(asked.deployment.load(Ordering::SeqCst), 0);
}

#[test]
fn a_command_is_refused_where_it_does_not_hold_before_it_writes() {
    let program = program();
    let tables = seeded(&program);
    let asked = Asked::default();
    // Carol wrote no `n1`: the predicate refuses, and nothing is written.
    let args = [
        Val::String("n1".into()),
        handle(BOB),
        Val::String("defaced".into()),
    ];
    let refused = command_asked(&program, &tables, "edit", CAROL, &args, &asked, true)
        .assert_err("Wrote(n1) does not hold for carol");
    assert!(
        refused.contains("authorization predicate `Wrote` denied the invocation"),
        "{refused}"
    );
    assert_eq!(
        text_of(&tables, ALICE, "n1").as_deref(),
        Some("n1 from alice")
    );
    assert_eq!(asked.program.load(Ordering::SeqCst), 1);
    assert_eq!(asked.deployment.load(Ordering::SeqCst), 0);
}

#[test]
fn a_query_is_refused_where_its_predicate_does_not_hold() {
    let program = program();
    let tables = seeded(&program);
    let asked = Asked::default();
    let n2 = Val::String("n2".into());
    let refused = query_asked(
        &program,
        &tables,
        "Shown",
        BOB,
        &[handle(BOB), n2.clone()],
        &asked,
    )
    .assert_err("bob wrote no n2");
    assert!(refused.contains("`Wrote` denied"), "{refused}");
    assert_eq!(
        query_asked(
            &program,
            &tables,
            "Shown",
            ALICE,
            &[handle(ALICE), n2],
            &asked
        ),
        Ok(vec![Val::Option(Some(Box::new(note(
            ALICE, "n2", CAROL, 2
        ))))])
    );
    assert_eq!(asked.deployment.load(Ordering::SeqCst), 0);
}

#[test]
fn a_requirement_with_no_component_is_the_deployments() {
    // The same command, its requirement naming no component: the order of
    // asking gives it to the deployment, whose answer decides.
    let mut program = program();
    let edit = program
        .components
        .get_mut("owned.app.edit")
        .assert_ok("the edit command");
    for export in &mut edit.contract.exports {
        if let Some(component) = export.component.as_mut() {
            for requirement in &mut component.authorization {
                requirement.component = None;
            }
        }
    }
    let tables = seeded(&program);
    let asked = Asked::default();
    let args = [
        Val::String("n1".into()),
        handle(BOB),
        Val::String("edited".into()),
    ];
    let refused = command_asked(&program, &tables, "edit", ALICE, &args, &asked, false)
        .assert_err("the deployment refuses");
    assert!(refused.contains("`Wrote` denied"), "{refused}");
    assert_eq!(asked.program.load(Ordering::SeqCst), 0);
    assert_eq!(asked.deployment.load(Ordering::SeqCst), 1);
}

#[test]
fn a_predicates_reads_see_its_commands_writes_and_no_other_reads_do() {
    // A command that wrote alice's `n9` and has not committed: its
    // predicate reads it, as the command's own reads do; a query does not.
    let program = program();
    let tables = seeded(&program);
    let mut staged = tables.begin().assert_ok("its turn");
    staged
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n9", BOB, 9)])
        .assert_ok("staged");
    let wrote = &program.components["owned.app.Wrote"];
    let reads = staged
        .predicate_host_fns(&wrote.contract, &wrote.granted())
        .assert_ok("a predicate's reads");
    let n9 = Val::String("n9".into());
    assert_eq!(
        ask(&program, "owned.app.Wrote", ALICE, reads, &[&n9]),
        Ok(true)
    );
    let published = tables
        .read_host_fns(&wrote.contract, &wrote.granted())
        .assert_ok("the published reads");
    assert_eq!(
        ask(&program, "owned.app.Wrote", ALICE, published, &[&n9]),
        Ok(false)
    );
    // A command's contract is no predicate's.
    let edit = program.component("edit");
    let refused = staged
        .predicate_host_fns(&edit.contract, &edit.granted())
        .assert_err("a command is no predicate");
    assert!(
        format!("{refused:?}").contains("one predicate export"),
        "{refused:?}"
    );
}
