//! Fixed relation windows through real compiled PostgreSQL calls.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/relations.rs");
mod common;
use pw_host::engine::CallTables;
use pw_table_postgres::PgTables;

fn q(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    q_counted(program, tables, id, caller, args, &Arc::default())
}

fn q_counted(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    calls: &Arc<AtomicUsize>,
) -> Result<Vec<Val>, String> {
    q_limited(
        program,
        tables,
        id,
        caller,
        args,
        calls,
        &pw_host::Limits::unbounded(),
    )
}

fn q_limited(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    calls: &Arc<AtomicUsize>,
    limits: &pw_host::Limits,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let snapshot = tables.snapshot().map_err(|e| format!("{e:?}"))?;
    let mut host = snapshot
        .functions(&component.contract, &granted)?
        .into_iter()
        .map(|(key, call)| (key, counted(call, calls)))
        .collect::<BTreeMap<_, _>>();
    let user = match caller {
        Caller::User(user) => user.as_str(),
        _ => "",
    };
    host.extend(platform(component, user));
    pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        limits,
        &host,
        &program.manifest,
        caller,
        &component.export(),
        args,
    )
}

#[test]
fn pg_relations_return_one_global_window_and_canonical_overlapping_groups_after_reopen() {
    let program = relation_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "compiled_relations");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
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
        .assert_ok("seed public targets/private memberships");
    drop(tables);
    let tables = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("reopened PostgreSQL");
    for (name, ids) in [("RelFirst", vec![1, 2]), ("RelLast", vec![4, 3])] {
        assert_eq!(
            q(
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
        q(
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
        q(
            &program,
            &tables,
            "RelLast",
            &as_user(BOB),
            &[handle(BOB), Val::S64(10), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![relation_root_value(5)])])
    );
    assert_eq!(
        q(
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
            q(
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
        q(
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
fn pg_relation_authority_and_whole_bad_requests_reach_no_data_sql() {
    let program = relation_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "relation_request_gate");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let before = tables.operation_statements();
    let forged = q(
        &program,
        &tables,
        "RelLast",
        &as_user(ALICE),
        &[handle(BOB), Val::S64(10), Val::S64(0)],
    );
    assert!(forged.is_err(), "forged reader even at zero: {forged:?}");
    let absent = q(
        &program,
        &tables,
        "RelLast",
        &Caller::Nobody,
        &[handle(ALICE), Val::S64(10), Val::S64(2)],
    );
    assert!(absent.is_err(), "no caller: {absent:?}");
    assert_eq!(tables.operation_statements() - before, 0, "no data SQL");
    for args in [
        vec![
            json!([{ "reader":ALICE,"topic":10},{"reader":ALICE,"topic":true}]),
            json!(0),
        ],
        vec![
            json!([{ "reader":ALICE,"topic":10},{"reader":ALICE}]),
            json!(2),
        ],
        vec![json!([{ "reader":ALICE,"topic":10,"extra":1}]), json!(2)],
        vec![json!([ALICE]), json!(2)],
        vec![json!([]), json!(true)],
    ] {
        let ops = tables.operation_statements();
        let rows = tables.returned_rows();
        let bytes = tables.returned_bytes();
        let result = tables.read("pw:tables/links#last-posts-by-reader-topic-many", &args);
        assert!(
            result.is_err(),
            "whole malformed request {args:?}: {result:?}"
        );
        assert_eq!(
            (
                tables.operation_statements() - ops,
                tables.returned_rows() - rows,
                tables.returned_bytes() - bytes
            ),
            (0, 0, 0)
        );
    }
    let before = tables.operation_statements();
    assert_eq!(
        tables
            .read(
                "pw:tables/links#last-posts-by-reader-topic-many",
                &[json!([]), json!(2)]
            )
            .assert_ok("empty set"),
        json!([])
    );
    assert_eq!(
        tables
            .read(
                "pw:tables/links#last-posts-by-reader-topic-many",
                &[json!([{ "reader":ALICE,"topic":999}]), json!(0)]
            )
            .assert_ok("zero set window"),
        json!([{"key":{"reader":ALICE,"topic":999},"rows":[]}])
    );
    assert_eq!(
        tables.operation_statements() - before,
        0,
        "empty/zero needs no SQL after whole validation"
    );
}

#[test]
fn a_relation_predicate_sees_both_staged_tables_and_pins_membership_with_the_body() {
    let program = relation_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "relation_snapshot");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");

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
fn relation_result_work_is_bounded_across_growing_membership_and_target_cardinality() {
    let program = relation_program();
    let Some(url) = common::database() else {
        return;
    };
    for size in [12i64, 1500] {
        let scratch = common::Scratch::new(&url, "relation_result_work");
        let tables =
            PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
        let text = "T".repeat(280);
        tables.transaction(|tx|{
            for author in 0..size {
                let author_id=format!("author-{author}");
                for topic in [10,20] {
                    tx.write("pw:tables/links#insert",&[json!({"reader":ALICE,"topic":topic,"author":author_id,"edge":author,"to":BOB,"secret":text})])?;
                }
                for item in 1..=3 {
                    let id=author*3+item;
                    tx.write("pw:tables/roots#insert",&[json!({"id":id,"author":author_id,"stamp":id,"text":text})])?;
                }
            }
            Ok(())
        }).assert_ok("many members and posts");
        let row = |id| {
            Val::Record(vec![
                ("id".into(), Val::S64(id)),
                (
                    "author".into(),
                    Val::String(format!("author-{}", (id - 1) / 3)),
                ),
                ("stamp".into(), Val::S64(id)),
                ("text".into(), Val::String(text.clone())),
            ])
        };
        let mut limits = pw_host::Limits::unbounded();
        limits.fuel = Some(10_000);
        let ops = tables.operation_statements();
        let rows = tables.returned_rows();
        let bytes = tables.returned_bytes();
        let scalar = q_limited(
            &program,
            &tables,
            "RelLast",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(10), Val::S64(2)],
            &Arc::default(),
            &limits,
        );
        assert_eq!(
            scalar,
            Ok(vec![Val::List(vec![row(size * 3), row(size * 3 - 1)])]),
            "same scalar fuel ceiling at{size}"
        );
        assert_eq!(
            tables.operation_statements() - ops,
            1,
            "one global query at{size}"
        );
        assert_eq!(
            tables.returned_rows() - rows,
            2,
            "only final target rows cross SQL at{size}"
        );
        assert!(
            tables.returned_bytes() - bytes <= 700,
            "bounded fixture payload at{size}: {}",
            tables.returned_bytes() - bytes
        );
        limits.fuel = Some(20_000);
        let ops = tables.operation_statements();
        let rows = tables.returned_rows();
        let bytes = tables.returned_bytes();
        let input = [
            handle(ALICE),
            Val::List(vec![
                Val::S64(20),
                Val::S64(999),
                Val::S64(10),
                Val::S64(10),
            ]),
            Val::S64(2),
        ];
        let group = |topic| {
            Val::Record(vec![
                (
                    "key".into(),
                    Val::Record(vec![
                        ("reader".into(), handle(ALICE)),
                        ("topic".into(), Val::S64(topic)),
                    ]),
                ),
                (
                    "rows".into(),
                    Val::List(if topic == 999 {
                        vec![]
                    } else {
                        vec![row(size * 3), row(size * 3 - 1)]
                    }),
                ),
            ])
        };
        assert_eq!(
            q_limited(
                &program,
                &tables,
                "RelMany",
                &as_user(ALICE),
                &input,
                &Arc::default(),
                &limits
            ),
            Ok(vec![Val::List(vec![group(10), group(20), group(999)])]),
            "same set fuel ceiling at{size}"
        );
        assert_eq!(
            tables.operation_statements() - ops,
            1,
            "one lateral query for all distinct keys"
        );
        assert_eq!(
            tables.returned_rows() - rows,
            4,
            "per-selector global limit, duplicates coalesced, no fake absent target"
        );
        assert!(
            tables.returned_bytes() - bytes <= 1400,
            "four bounded payloads including slots at{size}: {}",
            tables.returned_bytes() - bytes
        );
        limits.fuel = Some(1);
        assert!(
            q_limited(
                &program,
                &tables,
                "RelLast",
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(10), Val::S64(2)],
                &Arc::default(),
                &limits
            )
            .is_err(),
            "fuel ceiling has a failing neighbor"
        );
        let rows = tables.returned_rows();
        let bytes = tables.returned_bytes();
        let all = tables
            .read("pw:tables/links#of-reader", &[json!(ALICE)])
            .assert_ok("unbounded membership neighbor");
        assert_eq!(all.as_array().unwrap().len(), (size * 2) as usize);
        assert_eq!(tables.returned_rows() - rows, (size * 2) as u64);
        assert!(
            tables.returned_bytes() - bytes > (size * 2 * 280) as u64,
            "all-membership materialization grows"
        );
        let rows = tables.returned_rows();
        let all = tables
            .read(
                "pw:tables/links#last-posts-by-reader-topic",
                &[json!(ALICE), json!(10), json!(i64::MAX)],
            )
            .assert_ok("explicit all qualifying targets neighbor");
        assert_eq!(all.as_array().unwrap().len(), (size * 3) as usize);
        assert_eq!(
            tables.returned_rows() - rows,
            (size * 3) as u64,
            "many targets really qualify for the small global window"
        );
    }
}

/// **An owned `public table`'s relation is read by anyone, through an index
/// its owner does not lead** (the L4 review of 2026-11-16): the compiler
/// reads a public table by any key or index (L2), and until the review the
/// host refused the manifest it made ("has no declared source
/// selector/match lookup"). Lane 7 holds two owners' picks.
#[test]
fn a_public_owned_membership_is_read_through_any_index_by_anyone() {
    let program = relation_public_owned_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "public_owned_relation");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for row in relation_roots() {
                tx.write("pw:tables/roots#insert", &[row])?;
            }
            for row in relation_public_owned_picks() {
                tx.write("pw:tables/picks#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("public owned picks");
    assert_eq!(
        q(
            &program,
            &tables,
            "LaneLast",
            &Caller::Nobody,
            &[Val::S64(7), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![
            relation_root_value(4),
            relation_root_value(3)
        ])])
    );
    assert_eq!(
        q(
            &program,
            &tables,
            "LaneLast",
            &Caller::Nobody,
            &[Val::S64(8), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![
            relation_root_value(3),
            relation_root_value(1)
        ])])
    );
}

/// **A malformed relation request is refused alike, word for word**, on
/// both adapters, the import it read named (the L4 review of 2026-11-16:
/// PostgreSQL named "relation window"), called on each directly.
#[test]
fn a_malformed_relation_request_is_refused_alike_on_both_adapters() {
    let program = relation_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "relation_refusals");
    let on_postgres =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let on_memory = pw_table_memory::MemoryTables::new(program.tables.clone()).assert_ok("memory");
    let import = "pw:tables/links#first-posts-by-reader-topic";
    for args in [
        vec![json!(ALICE), json!("ten"), json!(2)],
        vec![json!(ALICE), json!(1.5), json!(2)],
        vec![json!(ALICE), json!(2)],
        vec![json!(ALICE), json!(10), json!("two")],
        vec![],
    ] {
        let memory = on_memory.read(import, &args).map_err(|e| match e {
            pw_table_memory::Error::Input(why) => why,
            other => format!("not an input refusal: {other:?}"),
        });
        let postgres = on_postgres.read(import, &args).map_err(|e| match e {
            pw_table_postgres::Error::Input(why) => why,
            other => format!("not an input refusal: {other:?}"),
        });
        assert!(memory.is_err(), "{args:?}: {memory:?}");
        assert_eq!(memory, postgres, "{args:?}");
    }
    let named = on_postgres
        .read(import, &[json!(ALICE), json!("ten"), json!(2)])
        .expect_err("a String topic");
    assert!(format!("{named:?}").contains(import), "{named:?}");
}

#[test]
fn public_relations_use_scalar_bool_group_keys_and_allow_anonymous_callers() {
    let program = relation_public_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "public_relation_keys");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");

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
        q(
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
        q(&program, &tables, "PlainMany", &Caller::Nobody, &args),
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
        q(&program, &tables, "PlainMany", &Caller::Nobody, &args),
        Ok(vec![Val::List(vec![
            relation_public_group(false, &[]),
            relation_public_group(true, &[])
        ])])
    );
    tables
        .transaction(|tx| tx.write("pw:tables/links#delete-of-topic", &[json!(true)]))
        .assert_ok("remove one public selector's members");
    assert_eq!(
        q(
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

/// **A window reads an owned `public table` as every reader reads it**, on
/// PostgreSQL as on memory (L7's G5, 2026-11-17): the posts their authors own
/// answer every reader's window, and again after the tables are reopened.
#[test]
fn a_window_reads_an_owned_public_target_as_every_reader_reads_it() {
    let program = relation_owned_target_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_public_target");
    let open =
        || PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let tables = open();
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
        .assert_ok("posts their authors own, and public memberships");
    for caller in [Caller::Nobody, as_user(ALICE), as_user(BOB)] {
        assert_eq!(
            q(
                &program,
                &tables,
                "PlainLast",
                &caller,
                &[Val::Bool(false), Val::S64(2)]
            ),
            Ok(vec![Val::List(vec![
                relation_root_value(3),
                relation_root_value(1)
            ])]),
            "before the reopen"
        );
    }
    drop(tables);
    let tables = open();
    for caller in [Caller::Nobody, as_user(ALICE), as_user(BOB)] {
        assert_eq!(
            q(
                &program,
                &tables,
                "PlainLast",
                &caller,
                &[Val::Bool(false), Val::S64(2)]
            ),
            Ok(vec![Val::List(vec![
                relation_root_value(3),
                relation_root_value(1)
            ])])
        );
    }
    let args = [
        Val::List(vec![Val::Bool(true), Val::Bool(false), Val::Bool(true)]),
        Val::S64(2),
    ];
    assert_eq!(
        q(&program, &tables, "PlainMany", &Caller::Nobody, &args),
        Ok(vec![Val::List(vec![
            relation_public_group(false, &[3, 1]),
            relation_public_group(true, &[4, 3])
        ])])
    );
}

/// **A command, made as `user`**, as the platform runs one: in a command of
/// its own, committed where it answers, undone where it fails or answers a
/// declared error (memory's `command_as`, on PostgreSQL).
fn pg_command(
    program: &Program,
    tables: &PgTables,
    id: &str,
    user: &str,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let staged = tables.begin().assert_ok("its turn");
    let mut host = staged
        .host_fns(&component.contract, &granted)
        .assert_ok("the gate admits the command's reads and writes");
    host.extend(platform(component, user));
    let answer = pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &program.manifest,
        &as_user(user),
        &component.export(),
        args,
    );
    let declared_error = matches!(answer.as_deref(), Ok([Val::Result(Err(_))]));
    if answer.is_ok() && !declared_error {
        staged
            .commit()
            .map_err(|error| format!("commit refused: {error:?}"))?;
    }
    answer
}

/// **The feed's own shape, on PostgreSQL** (G5's review, 2026-11-17): each
/// reader's window by their own handle, in the owner-led order, and a post
/// its author writes, read in its command's own window before it commits.
#[test]
fn the_feeds_shape_reads_each_readers_window_and_a_staged_post() {
    let program = relation_feed_shape_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "feed_shape_window");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for row in relation_feed_posts() {
                tx.write("pw:tables/roots#insert", &[row])?;
            }
            for row in relation_feed_links() {
                tx.write("pw:tables/links#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("posts their authors own, memberships their readers own");
    assert_eq!(
        q(
            &program,
            &tables,
            "Feed",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(1), Val::S64(3)]
        ),
        Ok(vec![Val::List(vec![
            relation_feed_post(3, CAROL, 7, "carol 3"),
            relation_feed_post(2, CAROL, 5, "carol 2"),
            relation_feed_post(9, BOB, 5, "bob 9"),
        ])]),
        "Alice's window, in the owner-led order"
    );
    let posted = pg_command(
        &program,
        &tables,
        "PostAndRead",
        ALICE,
        &[Val::S64(10), Val::S64(9), Val::S64(1), Val::S64(2)],
    );
    assert_eq!(
        posted,
        Ok(vec![Val::List(vec![
            relation_feed_post(10, ALICE, 9, "staged"),
            relation_feed_post(3, CAROL, 7, "carol 3"),
        ])]),
        "the post its author wrote, in the command's own window before it commits"
    );
    assert_eq!(
        q(
            &program,
            &tables,
            "Feed",
            &as_user(BOB),
            &[handle(BOB), Val::S64(1), Val::S64(1)]
        ),
        Ok(vec![Val::List(vec![relation_feed_post(
            10, ALICE, 9, "staged"
        )])]),
        "and Bob's, once it committed"
    );
}

#[test]
fn private_membership_match_is_usable_by_its_owner_and_no_foreign_sharer() {
    let program = private_match_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "private_relation_match");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
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
        .assert_ok("private membership and public target rows");
    for (reader, ids) in [(ALICE, vec![4, 3]), (BOB, vec![5])] {
        assert_eq!(
            q(
                &program,
                &tables,
                "PrivateMatch",
                &as_user(reader),
                &[handle(reader), Val::S64(2)]
            ),
            Ok(vec![Val::List(
                ids.into_iter().map(relation_root_value).collect()
            )])
        );
    }
    let before = tables.statements();
    assert!(
        q(
            &program,
            &tables,
            "PrivateMatch",
            &as_user(BOB),
            &[handle(ALICE), Val::S64(2)]
        )
        .is_err(),
        "a sharer of Alice's rows cannot read the relation selected by her private match field"
    );
    assert_eq!(
        tables.statements(),
        before,
        "foreign handle refused before data work"
    );
}
