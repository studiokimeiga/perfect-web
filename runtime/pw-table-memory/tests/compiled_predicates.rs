//! **A predicate the program gives a body, asked by the engine itself**
//! (track `row-ownership`, ADR-0384's H2; ADR-0115 as amended; the review
//! of the track, findings 3, 5 and 6).
//!
//! `examples/owned/`'s `Wrote(id)` reads the reader's note; the command
//! `edit` and the query `Shown` require it. The engine asks it before either
//! runs (`Prepared::call_authorized_with_tables_as`), running its compiled
//! component itself: as the export's caller, its reads on the export's own
//! tables (the command's transaction, `Command`'s `CallTables`; the
//! published tables a query reads, `MemoryTables`'), and under the export's
//! limits, spending its fuel. No embedder answers it, and the deployment is
//! never asked it, on any path; a requirement with no component is the
//! deployment's, as before.
#![allow(dead_code)]

include!("support/owned.rs");

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;

fn text_of(tables: &MemoryTables, author: &str, id: &str) -> Option<String> {
    let row = tables
        .read("pw:tables/notes#get", &[json!(author), json!(id)])
        .assert_ok("the adapter reads a note");
    row["text"].as_str().map(str::to_string)
}

fn unbounded() -> pw_host::Limits {
    pw_host::Limits::unbounded()
}

fn fuel(fuel: u64) -> pw_host::Limits {
    pw_host::Limits {
        fuel: Some(fuel),
        memory_bytes: None,
        table_elements: None,
    }
}

fn s(value: &str) -> Val {
    Val::String(value.to_string())
}

/// What one command asked and ran: its answer, how many program
/// predicates the engine asked, and how many of its own table functions
/// its body called.
struct Ran {
    answer: Result<Vec<Val>, String>,
    asked: usize,
    body: usize,
}

/// **A command, made as `user`, its predicates asked first**, committed
/// where it answers, as the platform commits one.
fn command_asked(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    user: &str,
    args: &[Val],
    deployment: bool,
    deployment_asked: &AtomicUsize,
) -> Ran {
    let staged = tables.begin().assert_ok("its turn");
    let watched = Watched::new(&staged);
    let answer = authorized(
        program,
        id,
        user,
        &watched,
        &unbounded(),
        args,
        deployment,
        deployment_asked,
    );
    let ran = Ran {
        answer,
        asked: watched.asked.load(Ordering::SeqCst),
        body: watched.body.load(Ordering::SeqCst),
    };
    if ran.answer.is_ok() {
        staged.commit().assert_ok("its commit");
    }
    ran
}

#[test]
fn a_command_runs_where_its_programs_predicate_holds() {
    let program = program();
    let tables = seeded(&program);
    let deployment = AtomicUsize::new(0);
    let args = [s("n1"), handle(BOB), s("edited")];
    let ran = command_asked(&program, &tables, "edit", ALICE, &args, false, &deployment);
    assert_eq!(ran.answer, Ok(vec![]));
    assert_eq!(text_of(&tables, ALICE, "n1").as_deref(), Some("edited"));
    // The engine asked the program's predicate once, its body read and wrote.
    assert_eq!(ran.asked, 1);
    assert!(ran.body > 0, "the body did not run");
    // A predicate the program gives a body is never the deployment's.
    assert_eq!(deployment.load(Ordering::SeqCst), 0);
}

/// A command whose body writes a note of `id` for its reader whether or not
/// one is there: only `Wrote(id)` keeps it from a reader who wrote none.
const REWRITE: &str = "module owned.app\n\nimport capability.{ User, UserId, id_of }\nimport context.{ current_user }\nimport Notes.{ Note, NoteId, Text }\nimport Notes\n\npredicate Wrote(id: NoteId)\n    says \"You can only change a note you wrote.\"\n{\n    match Notes.get(current_user(), id) {\n        Some(_) => true,\n        None => false,\n    }\n}\n\ncommand rewrite(id: NoteId, to: UserId, text: Text) -> ()\n    requires    Wrote(id)\n    transaction serializable\n{\n    Notes.put(current_user(), Note { author: id_of(current_user()), id: id, to: to, text: text, draft: text, added: 0 })\n}\n";

#[test]
fn a_refused_predicate_leaves_nothing_committed_and_its_body_never_runs() {
    // The review's finding 6: with the predicate asked after the body, every
    // test passed, since the harness committed nothing a refusal answered.
    // This harness commits what the command staged whatever it answered.
    let program = built(&[("app.pw", REWRITE)]);
    let tables = seeded(&program);
    let before = every_note(&tables);
    let deployment = AtomicUsize::new(0);
    let staged = tables.begin().assert_ok("its turn");
    let watched = Watched::new(&staged);
    let refused = authorized(
        &program,
        "rewrite",
        CAROL,
        &watched,
        &unbounded(),
        &[s("n1"), handle(BOB), s("defaced")],
        true,
        &deployment,
    )
    .assert_err("carol wrote no n1");
    assert!(
        refused.contains("authorization predicate `Wrote` denied the invocation"),
        "{refused}"
    );
    assert_eq!(watched.asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        watched.body.load(Ordering::SeqCst),
        0,
        "the body ran before its predicate refused"
    );
    staged.commit().assert_ok("a commit of nothing");
    assert_eq!(
        every_note(&tables),
        before,
        "a refused command's body wrote"
    );
    // The neighbour: alice wrote n1; the same harness commits her rewrite.
    let staged = tables.begin().assert_ok("its turn");
    let watched = Watched::new(&staged);
    assert_eq!(
        authorized(
            &program,
            "rewrite",
            ALICE,
            &watched,
            &unbounded(),
            &[s("n1"), handle(BOB), s("rewritten")],
            true,
            &deployment,
        ),
        Ok(vec![])
    );
    assert!(watched.body.load(Ordering::SeqCst) > 0);
    staged.commit().assert_ok("its commit");
    assert_eq!(text_of(&tables, ALICE, "n1").as_deref(), Some("rewritten"));
    assert_eq!(deployment.load(Ordering::SeqCst), 0);
}

#[test]
fn a_query_is_refused_where_its_predicate_does_not_hold() {
    let program = program();
    let tables = seeded(&program);
    let deployment = AtomicUsize::new(0);
    let refused = authorized(
        &program,
        "Shown",
        BOB,
        &tables,
        &unbounded(),
        &[handle(BOB), s("n2")],
        true,
        &deployment,
    )
    .assert_err("bob wrote no n2");
    assert!(refused.contains("`Wrote` denied"), "{refused}");
    assert_eq!(
        authorized(
            &program,
            "Shown",
            ALICE,
            &tables,
            &unbounded(),
            &[handle(ALICE), s("n2")],
            true,
            &deployment,
        ),
        Ok(vec![Val::Option(Some(Box::new(note(
            ALICE, "n2", CAROL, 2
        ))))])
    );
    assert_eq!(deployment.load(Ordering::SeqCst), 0);
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
    let deployment = AtomicUsize::new(0);
    let args = [s("n1"), handle(BOB), s("edited")];
    let ran = command_asked(&program, &tables, "edit", ALICE, &args, false, &deployment);
    let refused = ran.answer.assert_err("the deployment refuses");
    assert!(refused.contains("`Wrote` denied"), "{refused}");
    assert_eq!(ran.asked, 0);
    assert_eq!(deployment.load(Ordering::SeqCst), 1);
}

#[test]
fn a_predicates_reads_see_its_commands_writes_and_no_other_reads_do() {
    // A command that wrote alice's `n9` and has not committed: the
    // predicate the engine asks for it reads it, as the command's own reads
    // do; a query, on the published tables, does not.
    let program = program();
    let tables = seeded(&program);
    let deployment = AtomicUsize::new(0);
    let mut staged = tables.begin().assert_ok("its turn");
    staged
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n9", BOB, 9)])
        .assert_ok("staged");
    assert_eq!(
        authorized(
            &program,
            "edit",
            ALICE,
            &staged,
            &unbounded(),
            &[s("n9"), handle(BOB), s("edited")],
            false,
            &deployment,
        ),
        Ok(vec![])
    );
    let refused = authorized(
        &program,
        "Shown",
        ALICE,
        &tables,
        &unbounded(),
        &[handle(ALICE), s("n9")],
        false,
        &deployment,
    )
    .assert_err("n9 is not published");
    assert!(refused.contains("`Wrote` denied"), "{refused}");
    // A command's contract is no predicate's.
    let edit = program.component("edit");
    let refused = staged
        .predicate_host_fns(&edit.contract, &edit.granted())
        .assert_err("a command is no predicate");
    assert!(
        format!("{refused:?}").contains("one predicate export"),
        "{refused:?}"
    );
    assert_eq!(deployment.load(Ordering::SeqCst), 0);
}

/// The program's own `SignedIn`, which no one holds, whatever a
/// deployment's predicate of the same name says (the review's probe E).
const STAMP: &str = "module owned.app\n\npredicate SignedIn()\n    says \"No one may stamp.\"\n{\n    false\n}\n\ncommand stamp() -> Int\n    requires    SignedIn\n    transaction serializable\n{\n    42\n}\n";

#[test]
fn the_deployment_is_never_asked_a_predicate_the_program_gives_a_body() {
    // The review's finding 3: the development server's path,
    // `call_authorized_within`, asked its own `SignedIn` for the program's,
    // and the command ran.
    let program = built(&[("app.pw", STAMP)]);
    let tables = seeded(&program);
    let stamp = program.component("stamp");
    let place = stamp.contract.exports[0]
        .component
        .as_ref()
        .assert_ok("an export's place");
    let named: Vec<Option<&str>> = place
        .authorization
        .iter()
        .map(|r| r.component.as_deref())
        .collect();
    assert_eq!(named, [Some("owned.app.SignedIn")]);
    let deployment = AtomicUsize::new(0);
    // The engine asks the program's body, which refuses.
    let staged = tables.begin().assert_ok("its turn");
    let refused = authorized(
        &program,
        "stamp",
        ALICE,
        &staged,
        &unbounded(),
        &[],
        true,
        &deployment,
    )
    .assert_err("the program's SignedIn holds for no one");
    assert!(refused.contains("`SignedIn` denied"), "{refused}");
    drop(staged);
    // The deployment's paths refuse it unasked.
    let evaluated = AtomicUsize::new(0);
    let evaluate = |predicate: &str, _: &[&Val]| {
        evaluated.fetch_add(1, Ordering::SeqCst);
        Ok(predicate == "SignedIn")
    };
    let prepared = pw_host::engine::Prepared::compile(&stamp.bytes).assert_ok("it compiles");
    let refused = prepared
        .call_authorized_within(
            &stamp.contract,
            &stamp.granted(),
            &unbounded(),
            &program.platform(ALICE),
            &stamp.export(),
            &[],
            evaluate,
        )
        .assert_err("the deployment never answers the program's SignedIn");
    assert!(
        refused.contains(
            "authorization predicate `SignedIn` could not be evaluated: it is the program's"
        ),
        "{refused}"
    );
    let refused =
        pw_host::engine::authorize_export(&stamp.contract, &stamp.export(), &[], evaluate)
            .assert_err("nor on the deployment's own path");
    assert!(refused.contains("it is the program's"), "{refused}");
    assert_eq!(evaluated.load(Ordering::SeqCst), 0);
    assert_eq!(deployment.load(Ordering::SeqCst), 0);
    // The neighbour: a `SignedIn` the program gives no body is the
    // deployment's to answer, on its path.
    let theirs = built(&[(
        "app.pw",
        &STAMP.replace(
            "()\n    says \"No one may stamp.\"\n{\n    false\n}\n",
            "\n    says \"Sign in.\"\n",
        ),
    )]);
    let stamp = theirs.component("stamp");
    assert_eq!(
        pw_host::engine::Prepared::compile(&stamp.bytes)
            .assert_ok("it compiles")
            .call_authorized_within(
                &stamp.contract,
                &stamp.granted(),
                &unbounded(),
                &theirs.platform(ALICE),
                &stamp.export(),
                &[],
                evaluate,
            ),
        Ok(vec![Val::S64(42)])
    );
    assert_eq!(evaluated.load(Ordering::SeqCst), 1);
}

#[test]
fn a_program_predicate_is_asked_of_the_programs_compiled_predicates_alone() {
    // No embedder answers one: a call given none for it is refused, and
    // a table's function is never one of the platform's operations.
    let program = program();
    let tables = seeded(&program);
    let edit = program.component("edit");
    let prepared = pw_host::engine::Prepared::compile(&edit.bytes).assert_ok("it compiles");
    let staged = tables.begin().assert_ok("its turn");
    let args = [s("n1"), handle(BOB), s("edited")];
    let refused = prepared
        .call_authorized_with_tables_as(
            &edit.contract,
            &edit.granted(),
            &unbounded(),
            &program.platform(ALICE),
            &staged,
            &pw_host::engine::Predicates::none(),
            &as_user(ALICE),
            &edit.export(),
            &args,
            |_, _| Ok(true),
        )
        .assert_err("no compiled predicate was given");
    assert!(
        refused.contains(
            "its body, component `owned.app.Wrote`, is not among the program's predicates"
        ),
        "{refused}"
    );
    let mut platform = program.platform(ALICE);
    platform.insert(
        "pw:tables/notes#get".into(),
        Arc::new(|_: &[Val]| Ok(vec![Val::Option(None)])) as HostFn,
    );
    let refused = prepared
        .call_authorized_with_tables_as(
            &edit.contract,
            &edit.granted(),
            &unbounded(),
            &platform,
            &staged,
            &program.predicates(),
            &as_user(ALICE),
            &edit.export(),
            &args,
            |_, _| Ok(true),
        )
        .assert_err("a table's function among the platform's");
    assert!(
        refused.contains("is a table's function, which the call's tables give"),
        "{refused}"
    );
    // The neighbour: the program's predicates, and the platform's own.
    assert_eq!(
        prepared.call_authorized_with_tables_as(
            &edit.contract,
            &edit.granted(),
            &unbounded(),
            &program.platform(ALICE),
            &staged,
            &program.predicates(),
            &as_user(ALICE),
            &edit.export(),
            &args,
            |_, _| Ok(true),
        ),
        Ok(vec![])
    );
}

/// A predicate whose body never ends in any time a reader would wait: two
/// to the sixty-fourth calls (the review's probe L).
const LOOPS: &str = "module owned.app\n\nimport Notes.{ NoteId }\n\nfn spin(n: Int) -> Bool !{} {\n    match n <= 0 {\n        true => true,\n        false => spin(n - 1) == spin(n - 1),\n    }\n}\n\npredicate Loops(id: NoteId)\n    says \"Never asked to the end.\"\n{\n    spin(64)\n}\n\ncommand edit(id: NoteId) -> ()\n    requires    Loops(id)\n    transaction serializable\n{\n    ()\n}\n";

#[test]
fn a_predicate_runs_under_its_exports_limits() {
    // The review's finding 5: its embedder ran it unbounded, past five
    // seconds, holding the memory adapter's one open command. Under the
    // export's fuel it stops, and the command ends with it.
    let program = built(&[("app.pw", LOOPS)]);
    let tables = seeded(&program);
    let (told, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let staged = tables.begin().assert_ok("its turn");
        let answer = authorized(
            &program,
            "edit",
            ALICE,
            &staged,
            &fuel(5_000_000),
            &[s("n1")],
            true,
            &AtomicUsize::new(0),
        );
        drop(staged);
        // The adapter's one open command ended with the call: another
        // begins.
        let next = tables.begin().map(|_| ());
        let _ = told.send((answer, next));
    });
    let heard = heard.recv_timeout(Duration::from_secs(60));
    assert!(
        heard.is_ok(),
        "the predicate ran past a minute under five million fuel"
    );
    let Ok((answer, next)) = heard else {
        unreachable!()
    };
    let refused = answer.assert_err("its predicate ran out of fuel");
    assert!(
        refused.contains("authorization predicate `Loops` could not be evaluated")
            && refused.contains("out of fuel"),
        "{refused}"
    );
    assert!(next.is_ok(), "{next:?}");
}

/// A predicate and the command it guards, each spinning two to the
/// fifteenth calls.
const BUSY: &str = "module owned.app\n\nimport Notes.{ NoteId }\n\nfn spin(n: Int) -> Bool !{} {\n    match n <= 0 {\n        true => true,\n        false => spin(n - 1) == spin(n - 1),\n    }\n}\n\npredicate Spins(id: NoteId)\n    says \"Busy.\"\n{\n    spin(14)\n}\n\ncommand busy(id: NoteId) -> Bool\n    requires    Spins(id)\n    transaction serializable\n{\n    spin(14)\n}\n";

#[test]
fn a_predicate_spends_the_invocations_one_budget() {
    // What a predicate spends, the export it guards does not have: an
    // invocation's limits bound its predicates and its body together.
    let program = built(&[("app.pw", BUSY)]);
    let tables = seeded(&program);
    // Each alone, measured.
    let measured = |id: &str, strip: bool| -> u64 {
        let component = &program.components[&format!("owned.app.{id}")];
        let mut contract = component.contract.clone();
        if strip {
            for export in &mut contract.exports {
                if let Some(place) = export.component.as_mut() {
                    place.authorization.clear();
                }
            }
        }
        let (answer, usage) = pw_host::engine::Prepared::compile(&component.bytes)
            .assert_ok("it compiles")
            .call_measured(
                &contract,
                &component.granted(),
                &unbounded(),
                &program.platform(ALICE),
                &component.export(),
                &[s("n1")],
            )
            .assert_ok("it runs alone");
        assert_eq!(answer, vec![Val::Bool(true)]);
        usage.fuel
    };
    let (asked, body) = (measured("Spins", false), measured("busy", true));
    let most = asked.max(body);
    // Enough for either alone, with half again to instantiate it; not for
    // both.
    let budget = most + most / 2;
    assert!(asked + body > budget, "{asked} + {body} fits {budget}");
    let staged = tables.begin().assert_ok("its turn");
    let refused = authorized(
        &program,
        "busy",
        ALICE,
        &staged,
        &fuel(budget),
        &[s("n1")],
        true,
        &AtomicUsize::new(0),
    )
    .assert_err("the body has what the predicate left");
    // The predicate held; the body ran out.
    assert!(
        refused.contains("out of fuel") && !refused.contains("authorization predicate"),
        "{refused}"
    );
    drop(staged);
    // The neighbour: a budget for both.
    let staged = tables.begin().assert_ok("its turn");
    assert_eq!(
        authorized(
            &program,
            "busy",
            ALICE,
            &staged,
            &fuel(3 * most),
            &[s("n1")],
            true,
            &AtomicUsize::new(0),
        ),
        Ok(vec![Val::Bool(true)])
    );
}

/// The removal the concurrency test starts between a predicate and its
/// body: its thread, answering what the removal answered.
type Removal = std::thread::JoinHandle<Result<Vec<Val>, String>>;

#[test]
fn a_change_between_a_predicate_and_its_body_waits_for_the_command() {
    // ADR-0384's H2 acceptance: a predicate's read and its body's write are
    // one transaction, so a concurrent change between them cannot land.
    // Alice edits `n1` (`Wrote(n1)` first); between the predicate's read and
    // the body, alice's other tab removes `n1`. The memory adapter runs one
    // command at a time: the removal waits its turn until the edit commits.
    // What this proves is the memory adapter's serialization, with the
    // predicate inside it; a durable adapter, whose transactions run at
    // once and are refused on conflict, proves it again with its own.
    let program = Arc::new(program());
    let tables = Arc::new(seeded(&program));
    let order: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let log = |order: &Arc<Mutex<Vec<&'static str>>>, what| {
        if let Ok(mut order) = order.lock() {
            order.push(what);
        }
    };
    let (told, heard) = mpsc::channel::<&'static str>();
    let heard = Mutex::new(heard);
    {
        let (order, told) = (Arc::clone(&order), told.clone());
        tables.tell_on_wait(move || {
            log(&order, "remove waits");
            let _ = told.send("waits");
        });
    }
    let other: Arc<Mutex<Option<Removal>>> = Arc::default();
    let staged = tables.begin().assert_ok("the edit's turn");
    let mut watched = Watched::new(&staged);
    let once = Arc::new(AtomicBool::new(false));
    {
        let (program, tables, order, other) = (
            Arc::clone(&program),
            Arc::clone(&tables),
            Arc::clone(&order),
            Arc::clone(&other),
        );
        watched.after_read = Some(Arc::new(move || {
            if once.swap(true, Ordering::SeqCst) {
                return;
            }
            log(&order, "predicate read");
            let (program, tables, order, told) = (
                Arc::clone(&program),
                Arc::clone(&tables),
                Arc::clone(&order),
                told.clone(),
            );
            let removal = std::thread::spawn(move || {
                let staged = tables.begin().map_err(|e| format!("{e:?}"))?;
                log(&order, "remove began");
                let _ = told.send("began");
                let component = program.component("remove");
                let mut host = staged
                    .host_fns(&component.contract, &component.granted())
                    .map_err(|e| format!("{e:?}"))?;
                host.extend(program.platform(ALICE));
                let answer = pw_host::engine::call_with_tables_as(
                    &component.bytes,
                    &component.contract,
                    &component.granted(),
                    &pw_host::Limits::unbounded(),
                    &host,
                    &program.manifest,
                    &as_user(ALICE),
                    &component.export(),
                    &[s("n1"), handle(BOB)],
                )?;
                staged.commit().map_err(|e| format!("{e:?}"))?;
                log(&order, "remove committed");
                Ok(answer)
            });
            if let Ok(mut slot) = other.lock() {
                *slot = Some(removal);
            }
            // Until the removal waits its turn, or begins.
            if let Ok(heard) = heard.lock() {
                let _ = heard.recv_timeout(Duration::from_secs(30));
            }
        }));
    }
    let edited = authorized(
        &program,
        "edit",
        ALICE,
        &watched,
        &unbounded(),
        &[s("n1"), handle(BOB), s("edited")],
        false,
        &AtomicUsize::new(0),
    );
    assert_eq!(edited, Ok(vec![]));
    drop(watched);
    staged.commit().assert_ok("the edit's commit");
    log(&order, "edit committed");
    let removal = other
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .assert_ok("the removal began");
    let removed = removal.join();
    assert!(
        matches!(removed, Ok(Ok(ref answer)) if answer.is_empty()),
        "{removed:?}"
    );
    let order = order.lock().map(|order| order.clone()).unwrap_or_default();
    assert_eq!(
        order,
        [
            "predicate read",
            "remove waits",
            "edit committed",
            "remove began",
            "remove committed"
        ]
    );
    // The edit wrote against what its predicate read; the removal, after it.
    assert!(
        !every_note(&tables).contains(&(ALICE.to_string(), "n1".to_string())),
        "{:?}",
        every_note(&tables)
    );
}
